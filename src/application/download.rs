//! Use case: download a MySchools catalog snapshot.

use std::collections::BTreeSet;

use crate::domain::DistrictCode;

use super::ports::{
    CatalogDownloader, CatalogSnapshot, DownloadError, TestResultsDownloader, TestResultsSnapshot,
};

/// Which districts to keep from the download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DistrictFilter {
    All,
    Only(BTreeSet<DistrictCode>),
}

pub struct DownloadCatalog<D> {
    downloader: D,
}

impl<D: CatalogDownloader> DownloadCatalog<D> {
    pub fn new(downloader: D) -> Self {
        Self { downloader }
    }

    pub async fn execute(&self, filter: &DistrictFilter) -> Result<CatalogSnapshot, DownloadError> {
        self.downloader.download(filter).await
    }
}

pub struct DownloadTestResults<D> {
    downloader: D,
}

impl<D: TestResultsDownloader> DownloadTestResults<D> {
    pub fn new(downloader: D) -> Self {
        Self { downloader }
    }

    pub async fn execute(&self) -> Result<TestResultsSnapshot, DownloadError> {
        self.downloader.download().await
    }
}
