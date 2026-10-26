//! Application use cases and the ports they depend on.
//!
//! A use case takes its adapters as type parameters bounded by port traits
//! (REAS-ARCH-006). The composition root selects the concrete adapters.

pub mod download;
pub mod ports;
pub mod runs;
pub mod scan;
pub mod simulate;

pub use download::{DistrictFilter, DownloadCatalog};
pub use ports::{
    CatalogDownloader, CatalogSnapshot, Clock, DownloadError, NewRun, RepositoryError, Run, RunId,
    RunRepository,
};
pub use runs::RunQueries;
pub use scan::ScanCatalog;
pub use simulate::{SimulateError, SimulateStudent, SimulationRequest};
