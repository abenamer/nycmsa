//! Inbound adapter: HTTP. Axum routes with server-rendered pages (Askama)
//! and a JSON API. Handlers parse input, call a use case, and render; the
//! domain rules live elsewhere.
//!
//! Every endpoint is anonymous: the server is a local analysis tool and
//! binds to loopback by default. See `docs/api.md` for the endpoint table and
//! the limits applied here (body size, request timeout, bounded CPU work).

pub mod choices;
pub mod views;

use std::sync::Arc;
use std::time::Duration;

use askama::Template;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, RawQuery, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::sync::Semaphore;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::application::{
    Clock, RepositoryError, Run, RunId, RunQueries, RunRepository, ScanCatalog, SimulateStudent,
    SimulationRequest,
};
use crate::domain::{LotteryPrefix, ScanRequest, SimulationError};

use super::csv_export::scan_rows_csv;
use super::dto::{
    parameters_from_dto, parse_attributes, program_definitions_from_dto,
    program_definitions_to_dto, run_to_dto, scan_row_to_dto, student_from_dto, InvalidInput,
    ParametersDto, ScanResponseDto, SimulateRequestDto, StudentDto, DEFAULT_DEMAND_SD,
    DEFAULT_TRIALS,
};
use choices::{OutlookChoice, TalentTestChoice, WaitlistChoice};
use views::{HomePage, PickerPage, ProfilesPage, ProgramsPage, RunPage, ScanPage};

/// Request limits (REAS-SEC-001).
#[derive(Debug, Clone, Copy)]
pub struct HttpLimits {
    pub body_limit_bytes: usize,
    pub request_timeout: Duration,
    /// Simulations that may run at the same time on the blocking pool.
    pub cpu_permits: usize,
    /// Runs listed on the home page.
    pub recent_runs: usize,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            body_limit_bytes: 1024 * 1024,
            request_timeout: Duration::from_secs(60),
            cpu_permits: std::thread::available_parallelism().map_or(1, |n| n.get()),
            recent_runs: 20,
        }
    }
}

/// The stored program profiles, by code.
pub struct ProfileBook {
    by_code: std::collections::BTreeMap<String, crate::domain::ProgramProfile>,
}

impl ProfileBook {
    pub fn new(profiles: Vec<crate::domain::ProgramProfile>) -> Self {
        Self {
            by_code: profiles
                .into_iter()
                .map(|p| (p.code.as_str().to_owned(), p))
                .collect(),
        }
    }

    pub fn get(&self, code: &str) -> Option<&crate::domain::ProgramProfile> {
        self.by_code.get(code)
    }

    pub fn all(&self) -> impl Iterator<Item = &crate::domain::ProgramProfile> {
        self.by_code.values()
    }

    pub fn len(&self) -> usize {
        self.by_code.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_code.is_empty()
    }
}

/// Shared handler state. `R` and `C` are the adapter types the composition
/// root selected.
pub struct AppState<R, C> {
    pub simulate: Arc<SimulateStudent<R, C>>,
    pub runs: Arc<RunQueries<R>>,
    pub scan: Arc<ScanCatalog>,
    pub profiles: Arc<ProfileBook>,
    pub default_student: Arc<StudentDto>,
    pub cpu_permits: Arc<Semaphore>,
    pub recent_runs: usize,
}

impl<R, C> Clone for AppState<R, C> {
    fn clone(&self) -> Self {
        Self {
            simulate: Arc::clone(&self.simulate),
            runs: Arc::clone(&self.runs),
            scan: Arc::clone(&self.scan),
            profiles: Arc::clone(&self.profiles),
            default_student: Arc::clone(&self.default_student),
            cpu_permits: Arc::clone(&self.cpu_permits),
            recent_runs: self.recent_runs,
        }
    }
}

