//! Port traits. The application declares them; adapters implement them.

use std::time::SystemTime;

use crate::domain::ProgramProfile;
use crate::domain::{DistrictCode, ProgramDefinitions, Student, TestResults};

use super::download::DistrictFilter;

/// Typed error for the store ports.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepositoryError {
    #[error("database unavailable: {0}")]
    Unavailable(String),
    /// A stored row failed domain validation on the way back out.
    #[error("stored row {id} is not valid: {reason}")]
    Corrupt { id: u64, reason: String },
}

/// Outbound port: the current time. Supplied so use cases stay pure
/// (REAS-ARCH-005).
pub trait Clock {
    fn now(&self) -> SystemTime;
}

/// The raw MySchools catalog documents, one per school, kept verbatim so a
/// later `scan` can parse them with the catalog loader.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CatalogSnapshot {
    pub schools: Vec<serde_json::Value>,
}

impl CatalogSnapshot {
    pub fn len(&self) -> usize {
        self.schools.len()
    }

    /// The MySchools school ids in the snapshot, for detail lookups.
    pub fn school_ids(&self) -> Vec<u64> {
        self.schools
            .iter()
            .filter_map(|s| s.get("id").and_then(serde_json::Value::as_u64))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.schools.is_empty()
    }

    /// True when the document's school district passes the filter.
    pub fn school_matches(school: &serde_json::Value, filter: &DistrictFilter) -> bool {
        match filter {
            DistrictFilter::All => true,
            DistrictFilter::Only(codes) => school
                .get("school")
                .and_then(|s| s.get("district"))
                .and_then(|d| d.get("code"))
                .map(|code| match code {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .and_then(|raw| DistrictCode::parse(&raw).ok())
                .is_some_and(|code| codes.contains(&code)),
        }
    }
}

/// Typed error for the catalog downloader port.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DownloadError {
    #[error("request to {url} failed: {reason}")]
    Request { url: String, reason: String },
    #[error("response from {url} was not the expected JSON: {reason}")]
    Malformed { url: String, reason: String },
}

/// State test results with the source they came from.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TestResultsSnapshot {
    pub results: TestResults,
    /// Where the figures came from, for the record.
    pub source: String,
}

/// Outbound port: download of state test results per school.
pub trait TestResultsDownloader {
    // Awaited on the main task; no `Send` bound is needed.
    #[allow(async_fn_in_trait)]
    async fn download(&self) -> Result<TestResultsSnapshot, DownloadError>;
}

/// Outbound port: download of the MySchools catalog.
///
/// Native `async fn`; the composition root awaits it on the main task, so
/// the returned future needs no `Send` bound (REAS-ARCH-006).
pub trait CatalogDownloader {
    // The future is awaited on the main task and never sent across threads.
    #[allow(async_fn_in_trait)]
    async fn download(&self, filter: &DistrictFilter) -> Result<CatalogSnapshot, DownloadError>;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn district_filter_reads_string_and_numeric_codes() {
        let only = DistrictFilter::Only(
            ["15", "20"]
                .iter()
                .map(|c| DistrictCode::parse(c).expect("valid"))
                .collect::<BTreeSet<_>>(),
        );
        let d15 = serde_json::json!({"school": {"district": {"code": "15"}}});
        let d20 = serde_json::json!({"school": {"district": {"code": 20}}});
        let d17 = serde_json::json!({"school": {"district": {"code": "17"}}});
        let none = serde_json::json!({"school": {}});
        assert!(CatalogSnapshot::school_matches(&d15, &only));
        assert!(CatalogSnapshot::school_matches(&d20, &only));
        assert!(!CatalogSnapshot::school_matches(&d17, &only));
        assert!(!CatalogSnapshot::school_matches(&none, &only));
        assert!(CatalogSnapshot::school_matches(&none, &DistrictFilter::All));
    }
}

/// A stored reference snapshot with its provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored<T> {
    pub recorded_at: SystemTime,
    pub source: String,
    pub value: T,
}

/// Outbound port: versioned MySchools catalog snapshots. Every save adds a
/// snapshot; `latest` returns the most recent one.
#[async_trait::async_trait]
pub trait CatalogStore {
    async fn save_catalog(
        &self,
        snapshot: &CatalogSnapshot,
        source: &str,
        recorded_at: SystemTime,
    ) -> Result<(), RepositoryError>;
    async fn latest_catalog(&self) -> Result<Option<Stored<CatalogSnapshot>>, RepositoryError>;
}

/// Outbound port: versioned state test results.
#[async_trait::async_trait]
pub trait TestResultsStore {
    async fn save_test_results(
        &self,
        results: &TestResults,
        source: &str,
        recorded_at: SystemTime,
    ) -> Result<(), RepositoryError>;
    async fn latest_test_results(&self) -> Result<Option<Stored<TestResults>>, RepositoryError>;
}

/// Outbound port: versioned hand-entered program definitions.
#[async_trait::async_trait]
pub trait ProgramStore {
    async fn save_programs(
        &self,
        programs: &ProgramDefinitions,
        source: &str,
        recorded_at: SystemTime,
    ) -> Result<(), RepositoryError>;
    async fn latest_programs(&self) -> Result<Option<Stored<ProgramDefinitions>>, RepositoryError>;
}

/// Outbound port: versioned defaults that prefill the web form.
#[async_trait::async_trait]
pub trait StudentDefaultsStore {
    async fn save_student_defaults(
        &self,
        student: &Student,
        source: &str,
        recorded_at: SystemTime,
    ) -> Result<(), RepositoryError>;
    async fn latest_student_defaults(&self) -> Result<Option<Stored<Student>>, RepositoryError>;
}

/// Outbound port: the detail document of one school, as one unscored
/// profile per program. The use case runs the classification.
pub trait ProgramDetailsDownloader {
    // Awaited on the main task; no `Send` bound is needed.
    #[allow(async_fn_in_trait)]
    async fn download(&self, school_id: u64) -> Result<Vec<ProgramProfile>, DownloadError>;
}

/// Outbound port: versioned program profiles.
#[async_trait::async_trait]
pub trait ProfileStore {
    async fn save_profiles(
        &self,
        profiles: &[ProgramProfile],
        source: &str,
        recorded_at: SystemTime,
    ) -> Result<(), RepositoryError>;
    async fn latest_profiles(&self)
        -> Result<Option<Stored<Vec<ProgramProfile>>>, RepositoryError>;
}
