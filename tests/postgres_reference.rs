//! Integration test for the PostgreSQL reference store against a real
//! server (REAS-TEST-001). Ignored by default; run it with
//!
//! ```bash
//! NYCMSA_TEST_DATABASE_URL=postgres://user:pass@127.0.0.1/db cargo test --test postgres_reference -- --ignored
//! ```

use std::collections::BTreeSet;
use std::time::{Duration, UNIX_EPOCH};

use nycmsa::adapters::{PostgresReferenceData, PostgresRunRepository};
use nycmsa::application::{
    CatalogSnapshot, CatalogStore, ProfileStore, ProgramStore, StudentDefaultsStore,
    TestResultsStore,
};
use nycmsa::domain::{
    Attribute, Evidence, GroupDemand, PriorityGroup, Program, ProgramCode, ProgramDefinitions,
    ProgramFocus, ProgramProfile, Ranking, SchoolScores, SelectionKind, StandardDeviation, Student,
    TestResults,
};

#[tokio::test]
#[ignore = "needs NYCMSA_TEST_DATABASE_URL and a running PostgreSQL server"]
async fn reference_snapshots_round_trip_and_the_latest_wins() {
    let url = std::env::var("NYCMSA_TEST_DATABASE_URL")
        .expect("NYCMSA_TEST_DATABASE_URL must point at a PostgreSQL database");
    let runs = PostgresRunRepository::connect(&url, 2)
        .await
        .expect("connects");
    runs.migrate().await.expect("migrates");
    let store = PostgresReferenceData::new(runs.pool().clone());
    let at = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let tag = format!("test-{}", std::process::id());

    // Catalog: two snapshots, the second is the latest.
    let first = CatalogSnapshot {
        schools: vec![serde_json::json!({"school": {"dbn": "15K442"}, "programs": []})],
    };
    let second = CatalogSnapshot {
        schools: vec![
            serde_json::json!({"school": {"dbn": "15K442"}, "programs": []}),
            serde_json::json!({"school": {"dbn": "20K062"}, "programs": [{"program": {"code": "K062Z"}}]}),
        ],
    };
    store.save_catalog(&first, &tag, at).await.expect("saves");
    store
        .save_catalog(&second, &format!("{tag}-2"), at)
        .await
        .expect("saves");
    let latest = store
        .latest_catalog()
        .await
        .expect("reads")
        .expect("present");
    assert_eq!(latest.value, second);
    assert_eq!(latest.source, format!("{tag}-2"));
    assert_eq!(latest.recorded_at, at);

    // Test results.
    let mut results = TestResults::new();
    results.insert(
        "15K442".to_owned(),
        SchoolScores {
            year: 2023,
            ela_proficient_pct: Some(78.9),
            math_proficient_pct: None,
        },
    );
    store
        .save_test_results(&results, &tag, at)
        .await
        .expect("saves");
    let latest = store
        .latest_test_results()
        .await
        .expect("reads")
        .expect("present");
    assert_eq!(latest.value, results);

    // Programs.
    let programs = ProgramDefinitions::new(vec![Program::new(
        ProgramCode::parse("K442S").expect("valid"),
        "M.S. 442".to_owned(),
        81,
        494,
        SelectionKind::Lottery,
        vec![PriorityGroup {
            name: "District 15".to_owned(),
            requires: vec![Attribute::parse("d15").expect("valid")],
            demand: GroupDemand::new(370, 40, Some(34)).expect("valid"),
        }],
        Some("Open".to_owned()),
        None,
    )
    .expect("valid")])
    .expect("valid");
    store
        .save_programs(&programs, &tag, at)
        .await
        .expect("saves");
    let latest = store
        .latest_programs()
        .await
        .expect("reads")
        .expect("present");
    assert_eq!(latest.value, programs);

    // Student defaults.
    let student = Student {
        attributes: ["d15"]
            .iter()
            .map(|a| Attribute::parse(a).expect("valid"))
            .collect::<BTreeSet<_>>(),
        ranking: Ranking::new(vec![ProgramCode::parse("K442S").expect("valid")]).expect("valid"),
        lottery_number: None,
        talent_test_percentile: None,
        talent_test_sd: StandardDeviation::ZERO,
    };
    store
        .save_student_defaults(&student, &tag, at)
        .await
        .expect("saves");
    let latest = store
        .latest_student_defaults()
        .await
        .expect("reads")
        .expect("present");
    // Reading adds the derived borough attribute.
    assert_eq!(latest.value.ranking, student.ranking);
    assert!(latest
        .value
        .attributes
        .contains(&Attribute::parse("borough_brooklyn").expect("valid")));

    // Program profiles.
    let profiles = vec![ProgramProfile {
        code: ProgramCode::parse("K187A").expect("valid"),
        dbn: "20K187".to_owned(),
        program_name: "Academy for Scientific Research".to_owned(),
        school_name: "The Christa Mcauliffe School".to_owned(),
        method: "Screened".to_owned(),
        description: String::new(),
        overview: "A place of wonder.".to_owned(),
        academic_opportunities: String::new(),
        art_courses: "Chorus/Singing".to_owned(),
        language_classes: String::new(),
        scores: vec![nycmsa::domain::FocusScore {
            focus: ProgramFocus::Stem,
            confidence: 0.6,
        }],
        evidence: vec![Evidence {
            focus: ProgramFocus::Stem,
            keyword: "scien".to_owned(),
            label: "program name".to_owned(),
            weight: 0.6,
        }],
    }];
    store
        .save_profiles(&profiles, &tag, at)
        .await
        .expect("saves");
    let latest = store
        .latest_profiles()
        .await
        .expect("reads")
        .expect("present");
    assert_eq!(latest.value, profiles);

    for table in [
        "catalog_snapshots",
        "score_snapshots",
        "program_sets",
        "student_defaults",
        "profile_snapshots",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE source LIKE $1"))
            .bind(format!("{tag}%"))
            .execute(runs.pool())
            .await
            .expect("cleanup");
    }
}
