//! Outbound adapter: state test results per school.
//!
//! Reads and writes the CSV shape `dbn,year,ela_proficient_pct,
//! math_proficient_pct`, one row per school. The server takes its scores
//! from PostgreSQL; this file format serves the file-based `scan` command
//! and local exports.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::{SchoolScores, TestResults};

#[derive(Debug, thiserror::Error)]
pub enum TestResultsError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Csv {
        path: PathBuf,
        #[source]
        source: csv::Error,
    },
    #[error("{path}, row {row}: {reason}")]
    BadRow {
        path: PathBuf,
        row: usize,
        reason: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct ScoreRowDto {
    dbn: String,
    year: u32,
    ela_proficient_pct: Option<f64>,
    math_proficient_pct: Option<f64>,
}

/// Loads the project's results file. A file with only the header gives an
/// empty map.
pub fn load_test_results(path: &Path) -> Result<TestResults, TestResultsError> {
    let text = std::fs::read_to_string(path).map_err(|source| TestResultsError::Io {
        path: path.to_owned(),
        source,
    })?;
    let mut reader = csv::Reader::from_reader(text.as_bytes());
    let mut results = TestResults::new();
    for (index, record) in reader.deserialize::<ScoreRowDto>().enumerate() {
        let row = record.map_err(|source| TestResultsError::Csv {
            path: path.to_owned(),
            source,
        })?;
        let dbn = row.dbn.trim().to_uppercase();
        if dbn.is_empty() {
            return Err(TestResultsError::BadRow {
                path: path.to_owned(),
                row: index + 2,
                reason: "empty dbn".to_owned(),
            });
        }
        results.insert(
            dbn,
            SchoolScores {
                year: row.year,
                ela_proficient_pct: row.ela_proficient_pct,
                math_proficient_pct: row.math_proficient_pct,
            },
        );
    }
    Ok(results)
}

/// Writes the project's results file.
pub fn write_test_results(path: &Path, results: &TestResults) -> Result<(), TestResultsError> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    for (dbn, scores) in results {
        writer
            .serialize(ScoreRowDto {
                dbn: dbn.clone(),
                year: scores.year,
                ela_proficient_pct: scores.ela_proficient_pct,
                math_proficient_pct: scores.math_proficient_pct,
            })
            .map_err(|source| TestResultsError::Csv {
                path: path.to_owned(),
                source,
            })?;
    }
    let bytes = writer.into_inner().map_err(|e| TestResultsError::Io {
        path: path.to_owned(),
        source: e.into_error(),
    })?;
    std::fs::write(path, bytes).map_err(|source| TestResultsError::Io {
        path: path.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nycmsa-scores-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join(name);
        std::fs::write(&path, content).expect("write");
        path
    }

    #[test]
    fn results_file_round_trips() {
        let path = temp_file(
            "scores.csv",
            "dbn,year,ela_proficient_pct,math_proficient_pct\n15k442,2025,71.5,\n20K062,2025,40.0,38.2\n",
        );
        let results = load_test_results(&path).expect("loads");
        assert_eq!(results.len(), 2);
        let k442 = results.get("15K442").expect("uppercased");
        assert_eq!(k442.ela_proficient_pct, Some(71.5));
        assert_eq!(k442.math_proficient_pct, None);
        let out = temp_file("out.csv", "");
        write_test_results(&out, &results).expect("writes");
        assert_eq!(load_test_results(&out).expect("reloads"), results);
    }
}
