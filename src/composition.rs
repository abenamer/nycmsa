//! Composition root. This is the one place that selects concrete adapters
//! and connects them to the use cases (REAS-ARCH-001). The `nycmsa` binary
//! calls one function per subcommand.

use std::collections::BTreeSet;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;

use crate::adapters::cli::render::{render_report, render_scan};
use crate::adapters::cli::{
    scan_request, simulation_request, FetchArgs, ScanArgs, ServeArgs, SimArgs,
};
use crate::adapters::csv_export::scan_rows_csv;
use crate::adapters::dto::{run_to_dto, InvalidInput, StudentDto};
use crate::adapters::http::{router, AppState, HttpLimits};
use crate::adapters::json_files::{load_program_definitions, load_student, FileLoadError};
use crate::adapters::myschools::{load_catalog, CatalogLoadError};
use crate::adapters::myschools_api::{ClientBuildError, DEFAULT_BASE_URL};
use crate::adapters::postgres_runs::PostgresSetupError;
use crate::adapters::{
    InMemoryRunRepository, MySchoolsApiClient, PostgresRunRepository, SystemClock,
};
use crate::application::{
    Clock, DistrictFilter, DownloadCatalog, DownloadError, RunQueries, RunRepository, ScanCatalog,
    SimulateStudent,
};
use crate::domain::catalog::InvalidDistrictCode;
use crate::domain::{DistrictCode, SimulationError};

