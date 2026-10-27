//! Application use cases and the ports they depend on.
//!
//! A use case takes its adapters as type parameters bounded by port traits
//! (REAS-ARCH-006). The composition root selects the concrete adapters.

pub mod download;
pub mod ports;
pub mod reference;
pub mod scan;
pub mod simulate;

pub use download::{DistrictFilter, DownloadCatalog, DownloadTestResults};
pub use ports::{
    CatalogDownloader, CatalogSnapshot, CatalogStore, Clock, DemandDownloader, DemandStore,
    DownloadError, ProfileStore, ProgramDetailsDownloader, ProgramStore, RepositoryError, Stored,
    StudentDefaultsStore, TestResultsDownloader, TestResultsSnapshot, TestResultsStore,
};
pub use reference::{
    DemandOutcome, ImportSeeds, LoadReferenceData, ReclassifyProfiles, ReferenceData,
    ReferenceError, RefreshCatalog, RefreshDemand, RefreshOutcome, RefreshProfiles,
    RefreshTestResults,
};
pub use scan::ScanCatalog;
pub use simulate::{SimulateStudent, SimulationRequest};
