//! Outbound adapter: NYC Open Data (Socrata) state test results,
//! implementing [`TestResultsDownloader`].
//!
//! Datasets: "English Language Arts (ELA) Test Results 2013-2023"
//! (`iebs-5yhr`) and "Math Test Results 2013-2023" (`74kb-55u9`). The
//! two have different column names, recorded in [`Dataset`]. Rows are
//! school-level, all students, all grades, for the latest year each dataset
//! holds. A suppressed cell (`s`) becomes `None`.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::Value;

use crate::application::{DownloadError, TestResultsDownloader, TestResultsSnapshot};
use crate::domain::{SchoolScores, TestResults};

pub const DEFAULT_BASE_URL: &str = "https://data.cityofnewyork.us/resource/";

/// The column layout of one results dataset.
#[derive(Debug, Clone, Copy)]
pub struct Dataset {
    pub id: &'static str,
    pub dbn_column: &'static str,
    pub category_column: &'static str,
    pub proficient_column: &'static str,
}

pub const ELA: Dataset = Dataset {
    id: "iebs-5yhr",
    dbn_column: "geographic_subdivision",
    category_column: "category",
    proficient_column: "level_3_4_1",
};

pub const MATH: Dataset = Dataset {
    id: "74kb-55u9",
    dbn_column: "geographic_division",
    category_column: "student_category",
    proficient_column: "pct_level_3_and_4",
};

pub struct NycOpenDataClient {
    client: reqwest::Client,
    base_url: String,
}

#[derive(Debug, thiserror::Error)]
#[error("cannot build the HTTP client: {0}")]
pub struct ClientBuildError(#[from] reqwest::Error);

/// Percent of students at levels 3 and 4 per DBN for one subject and year.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SubjectResults {
    pub year: u32,
    pub by_dbn: BTreeMap<String, Option<f64>>,
}

/// Reads the school rows of one dataset for one year.
pub fn parse_rows(dataset: Dataset, year: u32, rows: &[Value]) -> SubjectResults {
    let mut by_dbn = BTreeMap::new();
    for row in rows {
        let Some(dbn) = row.get(dataset.dbn_column).and_then(Value::as_str) else {
            continue;
        };
        let dbn = dbn.trim().to_uppercase();
        if dbn.is_empty() {
            continue;
        }
        let proficient = row
            .get(dataset.proficient_column)
            .and_then(Value::as_str)
            .and_then(|v| v.trim().parse::<f64>().ok());
        by_dbn.insert(dbn, proficient);
    }
    SubjectResults { year, by_dbn }
}

/// Combines the two subjects. Each school takes the later of the two years;
/// a subject from an earlier year reads as unknown.
pub fn combine(ela: &SubjectResults, math: &SubjectResults) -> TestResults {
    let year = ela.year.max(math.year);
    let dbns: std::collections::BTreeSet<&String> =
        ela.by_dbn.keys().chain(math.by_dbn.keys()).collect();
    dbns.into_iter()
        .map(|dbn| {
            let pick = |subject: &SubjectResults| {
                if subject.year == year {
                    subject.by_dbn.get(dbn).copied().flatten()
                } else {
                    None
                }
            };
            (
                dbn.clone(),
                SchoolScores {
                    year,
                    ela_proficient_pct: pick(ela),
                    math_proficient_pct: pick(math),
                },
            )
        })
        .collect()
}

impl NycOpenDataClient {
    pub fn new(base_url: String) -> Result<Self, ClientBuildError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .user_agent("nycmsa")
            .build()?;
        Ok(Self { client, base_url })
    }

    async fn get(&self, url: &str) -> Result<Vec<Value>, DownloadError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| DownloadError::Request {
                url: url.to_owned(),
                reason: e.to_string(),
            })?;
        response
            .json::<Vec<Value>>()
            .await
            .map_err(|e| DownloadError::Malformed {
                url: url.to_owned(),
                reason: e.to_string(),
            })
    }

    async fn latest_year(&self, dataset: Dataset) -> Result<u32, DownloadError> {
        let url = format!(
            "{}{}.json?$select=max(year) AS year&$where=report_category='School'",
            self.base_url, dataset.id
        );
        let rows = self.get(&url).await?;
        rows.first()
            .and_then(|r| r.get("year"))
            .and_then(|y| {
                y.as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| y.as_u64().map(|v| v as u32))
            })
            .ok_or_else(|| DownloadError::Malformed {
                url,
                reason: "no year in the max(year) response".to_owned(),
            })
    }

    async fn subject(&self, dataset: Dataset) -> Result<SubjectResults, DownloadError> {
        let year = self.latest_year(dataset).await?;
        let url = format!(
            "{}{}.json?$where=report_category='School' AND year={year} AND {}='All Students' AND grade='All Grades'&$limit=50000",
            self.base_url, dataset.id, dataset.category_column
        );
        let rows = self.get(&url).await?;
        Ok(parse_rows(dataset, year, &rows))
    }
}

impl TestResultsDownloader for NycOpenDataClient {
    async fn download(&self) -> Result<TestResultsSnapshot, DownloadError> {
        let ela = self.subject(ELA).await?;
        let math = self.subject(MATH).await?;
        Ok(TestResultsSnapshot {
            results: combine(&ela, &math),
            source: format!(
                "NYC Open Data {} (ELA {}) and {} (Math {})",
                ELA.id, ela.year, MATH.id, math.year
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_map_dbn_and_proficiency_with_suppression() {
        let rows = vec![
            serde_json::json!({"geographic_subdivision": "15K442", "level_3_4_1": "71.5", "grade": "All Grades"}),
            serde_json::json!({"geographic_subdivision": "01M015", "level_3_4_1": "s"}),
            serde_json::json!({"school_name": "no dbn"}),
        ];
        let parsed = parse_rows(ELA, 2023, &rows);
        assert_eq!(parsed.by_dbn.len(), 2);
        assert_eq!(parsed.by_dbn["15K442"], Some(71.5));
        assert_eq!(parsed.by_dbn["01M015"], None);
    }

    #[test]
    fn combine_uses_the_later_year_and_blanks_the_other_subject() {
        let ela = SubjectResults {
            year: 2023,
            by_dbn: [
                ("15K442".to_owned(), Some(71.5)),
                ("20K062".to_owned(), None),
            ]
            .into_iter()
            .collect(),
        };
        let math = SubjectResults {
            year: 2022,
            by_dbn: [("15K442".to_owned(), Some(60.0))].into_iter().collect(),
        };
        let results = combine(&ela, &math);
        assert_eq!(results.len(), 2);
        assert_eq!(
            results["15K442"],
            SchoolScores {
                year: 2023,
                ela_proficient_pct: Some(71.5),
                math_proficient_pct: None
            }
        );
        let same_year = combine(&ela, &SubjectResults { year: 2023, ..math });
        assert_eq!(same_year["15K442"].math_proficient_pct, Some(60.0));
    }
}
