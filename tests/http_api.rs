//! HTTP adapter tests through the real router with the bundled data files.
//! The server stores nothing, so every test is a single request or a
//! sequence of independent ones.

use std::path::Path;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

use nycmsa::adapters::http::{router, HttpLimits};
use nycmsa::composition::{build_state, load_bundled};

fn app() -> Router {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bundled = load_bundled(
        &root.join("data"),
        &root.join("tests/fixtures/myschools_d15_d20.json"),
    )
    .expect("bundled data loads");
    let limits = HttpLimits {
        cpu_permits: 2,
        ..HttpLimits::default()
    };
    let state = build_state(
        bundled.programs,
        bundled.catalog,
        bundled.results,
        bundled.demand,
        bundled.profiles,
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
async fn home_page_opens_blank_and_states_the_privacy_rule() {
    let response = app().oneshot(get("/")).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("<h1>NYC Middle School Admissions</h1>"));
    assert!(html.contains(
        "What are the chances of your child getting into the NYC middle school of your choice?"
    ));
    assert!(
        html.contains("name=\"lottery_number\" value=\"\""),
        "no lottery prefix prefilled"
    );
    assert!(
        html.contains("name=\"ranking\" id=\"ranking\" value=\"\""),
        "no ranking prefilled"
    );
    assert!(
        !html.contains("value=\"d15\" checked") && !html.contains("value=\"frl\" checked"),
        "no district or family chip checked"
    );
    assert!(
        html.contains("value=\"unknown\" checked"),
        "the test placement defaults to Don't know"
    );
    assert!(html.contains("District 15"), "district chip");
    assert!(html.contains("Brooklyn"), "borough shown on the chip");
    assert!(html.contains("Your privacy."), "privacy note in the footer");
    assert!(html.contains("None in this browser yet"));
}

#[tokio::test]
async fn json_simulate_returns_the_report_and_stores_nothing() {
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
    assert_eq!(response.status(), StatusCode::OK);
    let report: serde_json::Value = serde_json::from_str(&body_text(response).await).expect("json");
    assert!(
        report.get("id").is_none(),
        "nothing is stored, so nothing has an id"
    );
    assert_eq!(report["rows"].as_array().expect("rows").len(), 3);
    assert_eq!(report["parameters"]["trials"], 2000);
    assert_eq!(report["parameters"]["waitlist_numbers"], "redrawn");
    let p_any = report["summary"]["p_any_offer"].as_f64().expect("number");
    assert!((0.0..=1.0).contains(&p_any));

    for path in ["/api/runs", "/api/runs/1", "/runs/1"] {
        let response = app.clone().oneshot(get(path)).await.expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path} is gone");
    }
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
async fn form_simulate_renders_the_result_with_the_answers_for_the_browser() {
    let form = "ranking=K442S%2CK443M&attribute=d15&lottery_number=BD&talent_test=not_taken&waitlist=reused&outlook=as_published";
    let request = Request::builder()
        .method("POST")
        .uri("/simulate")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form))
        .expect("request");
    let response = app().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers().get(header::SET_COOKIE).is_none(),
        "no cookie"
    );
    let html = body_text(response).await;
    assert!(html.contains("<h1>Your chances</h1>"));
    assert!(html.contains("K443M"));
    assert!(html.contains("chance of an offer from one of these programs"));
    assert!(html.contains("3. Waitlists"));
    assert!(
        html.contains("data-answers=\"{&#34;ranking&#34;:&#34;K442S,K443M&#34;,&#34;lottery_number&#34;:&#34;BD&#34;"),
        "the answers go back to the browser, escaped: {html}"
    );
    assert!(
        html.contains("data-lottery=\"BD\"") && html.contains("data-programs=\"K442S, K443M\"")
    );
}

#[tokio::test]
async fn form_simulate_shows_validation_errors_on_the_form() {
    let form = "ranking=K442S&attribute=d15&lottery_number=zz&talent_test=unknown&waitlist=redrawn&outlook=as_published";
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
    assert!(rows[0]["latitude"].as_f64().is_some_and(|v| v > 40.0));
}

