//! Inbound adapter: HTTP. Axum routes with server-rendered pages (Askama)
//! and a JSON API. Handlers parse input, call a use case, and render; the
//! domain rules live elsewhere.
//!
//! Every endpoint is anonymous and stateless: the server computes a result
//! and forgets the request; it sets no cookies and stores nothing a visitor
//! types (ADR 0005). See `docs/api.md` for the endpoint table and the limits
//! applied here (body size, request timeout, bounded CPU work).

pub mod choices;
pub mod views;

use std::sync::Arc;
use std::time::Duration;

use askama::Template;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, RawQuery, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::sync::Semaphore;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::application::{ScanCatalog, SimulateStudent, SimulationRequest};
use crate::domain::{LotteryPrefix, Report, ScanRequest, SimulationError};

use super::csv_export::scan_rows_csv;
use super::dto::{
    parameters_from_dto, parse_attributes, program_definitions_from_dto,
    program_definitions_to_dto, report_to_dto, scan_row_to_dto, student_from_dto, InvalidInput,
    ParametersDto, ScanResponseDto, SimulateRequestDto, StudentDto, DEFAULT_DEMAND_SD,
    DEFAULT_TRIALS,
};
use choices::{OutlookChoice, TalentTestChoice, WaitlistChoice};
use views::{
    HomePage, PickerPage, ProfilesPage, ProgramsPage, ResultPage, ScanFacts, ScanPage, SchoolPage,
};

/// Request limits (REAS-SEC-001).
#[derive(Debug, Clone, Copy)]
pub struct HttpLimits {
    pub body_limit_bytes: usize,
    pub request_timeout: Duration,
    /// Simulations that may run at the same time on the blocking pool.
    pub cpu_permits: usize,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            body_limit_bytes: 1024 * 1024,
            request_timeout: Duration::from_secs(60),
            cpu_permits: std::thread::available_parallelism().map_or(1, |n| n.get()),
        }
    }
}

/// The stored program profiles by code, with the kind tags of every
/// catalog program computed once: from the profile when there is one,
/// else from the names and method.
pub struct ProfileBook {
    by_code: std::collections::BTreeMap<String, crate::domain::ProgramProfile>,
    tags: std::collections::BTreeMap<String, Vec<crate::domain::ProgramFocus>>,
}

impl ProfileBook {
    pub fn new(
        profiles: Vec<crate::domain::ProgramProfile>,
        catalog: &[crate::domain::CatalogProgram],
    ) -> Self {
        let by_code: std::collections::BTreeMap<String, crate::domain::ProgramProfile> = profiles
            .into_iter()
            .map(|p| (p.code.as_str().to_owned(), p))
            .collect();
        let tags = catalog
            .iter()
            .map(|p| {
                let code = p.code.as_str().to_owned();
                let tags = match by_code.get(&code) {
                    Some(profile) => profile.tags(),
                    None => {
                        crate::domain::classify_focus(&p.program_name, &p.school_name, &p.method)
                    }
                };
                (code, tags)
            })
            .collect();
        Self { by_code, tags }
    }

    pub fn get(&self, code: &str) -> Option<&crate::domain::ProgramProfile> {
        self.by_code.get(code)
    }