/// Builds the router. The type bounds are what `spawn_blocking` and the
/// multi-threaded runtime require.
pub fn router<R, C>(state: AppState<R, C>, limits: &HttpLimits) -> Router
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    Router::new()
        .route("/", get(home::<R, C>))
        .route("/simulate", post(simulate_form::<R, C>))
        .route("/pick", get(pick_page::<R, C>))
        .route("/profiles", get(profiles_page::<R, C>))
        .route("/runs/{id}", get(run_page::<R, C>))
        .route("/scan", get(scan_page::<R, C>))
        .route("/programs", get(programs_page::<R, C>))
        .route("/api/simulate", post(simulate_json::<R, C>))
        .route("/api/runs", get(runs_json::<R, C>))
        .route("/api/runs/{id}", get(run_json::<R, C>))
        .route("/api/scan", get(scan_json::<R, C>))
        .route("/api/scan.csv", get(scan_csv::<R, C>))
        .route("/api/programs", get(programs_json::<R, C>))
        .route("/healthz", get(healthz))
        .layer(DefaultBodyLimit::max(limits.body_limit_bytes))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            limits.request_timeout,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Typed handler error. The inbound adapter converts it to a response
/// (REAS-RUST-005).
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("{0}")]
    Invalid(#[from] InvalidInput),
    #[error("{0}")]
    Simulation(#[from] SimulationError),
    #[error("{0}")]
    Form(String),
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Repository(#[from] RepositoryError),
    #[error("cannot render page: {0}")]
    Render(#[from] askama::Error),
    #[error("cannot render CSV: {0}")]
    Csv(#[from] csv::Error),
    #[error("{0}")]
    Internal(String),
}

impl HttpError {
    fn status(&self) -> StatusCode {
        match self {
            HttpError::Invalid(_) | HttpError::Simulation(_) | HttpError::Form(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            HttpError::NotFound => StatusCode::NOT_FOUND,
            HttpError::Repository(_) => StatusCode::SERVICE_UNAVAILABLE,
            HttpError::Render(_) | HttpError::Csv(_) | HttpError::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status.is_server_error() {
            tracing::error!(error = %self, "request failed");
        }
        let body = serde_json::json!({ "error": self.to_string() });
        (status, Json(body)).into_response()
    }
}

fn page<T: Template>(template: &T) -> Result<Html<String>, HttpError> {
    Ok(Html(template.render()?))
}

/// Runs the CPU step on the blocking pool under a concurrency permit, then
/// the I/O step on the executor (REAS-RUST-007). A started simulation cannot
/// be cancelled; the trial cap bounds its duration.
async fn simulate_and_record<R, C>(
    state: &AppState<R, C>,
    request: SimulationRequest,
) -> Result<Run, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let _permit = state
        .cpu_permits
        .acquire()
        .await
        .map_err(|_| HttpError::Internal("simulation pool is closed".to_owned()))?;
    let simulate = Arc::clone(&state.simulate);
    let student = request.student.clone();
    let report = tokio::task::spawn_blocking(move || simulate.compute(&request))
        .await
        .map_err(|e| HttpError::Internal(format!("simulation task failed: {e}")))??;
    Ok(state.simulate.record(student, report).await?)
}

async fn healthz() -> &'static str {
    "ok\n"
}

/// The `ranking` value of a query string, if any.
fn ranking_param(raw: Option<&str>) -> Option<String> {
    raw.and_then(|r| {
        pairs(r.as_bytes())
            .into_iter()
            .find(|(k, _)| k == "ranking")
            .map(|(_, v)| v)
    })
}

/// Districts named by repeated `district` keys in a query string.
fn district_filter(raw: Option<&str>) -> Vec<String> {
    raw.map(|r| {
        pairs(r.as_bytes())
            .into_iter()
            .filter(|(k, _)| k == "district")
            .map(|(_, v)| v.trim().trim_start_matches('d').to_owned())
            .filter(|v| !v.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

async fn home<R, C>(
    State(state): State<AppState<R, C>>,
    RawQuery(raw): RawQuery,
) -> Result<Html<String>, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let recent = state.runs.recent(state.recent_runs).await?;
    let mut form = views::FormValues::from_student(&state.default_student, &state.scan.districts());
    if let Some(ranking) = ranking_param(raw.as_deref()) {
        form.ranking = split_list(&ranking).collect::<Vec<_>>().join(", ");
    }
    let template = HomePage::new(form, &recent, None);
    page(&template)
}

async fn pick_page<R, C>(State(state): State<AppState<R, C>>) -> Result<Html<String>, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    page(&PickerPage::new(&state))
}

async fn profiles_page<R, C>(
    State(state): State<AppState<R, C>>,
    RawQuery(raw): RawQuery,
) -> Result<Html<String>, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    page(&ProfilesPage::new(&state, &district_filter(raw.as_deref())))
}

/// The simulate form. Chips arrive as repeated `attribute` keys and one
/// `talent_test`, `waitlist`, and `outlook` key each. Trials, seed, and
/// demand noise are fixed; see `docs/api.md`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SimulateForm {
    pub ranking: String,
    pub lottery_number: String,
    /// District and FRL chips, plus any raw attribute.
    pub attributes: Vec<String>,
    /// Program codes where a sibling will be enrolled.
    pub sibling_codes: String,
    /// Program codes the child is zoned for.
    pub zoned_codes: String,
    /// Program codes the child already attends (continuing student).
    pub continuing_codes: String,
    pub talent_test: String,
    pub waitlist: String,
    pub outlook: String,
}

/// Collects repeated keys, which `serde_urlencoded` cannot.
fn pairs(body: &[u8]) -> Vec<(String, String)> {
    form_urlencoded::parse(body)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn split_list(raw: &str) -> impl Iterator<Item = &str> {
    raw.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
}

fn choice<T>(name: &str, raw: &str, parse: impl Fn(&str) -> Option<T>) -> Result<T, HttpError> {
    parse(raw).ok_or_else(|| HttpError::Form(format!("{name}: {raw:?} is not one of the choices")))
}

impl SimulateForm {
    pub fn from_body(body: &[u8]) -> Self {
        let mut form = Self::default();
        for (key, value) in pairs(body) {
            match key.as_str() {
                "ranking" => form.ranking = value,
                "lottery_number" => form.lottery_number = value,
                "attribute" => form.attributes.push(value),
                "sibling_codes" => form.sibling_codes = value,
                "zoned_codes" => form.zoned_codes = value,
                "continuing_codes" => form.continuing_codes = value,
                "talent_test" => form.talent_test = value,
                "waitlist" => form.waitlist = value,
                "outlook" => form.outlook = value,
                _ => {}
            }
        }
        form
    }

    pub fn talent_test(&self) -> Result<TalentTestChoice, HttpError> {
        choice("screened test", &self.talent_test, TalentTestChoice::parse)
    }

    pub fn waitlist(&self) -> Result<WaitlistChoice, HttpError> {
        choice("waitlists", &self.waitlist, WaitlistChoice::parse)
    }

    pub fn outlook(&self) -> Result<OutlookChoice, HttpError> {
        choice("outlook", &self.outlook, OutlookChoice::parse)
    }

    pub fn to_request(&self) -> Result<SimulationRequest, HttpError> {
        let talent = self.talent_test()?;
        let waitlist = self.waitlist()?;
        let outlook = self.outlook()?;
        let mut attributes: Vec<String> = self
            .attributes
            .iter()
            .flat_map(|a| split_list(a).map(str::to_owned))
            .filter(|a| a != "mark_twain_test")
            .collect();
        for (prefix, codes) in [
            ("sibling", &self.sibling_codes),
            ("zoned", &self.zoned_codes),
            ("continuing", &self.continuing_codes),
        ] {
            for code in split_list(codes) {
                attributes.push(format!("{prefix}_{}", code.trim().to_uppercase()));
            }
        }
        if talent.took_test() {
            attributes.push("mark_twain_test".to_owned());
        }
        let (percentile, sd) = match talent.percentile() {
            Some((p, sd)) => (Some(p), sd),
            None => (None, 0.0),
        };
        let student = student_from_dto(StudentDto {
            notes: vec![],
            attributes,
            ranking: split_list(&self.ranking).map(str::to_owned).collect(),
            lottery_number: Some(self.lottery_number.clone()),
            talent_test_percentile: percentile,
            talent_test_sd: sd,
        })?;
        let parameters = parameters_from_dto(ParametersDto {
            trials: DEFAULT_TRIALS,
            seed: 0,
            demand_sd: DEFAULT_DEMAND_SD,
            attrition: outlook.attrition(),
            waitlist_numbers: waitlist.dto(),
        })?;
        Ok(SimulationRequest {
            student,
            parameters,
            programs: None,
        })
    }
}

async fn simulate_form<R, C>(
    State(state): State<AppState<R, C>>,
    body: Bytes,
) -> Result<Response, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let form = SimulateForm::from_body(&body);
    let request = match form.to_request() {
        Ok(request) => request,
        Err(error @ (HttpError::Invalid(_) | HttpError::Form(_))) => {
            return invalid_form(&state, &form, error.to_string()).await;
        }
        Err(other) => return Err(other),
    };
    match simulate_and_record(&state, request).await {
        Ok(run) => Ok(Redirect::to(&format!("/runs/{}", run.id)).into_response()),
        Err(HttpError::Simulation(error)) => invalid_form(&state, &form, error.to_string()).await,
        Err(other) => Err(other),
    }
}

/// Re-renders the form with the submitted values and the error, status 422.
async fn invalid_form<R, C>(
    state: &AppState<R, C>,
    form: &SimulateForm,
    message: String,
) -> Result<Response, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let recent = state.runs.recent(state.recent_runs).await?;
    let template = HomePage::new(
        views::FormValues::from_form(form, &state.scan.districts()),
        &recent,
        Some(message),
    );
    Ok((StatusCode::UNPROCESSABLE_ENTITY, page(&template)?).into_response())
}

fn parse_run_id(raw: &str) -> Result<RunId, HttpError> {
    RunId::parse(raw).map_err(|_| HttpError::NotFound)
}

async fn find_run<R, C>(state: &AppState<R, C>, raw_id: &str) -> Result<Run, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let id = parse_run_id(raw_id)?;
    state.runs.find(id).await?.ok_or(HttpError::NotFound)
}

async fn run_page<R, C>(
    State(state): State<AppState<R, C>>,
    Path(id): Path<String>,
) -> Result<Html<String>, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let run = find_run(&state, &id).await?;
    page(&RunPage::new(&run))
}

async fn run_json<R, C>(
    State(state): State<AppState<R, C>>,
    Path(id): Path<String>,
) -> Result<Response, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let run = find_run(&state, &id).await?;
    Ok(Json(run_to_dto(&run)).into_response())
}

