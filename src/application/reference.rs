//! Use cases for the reference data: the catalog, the test results, the
//! hand-entered programs, and the form defaults. Each lives in a versioned
//! store; the web server reads the latest snapshot of each at startup.

use std::fmt;

use crate::domain::{DemandHistory, ProgramDefinitions, ProgramProfile, Student, TestResults};

use super::download::DistrictFilter;
use super::ports::{
    CatalogDownloader, CatalogSnapshot, CatalogStore, Clock, DemandDownloader, DemandStore,
    DownloadError, ProfileStore, ProgramDetailsDownloader, ProgramStore, RepositoryError, Stored,
    StudentDefaultsStore, TestResultsDownloader, TestResultsStore,
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

/// What a profile refresh did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshOutcome {
    pub profiles: usize,
    /// Schools whose lookup failed, with the reason. Their programs are
    /// absent from the snapshot.
    pub failed_schools: Vec<(u64, String)>,
}

/// Looks up every school in the latest catalog snapshot, classifies each
/// program from its texts, and stores the profiles as a new snapshot.
/// A failed lookup skips that school; the snapshot is saved when at least
/// one lookup succeeded.
pub struct RefreshProfiles<D, S, C> {
    downloader: D,
    store: S,
    clock: C,
}

impl<D: ProgramDetailsDownloader, S: CatalogStore + ProfileStore, C: Clock>
    RefreshProfiles<D, S, C>
{
    pub fn new(downloader: D, store: S, clock: C) -> Self {
        Self {
            downloader,
            store,
            clock,
        }
    }

    /// `progress` is called after each school with the count so far and
    /// the total.
    pub async fn execute(
        &self,
        source: &str,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<RefreshOutcome, ReferenceError> {
        let catalog = self
            .store
            .latest_catalog()
            .await?
            .ok_or(ReferenceError::Missing("catalog snapshot"))?;
        let ids = catalog.value.school_ids();
        let mut profiles = Vec::new();
        let mut failed_schools = Vec::new();
        for (done, id) in ids.iter().enumerate() {
            match self.downloader.download(*id).await {
                Ok(found) => profiles.extend(found.into_iter().map(ProgramProfile::reclassified)),
                Err(error) => failed_schools.push((*id, error.to_string())),
            }
            progress(done + 1, ids.len());
        }
        if profiles.is_empty() {
            if let Some((_, reason)) = failed_schools.first() {
                return Err(ReferenceError::Download(DownloadError::Request {
                    url: source.to_owned(),
                    reason: format!(
                        "every school lookup failed ({} schools); first error: {reason}",
                        failed_schools.len()
                    ),
                }));
            }
        }
        let source = if failed_schools.is_empty() {
            source.to_owned()
        } else {
            format!(
                "{source}; {} of {} schools failed",
                failed_schools.len(),
                ids.len()
            )
        };
        self.store
            .save_profiles(&profiles, &source, self.clock.now())
            .await?;
        Ok(RefreshOutcome {
            profiles: profiles.len(),
            failed_schools,
        })
    }
}

/// What a demand refresh did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemandOutcome {
    /// The admissions years stored, ascending.
    pub years: Vec<u32>,
    pub schools: usize,
    /// Years whose report could not be downloaded or read, with the
    /// reason. They are absent from the snapshot.
    pub failed_years: Vec<(u32, String)>,
}

/// Downloads every year of the DOE's school demand reports and stores
/// them as one snapshot. A failed year is skipped; the snapshot is saved
/// when at least one year succeeded.
pub struct RefreshDemand<D, S, C> {
    downloader: D,
    store: S,
    clock: C,
}

impl<D: DemandDownloader, S: DemandStore, C: Clock> RefreshDemand<D, S, C> {
    pub fn new(downloader: D, store: S, clock: C) -> Self {
        Self {
            downloader,
            store,
            clock,
        }
    }

