//! Use cases for the reference data: the catalog, the test results, the
//! hand-entered programs, and the form defaults. Each lives in a versioned
//! store; the web server reads the latest snapshot of each at startup.

use std::fmt;

use crate::domain::{ProgramDefinitions, Student, TestResults};

use super::download::DistrictFilter;
use super::ports::{
    CatalogDownloader, CatalogSnapshot, CatalogStore, Clock, DownloadError, ProgramStore,
    RepositoryError, Stored, StudentDefaultsStore, TestResultsDownloader, TestResultsStore,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReferenceError {
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error("no {0} in the database; run `nycmsa bootstrap` first")]
    Missing(&'static str),
}

/// Downloads the catalog and stores it as a new snapshot.
pub struct RefreshCatalog<D, S, C> {
    downloader: D,
    store: S,
    clock: C,
}

impl<D: CatalogDownloader, S: CatalogStore, C: Clock> RefreshCatalog<D, S, C> {
    pub fn new(downloader: D, store: S, clock: C) -> Self {
        Self {
            downloader,
            store,
            clock,
        }
    }

    /// Returns the number of schools stored.
    pub async fn execute(
        &self,
        filter: &DistrictFilter,
        source: &str,
    ) -> Result<usize, ReferenceError> {
        let snapshot = self.downloader.download(filter).await?;
        self.store
            .save_catalog(&snapshot, source, self.clock.now())
            .await?;
        Ok(snapshot.len())
    }
}

/// Downloads the test results and stores them as a new snapshot.
pub struct RefreshTestResults<D, S, C> {
    downloader: D,
    store: S,
    clock: C,
}

impl<D: TestResultsDownloader, S: TestResultsStore, C: Clock> RefreshTestResults<D, S, C> {
    pub fn new(downloader: D, store: S, clock: C) -> Self {
        Self {
            downloader,
            store,
            clock,
        }
    }

    /// Returns the number of schools stored and the source description.
    pub async fn execute(&self) -> Result<(usize, String), ReferenceError> {
        let snapshot = self.downloader.download().await?;
        self.store
            .save_test_results(&snapshot.results, &snapshot.source, self.clock.now())
            .await?;
        Ok((snapshot.results.len(), snapshot.source))
    }
}

/// Stores the hand-entered programs and the form defaults read from the
/// seed files.
pub struct ImportSeeds<S, C> {
    store: S,
    clock: C,
}

impl<S: ProgramStore + StudentDefaultsStore, C: Clock> ImportSeeds<S, C> {
    pub fn new(store: S, clock: C) -> Self {
        Self { store, clock }
    }

    pub async fn execute(
        &self,
        programs: &ProgramDefinitions,
        programs_source: &str,
        student: &Student,
        student_source: &str,
    ) -> Result<(), ReferenceError> {
        let now = self.clock.now();
        self.store
            .save_programs(programs, programs_source, now)
            .await?;
        self.store
            .save_student_defaults(student, student_source, now)
            .await?;
        Ok(())
    }
}

/// The latest snapshot of each reference set.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceData {
    pub catalog: Stored<CatalogSnapshot>,
    pub results: Stored<TestResults>,
    pub programs: Stored<ProgramDefinitions>,
    pub student: Stored<Student>,
}

impl fmt::Display for ReferenceData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "catalog: {} schools ({}); scores: {} schools ({}); programs: {} ({}); defaults ({})",
            self.catalog.value.len(),
            self.catalog.source,
            self.results.value.len(),
            self.results.source,
            self.programs.value.len(),
            self.programs.source,
            self.student.source
        )
    }
}

/// Which reference sets the store holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceInventory {
    pub catalog: bool,
    pub results: bool,
    pub programs: bool,
    pub student: bool,
}

/// Reads the latest snapshots. Every set must exist.
pub struct LoadReferenceData<S> {
    store: S,
}

impl<S> LoadReferenceData<S>
where
    S: CatalogStore + TestResultsStore + ProgramStore + StudentDefaultsStore,
{
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn inventory(&self) -> Result<ReferenceInventory, ReferenceError> {
        Ok(ReferenceInventory {
            catalog: self.store.latest_catalog().await?.is_some(),
            results: self.store.latest_test_results().await?.is_some(),
            programs: self.store.latest_programs().await?.is_some(),
            student: self.store.latest_student_defaults().await?.is_some(),
        })
    }

    pub async fn execute(&self) -> Result<ReferenceData, ReferenceError> {
        Ok(ReferenceData {
            catalog: self
                .store
                .latest_catalog()
                .await?
                .ok_or(ReferenceError::Missing("catalog snapshot"))?,
            results: self
                .store
                .latest_test_results()
                .await?
                .ok_or(ReferenceError::Missing("test results"))?,
            programs: self
                .store
                .latest_programs()
                .await?
                .ok_or(ReferenceError::Missing("program definitions"))?,
            student: self
                .store
                .latest_student_defaults()
                .await?
                .ok_or(ReferenceError::Missing("student defaults"))?,
        })
    }
}
