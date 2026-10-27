//! Outbound adapter: the DOE InfoHub school-level test results, implementing
//! [`TestResultsDownloader`].
//!
//! InfoHub publishes one workbook per subject
//! (`school-ela-results-public.xlsx`, `school-math-results-public.xlsx`,
//! about 50 MB each) covering 2018 to the latest year. The sheet `ELA - All`
//! or `Math - All` has one row per school, grade, year, and category with
//! the columns `DBN`, `School Name`, `Grade`, `Year`, `Category`, ...,
//! `% Level 3+4`. The reader keeps the `All Grades`, `All Students` rows of
//! the latest year. A suppressed cell holds the string `s` and reads as
//! unknown. The unit tests cover this layout with small fixture workbooks;
//! a change in the DOE's layout fails loudly on a missing sheet or column.

use std::collections::BTreeMap;
use std::io::{Read, Seek};
use std::time::Duration;

use calamine::{Data, Reader, Xlsx};

use crate::application::{DownloadError, TestResultsDownloader, TestResultsSnapshot};
use crate::domain::{combine_subjects, SubjectResults};

pub const ELA_URL: &str = "https://infohub.nyced.org/docs/default-source/default-document-library/school-ela-results-public.xlsx";
pub const MATH_URL: &str = "https://infohub.nyced.org/docs/default-source/default-document-library/school-math-results-public.xlsx";
pub const ELA_SHEET: &str = "ELA - All";
pub const MATH_SHEET: &str = "Math - All";