#[tokio::test]
async fn scan_page_opens_blank_without_a_query() {
    let response = app().oneshot(get("/scan")).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(
        html.contains("name=\"lottery\" value=\"\""),
        "no lottery prefix prefilled: {html}"
    );
    assert!(!html.contains("K448S"), "no rows without a query");
    let response = app()
        .oneshot(get("/scan?attr=d15"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(
        html.contains("value=\"d15\" checked"),
        "the chip stays chosen"
    );
    assert!(
        !html.contains("K448S") && !html.contains("Lottery number starting"),
        "no prefix is substituted: {html}"
    );
}

#[tokio::test]
async fn scan_page_accepts_chip_parameters() {
    let response = app()
        .oneshot(get("/scan?attr=d15&attr=d20&frl=1&lottery=BD"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("value=\"d15\" checked"));
    assert!(html.contains("Everyone in your group got in"));
    assert!(html.contains("K448S"));
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
    assert!(csv.starts_with("district,school,code,program,method,seats,applicants,filled,group,group_order,n_groups,your_group_applied,your_group_offered,your_group_share,reach,dbn,scores_year,ela_proficient_pct,math_proficient_pct,latitude,longitude,address,school_demand_year,school_listed,school_considered,school_offers\n"));
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
async fn picker_and_profiles_pages_render() {
    let response = app().oneshot(get("/pick")).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("data-code=\"K442S\""));
    assert!(
        html.contains("data-lat=\"40.") && html.contains("data-lon=\"-73."),
        "school coordinates reach the picker rows"
    );
    assert!(html.contains("id=\"home-address\""), "the address box");
    assert!(
        html.contains("https://www.google.com/maps/search/?api=1&#38;query=40.")
            && html.contains("travelmode=transit")
            && !html.contains("origin="),
        "map and directions links carry the school only: {html}"
    );
    assert!(html.contains("Brooklyn, NY"), "the school address is shown");
    assert!(html.contains("Dual language"), "focus chips");
    assert!(
        html.contains("guessed from the name"),
        "no profiles loaded in tests"
    );
    let response = app().oneshot(get("/profiles")).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_text(response)
        .await
        .contains("Pick a district to see its programs"));
    let response = app()
        .oneshot(get("/profiles?district=15"))
        .await
        .expect("response");
    assert!(body_text(response)
        .await
        .contains("No profiles for these districts"));
    let response = app()
        .oneshot(get("/?ranking=K443M%2CK442S"))
        .await
        .expect("response");
    let html = body_text(response).await;
    assert!(
        html.contains("value=\"K443M, K442S\""),
        "ranking prefilled from the picker"
    );
}

#[tokio::test]
async fn programs_pages_list_the_definitions() {
    let response = app().oneshot(get("/programs")).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(
        html.contains("K442S") && html.contains("K062Z"),
        "all districts by default"
    );
    assert!(html.contains("Showing districts all districts"));
    let response = app()
        .oneshot(get("/programs?district=20"))
        .await
        .expect("response");
    let html = body_text(response).await;
    assert!(
        html.contains("K062Z") && !html.contains("K442S"),
        "district filter"
    );
    let response = app().oneshot(get("/api/programs")).await.expect("response");
    let json: serde_json::Value = serde_json::from_str(&body_text(response).await).expect("json");
    let programs = json["programs"].as_object().expect("map");
    assert!(
        programs.len() > 50,
        "derived from the catalog: {}",
        programs.len()
    );
    assert_eq!(
        programs["K442S"]["name"], "M.S. 442 School for Innovation",
        "curated programs.json wins over the derived entry"
    );
    assert_eq!(
        programs["K062Z"]["type"], "lottery",
        "derived zoned program"
    );
}

/// The router with a small demand history attached, as `serve` builds it
/// after `fetch-demand`.
fn app_with_demand() -> Router {
    use std::collections::BTreeMap;

    use nycmsa::domain::{DemandFigures, DemandHistory, SchoolDemandYear};

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bundled = load_bundled(
        &root.join("data"),
        &root.join("tests/fixtures/myschools_d15_d20.json"),
    )
    .expect("bundled data loads");
    let mut demand = DemandHistory::new();
    demand.insert(
        "15K442",
        SchoolDemandYear {
            year: 2024,
            all: DemandFigures {
                seats: Some(110),
                listed: Some(1500),
                considered: Some(550),
                offers: Some(130),
            },
            by_residential_district: BTreeMap::new(),
        },
    );
    demand.insert(
        "15K442",
        SchoolDemandYear {
            year: 2025,
            all: DemandFigures {
                seats: Some(115),
                listed: Some(1585),
                considered: Some(564),
                offers: Some(138),
            },
            by_residential_district: [
                (
                    "15".to_owned(),
                    DemandFigures {
                        seats: None,
                        listed: Some(900),
                        considered: Some(400),
                        offers: Some(100),
                    },
                ),
                (
                    "13".to_owned(),
                    DemandFigures {
                        seats: None,
                        listed: Some(60),
                        considered: None,
                        offers: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
    );
    let limits = HttpLimits {
        cpu_permits: 2,
        ..HttpLimits::default()
    };
    let state = build_state(
        bundled.programs,
        bundled.catalog,
        bundled.results,
        demand,
        bundled.profiles,
        &limits,
    );
    router(state, &limits)
}

#[tokio::test]
async fn school_page_shows_the_demand_history_newest_first() {
    let response = app_with_demand()
        .oneshot(get("/schools/15k442"))
        .await
        .expect("response");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the DBN is matched case-insensitively"
    );
    let html = body_text(response).await;
    assert!(html.contains("<h1>M.S. 442"), "{html}");
    let first = html.find("data-v=\"2025\"").expect("2025 row");
    let second = html.find("data-v=\"2024\"").expect("2024 row");
    assert!(first < second, "newest year first");
    assert!(html.contains(">1585<"), "families who listed it");
    assert!(html.contains(">564<"), "families who competed");
    assert!(
        html.contains(">24%<"),
        "138 offers for 564 competing families"
    );
    assert!(html.contains(">13.8<"), "listed per seat");
    assert!(html.contains("Fall 2025, by where the applicants live"));
    let d15 = html.find("District 15").expect("district 15 row");
    let d13 = html.find("District 13").expect("district 13 row");
    assert!(d15 < d13, "most competing families first");
}

#[tokio::test]
async fn school_page_without_history_and_unknown_school() {
    let response = app()
        .oneshot(get("/schools/15K442"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("No demand history is loaded for this school"));
    assert!(html.contains("K442S"), "the catalog programs are listed");

    let response = app()
        .oneshot(get("/schools/99Z999"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = app()
        .oneshot(get("/schools/%3Cscript%3E"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn scan_and_picker_carry_the_school_demand_columns() {
    let response = app_with_demand()
        .oneshot(get("/scan?attr=d15&lottery=b0"))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("<th class=\"num\">Listed the school</th>"));
    assert!(html.contains("data-v=\"1585\">1585<"), "{html}");
    assert!(html.contains("data-v=\"564\">564<"));
    assert!(
        html.contains("href=\"/schools/15K442\""),
        "the school name links to its page"
    );
    assert!(html.contains("fall 2025 figures"));

    let response = app_with_demand()
        .oneshot(get("/pick"))
        .await
        .expect("response");
    let html = body_text(response).await;
    assert!(html.contains("data-v=\"1585\">1585<"));
    assert!(html.contains("href=\"/schools/15K442\""));

    let response = app_with_demand()
        .oneshot(get("/api/scan?attrs=d15&lottery=b0"))
        .await
        .expect("response");
    let scan: serde_json::Value = serde_json::from_str(&body_text(response).await).expect("json");
    let row = scan["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|r| r["dbn"] == "15K442")
        .expect("an M.S. 442 row");
    assert_eq!(row["school_demand_year"], 2025);
    assert_eq!(row["school_listed"], 1585);
    assert_eq!(row["school_considered"], 564);
    assert_eq!(row["school_offers"], 138);
}
