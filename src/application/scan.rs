//! Use case: scan the loaded MySchools catalog for one student.
//!
//! The catalog is loaded once at startup and held in memory. There is no
//! port: the catalog has one source and the scan needs no test double beyond
//! a `Vec` (REAS-ARCH-004).

use crate::domain::{scan, CatalogProgram, DistrictCode, ScanRequest, ScanRow};

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

    /// The districts present in the catalog, sorted, each once.
    pub fn districts(&self) -> Vec<DistrictCode> {
        let mut districts: Vec<DistrictCode> =
            self.catalog.iter().map(|p| p.district.clone()).collect();
        districts.sort_by(|a, b| {
            let numeric = |d: &DistrictCode| d.as_str().parse::<u32>().ok();
            numeric(a)
                .cmp(&numeric(b))
                .then_with(|| a.as_str().cmp(b.as_str()))
        });
        districts.dedup();
        districts
    }

    pub fn execute(&self, request: &ScanRequest) -> Vec<ScanRow> {
        scan(&self.catalog, request)
    }
}