async fn runs_json<R, C>(State(state): State<AppState<R, C>>) -> Result<Response, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let runs = state.runs.recent(state.recent_runs).await?;
    let body: Vec<_> = runs.iter().map(run_to_dto).collect();
    Ok(Json(body).into_response())
}

async fn simulate_json<R, C>(
    State(state): State<AppState<R, C>>,
    Json(body): Json<SimulateRequestDto>,
) -> Result<Response, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let request = SimulationRequest {
        student: student_from_dto(body.student)?,
        parameters: parameters_from_dto(body.parameters)?,
        programs: body
            .programs
            .map(program_definitions_from_dto)
            .transpose()?,
    };
    let run = simulate_and_record(&state, request).await?;
    Ok((StatusCode::CREATED, Json(run_to_dto(&run))).into_response())
}

/// Query string of the scan endpoints. Districts and roles arrive as
/// repeated `attr` keys from the form, or as a comma list in `attrs` from
/// the API; both are accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanQuery {
    pub attributes: Vec<String>,
    pub frl: bool,
    pub lottery: String,
}

impl Default for ScanQuery {
    /// The Python scanner's defaults.
    fn default() -> Self {
        Self {
            attributes: vec!["d15".to_owned(), "d20".to_owned()],
            frl: false,
            lottery: "BD".to_owned(),
        }
    }
}

