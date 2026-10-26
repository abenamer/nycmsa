//! Inbound adapter: HTTP. Axum routes with server-rendered pages (Askama)
//! and a JSON API. Handlers parse input, call a use case, and render; the
//! domain rules live elsewhere.
//!
//! Every endpoint is anonymous: the server is a local analysis tool and
//! binds to loopback by default. See `docs/api.md` for the endpoint table and
//! the limits applied here (body size, request timeout, bounded CPU work).

pub mod views;

use std::sync::Arc;
use std::time::Duration;

use askama::Template;
use axum::extract::{DefaultBodyLimit, Form, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
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
    ParametersDto, ScanResponseDto, SimulateRequestDto, StudentDto, WaitlistNumbersDto,
};
use views::{HomePage, ProgramsPage, RunPage, ScanPage};

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

/// Shared handler state. `R` and `C` are the adapter types the composition
/// root selected.
pub struct AppState<R, C> {
    pub simulate: Arc<SimulateStudent<R, C>>,
    pub runs: Arc<RunQueries<R>>,
    pub scan: Arc<ScanCatalog>,
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

async fn home<R, C>(State(state): State<AppState<R, C>>) -> Result<Html<String>, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let recent = state.runs.recent(state.recent_runs).await?;
    let template = HomePage::new(
        views::FormValues::from_student(&state.default_student),
        state.simulate.default_programs(),
        &recent,
        None,
    );
    page(&template)
}

/// Fields of the simulate form. Everything arrives as text; empty optional
/// fields mean "unknown".
#[derive(Debug, Clone, Deserialize)]
pub struct SimulateForm {
    #[serde(default)]
    pub attributes: String,
    #[serde(default)]
    pub ranking: String,
    #[serde(default)]
    pub lottery_number: String,
    /// Percent, `0` best, `100` worst.
    #[serde(default)]
    pub talent_test_percentile: String,
    /// Percent.
    #[serde(default)]
    pub talent_test_sd: String,
    #[serde(default)]
    pub trials: String,
    #[serde(default)]
    pub seed: String,
    #[serde(default)]
    pub demand_sd: String,
    #[serde(default)]
    pub attrition: String,
    #[serde(default)]
    pub waitlist_numbers: String,
}

fn field<T: std::str::FromStr>(name: &str, raw: &str) -> Result<T, HttpError> {
    raw.trim()
        .parse()
        .map_err(|_| HttpError::Form(format!("{name}: {raw:?} is not a valid number")))
}

fn optional_field<T: std::str::FromStr>(name: &str, raw: &str) -> Result<Option<T>, HttpError> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    field(name, raw).map(Some)
}

fn split_list(raw: &str) -> impl Iterator<Item = &str> {
    raw.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
}

impl SimulateForm {
    pub fn to_request(&self) -> Result<SimulationRequest, HttpError> {
        let percentile: Option<f64> =
            optional_field("talent test percentile", &self.talent_test_percentile)?;
        let sd: Option<f64> = optional_field("talent test spread", &self.talent_test_sd)?;
        let waitlist_numbers = match self.waitlist_numbers.trim() {
            "" | "redrawn" => WaitlistNumbersDto::Redrawn,
            "reused" => WaitlistNumbersDto::Reused,
            other => {
                return Err(HttpError::Form(format!(
                    "waitlist numbers: {other:?} is not \"redrawn\" or \"reused\""
                )))
            }
        };
        let student = student_from_dto(StudentDto {
            notes: vec![],
            attributes: split_list(&self.attributes).map(str::to_owned).collect(),
            ranking: split_list(&self.ranking).map(str::to_owned).collect(),
            lottery_number: Some(self.lottery_number.clone()),
            talent_test_percentile: percentile.map(|p| p / 100.0),
            talent_test_sd: sd.map_or(0.0, |s| s / 100.0),
        })?;
        let parameters = parameters_from_dto(ParametersDto {
            trials: field("trials", &self.trials)?,
            seed: field("seed", &self.seed)?,
            demand_sd: field("demand sd", &self.demand_sd)?,
            attrition: field("attrition", &self.attrition)?,
            waitlist_numbers,
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
    Form(form): Form<SimulateForm>,
) -> Result<Response, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
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
        views::FormValues::from_form(form),
        state.simulate.default_programs(),
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

/// Query string of the scan endpoints.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ScanQuery {
    #[serde(default)]
    pub attrs: Option<String>,
    #[serde(default)]
    pub frl: Option<String>,
    #[serde(default)]
    pub lottery: Option<String>,
}

impl ScanQuery {
    fn attrs(&self) -> &str {
        self.attrs.as_deref().unwrap_or("d15,d20")
    }

    fn frl(&self) -> bool {
        matches!(
            self.frl.as_deref().map(str::trim),
            Some("1" | "true" | "on" | "yes")
        )
    }

    fn lottery(&self) -> &str {
        self.lottery
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("BD")
    }

    pub fn to_request(&self) -> Result<ScanRequest, HttpError> {
        Ok(ScanRequest {
            attributes: parse_attributes(split_list(self.attrs()))?,
            frl: self.frl(),
            lottery: LotteryPrefix::parse(self.lottery()).map_err(InvalidInput::from)?,
        })
    }
}

async fn scan_page<R, C>(
    State(state): State<AppState<R, C>>,
    Query(query): Query<ScanQuery>,
) -> Result<Response, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    match query.to_request() {
        Ok(request) => {
            let rows = state.scan.execute(&request);
            let template = ScanPage::new(&request, &rows, state.scan.program_count(), None);
            Ok(page(&template)?.into_response())
        }
        Err(error @ (HttpError::Invalid(_) | HttpError::Form(_))) => {
            let template = ScanPage::invalid(&query, state.scan.program_count(), error.to_string());
            Ok((StatusCode::UNPROCESSABLE_ENTITY, page(&template)?).into_response())
        }
        Err(other) => Err(other),
    }
}

async fn scan_json<R, C>(
    State(state): State<AppState<R, C>>,
    Query(query): Query<ScanQuery>,
) -> Result<Response, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let request = query.to_request()?;
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
    Query(query): Query<ScanQuery>,
) -> Result<Response, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    let request = query.to_request()?;
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

