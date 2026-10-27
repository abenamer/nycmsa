//! Outbound adapter: the DOE's reports on school applications, offers and
//! seats (Local Law 72 of 2018, formerly 1486-A), implementing
//! [`DemandDownloader`].
//!
//! One workbook per admissions year, 8 to 14 MB each. The `School` sheet
//! has one row per school and category with the columns `School DBN`,
//! `School Name`, `Category`, and per grade `Seats Available`,
//! `Applicants` (or `Total Applicants` from fall 2023), `True Applicants`
//! (from fall 2023) and `Offers`. The reader keeps the grade 6 figures of
//! the `All Students` row and of the `Residential District NN` rows.
//! A suppressed cell holds `s` or `s^`, a missing one `.` or `N/A`; both
//! read as unknown. Column names are matched case-insensitively because
//! the DOE's capitalisation differs between years. The unit tests cover
//! both layouts with small fixture workbooks; a change in the layout fails
//! loudly on a missing sheet or column.
//!
//! Fall 2020 and fall 2021 are published on NYC Open Data as attachments
//! of the district-level datasets; the other years are on InfoHub.

use std::collections::BTreeMap;
use std::io::{Read, Seek};
use std::time::Duration;

use calamine::{Data, Reader, Xlsx};

use crate::application::{DemandDownloader, DownloadError};
use crate::domain::{DemandFigures, DemandHistory, SchoolDemandYear};

/// One year's report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DemandSource {
    /// The fall of entry.
    pub year: u32,
    pub url: &'static str,
}

const INFOHUB: &str = "https://infohub.nyced.org/docs/default-source/default-document-library";
const OPEN_DATA: &str = "https://data.cityofnewyork.us/api/views";

/// Every published year, ascending.
pub const SOURCES: [DemandSource; 8] = [
    DemandSource {
        year: 2018,
        url: "https://infohub.nyced.org/docs/default-source/default-document-library/fall-2018-admissions-report-for-city-council-1486_suppressed.xlsx",
    },
    DemandSource {
        year: 2019,
        url: "https://infohub.nyced.org/docs/default-source/default-document-library/fall-2019-admissions_1486_suppressed.xlsx",
    },
    DemandSource {
        year: 2020,
        url: "https://data.cityofnewyork.us/api/views/5f9p-h9ar/files/d5d2dc4c-6464-49ae-94b5-9801f382bf7a?download=true&filename=fall-2020-admissions.xlsx",
    },
    DemandSource {
        year: 2021,
        url: "https://data.cityofnewyork.us/api/views/naa7-pasa/files/66b8a382-2168-43fe-8e98-3e99b1ce417a?download=true&filename=fall-2021-admissions.xlsx",
    },
    DemandSource {
        year: 2022,
        url: "https://infohub.nyced.org/docs/default-source/default-document-library/fall-2022-admissions_1486_suppressed.xlsx",
    },
    DemandSource {
        year: 2023,
        url: "https://infohub.nyced.org/docs/default-source/default-document-library/fall-2023-admissions_1486_suppressed.xlsx",
    },
    DemandSource {
        year: 2024,
        url: "https://infohub.nyced.org/docs/default-source/default-document-library/ose/fall-2024-admissions-72-suppressed.xlsx",
    },
    DemandSource {
        year: 2025,
        url: "https://infohub.nyced.org/docs/default-source/default-document-library/ose/fall-2025-admissions_72_suppressed.xlsx",
    },
];

/// A one-line description of where the reports come from.
pub fn sources_description() -> String {
    format!(
        "DOE Local Law 72 reports on applications and offers, {} to {} ({INFOHUB} and {OPEN_DATA})",
        SOURCES[0].year,
        SOURCES[SOURCES.len() - 1].year
    )
}

pub const SCHOOL_SHEET: &str = "School";

