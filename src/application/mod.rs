//! Application use cases and the ports they depend on.
//!
//! A use case takes its adapters as type parameters bounded by port traits
//! (REAS-ARCH-006). The composition root selects the concrete adapters.

pub mod download;
pub mod ports;
pub mod reference;
pub mod runs;
pub mod scan;
pub mod simulate;

pub use download::{DistrictFilter, DownloadCatalog, DownloadTestResults};
pub use ports::{
    CatalogDownloader, CatalogSnapshot, CatalogStore, Clock, DownloadError, NewRun, ProfileStore,
    ProgramDetailsDownloader, ProgramStore, RepositoryError, Run, RunId, RunRepository, Stored,
    StudentDefaultsStore, TestResultsDownloader, TestResultsSnapshot, TestResultsStore,
};
pub use reference::{
    ImportSeeds, LoadReferenceData, ReclassifyProfiles, ReferenceData, ReferenceError,
    RefreshCatalog, RefreshOutcome, RefreshProfiles, RefreshTestResults,
};
pub use runs::RunQueries;
pub use scan::ScanCatalog;
pub use simulate::{SimulateError, SimulateStudent, SimulationRequest};
