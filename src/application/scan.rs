//! Use case: scan the loaded MySchools catalog for one student.
//!
//! The catalog is loaded once at startup and held in memory. There is no
//! port: the catalog has one source and the scan needs no test double beyond
//! a `Vec` (REAS-ARCH-004).

use crate::domain::{scan, CatalogProgram, ScanRequest, ScanRow};

pub struct ScanCatalog {
    catalog: Vec<CatalogProgram>,
}

impl ScanCatalog {
    pub fn new(catalog: Vec<CatalogProgram>) -> Self {
        Self { catalog }
    }

    /// Number of programs in the loaded catalog.
    pub fn program_count(&self) -> usize {
        self.catalog.len()
    }

    pub fn execute(&self, request: &ScanRequest) -> Vec<ScanRow> {
        scan(&self.catalog, request)
    }
}
