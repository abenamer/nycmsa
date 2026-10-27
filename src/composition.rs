//! Composition root. This is the one place that selects concrete adapters
//! and connects them to the use cases (REAS-ARCH-001). The `nycmsa` binary
//! calls one function per subcommand.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;

use crate::adapters::cli::render::{render_report, render_scan};
use crate::adapters::cli::{
    scan_request, simulation_request, BootstrapArgs, DatabaseArgs, FetchArgs, FetchDemandArgs,
    FetchProfilesArgs, FetchScoresArgs, ImportArgs, ScanArgs, ScoreSource, ServeArgs, SimArgs,
};
use crate::adapters::csv_export::scan_rows_csv;
use crate::adapters::dto::{student_from_dto, InvalidInput};
use crate::adapters::http::{router, AppState, HttpLimits, ProfileBook};
use crate::adapters::json_files::{load_program_definitions, load_student, FileLoadError};
use crate::adapters::myschools::{catalog_from_documents, load_catalog, CatalogLoadError};
use crate::adapters::myschools_api::{ClientBuildError, DEFAULT_BASE_URL};
use crate::adapters::postgres::{self, PostgresSetupError};
use crate::adapters::{infohub, infohub_demand, open_data};

use crate::adapters::{
    InfoHubClient, InfoHubDemandClient, MySchoolsApiClient, NycOpenDataClient,
    PostgresReferenceData, SystemClock,
};
use crate::application::{
    DistrictFilter, DownloadError, ImportSeeds, LoadReferenceData, ReclassifyProfiles,
    ReferenceError, RefreshCatalog, RefreshDemand, RefreshProfiles, RefreshTestResults,
    ScanCatalog, SimulateStudent,
};
use crate::domain::catalog::InvalidDistrictCode;
use crate::domain::{
    programs_from_catalog, CatalogProgram, DemandHistory, DistrictCode, ProgramDefinitions,
    ProgramProfile, SimulationError, TestResults,
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
    InfoHubDemandClient(#[from] infohub_demand::ClientBuildError),
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
/// work runs inline.
pub fn run_sim(args: &SimArgs) -> Result<String, AppError> {
    let programs = load_program_definitions(&args.programs)?;
    let student = load_student(&args.student)?;
    let request = simulation_request(args, student)?;
    let report = SimulateStudent::new(programs).execute(&request)?;
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

/// Opens the database, applies migrations, and returns the reference
/// store. The database holds reference data only; see ADR 0005.
async fn connect(database: &DatabaseArgs) -> Result<PostgresReferenceData, AppError> {
    let pool = postgres::connect(&database.database_url, 5).await?;
    postgres::migrate(&pool).await?;
    Ok(PostgresReferenceData::new(pool))
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

async fn fetch_demand(reference: PostgresReferenceData) -> Result<String, AppError> {
    let client = InfoHubDemandClient::new(infohub_demand::SOURCES.to_vec())?;
    let outcome = RefreshDemand::new(client, reference, SystemClock)
        .execute(&infohub_demand::sources_description())
        .await?;
    for (year, reason) in &outcome.failed_years {
        tracing::warn!(year, %reason, "demand report not read; that year is missing");
    }
    Ok(format!(
        "stored grade 6 demand for {} schools, {}{}\n",
        outcome.schools,
        match (outcome.years.first(), outcome.years.last()) {
            (Some(first), Some(last)) if first != last => format!("fall {first} to {last}"),
            (Some(only), _) => format!("fall {only}"),
            _ => "no years".to_owned(),
        },
        if outcome.failed_years.is_empty() {
            String::new()
        } else {
            format!(
                " ({} years failed, see the log)",
                outcome.failed_years.len()
            )
        }
    ))
}

async fn fetch_profiles(reference: PostgresReferenceData) -> Result<String, AppError> {
    let client = MySchoolsApiClient::new(DEFAULT_BASE_URL.to_owned(), Duration::from_millis(250))?;
    let outcome = RefreshProfiles::new(client, reference, SystemClock)
        .execute(
            &format!("MySchools API {DEFAULT_BASE_URL}<school id>/"),
            |done, total| {
                if done % 50 == 0 || done == total {
                    tracing::info!(done, total, "looked up schools");
                }
            },
        )
        .await?;
    for (id, reason) in &outcome.failed_schools {
        tracing::warn!(school_id = id, %reason, "school lookup failed; its programs have no profile");
    }
    Ok(format!(
        "stored profiles for {} programs{}\n",
        outcome.profiles,
        if outcome.failed_schools.is_empty() {
            String::new()
        } else {
            format!(
                " ({} school lookups failed, see the log)",
                outcome.failed_schools.len()
            )
        }
    ))
}

/// `nycmsa reclassify-profiles`: apply the current rules to the stored
/// texts.
pub async fn run_reclassify_profiles(args: &FetchProfilesArgs) -> Result<String, AppError> {
    let reference = connect(&args.database).await?;
    let count = ReclassifyProfiles::new(reference, SystemClock)
        .execute()
        .await?;
    Ok(format!("reclassified {count} program profiles\n"))
}

/// `nycmsa fetch-demand`: download the DOE's yearly demand reports.
pub async fn run_fetch_demand(args: &FetchDemandArgs) -> Result<String, AppError> {
    let reference = connect(&args.database).await?;
    fetch_demand(reference).await
}

/// `nycmsa fetch-profiles`: look up and classify every program.
pub async fn run_fetch_profiles(args: &FetchProfilesArgs) -> Result<String, AppError> {
    let reference = connect(&args.database).await?;
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
    let reference = connect(&args.database).await?;
    fetch_catalog(reference, &args.districts).await
}

/// `nycmsa fetch-scores`: download the test results into the database.
pub async fn run_fetch_scores(args: &FetchScoresArgs) -> Result<String, AppError> {
    let reference = connect(&args.database).await?;
    fetch_scores(reference, args.source).await
}

/// `nycmsa import`: store the seed files.
pub async fn run_import(args: &ImportArgs) -> Result<String, AppError> {
    let reference = connect(&args.database).await?;
    import_seeds(reference, &args.programs, &args.student).await
}

/// `nycmsa bootstrap`: import the seeds and download whatever is missing,
/// so a fresh database can serve.
pub async fn run_bootstrap(args: &BootstrapArgs) -> Result<String, AppError> {
    let reference = connect(&args.database).await?;
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
        report.push_str(&fetch_profiles(reference.clone()).await?);
    }
    if !inventory.demand {
        // Supplementary data: a failed download must not keep the site
        // from starting. The next bootstrap tries again.
        match fetch_demand(reference).await {
            Ok(text) => report.push_str(&text),
            Err(error) => {
                tracing::warn!(%error, "demand reports not downloaded; the school pages show no history until the next bootstrap");
                report.push_str("grade 6 demand history not downloaded (see the log)\n");
            }
        }
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
    let reference = connect(&args.database).await?;
    let data = LoadReferenceData::new(reference).execute().await?;
    tracing::info!(%data, "loaded reference data");
    let catalog = catalog_from_documents(&data.catalog.value.schools)?;
    let programs = simulation_programs(&catalog, data.programs.value)?;
    tracing::info!(
        programs = programs.len(),
        catalog_programs = catalog.len(),
        located = catalog.iter().filter(|p| p.location.is_some()).count(),
        "built the simulation programs"
    );
    if catalog.iter().any(|p| p.location.is_none()) {
        tracing::warn!(
            "some catalog programs have no usable coordinates; their distance column stays blank"
        );
    }
    let demand = data.demand.map(|d| d.value).unwrap_or_default();
    if demand.is_empty() {
        tracing::warn!("no demand history loaded; run `nycmsa fetch-demand` to show it");
    } else {
        tracing::info!(
            schools = demand.schools(),
            years = ?demand.years(),
            "loaded the grade 6 demand history"
        );
    }
    serve_with(
        args,
        programs,
        catalog,
        data.results.value,
        demand,
        data.profiles.value,
    )
    .await
}

async fn serve_with(
    args: &ServeArgs,
    programs: ProgramDefinitions,
    catalog: Vec<CatalogProgram>,
    results: TestResults,
    demand: DemandHistory,
    profiles: Vec<ProgramProfile>,
) -> Result<(), AppError> {
    let limits = HttpLimits::default();
    let state = build_state(programs, catalog, results, demand, profiles, &limits);
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
pub fn build_state(
    programs: ProgramDefinitions,
    catalog: Vec<CatalogProgram>,
    results: TestResults,
    demand: DemandHistory,
    profiles: Vec<ProgramProfile>,
    limits: &HttpLimits,
) -> AppState {
    AppState {
        simulate: Arc::new(SimulateStudent::new(programs)),
        profiles: Arc::new(ProfileBook::new(profiles, &catalog)),
        scan: Arc::new(ScanCatalog::new(catalog, results).with_demand(demand)),
        cpu_permits: Arc::new(Semaphore::new(limits.cpu_permits.max(1))),
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
    pub catalog: Vec<CatalogProgram>,
    pub results: TestResults,
    pub demand: DemandHistory,
    pub profiles: Vec<ProgramProfile>,
}

/// Loads the seed files and a catalog file for tests and local tools, with
/// no test results and no demand history. The programs are derived from the catalog with the seed
/// programs merged on top, as `serve` does.
pub fn load_bundled(seed_dir: &Path, catalog_file: &Path) -> Result<BundledData, AppError> {
    let curated = load_program_definitions(&seed_dir.join("programs.json"))?;
    let catalog = load_catalog(catalog_file)?;
    Ok(BundledData {
        programs: simulation_programs(&catalog, curated)?,
        catalog,
        results: TestResults::new(),
        demand: DemandHistory::new(),
        profiles: Vec::new(),
    })
}
