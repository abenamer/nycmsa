//! HTTP adapter tests through the real router with the bundled data files
//! and the in-memory run store.

use std::num::NonZeroUsize;
use std::path::Path;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

use nycmsa::adapters::http::{router, HttpLimits};
use nycmsa::adapters::{InMemoryRunRepository, SystemClock};
use nycmsa::composition::{build_state, load_bundled};

fn app() -> Router {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    let (programs, student, catalog) = load_bundled(&data).expect("bundled data loads");
    let limits = HttpLimits {
        cpu_permits: 2,
        ..HttpLimits::default()
    };
    let state = build_state(
        InMemoryRunRepository::new(NonZeroUsize::new(10).expect("non-zero")),
        SystemClock,
        programs,
        student,
        catalog,
        &limits,
    );
    router(state, &limits)
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    String::from_utf8(bytes.to_vec()).expect("utf-8")
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .body(Body::empty())
        .expect("request")
}

#[tokio::test]
async fn healthz_is_ok() {
    let response = app().oneshot(get("/healthz")).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_text(response).await, "ok\n");
}

#[tokio::test]
async fn home_page_prefills_the_student_file() {
    let response = app().oneshot(get("/")).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("value=\"BD\""), "lottery prefix prefilled");
    assert!(html.contains("K187A, K239SC"), "ranking prefilled");
    assert!(html.contains("No runs yet"));
}

#[tokio::test]
async fn json_simulate_then_fetch_the_run() {
    let app = app();
    let body = serde_json::json!({
        "student": {
            "attributes": ["d15", "d20", "mark_twain_test"],
            "ranking": ["K187A", "K239SC", "K442S"],
            "lottery_number": "BD",
            "talent_test_percentile": 0.10,
            "talent_test_sd": 0.10
        },
        "parameters": {"trials": 2000, "seed": 1}
    });
    let request = Request::builder()
        .method("POST")
        .uri("/api/simulate")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    assert_eq!(created["id"], 1);
    assert_eq!(created["report"]["rows"].as_array().expect("rows").len(), 3);
    assert_eq!(created["report"]["parameters"]["trials"], 2000);
    assert_eq!(
        created["report"]["parameters"]["waitlist_numbers"],
        "redrawn"
    );
    let p_any = created["report"]["summary"]["p_any_offer"]
        .as_f64()
        .expect("number");
    assert!((0.0..=1.0).contains(&p_any));

    let response = app
        .clone()
        .oneshot(get("/api/runs/1"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let fetched: serde_json::Value =
        serde_json::from_str(&body_text(response).await).expect("json");
    assert_eq!(fetched["report"], created["report"]);

    let response = app.clone().oneshot(get("/runs/1")).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("Run #1"));
    assert!(html.contains("K239SC"));

    let response = app.oneshot(get("/api/runs")).await.expect("response");
    let list: serde_json::Value = serde_json::from_str(&body_text(response).await).expect("json");
    assert_eq!(list.as_array().expect("array").len(), 1);
}

#[tokio::test]
async fn json_simulate_rejects_an_unknown_program() {
    let body = serde_json::json!({
        "student": {"attributes": [], "ranking": ["ZZZ"]},
        "parameters": {"trials": 10}
    });
    let request = Request::builder()
        .method("POST")
        .uri("/api/simulate")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request");
    let response = app().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let error: serde_json::Value = serde_json::from_str(&body_text(response).await).expect("json");
    assert!(error["error"]
        .as_str()
        .expect("message")
        .contains("unknown program code ZZZ"));
}

#[tokio::test]
async fn json_simulate_rejects_too_many_trials() {
    let body = serde_json::json!({
        "student": {"attributes": [], "ranking": ["K442S"]},
        "parameters": {"trials": 6_000_000}
    });
    let request = Request::builder()
        .method("POST")
        .uri("/api/simulate")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request");
    let response = app().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn form_simulate_redirects_to_the_run_page() {
    let form = "ranking=K442S%2CK443M&attributes=d15&lottery_number=BD&talent_test_percentile=&talent_test_sd=&trials=1000&seed=2&demand_sd=0.15&attrition=1&waitlist_numbers=reused";
    let request = Request::builder()
        .method("POST")
        .uri("/simulate")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form))
        .expect("request");
    let response = app().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response
            .headers()
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some("/runs/1")
    );
}

#[tokio::test]
async fn form_simulate_shows_validation_errors_on_the_form() {
    let form = "ranking=K442S&attributes=d15&lottery_number=zz&trials=1000&seed=2&demand_sd=0.15&attrition=1";
    let request = Request::builder()
        .method("POST")
        .uri("/simulate")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form))
        .expect("request");
    let response = app().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = body_text(response).await;
    assert!(html.contains("lottery number must be hexadecimal"));
    assert!(html.contains("value=\"zz\""), "submitted values are kept");
}

#[tokio::test]
async fn unknown_run_is_not_found() {
    let response = app().oneshot(get("/api/runs/999")).await.expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = app().oneshot(get("/runs/abc")).await.expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn scan_json_matches_the_python_defaults() {
    let response = app().oneshot(get("/api/scan")).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let scan: serde_json::Value = serde_json::from_str(&body_text(response).await).expect("json");
    assert_eq!(scan["lottery"], "BD");
    assert_eq!(scan["frl"], false);
    let rows = scan["rows"].as_array().expect("rows");
    assert!(rows.len() > 40, "{}", rows.len());
    assert_eq!(rows[0]["code"], "K448S");
    assert_eq!(rows[0]["reach"], "YES all offered");
}

#[tokio::test]
async fn scan_csv_has_the_python_columns() {
    let response = app()
        .oneshot(get("/api/scan.csv?attrs=d15&lottery=b0&frl=1"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/csv; charset=utf-8")
    );
    let csv = body_text(response).await;
    assert!(csv.starts_with("district,school,code,program,method,seats,applicants,filled,group,group_order,n_groups,your_group_applied,your_group_offered,your_group_share,reach\n"));
}

#[tokio::test]
async fn scan_page_reports_a_bad_lottery_prefix() {
    let response = app()
        .oneshot(get("/scan?attrs=d15&lottery=xyz"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body_text(response).await.contains("must be hexadecimal"));
}

#[tokio::test]
async fn programs_pages_list_the_definitions() {
    let response = app().oneshot(get("/programs")).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_text(response).await.contains("K239SC"));
    let response = app().oneshot(get("/api/programs")).await.expect("response");
    let json: serde_json::Value = serde_json::from_str(&body_text(response).await).expect("json");
    assert_eq!(json["programs"].as_object().expect("map").len(), 9);
}
