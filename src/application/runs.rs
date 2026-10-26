//! Use case: read stored runs. It delegates to the port directly; that
//! delegation is the application boundary (REAS-ARCH-001).

use super::ports::{RepositoryError, Run, RunId, RunRepository};

pub struct RunQueries<R> {
    runs: R,
}

impl<R: RunRepository> RunQueries<R> {
    pub fn new(runs: R) -> Self {
        Self { runs }
    }

    pub async fn find(&self, id: RunId) -> Result<Option<Run>, RepositoryError> {
        self.runs.find(id).await
    }

    pub async fn recent(&self, limit: usize) -> Result<Vec<Run>, RepositoryError> {
        self.runs.recent(limit).await
    }
}