    /// The kind tags of a catalog program.
    pub fn tags(&self, code: &str) -> &[crate::domain::ProgramFocus] {
        self.tags.get(code).map_or(&[], Vec::as_slice)
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

/// Shared handler state: reference data loaded at startup, read-only
/// while the server runs. There is no session and no per-visitor state.
#[derive(Clone)]
pub struct AppState {
    pub simulate: Arc<SimulateStudent>,
    pub scan: Arc<ScanCatalog>,
    pub profiles: Arc<ProfileBook>,
    pub cpu_permits: Arc<Semaphore>,
}

/// Builds the router.
pub fn router(state: AppState, limits: &HttpLimits) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/simulate", post(simulate_form))
        .route("/pick", get(pick_page))
        .route("/profiles", get(profiles_page))
        .route("/scan", get(scan_page))
        .route("/programs", get(programs_page))
        .route("/schools/{dbn}", get(school_page))
        .route("/api/simulate", post(simulate_json))
        .route("/api/scan", get(scan_json))
        .route("/api/scan.csv", get(scan_csv))
        .route("/api/programs", get(programs_json))
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
    #[error("cannot render page: {0}")]
    Render(#[from] askama::Error),
    #[error("cannot render CSV: {0}")]
    Csv(#[from] csv::Error),
    #[error("{0}")]
    Internal(String),
    #[error("{0}")]
    NotFound(String),
}

impl HttpError {
    fn status(&self) -> StatusCode {
        match self {
            HttpError::Invalid(_) | HttpError::Simulation(_) | HttpError::Form(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            HttpError::NotFound(_) => StatusCode::NOT_FOUND,
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

/// Runs the simulation on the blocking pool under a concurrency permit
/// (REAS-RUST-007). A started simulation cannot be cancelled; the trial
/// cap bounds its duration. Nothing is stored.
async fn run_simulation(state: &AppState, request: SimulationRequest) -> Result<Report, HttpError> {
    let _permit = state
        .cpu_permits
        .acquire()
        .await
        .map_err(|_| HttpError::Internal("simulation pool is closed".to_owned()))?;
    let simulate = Arc::clone(&state.simulate);
    let report = tokio::task::spawn_blocking(move || simulate.execute(&request))
        .await
        .map_err(|e| HttpError::Internal(format!("simulation task failed: {e}")))??;
    Ok(report)
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

/// The questions page opens blank: no answer is prefilled by the server.
/// The browser may refill it from its own storage.
async fn home(
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
) -> Result<Html<String>, HttpError> {
    let mut form = views::FormValues::blank(&state.scan.districts());
    if let Some(ranking) = ranking_param(raw.as_deref()) {
        form.ranking = split_list(&ranking).collect::<Vec<_>>().join(", ");
    }
    page(&HomePage::new(form, None))
}

async fn pick_page(State(state): State<AppState>) -> Result<Html<String>, HttpError> {
    page(&PickerPage::new(&state))
}

async fn profiles_page(
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
) -> Result<Html<String>, HttpError> {
    page(&ProfilesPage::new(&state, &district_filter(raw.as_deref())))
}

/// The simulate form. Chips arrive as repeated `attribute` keys and one
/// `talent_test`, `waitlist`, and `outlook` key each. Trials, seed, and
/// demand noise are fixed; see `docs/api.md`. The result page hands the
/// same fields back to the browser, serialized, so it can remember them.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
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

/// Computes and renders the result in the same response. There is no
/// result URL: the page carries the answers so the browser can keep them.
async fn simulate_form(State(state): State<AppState>, body: Bytes) -> Result<Response, HttpError> {
    let form = SimulateForm::from_body(&body);
    let request = match form.to_request() {
        Ok(request) => request,
        Err(error @ (HttpError::Invalid(_) | HttpError::Form(_))) => {
            return invalid_form(&state, &form, error.to_string());
        }
        Err(other) => return Err(other),
    };
    let student = request.student.clone();
    match run_simulation(&state, request).await {
        Ok(report) => Ok(page(&ResultPage::new(&form, &student, &report))?.into_response()),
        Err(HttpError::Simulation(error)) => invalid_form(&state, &form, error.to_string()),
        Err(other) => Err(other),
    }
}

/// Re-renders the form with the submitted values and the error, status 422.
fn invalid_form(
    state: &AppState,
    form: &SimulateForm,
    message: String,
) -> Result<Response, HttpError> {
    let template = HomePage::new(
        views::FormValues::from_form(form, &state.scan.districts()),
        Some(message),
    );
    Ok((StatusCode::UNPROCESSABLE_ENTITY, page(&template)?).into_response())
}

async fn simulate_json(
    State(state): State<AppState>,
    Json(body): Json<SimulateRequestDto>,
) -> Result<Response, HttpError> {
    let request = SimulationRequest {
        student: student_from_dto(body.student)?,
        parameters: parameters_from_dto(body.parameters)?,
        programs: body
            .programs
            .map(program_definitions_from_dto)
            .transpose()?,
    };
    let report = run_simulation(&state, request).await?;
    Ok(Json(report_to_dto(&report)).into_response())
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
    /// Nothing chosen and no lottery prefix: what the page shows first.
    pub fn blank() -> Self {
        Self {
            attributes: vec![],
            frl: false,
            lottery: String::new(),
        }
    }

    /// The JSON and CSV endpoints: the Python scanner's defaults fill in
    /// whatever the query leaves out, including the lottery prefix.
    pub fn from_raw(raw: Option<&str>) -> Self {
        match raw.filter(|r| !r.trim().is_empty()) {
            None => Self::default(),
            Some(raw) => Self::parse(raw, Some("BD")),
        }
    }

    /// The HTML page: nothing is filled in for the visitor. Without a
    /// query string the questions are blank; with one, a missing lottery
    /// prefix stays missing and the page shows no result.
    pub fn for_page(raw: Option<&str>) -> Self {
        match raw.filter(|r| !r.trim().is_empty()) {
            None => Self::blank(),
            Some(raw) => Self::parse(raw, None),
        }
    }

    fn parse(raw: &str, lottery_fallback: Option<&str>) -> Self {
        let mut query = Self::blank();
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
            if let Some(fallback) = lottery_fallback {
                query.lottery = fallback.to_owned();
            }
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

/// The server prefills no lottery prefix: without one the page shows the
/// questions and no result. A bad prefix shows the message, status 422.
async fn scan_page(
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
) -> Result<Response, HttpError> {
    let query = ScanQuery::for_page(raw.as_deref());
    let (computed, error) = if query.lottery.trim().is_empty() {
        (None, None)
    } else {
        match query.to_request() {
            Ok(request) => {
                let rows = state.scan.execute(&request);
                (Some((request, rows)), None)
            }
            Err(error @ (HttpError::Invalid(_) | HttpError::Form(_))) => {
                (None, Some(error.to_string()))
            }
            Err(other) => return Err(other),
        }
    };
    let status = if error.is_some() {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::OK
    };
    let template = ScanPage::new(
        &query,
        computed.as_ref().map(|(r, rows)| (r, rows.as_slice())),
        &ScanFacts::of(&state.scan),
        error,
    );
    Ok((status, page(&template)?).into_response())
}

async fn scan_json(
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
) -> Result<Response, HttpError> {
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

async fn scan_csv(
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
) -> Result<Response, HttpError> {
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

async fn programs_page(
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
) -> Result<Html<String>, HttpError> {
    page(&ProgramsPage::new(&state, &district_filter(raw.as_deref())))
}

/// One school: its programs and the DOE's demand history for it. The DBN
/// is matched case-insensitively against the catalog; anything else is
/// 404, so the path never reaches a query.
async fn school_page(
    State(state): State<AppState>,
    axum::extract::Path(dbn): axum::extract::Path<String>,
) -> Result<Html<String>, HttpError> {
    let template = SchoolPage::new(&state, &dbn)
        .ok_or_else(|| HttpError::NotFound(format!("no school {dbn} in the catalog")))?;
    page(&template)
}

async fn programs_json(State(state): State<AppState>) -> Result<Response, HttpError> {
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
        assert_eq!(
            no_lottery.lottery, "BD",
            "the API keeps the scanner default"
        );
        let page = ScanQuery::for_page(Some("attr=d15"));
        assert_eq!(page.lottery, "", "the page prefills nothing");
        assert_eq!(page.attributes, vec!["d15"]);
        assert_eq!(ScanQuery::for_page(None).attributes.len(), 0);
        let blank = ScanQuery::from_raw(Some("attr=d15&lottery="));
        assert!(blank.to_request().is_err());
    }
}