/// Every failure the binary can report. Each variant keeps its source.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    File(#[from] FileLoadError),
    #[error(transparent)]
    Catalog(#[from] CatalogLoadError),
    #[error(transparent)]
    Invalid(#[from] InvalidInput),
    #[error(transparent)]
    Simulation(#[from] SimulationError),
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error(transparent)]
    Client(#[from] ClientBuildError),
    #[error(transparent)]
    Postgres(#[from] PostgresSetupError),
    #[error("cannot write CSV: {0}")]
    Csv(#[from] csv::Error),
    #[error("cannot encode JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cannot write {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot listen on {addr}: {source}")]
    Bind {
        addr: String,
        #[source]
        source: std::io::Error,
    },
    #[error("server failed: {0}")]
    Server(#[source] std::io::Error),
    #[error("cannot start the async runtime: {0}")]
    Runtime(#[source] std::io::Error),
    #[error("--max-runs must be at least 1")]
    ZeroMaxRuns,
    #[error("--districts: {0}")]
    Districts(#[from] InvalidDistrictCode),
}

/// `nycmsa sim`: load the files, apply overrides, run, render.
///
/// The CLI runs on its own thread with nothing else scheduled, so the CPU
/// work runs inline. No run is stored.
pub fn run_sim(args: &SimArgs) -> Result<String, AppError> {
    let programs = load_program_definitions(&args.programs)?;
    let student = load_student(&args.student)?;
    let request = simulation_request(args, student)?;
    let use_case = SimulateStudent::new(
        programs,
        InMemoryRunRepository::new(NonZeroUsize::MIN),
        SystemClock,
    );
    let report = use_case.compute(&request)?;
    if args.json {
        let dto = crate::adapters::dto::report_to_dto(&report);
        return Ok(format!("{}\n", serde_json::to_string_pretty(&dto)?));
    }
    Ok(render_report(&request.student, &report))
}

/// `nycmsa scan`: load the catalog, scan, render, and optionally write CSV.
pub fn run_scan(args: &ScanArgs) -> Result<String, AppError> {
    let catalog = load_catalog(&args.data)?;
    let use_case = ScanCatalog::new(catalog);
    let request = scan_request(args)?;
    let rows = use_case.execute(&request);
    let mut text = render_scan(&request, &rows);
    if let Some(path) = &args.csv {
        let csv = scan_rows_csv(&rows)?;
        std::fs::write(path, csv).map_err(|source| AppError::Write {
            path: path.display().to_string(),
            source,
        })?;
        text.push_str(&format!("\nwrote {}\n", path.display()));
    }
    Ok(text)
}

fn district_filter(raw: &str) -> Result<DistrictFilter, AppError> {
    if raw.trim().eq_ignore_ascii_case("all") {
        return Ok(DistrictFilter::All);
    }
    let codes = raw
        .split(',')
        .map(DistrictCode::parse)
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(DistrictFilter::Only(codes))
}

/// `nycmsa fetch`: download the catalog and write it verbatim.
pub async fn run_fetch(args: &FetchArgs) -> Result<String, AppError> {
    let filter = district_filter(&args.districts)?;
    let client = MySchoolsApiClient::new(DEFAULT_BASE_URL.to_owned(), Duration::from_millis(500))?;
    let use_case = DownloadCatalog::new(client);
    let snapshot = use_case.execute(&filter).await?;
    let text = serde_json::to_string(&snapshot.schools)?;
    std::fs::write(&args.out, text).map_err(|source| AppError::Write {
        path: args.out.display().to_string(),
        source,
    })?;
    Ok(format!(
        "wrote {} schools to {}\n",
        snapshot.len(),
        args.out.display()
    ))
}

/// `nycmsa serve`: select the run store, build the use cases, and serve.
pub async fn run_serve(args: &ServeArgs) -> Result<(), AppError> {
    let programs = load_program_definitions(&args.programs)?;
    let student = load_student(&args.student)?;
    let catalog = load_catalog(&args.catalog)?;
    tracing::info!(
        programs = programs.len(),
        catalog_programs = catalog.len(),
        "loaded data files"
    );
    match &args.database_url {
        Some(url) => {
            let repository = PostgresRunRepository::connect(url, 5).await?;
            repository.migrate().await?;
            tracing::info!("runs are stored in PostgreSQL");
            serve_with(args, repository, programs, student, catalog).await
        }
        None => {
            let capacity = NonZeroUsize::new(args.max_runs).ok_or(AppError::ZeroMaxRuns)?;
            tracing::info!(capacity, "runs are stored in memory");
            serve_with(
                args,
                InMemoryRunRepository::new(capacity),
                programs,
                student,
                catalog,
            )
            .await
        }
    }
}

/// Generic over the run store so both stores use static dispatch; the
/// concrete type is visible in `run_serve` (REAS-ARCH-006).
async fn serve_with<R>(
    args: &ServeArgs,
    repository: R,
    programs: crate::domain::ProgramDefinitions,
    student: StudentDto,
    catalog: Vec<crate::domain::CatalogProgram>,
) -> Result<(), AppError>
where
    R: RunRepository + Clone + Send + Sync + 'static,
{
    let limits = HttpLimits::default();
    let state = build_state(repository, SystemClock, programs, student, catalog, &limits);
    let app = router(state, &limits);
    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .map_err(|source| AppError::Bind {
            addr: args.bind.clone(),
            source,
        })?;
    tracing::info!(addr = %args.bind, "listening; every endpoint is anonymous");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(AppError::Server)
}

/// Builds the handler state. Shared with the HTTP tests.
pub fn build_state<R, C>(
    repository: R,
    clock: C,
    programs: crate::domain::ProgramDefinitions,
    student: StudentDto,
    catalog: Vec<crate::domain::CatalogProgram>,
    limits: &HttpLimits,
) -> AppState<R, C>
where
    R: RunRepository + Clone,
    C: Clock,
{
    AppState {
        simulate: Arc::new(SimulateStudent::new(programs, repository.clone(), clock)),
        runs: Arc::new(RunQueries::new(repository)),
        scan: Arc::new(ScanCatalog::new(catalog)),
        default_student: Arc::new(student),
        cpu_permits: Arc::new(Semaphore::new(limits.cpu_permits.max(1))),
        recent_runs: limits.recent_runs,
    }
}

/// Resolves on Ctrl-C. In-flight requests then finish; the binary applies a
/// shutdown deadline to the runtime (see `main`).
async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "cannot listen for Ctrl-C; running until killed");
        std::future::pending::<()>().await;
    }
    tracing::info!("shutdown requested");
}

/// Loads the bundled data files for tests and local tools.
pub fn load_bundled(
    data_dir: &Path,
) -> Result<
    (
        crate::domain::ProgramDefinitions,
        StudentDto,
        Vec<crate::domain::CatalogProgram>,
    ),
    AppError,
> {
    Ok((
        load_program_definitions(&data_dir.join("programs.json"))?,
        load_student(&data_dir.join("student.json"))?,
        load_catalog(&data_dir.join("myschools_d15_d20.json"))?,
    ))
}

#[allow(dead_code)]
fn _assert_run_dto_is_used() {
    let _ = run_to_dto;
}