impl ScanQuery {
    pub fn from_raw(raw: Option<&str>) -> Self {
        let Some(raw) = raw.filter(|r| !r.trim().is_empty()) else {
            return Self::default();
        };
        let mut query = Self {
            attributes: vec![],
            frl: false,
            lottery: String::new(),
        };
        let mut lottery_given = false;
        for (key, value) in pairs(raw.as_bytes()) {
            match key.as_str() {
                "attr" => query.attributes.push(value),
                "attrs" => query
                    .attributes
                    .extend(split_list(&value).map(str::to_owned)),
                "frl" => query.frl = matches!(value.trim(), "1" | "true" | "on" | "yes"),
                "lottery" => {
                    lottery_given = true;
                    query.lottery = value;
                }
                _ => {}
            }
        }
        if !lottery_given {
            query.lottery = "BD".to_owned();
        }
        query
    }

    pub fn to_request(&self) -> Result<ScanRequest, HttpError> {
        Ok(ScanRequest {
            attributes: parse_attributes(self.attributes.iter().flat_map(|a| split_list(a)))?,
            frl: self.frl,
            lottery: LotteryPrefix::parse(&self.lottery).map_err(InvalidInput::from)?,
        })
    }
}

async fn scan_page<R, C>(
    State(state): State<AppState<R, C>>,
    RawQuery(raw): RawQuery,
) -> Result<Response, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let query = ScanQuery::from_raw(raw.as_deref());
    let districts = state.scan.districts();
    match query.to_request() {
        Ok(request) => {
            let rows = state.scan.execute(&request);
            let template = ScanPage::new(
                &query,
                Some((&request, &rows)),
                &districts,
                state.scan.program_count(),
                state.scan.schools_with_scores(),
                None,
            );
            Ok(page(&template)?.into_response())
        }
        Err(error @ (HttpError::Invalid(_) | HttpError::Form(_))) => {
            let template = ScanPage::new(
                &query,
                None,
                &districts,
                state.scan.program_count(),
                state.scan.schools_with_scores(),
                Some(error.to_string()),
            );
            Ok((StatusCode::UNPROCESSABLE_ENTITY, page(&template)?).into_response())
        }
        Err(other) => Err(other),
    }
}

