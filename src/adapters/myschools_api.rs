//! Outbound adapter: the public MySchools API, implementing
//! [`CatalogDownloader`].

use std::time::Duration;

use serde::Deserialize;

use crate::application::{CatalogDownloader, CatalogSnapshot, DistrictFilter, DownloadError};

/// Middle school admissions process on MySchools.
pub const DEFAULT_BASE_URL: &str = "https://www.myschools.nyc/en/api/v2/schools/process/6/";

#[derive(Debug, Deserialize)]
struct PageDto {
    #[serde(default)]
    results: Vec<serde_json::Value>,
    next: Option<String>,
}

pub struct MySchoolsApiClient {
    client: reqwest::Client,
    base_url: String,
    /// Pause between pages, as the Python fetcher did.
    page_delay: Duration,
}

#[derive(Debug, thiserror::Error)]
#[error("cannot build the HTTP client: {0}")]
pub struct ClientBuildError(#[from] reqwest::Error);

impl MySchoolsApiClient {
    pub fn new(base_url: String, page_delay: Duration) -> Result<Self, ClientBuildError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent("Mozilla/5.0 (nycmsa)")
            .build()?;
        Ok(Self {
            client,
            base_url,
            page_delay,
        })
    }

    async fn page(&self, url: &str) -> Result<PageDto, DownloadError> {
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| DownloadError::Request {
                url: url.to_owned(),
                reason: e.to_string(),
            })?;
        response
            .json::<PageDto>()
            .await
            .map_err(|e| DownloadError::Malformed {
                url: url.to_owned(),
                reason: e.to_string(),
            })
    }
}

impl CatalogDownloader for MySchoolsApiClient {
    async fn download(&self, filter: &DistrictFilter) -> Result<CatalogSnapshot, DownloadError> {
        let mut snapshot = CatalogSnapshot::default();
        let mut next = Some(format!("{}?page=1", self.base_url));
        while let Some(url) = next {
            let page = self.page(&url).await?;
            snapshot.schools.extend(
                page.results
                    .into_iter()
                    .filter(|school| CatalogSnapshot::school_matches(school, filter)),
            );
            // The API returns plain-HTTP `next` links; keep the download on TLS.
            next = page.next.map(|u| u.replacen("http://", "https://", 1));
            if next.is_some() {
                tokio::time::sleep(self.page_delay).await;
            }
        }
        Ok(snapshot)
    }
}
