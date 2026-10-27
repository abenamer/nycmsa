//! Use case: scan the loaded MySchools catalog for one student.
//!
//! The catalog is loaded once at startup and held in memory. There is no
//! port: the catalog has one source and the scan needs no test double beyond
//! a `Vec` (REAS-ARCH-004).

use crate::domain::{scan, CatalogProgram, DistrictCode, ScanRequest, ScanRow, TestResults};

pub struct ScanCatalog {
    catalog: Vec<CatalogProgram>,
    results: TestResults,
}

impl ScanCatalog {
    pub fn new(catalog: Vec<CatalogProgram>, results: TestResults) -> Self {
        Self { catalog, results }
    }

    /// The loaded catalog, in file order.
    pub fn programs(&self) -> &[CatalogProgram] {
        &self.catalog
    }

    /// Number of schools with test results.
    pub fn schools_with_scores(&self) -> usize {
        self.results.len()
    }

    /// The latest test year among the loaded results.
    pub fn scores_year(&self) -> Option<u32> {
        self.results.values().map(|s| s.year).max()
    }

    /// The latest scores for a school, by DBN.
    pub fn scores_for(&self, dbn: &str) -> Option<crate::domain::SchoolScores> {
        self.results.get(dbn).copied()
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
        scan(&self.catalog, &self.results, request)
    }
}