async fn scan_json<R, C>(
    State(state): State<AppState<R, C>>,
    RawQuery(raw): RawQuery,
) -> Result<Response, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let request = ScanQuery::from_raw(raw.as_deref()).to_request()?;
    let rows = state.scan.execute(&request);
    let body = ScanResponseDto {
        attributes: request
            .attributes
            .iter()
            .map(|a| a.as_str().to_owned())
            .collect(),
        frl: request.frl,
        lottery: request.lottery.to_string(),
        lottery_fraction: request.lottery.fraction().get(),
        rows: rows.iter().map(scan_row_to_dto).collect(),
    };
    Ok(Json(body).into_response())
}

async fn scan_csv<R, C>(
    State(state): State<AppState<R, C>>,
    RawQuery(raw): RawQuery,
) -> Result<Response, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let request = ScanQuery::from_raw(raw.as_deref()).to_request()?;
    let rows = state.scan.execute(&request);
    let csv = scan_rows_csv(&rows)?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"scan.csv\"",
            ),
        ],
        csv,
    )
        .into_response())
}

async fn programs_page<R, C>(
    State(state): State<AppState<R, C>>,
    RawQuery(raw): RawQuery,
) -> Result<Html<String>, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    page(&ProgramsPage::new(&state, &district_filter(raw.as_deref())))
}

