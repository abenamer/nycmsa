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
    scan_request, simulation_request, BootstrapArgs, DatabaseArgs, FetchArgs, FetchProfilesArgs,
    FetchScoresArgs, ImportArgs, ScanArgs, ScoreSource, ServeArgs, SimArgs,
};
use crate::adapters::csv_export::scan_rows_csv;
use crate::adapters::dto::{student_from_dto, student_to_dto, InvalidInput, StudentDto};
use crate::adapters::http::{router, AppState, HttpLimits, ProfileBook};
use crate::adapters::json_files::{load_program_definitions, load_student, FileLoadError};
use crate::adapters::myschools::{catalog_from_documents, load_catalog, CatalogLoadError};
use crate::adapters::myschools_api::{ClientBuildError, DEFAULT_BASE_URL};
use crate::adapters::postgres_runs::PostgresSetupError;
use crate::adapters::{infohub, open_data};

use crate::adapters::{
    InMemoryRunRepository, InfoHubClient, MySchoolsApiClient, NycOpenDataClient,
    PostgresReferenceData, PostgresRunRepository, SystemClock,
};
use crate::application::{
    Clock, DistrictFilter, DownloadError, ImportSeeds, LoadReferenceData, ReclassifyProfiles,
    ReferenceError, RefreshCatalog, RefreshProfiles, RefreshTestResults, RunQueries, RunRepository,
    ScanCatalog, SimulateStudent,
};
use crate::domain::catalog::InvalidDistrictCode;
use crate::domain::{
    programs_from_catalog, CatalogProgram, DistrictCode, ProgramDefinitions, ProgramProfile,
    SimulationError, TestResults,
};