#[derive(Debug, thiserror::Error)]
pub enum WorkbookError {
    #[error("cannot read the workbook: {0}")]
    Xlsx(#[from] calamine::XlsxError),
    #[error("the workbook has no sheet named {0:?}")]
    MissingSheet(String),
    #[error("sheet {sheet:?} has no {column:?} column")]
    MissingColumn { sheet: String, column: &'static str },
    #[error("sheet {sheet:?} is empty")]
    Empty { sheet: String },
    #[error("sheet {sheet:?}, row {row}: year {value:?} is not a number")]
    BadYear {
        sheet: String,
        row: usize,
        value: String,
    },
}

fn text(cell: &Data) -> String {
    match cell {
        Data::String(s) => s.trim().to_owned(),
        Data::Empty => String::new(),
        other => other.to_string().trim().to_owned(),
    }
}

fn number(cell: &Data) -> Option<f64> {
    match cell {
        Data::Float(f) => Some(*f),
        Data::Int(i) => Some(*i as f64),
        Data::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Reads one subject workbook: the `All Grades`, `All Students` rows of the
/// latest year on the named sheet.
pub fn parse_workbook<R: Read + Seek>(
    reader: R,
    sheet: &str,
) -> Result<SubjectResults, WorkbookError> {
    let mut workbook = Xlsx::new(reader)?;
    let range = workbook
        .worksheet_range(sheet)
        .map_err(|_| WorkbookError::MissingSheet(sheet.to_owned()))?;
    let mut rows = range.rows();
    let header: Vec<String> = rows
        .next()
        .ok_or_else(|| WorkbookError::Empty {
            sheet: sheet.to_owned(),
        })?
        .iter()
        .map(|c| text(c).to_lowercase())
        .collect();
    let column = |name: &'static str| -> Result<usize, WorkbookError> {
        header
            .iter()
            .position(|h| h == name)
            .ok_or(WorkbookError::MissingColumn {
                sheet: sheet.to_owned(),
                column: name,
            })
    };
    let dbn = column("dbn")?;
    let grade = column("grade")?;
    let year = column("year")?;
    let category = column("category")?;
    let proficient = column("% level 3+4")?;

    // (year, dbn) -> proficiency, then keep the latest year.
    let mut by_year: BTreeMap<u32, BTreeMap<String, Option<f64>>> = BTreeMap::new();
    for (index, row) in rows.enumerate() {
        let cell = |i: usize| row.get(i).unwrap_or(&Data::Empty);
        if !text(cell(grade)).eq_ignore_ascii_case("all grades")
            || !text(cell(category)).eq_ignore_ascii_case("all students")
        {
            continue;
        }
        let key = text(cell(dbn)).to_uppercase();
        if key.is_empty() {
            continue;
        }
        let Some(year_value) = number(cell(year)).map(|y| y as u32) else {
            return Err(WorkbookError::BadYear {
                sheet: sheet.to_owned(),
                row: index + 2,
                value: text(cell(year)),
            });
        };
        by_year
            .entry(year_value)
            .or_default()
            .insert(key, number(cell(proficient)));
    }
    let (year, by_dbn) = by_year.into_iter().next_back().unwrap_or_default();
    Ok(SubjectResults { year, by_dbn })
}

pub struct InfoHubClient {
    client: reqwest::Client,
    ela_url: String,
    math_url: String,
}

#[derive(Debug, thiserror::Error)]
#[error("cannot build the HTTP client: {0}")]
pub struct ClientBuildError(#[from] reqwest::Error);

impl InfoHubClient {
    pub fn new(ela_url: String, math_url: String) -> Result<Self, ClientBuildError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .user_agent("nycmsa")
            .build()?;
        Ok(Self {
            client,
            ela_url,
            math_url,
        })
    }

    async fn workbook(&self, url: &str, sheet: &str) -> Result<SubjectResults, DownloadError> {
        let bytes = self
            .client
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| DownloadError::Request {
                url: url.to_owned(),
                reason: e.to_string(),
            })?
            .bytes()
            .await
            .map_err(|e| DownloadError::Request {
                url: url.to_owned(),
                reason: e.to_string(),
            })?;
        // Parsing a 50 MB workbook is CPU work; keep it off the executor
        // (REAS-RUST-007).
        let sheet = sheet.to_owned();
        let url_owned = url.to_owned();
        tokio::task::spawn_blocking(move || {
            parse_workbook(std::io::Cursor::new(bytes), &sheet).map_err(|e| {
                DownloadError::Malformed {
                    url: url_owned,
                    reason: e.to_string(),
                }
            })
        })
        .await
        .map_err(|e| DownloadError::Malformed {
            url: url.to_owned(),
            reason: format!("parse task failed: {e}"),
        })?
    }
}

impl TestResultsDownloader for InfoHubClient {
    async fn download(&self) -> Result<TestResultsSnapshot, DownloadError> {
        let ela = self.workbook(&self.ela_url, ELA_SHEET).await?;
        let math = self.workbook(&self.math_url, MATH_SHEET).await?;
        Ok(TestResultsSnapshot {
            results: combine_subjects(&ela, &math),
            source: format!(
                "DOE InfoHub school-ela-results-public.xlsx (ELA {}) and school-math-results-public.xlsx (Math {})",
                ela.year, math.year
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn fixture(name: &str) -> std::fs::File {
        std::fs::File::open(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
        )
        .expect("fixture exists")
    }

    #[test]
    fn ela_fixture_keeps_all_grades_all_students_of_the_latest_year() {
        let ela = parse_workbook(fixture("infohub-ela-sample.xlsx"), ELA_SHEET).expect("parses");
        assert_eq!(ela.year, 2026);
        assert_eq!(ela.by_dbn.len(), 2);
        assert_eq!(
            ela.by_dbn["15K442"],
            Some(75.0),
            "2026 All Grades row, not 2025 or grade 6"
        );
        assert_eq!(
            ela.by_dbn["20K062"], None,
            "suppressed cell reads as unknown"
        );
    }

    #[test]
    fn math_fixture_and_combination() {
        let ela = parse_workbook(fixture("infohub-ela-sample.xlsx"), ELA_SHEET).expect("parses");
        let math = parse_workbook(fixture("infohub-math-sample.xlsx"), MATH_SHEET).expect("parses");
        assert_eq!(math.year, 2026);
        assert_eq!(
            math.by_dbn.len(),
            1,
            "the 2025-only school is not in the latest year"
        );
        let results = combine_subjects(&ela, &math);
        assert_eq!(results["15K442"].ela_proficient_pct, Some(75.0));
        assert_eq!(results["15K442"].math_proficient_pct, Some(80.0));
        assert_eq!(results["20K062"].math_proficient_pct, None);
        assert_eq!(results["20K062"].year, 2026);
    }

    #[test]
    fn a_missing_sheet_or_column_is_an_error() {
        let err = parse_workbook(fixture("infohub-ela-sample.xlsx"), "Math - All")
            .expect_err("no such sheet");
        assert!(matches!(err, WorkbookError::MissingSheet(_)), "{err}");
        let err =
            parse_workbook(fixture("infohub-ela-sample.xlsx"), "NOTES").expect_err("no columns");
        assert!(
            matches!(err, WorkbookError::MissingColumn { column: "dbn", .. }),
            "{err}"
        );
    }
}