async fn programs_page<R, C>(State(state): State<AppState<R, C>>) -> Result<Html<String>, HttpError>
where
    R: RunRepository + Send + Sync + 'static,
    C: Clock + Send + Sync + 'static,
{
    page(&ProgramsPage::new(state.simulate.default_programs()))
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
    fn form_percentages_become_fractions() {
        let form = SimulateForm {
            attributes: "d15, d20 mark_twain_test".to_owned(),
            ranking: "K187A,K239SC".to_owned(),
            lottery_number: "BD".to_owned(),
            talent_test_percentile: "10".to_owned(),
            talent_test_sd: "5".to_owned(),
            trials: "1000".to_owned(),
            seed: "3".to_owned(),
            demand_sd: "0.15".to_owned(),
            attrition: "1".to_owned(),
            waitlist_numbers: "reused".to_owned(),
        };
        let request = form.to_request().expect("valid");
        assert_eq!(request.student.attributes.len(), 3);
        assert_eq!(request.student.ranking.len(), 2);
        assert_eq!(
            request.student.talent_test_percentile.map(|p| p.get()),
            Some(0.1)
        );
        assert!((request.student.talent_test_sd.get() - 0.05).abs() < 1e-12);
        assert_eq!(request.parameters.trials.get(), 1000);
        assert_eq!(
            request.parameters.waitlist_numbers,
            crate::domain::WaitlistNumbers::Reused
        );
    }

    #[test]
    fn form_blank_optionals_mean_unknown() {
        let form = SimulateForm {
            attributes: String::new(),
            ranking: "K1".to_owned(),
            lottery_number: String::new(),
            talent_test_percentile: String::new(),
            talent_test_sd: String::new(),
            trials: "10".to_owned(),
            seed: "0".to_owned(),
            demand_sd: "0".to_owned(),
            attrition: "1".to_owned(),
            waitlist_numbers: String::new(),
        };
        let request = form.to_request().expect("valid");
        assert_eq!(request.student.lottery_number, None);
        assert_eq!(request.student.talent_test_percentile, None);
        assert_eq!(request.student.talent_test_sd.get(), 0.0);
    }

    #[test]
    fn form_reports_the_bad_field() {
        let form = SimulateForm {
            attributes: String::new(),
            ranking: "K1".to_owned(),
            lottery_number: String::new(),
            talent_test_percentile: String::new(),
            talent_test_sd: String::new(),
            trials: "lots".to_owned(),
            seed: "0".to_owned(),
            demand_sd: "0".to_owned(),
            attrition: "1".to_owned(),
            waitlist_numbers: String::new(),
        };
        let err = form.to_request().expect_err("invalid");
        assert_eq!(err.to_string(), "trials: \"lots\" is not a valid number");
        assert_eq!(err.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn scan_query_defaults_match_the_python_scanner() {
        let request = ScanQuery::default().to_request().expect("valid");
        assert_eq!(request.attributes.len(), 2);
        assert!(!request.frl);
        assert_eq!(request.lottery.as_str(), "bd");
        let frl = ScanQuery {
            frl: Some("on".to_owned()),
            ..ScanQuery::default()
        };
        assert!(frl.to_request().expect("valid").frl);
    }
}
