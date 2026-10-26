//! Outbound adapter: the `programs.json` and `student.json` files.

use std::path::{Path, PathBuf};

use crate::domain::ProgramDefinitions;

use super::dto::{program_definitions_from_dto, InvalidInput, ProgramsFileDto, StudentDto};

#[derive(Debug, thiserror::Error)]
pub enum FileLoadError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not valid JSON: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("{path}: {source}")]
    Invalid {
        path: PathBuf,
        #[source]
        source: InvalidInput,
    },
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, FileLoadError> {
    let text = std::fs::read_to_string(path).map_err(|source| FileLoadError::Io {
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|source| FileLoadError::Json {
        path: path.to_owned(),
        source,
    })
}

/// Loads and validates a `programs.json` file.
pub fn load_program_definitions(path: &Path) -> Result<ProgramDefinitions, FileLoadError> {
    let file: ProgramsFileDto = read_json(path)?;
    program_definitions_from_dto(file.programs).map_err(|source| FileLoadError::Invalid {
        path: path.to_owned(),
        source,
    })
}

/// Loads a `student.json` file. The result stays a DTO so a caller can apply
/// command-line overrides before validation.
pub fn load_student(path: &Path) -> Result<StudentDto, FileLoadError> {
    read_json(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ProgramCode, SelectionKind};

    fn data(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join(name)
    }

    #[test]
    fn the_bundled_programs_file_loads() {
        let definitions = load_program_definitions(&data("programs.json")).expect("loads");
        assert_eq!(definitions.len(), 9);
        let twain = definitions
            .get(&ProgramCode::parse("K239SC").expect("valid"))
            .expect("present");
        assert_eq!(twain.selection(), SelectionKind::Screened);
        assert_eq!(twain.groups()[0].demand.initial_offers(), 27);
    }

    #[test]
    fn the_bundled_student_file_loads() {
        let student = load_student(&data("student.json")).expect("loads");
        assert_eq!(student.ranking.len(), 6);
        assert_eq!(student.lottery_number.as_deref(), Some("BD"));
        assert_eq!(student.talent_test_percentile, Some(0.10));
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let err = load_program_definitions(&data("absent.json")).expect_err("missing");
        assert!(matches!(err, FileLoadError::Io { .. }), "{err}");
    }

    #[test]
    fn malformed_json_is_reported_with_the_path() {
        let dir = std::env::temp_dir().join(format!("nycmsa-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("bad.json");
        std::fs::write(&path, "{ not json").expect("write");
        let err = load_student(&path).expect_err("malformed");
        assert!(matches!(err, FileLoadError::Json { .. }), "{err}");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