/// Every failure the binary can report. Each variant keeps its source.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    File(#[from] FileLoadError),
    #[error(transparent)]
    Catalog(#[from] CatalogLoadError),
    #[error(transparent)]
    Reference(#[from] ReferenceError),
    #[error(transparent)]
    Invalid(#[from] InvalidInput),
    #[error(transparent)]
    Simulation(#[from] SimulationError),
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error(transparent)]
    Client(#[from] ClientBuildError),
    #[error(transparent)]
    OpenDataClient(#[from] open_data::ClientBuildError),
    #[error(transparent)]
    InfoHubClient(#[from] infohub::ClientBuildError),
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

/// `nycmsa scan`: load a catalog file, scan, render, and optionally write
/// CSV. The CLI scan shows no test scores; the web page does.
pub fn run_scan(args: &ScanArgs) -> Result<String, AppError> {
    let catalog = load_catalog(&args.data)?;
    let use_case = ScanCatalog::new(catalog, TestResults::new());
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

/// Opens the database, applies migrations, and returns the two adapters
/// that share its pool.
async fn connect(
    database: &DatabaseArgs,
) -> Result<(PostgresRunRepository, PostgresReferenceData), AppError> {
    let runs = PostgresRunRepository::connect(&database.database_url, 5).await?;
    runs.migrate().await?;
    let reference = PostgresReferenceData::new(runs.pool().clone());
    Ok((runs, reference))
}

async fn fetch_catalog(
    reference: PostgresReferenceData,
    districts: &str,
) -> Result<String, AppError> {
    let filter = district_filter(districts)?;
    let client = MySchoolsApiClient::new(DEFAULT_BASE_URL.to_owned(), Duration::from_millis(500))?;
    let count = RefreshCatalog::new(client, reference, SystemClock)
        .execute(
            &filter,
            &format!("MySchools API {DEFAULT_BASE_URL} ({districts})"),
        )
        .await?;
    Ok(format!("stored a catalog snapshot of {count} schools\n"))
}

async fn fetch_scores(
    reference: PostgresReferenceData,
    source: ScoreSource,
) -> Result<String, AppError> {
    let (count, description) = match source {
        ScoreSource::Infohub => {
            let client =
                InfoHubClient::new(infohub::ELA_URL.to_owned(), infohub::MATH_URL.to_owned())?;
            RefreshTestResults::new(client, reference, SystemClock)
                .execute()
                .await?
        }
        ScoreSource::OpenData => {
            let client = NycOpenDataClient::new(open_data::DEFAULT_BASE_URL.to_owned())?;
            RefreshTestResults::new(client, reference, SystemClock)
                .execute()
                .await?
        }
    };
    Ok(format!(
        "stored test results for {count} schools from {description}\n"
    ))
}

async fn fetch_profiles(reference: PostgresReferenceData) -> Result<String, AppError> {
    let client = MySchoolsApiClient::new(DEFAULT_BASE_URL.to_owned(), Duration::from_millis(250))?;
    let count = RefreshProfiles::new(client, reference, SystemClock)
        .execute(
            &format!("MySchools API {DEFAULT_BASE_URL}<school id>/"),
            |done, total| {
                if done % 50 == 0 || done == total {
                    tracing::info!(done, total, "looked up schools");
                }
            },
        )
        .await?;
    Ok(format!("stored profiles for {count} programs\n"))
}

/// `nycmsa reclassify-profiles`: apply the current rules to the stored
/// texts.
pub async fn run_reclassify_profiles(args: &FetchProfilesArgs) -> Result<String, AppError> {
    let (_, reference) = connect(&args.database).await?;
    let count = ReclassifyProfiles::new(reference, SystemClock)
        .execute()
        .await?;
    Ok(format!("reclassified {count} program profiles\n"))
}

/// `nycmsa fetch-profiles`: look up and classify every program.
pub async fn run_fetch_profiles(args: &FetchProfilesArgs) -> Result<String, AppError> {
    let (_, reference) = connect(&args.database).await?;
    fetch_profiles(reference).await
}

async fn import_seeds(
    reference: PostgresReferenceData,
    programs: &Path,
    student: &Path,
) -> Result<String, AppError> {
    let definitions = load_program_definitions(programs)?;
    let defaults = student_from_dto(load_student(student)?)?;
    ImportSeeds::new(reference, SystemClock)
        .execute(
            &definitions,
            &programs.display().to_string(),
            &defaults,
            &student.display().to_string(),
        )
        .await?;
    Ok(format!(
        "stored {} hand-entered programs and the form defaults\n",
        definitions.len()
    ))
}

/// `nycmsa fetch`: download the catalog into the database.
pub async fn run_fetch(args: &FetchArgs) -> Result<String, AppError> {
    let (_, reference) = connect(&args.database).await?;
    fetch_catalog(reference, &args.districts).await
}

/// `nycmsa fetch-scores`: download the test results into the database.
pub async fn run_fetch_scores(args: &FetchScoresArgs) -> Result<String, AppError> {
    let (_, reference) = connect(&args.database).await?;
    fetch_scores(reference, args.source).await
}

/// `nycmsa import`: store the seed files.
pub async fn run_import(args: &ImportArgs) -> Result<String, AppError> {
    let (_, reference) = connect(&args.database).await?;
    import_seeds(reference, &args.programs, &args.student).await
}

/// `nycmsa bootstrap`: import the seeds and download whatever is missing,
/// so a fresh database can serve.
pub async fn run_bootstrap(args: &BootstrapArgs) -> Result<String, AppError> {
    let (_, reference) = connect(&args.database).await?;
    let inventory = LoadReferenceData::new(reference.clone())
        .inventory()
        .await?;
    let mut report = String::new();
    if !(inventory.programs && inventory.student) {
        report.push_str(&import_seeds(reference.clone(), &args.programs, &args.student).await?);
    }
    if !inventory.catalog {
        report.push_str(&fetch_catalog(reference.clone(), &args.districts).await?);
    }
    if !inventory.results {
        report.push_str(&fetch_scores(reference.clone(), args.source).await?);
    }
    if !inventory.profiles {
        report.push_str(&fetch_profiles(reference).await?);
    }
    if report.is_empty() {
        report.push_str("the database already holds every reference set\n");
    }
    Ok(report)
}

/// The simulation's programs: every catalog program with figures, with the
/// hand-entered entries replacing the codes they cover.
pub fn simulation_programs(
    catalog: &[CatalogProgram],
    curated: ProgramDefinitions,
) -> Result<ProgramDefinitions, AppError> {
    let derived =
        ProgramDefinitions::new(programs_from_catalog(catalog)).map_err(InvalidInput::from)?;
    Ok(derived.merged_with(curated))
}

/// `nycmsa serve`: read the latest reference snapshots and serve.
pub async fn run_serve(args: &ServeArgs) -> Result<(), AppError> {
    let (runs, reference) = connect(&args.database).await?;
    let data = LoadReferenceData::new(reference).execute().await?;
    tracing::info!(%data, "loaded reference data");
    let catalog = catalog_from_documents(&data.catalog.value.schools)?;
    let programs = simulation_programs(&catalog, data.programs.value)?;
    tracing::info!(
        programs = programs.len(),
        catalog_programs = catalog.len(),
        "built the simulation programs"
    );
    serve_with(
        args,
        runs,
        programs,
        student_to_dto(&data.student.value),
        catalog,
        data.results.value,
        data.profiles.value,
    )
    .await
}

/// Generic over the run store so both stores use static dispatch; the
/// concrete type is visible in `run_serve` (REAS-ARCH-006).
async fn serve_with<R>(
    args: &ServeArgs,
    repository: R,
    programs: ProgramDefinitions,
    student: StudentDto,
    catalog: Vec<CatalogProgram>,
    results: TestResults,
    profiles: Vec<ProgramProfile>,
) -> Result<(), AppError>
where
    R: RunRepository + Clone + Send + Sync + 'static,
{
    let limits = HttpLimits::default();
    let state = build_state(
        repository,
        SystemClock,
        programs,
        student,
        catalog,
        results,
        profiles,
        &limits,
    );
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
#[allow(clippy::too_many_arguments)]
pub fn build_state<R, C>(
    repository: R,
    clock: C,
    programs: ProgramDefinitions,
    student: StudentDto,
    catalog: Vec<CatalogProgram>,
    results: TestResults,
    profiles: Vec<ProgramProfile>,
    limits: &HttpLimits,
) -> AppState<R, C>
where
    R: RunRepository + Clone,
    C: Clock,
{
    AppState {
        simulate: Arc::new(SimulateStudent::new(programs, repository.clone(), clock)),
        runs: Arc::new(RunQueries::new(repository)),
        scan: Arc::new(ScanCatalog::new(catalog, results)),
        profiles: Arc::new(ProfileBook::new(profiles)),
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

/// The data files loaded for the web server.
pub struct BundledData {
    pub programs: ProgramDefinitions,
    pub student: StudentDto,
    pub catalog: Vec<CatalogProgram>,
    pub results: TestResults,
    pub profiles: Vec<ProgramProfile>,
}

/// Loads the seed files and a catalog file for tests and local tools, with
/// no test results. The programs are derived from the catalog with the seed
/// programs merged on top, as `serve` does.
pub fn load_bundled(seed_dir: &Path, catalog_file: &Path) -> Result<BundledData, AppError> {
    let curated = load_program_definitions(&seed_dir.join("programs.json"))?;
    let catalog = load_catalog(catalog_file)?;
    Ok(BundledData {
        programs: simulation_programs(&catalog, curated)?,
        student: load_student(&seed_dir.join("student.json"))?,
        catalog,
        results: TestResults::new(),
        profiles: Vec::new(),
    })
}