    pub async fn execute(&self, source: &str) -> Result<DemandOutcome, ReferenceError> {
        let mut history = DemandHistory::new();
        let mut failed_years = Vec::new();
        for year in self.downloader.years() {
            match self.downloader.download(year).await {
                Ok(found) => history.extend(found),
                Err(error) => failed_years.push((year, error.to_string())),
            }
        }
        if history.is_empty() {
            let reason = failed_years
                .first()
                .map_or_else(|| "no years configured".to_owned(), |(_, r)| r.clone());
            return Err(ReferenceError::Download(DownloadError::Request {
                url: source.to_owned(),
                reason: format!(
                    "every year failed ({} years); first error: {reason}",
                    failed_years.len()
                ),
            }));
        }
        let source = if failed_years.is_empty() {
            source.to_owned()
        } else {
            format!(
                "{source}; years missing: {}",
                failed_years
                    .iter()
                    .map(|(y, _)| y.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        self.store
            .save_demand(&history, &source, self.clock.now())
            .await?;
        Ok(DemandOutcome {
            years: history.years(),
            schools: history.schools(),
            failed_years,
        })
    }
}

/// Re-runs the classification rules on the latest stored profiles and
/// stores the result as a new snapshot, without a download.
pub struct ReclassifyProfiles<S, C> {
    store: S,
    clock: C,
}

impl<S: ProfileStore, C: Clock> ReclassifyProfiles<S, C> {
    pub fn new(store: S, clock: C) -> Self {
        Self { store, clock }
    }

    pub async fn execute(&self) -> Result<usize, ReferenceError> {
        let latest = self
            .store
            .latest_profiles()
            .await?
            .ok_or(ReferenceError::Missing("program profiles"))?;
        let profiles: Vec<ProgramProfile> = latest
            .value
            .into_iter()
            .map(ProgramProfile::reclassified)
            .collect();
        const SUFFIX: &str = " (reclassified)";
        let base = latest.source.trim_end_matches(SUFFIX);
        self.store
            .save_profiles(&profiles, &format!("{base}{SUFFIX}"), self.clock.now())
            .await?;
        Ok(profiles.len())
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
    /// The seed student of the file-based `sim` command. The web server
    /// does not read it (ADR 0005), so it may be absent.
    pub student: Option<Stored<Student>>,
    pub profiles: Stored<Vec<ProgramProfile>>,
    /// The DOE's school demand reports. Supplementary: the pages show no
    /// history when the download has not succeeded yet.
    pub demand: Option<Stored<DemandHistory>>,
}

impl fmt::Display for ReferenceData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "catalog: {} schools ({}); scores: {} schools ({}); programs: {} ({}); defaults ({}); profiles: {} ({}); demand: {} school-years ({})",
            self.catalog.value.len(),
            self.catalog.source,
            self.results.value.len(),
            self.results.source,
            self.programs.value.len(),
            self.programs.source,
            self.student.as_ref().map_or("none", |s| s.source.as_str()),
            self.profiles.value.len(),
            self.profiles.source,
            self.demand.as_ref().map_or(0, |d| d.value.len()),
            self.demand.as_ref().map_or("none", |d| d.source.as_str())
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
    pub profiles: bool,
    pub demand: bool,
}

/// Reads the latest snapshots. Every set must exist.
pub struct LoadReferenceData<S> {
    store: S,
}

impl<S> LoadReferenceData<S>
where
    S: CatalogStore
        + TestResultsStore
        + ProgramStore
        + StudentDefaultsStore
        + ProfileStore
        + DemandStore,
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
            profiles: self.store.latest_profiles().await?.is_some(),
            demand: self.store.latest_demand().await?.is_some(),
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
            student: self.store.latest_student_defaults().await?,
            profiles: self
                .store
                .latest_profiles()
                .await?
                .ok_or(ReferenceError::Missing("program profiles"))?,
            demand: self.store.latest_demand().await?,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::application::ports::{
        CatalogSnapshot, CatalogStore, Clock, DemandDownloader, DemandStore, DownloadError,
        ProfileStore, ProgramDetailsDownloader, RepositoryError, Stored,
    };
    use crate::domain::ProgramCode;

    /// One school document per id, as the catalog store returns them.
    struct Store {
        catalog: CatalogSnapshot,
        saved: Mutex<Vec<(usize, String)>>,
    }

    impl Store {
        fn with_school_ids(ids: &[u64]) -> Self {
            Self {
                catalog: CatalogSnapshot {
                    schools: ids
                        .iter()
                        .map(|id| serde_json::json!({ "id": id }))
                        .collect(),
                },
                saved: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl CatalogStore for &Store {
        async fn save_catalog(
            &self,
            _: &CatalogSnapshot,
            _: &str,
            _: SystemTime,
        ) -> Result<(), RepositoryError> {
            unreachable!("not used by RefreshProfiles")
        }

        async fn latest_catalog(&self) -> Result<Option<Stored<CatalogSnapshot>>, RepositoryError> {
            Ok(Some(Stored {
                recorded_at: SystemTime::UNIX_EPOCH,
                source: "test".to_owned(),
                value: self.catalog.clone(),
            }))
        }
    }

    #[async_trait::async_trait]
    impl ProfileStore for &Store {
        async fn save_profiles(
            &self,
            profiles: &[ProgramProfile],
            source: &str,
            _: SystemTime,
        ) -> Result<(), RepositoryError> {
            self.saved
                .lock()
                .expect("not poisoned")
                .push((profiles.len(), source.to_owned()));
            Ok(())
        }

        async fn latest_profiles(
            &self,
        ) -> Result<Option<Stored<Vec<ProgramProfile>>>, RepositoryError> {
            Ok(None)
        }
    }

    /// Fails for the listed school ids and returns one profile for the rest.
    struct Downloader {
        failing: Vec<u64>,
    }

    impl ProgramDetailsDownloader for Downloader {
        async fn download(&self, school_id: u64) -> Result<Vec<ProgramProfile>, DownloadError> {
            if self.failing.contains(&school_id) {
                return Err(DownloadError::Request {
                    url: format!("school/{school_id}"),
                    reason: "HTTP 502".to_owned(),
                });
            }
            Ok(vec![ProgramProfile {
                code: ProgramCode::parse(&format!("K{school_id:03}A")).expect("valid"),
                dbn: format!("20K{school_id:03}"),
                program_name: "Science Academy".to_owned(),
                school_name: String::new(),
                method: "Screened".to_owned(),
                description: String::new(),
                overview: String::new(),
                academic_opportunities: String::new(),
                art_courses: String::new(),
                language_classes: String::new(),
                scores: Vec::new(),
                evidence: Vec::new(),
            }])
        }
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> SystemTime {
            SystemTime::UNIX_EPOCH + Duration::from_secs(1)
        }
    }

    #[tokio::test]
    async fn a_failed_school_is_skipped_and_counted() {
        let store = Store::with_school_ids(&[1, 2, 3]);
        let refresh = RefreshProfiles::new(Downloader { failing: vec![2] }, &store, FixedClock);
        let outcome = refresh
            .execute("MySchools", |_, _| {})
            .await
            .expect("succeeds");
        assert_eq!(outcome.profiles, 2);
        assert_eq!(outcome.failed_schools.len(), 1);
        assert_eq!(outcome.failed_schools[0].0, 2);
        let saved = store.saved.lock().expect("not poisoned");
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].0, 2);
        assert_eq!(saved[0].1, "MySchools; 1 of 3 schools failed");
    }

    /// Fails for the listed years and returns one school for the rest.
    struct DemandDownloads {
        years: Vec<u32>,
        failing: Vec<u32>,
    }

    impl DemandDownloader for DemandDownloads {
        fn years(&self) -> Vec<u32> {
            self.years.clone()
        }

        async fn download(&self, year: u32) -> Result<DemandHistory, DownloadError> {
            if self.failing.contains(&year) {
                return Err(DownloadError::Request {
                    url: format!("report/{year}"),
                    reason: "HTTP 404".to_owned(),
                });
            }
            let mut history = DemandHistory::new();
            history.insert(
                "15K442",
                crate::domain::SchoolDemandYear {
                    year,
                    all: crate::domain::DemandFigures {
                        seats: Some(100),
                        listed: Some(1000),
                        considered: None,
                        offers: Some(110),
                    },
                    by_residential_district: Default::default(),
                },
            );
            Ok(history)
        }
    }

    struct DemandSaves(Mutex<Vec<(usize, String)>>);

    #[async_trait::async_trait]
    impl DemandStore for &DemandSaves {
        async fn save_demand(
            &self,
            history: &DemandHistory,
            source: &str,
            _: SystemTime,
        ) -> Result<(), RepositoryError> {
            self.0
                .lock()
                .expect("not poisoned")
                .push((history.len(), source.to_owned()));
            Ok(())
        }

        async fn latest_demand(&self) -> Result<Option<Stored<DemandHistory>>, RepositoryError> {
            Ok(None)
        }
    }

    #[tokio::test]
    async fn a_failed_year_is_skipped_and_named_in_the_source() {
        let saves = DemandSaves(Mutex::new(Vec::new()));
        let refresh = RefreshDemand::new(
            DemandDownloads {
                years: vec![2023, 2024, 2025],
                failing: vec![2024],
            },
            &saves,
            FixedClock,
        );
        let outcome = refresh.execute("InfoHub").await.expect("succeeds");
        assert_eq!(outcome.years, vec![2023, 2025]);
        assert_eq!(outcome.schools, 1);
        assert_eq!(outcome.failed_years.len(), 1);
        let saved = saves.0.lock().expect("not poisoned");
        assert_eq!(saved[0], (2, "InfoHub; years missing: 2024".to_owned()));
    }

    #[tokio::test]
    async fn nothing_is_saved_when_every_year_fails() {
        let saves = DemandSaves(Mutex::new(Vec::new()));
        let refresh = RefreshDemand::new(
            DemandDownloads {
                years: vec![2024, 2025],
                failing: vec![2024, 2025],
            },
            &saves,
            FixedClock,
        );
        let error = refresh.execute("InfoHub").await.expect_err("fails");
        assert!(
            error.to_string().contains("every year failed (2 years)"),
            "{error}"
        );
        assert!(saves.0.lock().expect("not poisoned").is_empty());
    }

    #[tokio::test]
    async fn nothing_is_saved_when_every_school_fails() {
        let store = Store::with_school_ids(&[1, 2]);
        let refresh = RefreshProfiles::new(
            Downloader {
                failing: vec![1, 2],
            },
            &store,
            FixedClock,
        );
        let error = refresh
            .execute("MySchools", |_, _| {})
            .await
            .expect_err("fails");
        assert!(matches!(error, ReferenceError::Download(_)));
        assert!(error
            .to_string()
            .contains("every school lookup failed (2 schools)"));
        assert!(store.saved.lock().expect("not poisoned").is_empty());
    }
}
