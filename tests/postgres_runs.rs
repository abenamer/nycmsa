//! Integration test for the PostgreSQL adapter against a real server
//! (REAS-TEST-001). It is ignored by default; run it with
//!
//! ```bash
//! NYCMSA_TEST_DATABASE_URL=postgres://user:pass@127.0.0.1/db cargo test --test postgres_runs -- --ignored
//! ```

use std::collections::BTreeSet;
use std::time::{Duration, UNIX_EPOCH};

use nycmsa::adapters::PostgresRunRepository;
use nycmsa::application::{NewRun, RunRepository};
use nycmsa::domain::{
    Attrition, LotteryPrefix, Parameters, ProgramCode, ProgramRow, Ranking, Report, Seed,
    SelectionKind, StandardDeviation, Student, Summary, Trials, WaitlistNumbers,
};

fn new_run(seed: u64) -> NewRun {
    NewRun {
        created_at: UNIX_EPOCH + Duration::from_secs(1_700_000_000 + seed),
        student: Student {
            attributes: ["d15", "d20"]
                .iter()
                .map(|a| nycmsa::domain::Attribute::parse(a).expect("valid"))
                .collect::<BTreeSet<_>>(),
            ranking: Ranking::new(vec![ProgramCode::parse("K442S").expect("valid")])
                .expect("valid"),
            lottery_number: Some(LotteryPrefix::parse("BD").expect("valid")),
            talent_test_percentile: None,
            talent_test_sd: StandardDeviation::ZERO,
        },
        report: Report {
            parameters: Parameters {
                trials: Trials::new(10).expect("valid"),
                seed: Seed::new(seed),
                demand_sd: StandardDeviation::new(0.15).expect("valid"),
                attrition: Attrition::ONE,
                waitlist_numbers: WaitlistNumbers::Reused,
            },
            lottery_fraction: Some(0.7382),
            rows: vec![ProgramRow {
                rank: 1,
                code: ProgramCode::parse("K442S").expect("valid"),
                name: "M.S. 442".to_owned(),
                selection: SelectionKind::Lottery,
                group_name: Some("District 15 residents, non-FRL".to_owned()),
                initial_rate: 0.09,
                waitlist_rate: 0.02,
                p_initial: 0.1,
                p_waitlist: 0.0,
                p_any: 0.1,
            }],
            summary: Summary {
                p_initial_offer: 0.1,
                p_any_offer: 0.1,
                p_no_offer: 0.9,
                p_lottery_offer: Some(0.1),
                p_screened_offer: None,
            },
        },
    }
}

#[tokio::test]
#[ignore = "needs NYCMSA_TEST_DATABASE_URL and a running PostgreSQL server"]
async fn runs_round_trip_through_postgres() {
    let url = std::env::var("NYCMSA_TEST_DATABASE_URL")
        .expect("NYCMSA_TEST_DATABASE_URL must point at a PostgreSQL database");
    let repo = PostgresRunRepository::connect(&url, 2)
        .await
        .expect("connects");
    repo.migrate().await.expect("migrates");
    repo.migrate().await.expect("a second migrate is a no-op");

    let seed = u64::from(std::process::id()) * 1000;
    let first = repo.insert(new_run(seed)).await.expect("stores");
    let second = repo.insert(new_run(seed + 1)).await.expect("stores");
    assert!(second > first);

    let found = repo.find(first).await.expect("reads").expect("present");
    let expected = new_run(seed);
    assert_eq!(found.id, first);
    assert_eq!(found.created_at, expected.created_at);
    assert_eq!(found.student, expected.student);
    assert_eq!(found.report, expected.report);

    let recent = repo.recent(2).await.expect("reads");
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].id, second);
    assert_eq!(recent[1].id, first);

    let missing =
        nycmsa::application::RunId::new(std::num::NonZeroU64::new(u64::MAX).expect("non-zero"));
    assert_eq!(repo.find(missing).await.expect("reads"), None);

    sqlx::query("DELETE FROM runs WHERE id = ANY($1)")
        .bind(vec![first.get() as i64, second.get() as i64])
        .execute(repo.pool())
        .await
        .expect("cleanup");
}