#[derive(Debug, thiserror::Error)]
pub enum DemandWorkbookError {
    #[error("cannot read the workbook: {0}")]
    Xlsx(#[from] calamine::XlsxError),
    #[error("the workbook has no sheet named {0:?}")]
    MissingSheet(String),
    #[error("sheet {sheet:?} has no {column:?} column")]
    MissingColumn { sheet: String, column: &'static str },
    #[error("sheet {sheet:?} is empty")]
    Empty { sheet: String },
}

fn text(cell: &Data) -> String {
    match cell {
        Data::String(s) => s.trim().to_owned(),
        Data::Empty => String::new(),
        other => other.to_string().trim().to_owned(),
    }
}

/// A count; `None` for a suppressed (`s`, `s^`), missing (`.`, `N/A`,
/// empty) or non-numeric cell.
fn count(cell: &Data) -> Option<u32> {
    match cell {
        Data::Float(f) if *f >= 0.0 => Some(f.round() as u32),
        Data::Int(i) => u32::try_from(*i).ok(),
        Data::String(s) => s.trim().replace(',', "").parse().ok(),
        _ => None,
    }
}

/// The residential district code of a category such as
/// `Residential District 01`, without leading zeros.
fn residential_district(category: &str) -> Option<String> {
    let rest = category.strip_prefix("Residential District")?.trim();
    let code = rest.trim_start_matches('0');
    if code.is_empty() {
        return Some("0".to_owned());
    }
    Some(code.to_owned())
}

/// Reads one year's workbook: the grade 6 figures per school, overall and
/// by residential district.
pub fn parse_demand_workbook<R: Read + Seek>(
    reader: R,
    year: u32,
) -> Result<DemandHistory, DemandWorkbookError> {
    let mut workbook = Xlsx::new(reader)?;
    let sheet = workbook
        .sheet_names()
        .iter()
        .find(|name| name.eq_ignore_ascii_case(SCHOOL_SHEET))
        .cloned()
        .ok_or_else(|| DemandWorkbookError::MissingSheet(SCHOOL_SHEET.to_owned()))?;
    let range = workbook
        .worksheet_range(&sheet)
        .map_err(|_| DemandWorkbookError::MissingSheet(sheet.clone()))?;
    let mut rows = range.rows();
    let header: Vec<String> = rows
        .next()
        .ok_or_else(|| DemandWorkbookError::Empty {
            sheet: sheet.clone(),
        })?
        .iter()
        .map(|c| {
            text(c)
                .to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let find = |names: &[&'static str]| -> Option<usize> {
        names
            .iter()
            .find_map(|name| header.iter().position(|h| h == name))
    };
    let column = |names: &[&'static str]| -> Result<usize, DemandWorkbookError> {
        find(names).ok_or(DemandWorkbookError::MissingColumn {
            sheet: sheet.clone(),
            column: names[0],
        })
    };
    let dbn = column(&["school dbn", "dbn"])?;
    let category = column(&["category"])?;
    let seats = column(&["grade 6 seats available"])?;
    let listed = column(&["grade 6 total applicants", "grade 6 applicants"])?;
    let considered = find(&["grade 6 true applicants"]);
    let offers = column(&["grade 6 offers"])?;

    let mut schools: BTreeMap<String, SchoolDemandYear> = BTreeMap::new();
    for row in rows {
        let cell = |i: usize| row.get(i).unwrap_or(&Data::Empty);
        let key = text(cell(dbn)).to_uppercase();
        if key.is_empty() {
            continue;
        }
        let figures = DemandFigures {
            seats: count(cell(seats)),
            listed: count(cell(listed)),
            considered: considered.and_then(|c| count(cell(c))),
            offers: count(cell(offers)),
        };
        if figures.is_empty() {
            continue;
        }
        let label = text(cell(category));
        let entry = schools.entry(key).or_insert_with(|| SchoolDemandYear {
            year,
            ..SchoolDemandYear::default()
        });
        if label.eq_ignore_ascii_case("all students") {
            entry.all = figures;
        } else if let Some(district) = residential_district(&label) {
            entry.by_residential_district.insert(
                district,
                DemandFigures {
                    seats: None,
                    ..figures
                },
            );
        }
    }
    let mut history = DemandHistory::new();
    for (dbn, school) in schools {
        // A school whose only grade 6 rows are district breakdowns of
        // nothing is not a grade 6 school.
        if school.all.is_empty() && school.by_residential_district.is_empty() {
            continue;
        }
        history.insert(&dbn, school);
    }
    Ok(history)
}

pub struct InfoHubDemandClient {
    client: reqwest::Client,
    sources: Vec<DemandSource>,
}

#[derive(Debug, thiserror::Error)]
#[error("cannot build the HTTP client: {0}")]
pub struct ClientBuildError(#[from] reqwest::Error);

impl InfoHubDemandClient {
    pub fn new(sources: Vec<DemandSource>) -> Result<Self, ClientBuildError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .user_agent("nycmsa")
            .build()?;
        Ok(Self { client, sources })
    }
}

impl DemandDownloader for InfoHubDemandClient {
    fn years(&self) -> Vec<u32> {
        self.sources.iter().map(|s| s.year).collect()
    }

    async fn download(&self, year: u32) -> Result<DemandHistory, DownloadError> {
        let source = self
            .sources
            .iter()
            .find(|s| s.year == year)
            .ok_or_else(|| DownloadError::Request {
                url: String::new(),
                reason: format!("no report is configured for {year}"),
            })?;
        let url = source.url;
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
        // Parsing a 10 MB workbook is CPU work; keep it off the executor
        // (REAS-RUST-007).
        tokio::task::spawn_blocking(move || {
            parse_demand_workbook(std::io::Cursor::new(bytes), year).map_err(|e| {
                DownloadError::Malformed {
                    url: url.to_owned(),
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
    fn the_2025_layout_gives_listed_considered_offers_and_districts() {
        let history = parse_demand_workbook(fixture("infohub-demand-2025-sample.xlsx"), 2025)
            .expect("parses");
        assert_eq!(history.years(), vec![2025]);
        assert_eq!(
            history.schools(),
            2,
            "the elementary school has no grade 6 figures"
        );
        let school = &history.years_of("15K442")[0];
        assert_eq!(
            school.all,
            DemandFigures {
                seats: Some(115),
                listed: Some(1585),
                considered: Some(564),
                offers: Some(138),
            }
        );
        assert_eq!(
            school.by_residential_district["15"],
            DemandFigures {
                seats: None,
                listed: Some(900),
                considered: Some(400),
                offers: Some(100),
            }
        );
        assert_eq!(
            school.by_residential_district["13"].listed,
            Some(60),
            "a district row with a suppressed offers cell is kept"
        );
        assert_eq!(school.by_residential_district["13"].offers, None);
        let suppressed = &history.years_of("20K062")[0];
        assert_eq!(suppressed.all.offers, None, "s^ reads as unknown");
        assert_eq!(suppressed.all.listed, Some(300));
    }

    #[test]
    fn the_2019_layout_has_no_considered_column() {
        let history = parse_demand_workbook(fixture("infohub-demand-2019-sample.xlsx"), 2019)
            .expect("parses");
        let school = &history.years_of("15K442")[0];
        assert_eq!(school.year, 2019);
        assert_eq!(school.all.listed, Some(1400));
        assert_eq!(school.all.considered, None);
        assert_eq!(school.all.offers, Some(120));
        assert_eq!(school.by_residential_district["15"].listed, Some(800));
    }

    #[test]
    fn a_missing_sheet_or_column_is_an_error() {
        let err = parse_demand_workbook(fixture("infohub-ela-sample.xlsx"), 2025)
            .expect_err("no School sheet");
        assert!(matches!(err, DemandWorkbookError::MissingSheet(_)), "{err}");
    }

    #[test]
    fn cells_read_as_counts_or_unknown() {
        assert_eq!(count(&Data::String("1,585".to_owned())), Some(1585));
        assert_eq!(count(&Data::Float(138.0)), Some(138));
        assert_eq!(count(&Data::String("s^".to_owned())), None);
        assert_eq!(count(&Data::String(".".to_owned())), None);
        assert_eq!(count(&Data::String("N/A".to_owned())), None);
        assert_eq!(
            residential_district("Residential District 01"),
            Some("1".to_owned())
        );
        assert_eq!(
            residential_district("Residential District 15"),
            Some("15".to_owned())
        );
        assert_eq!(residential_district("Zip Code 11215"), None);
    }

    #[test]
    fn the_sources_are_ascending_and_distinct() {
        let years: Vec<u32> = SOURCES.iter().map(|s| s.year).collect();
        let mut sorted = years.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(years, sorted);
        assert!(sources_description().contains("2018 to 2025"));
    }
}