async fn programs_json<R, C>(State(state): State<AppState<R, C>>) -> Result<Response, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    Ok(Json(program_definitions_to_dto(
        state.simulate.default_programs(),
    ))
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_chips_become_model_inputs() {
        let body = b"ranking=K187A%2CK239SC&lottery_number=BD&attribute=d15&attribute=d20&talent_test=top_quarter&waitlist=reused&outlook=hopeful";
        let form = SimulateForm::from_body(body);
        assert_eq!(form.attributes, vec!["d15", "d20"]);
        let request = form.to_request().expect("valid");
        let attrs: Vec<&str> = request
            .student
            .attributes
            .iter()
            .map(|a| a.as_str())
            .collect();
        assert_eq!(
            attrs,
            vec!["borough_brooklyn", "d15", "d20", "mark_twain_test"]
        );
        assert_eq!(request.student.ranking.len(), 2);
        assert_eq!(
            request.student.talent_test_percentile.map(|p| p.get()),
            Some(0.125)
        );
        assert!((request.student.talent_test_sd.get() - 0.10).abs() < 1e-12);
        assert_eq!(request.parameters.trials.get(), DEFAULT_TRIALS);
        assert_eq!(request.parameters.attrition.get(), 1.3);
        assert_eq!(
            request.parameters.waitlist_numbers,
            crate::domain::WaitlistNumbers::Reused
        );
    }

    #[test]
    fn form_code_fields_become_per_program_attributes() {
        let body = b"ranking=K442S&attribute=d15&sibling_codes=k442s%2C+K443M&zoned_codes=K062Z&continuing_codes=&talent_test=not_taken&waitlist=redrawn&outlook=as_published";
        let request = SimulateForm::from_body(body).to_request().expect("valid");
        let attrs: Vec<&str> = request
            .student
            .attributes
            .iter()
            .map(|a| a.as_str())
            .collect();
        assert_eq!(
            attrs,
            vec![
                "borough_brooklyn",
                "d15",
                "sibling_K442S",
                "sibling_K443M",
                "zoned_K062Z"
            ]
        );
    }

    #[test]
    fn ranking_param_is_read_from_the_query() {
        assert_eq!(
            ranking_param(Some("ranking=K442S%2CK443M&x=1")),
            Some("K442S,K443M".to_owned())
        );
        assert_eq!(ranking_param(Some("x=1")), None);
    }

    #[test]
    fn district_filter_reads_repeated_keys() {
        assert_eq!(
            district_filter(Some("district=15&district=d20&x=1")),
            vec!["15", "20"]
        );
        assert!(district_filter(None).is_empty());
    }

    #[test]
    fn form_not_taken_drops_the_test_attribute() {
        let body = b"ranking=K1&lottery_number=&attribute=mark_twain_test&talent_test=not_taken&waitlist=redrawn&outlook=as_published";
        let request = SimulateForm::from_body(body).to_request().expect("valid");
        assert!(request.student.attributes.is_empty());
        assert_eq!(request.student.lottery_number, None);
        assert_eq!(request.student.talent_test_percentile, None);
    }

    #[test]
    fn form_reports_a_bad_choice() {
        let body = b"ranking=K1&talent_test=genius&waitlist=redrawn&outlook=as_published";
        let err = SimulateForm::from_body(body)
            .to_request()
            .expect_err("invalid");
        assert_eq!(
            err.to_string(),
            "screened test: \"genius\" is not one of the choices"
        );
        assert_eq!(err.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn scan_query_defaults_match_the_python_scanner() {
        let request = ScanQuery::from_raw(None).to_request().expect("valid");
        assert_eq!(request.attributes.len(), 2);
        assert!(!request.frl);
        assert_eq!(request.lottery.as_str(), "bd");
    }

    #[test]
    fn scan_query_accepts_chips_and_comma_lists() {
        let query = ScanQuery::from_raw(Some(
            "attr=d15&attr=sibling&attrs=d20,zoned&frl=on&lottery=b0",
        ));
        let request = query.to_request().expect("valid");
        let attrs: Vec<&str> = request.attributes.iter().map(|a| a.as_str()).collect();
        assert_eq!(attrs, vec!["d15", "d20", "sibling", "zoned"]);
        assert!(request.frl);
        assert_eq!(request.lottery.as_str(), "b0");
        let no_lottery = ScanQuery::from_raw(Some("attr=d15"));
        assert_eq!(no_lottery.lottery, "BD");
        let blank = ScanQuery::from_raw(Some("attr=d15&lottery="));
        assert!(blank.to_request().is_err());
    }
}
