//! Outbound adapter: runs kept in process memory.
//!
//! The store is bounded; when it is full the oldest run is dropped. Runs do
//! not survive a restart. Clones share one store (`Arc<Mutex<_>>`, used here
//! for shared ownership with synchronized mutation, REAS-RUST-004).

use std::collections::VecDeque;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::{Arc, Mutex};

use crate::application::{NewRun, RepositoryError, Run, RunId, RunRepository};

struct Store {
    next_id: u64,
    capacity: usize,
    runs: VecDeque<Run>,
}

#[derive(Clone)]
pub struct InMemoryRunRepository {
    inner: Arc<Mutex<Store>>,
}

impl InMemoryRunRepository {
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Store {
                next_id: 1,
                capacity: capacity.get(),
                runs: VecDeque::with_capacity(capacity.get()),
            })),
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Store>, RepositoryError> {
        self.inner
            .lock()
            .map_err(|_| RepositoryError::Unavailable("run store lock is poisoned".to_owned()))
    }
}

#[async_trait::async_trait]
impl RunRepository for InMemoryRunRepository {
    async fn insert(&self, run: NewRun) -> Result<RunId, RepositoryError> {
        let mut store = self.lock()?;
        let raw = NonZeroU64::new(store.next_id)
            .ok_or_else(|| RepositoryError::Unavailable("run id space exhausted".to_owned()))?;
        store.next_id += 1;
        let id = RunId::new(raw);
        store.runs.push_back(Run {
            id,
            created_at: run.created_at,
            student: run.student,
            report: run.report,
        });
        while store.runs.len() > store.capacity {
            store.runs.pop_front();
        }
        Ok(id)
    }

    async fn find(&self, id: RunId) -> Result<Option<Run>, RepositoryError> {
        Ok(self.lock()?.runs.iter().find(|r| r.id == id).cloned())
    }

    async fn recent(&self, limit: usize) -> Result<Vec<Run>, RepositoryError> {
        Ok(self
            .lock()?
            .runs
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::UNIX_EPOCH;

    use super::*;
    use crate::domain::{
        Attrition, Parameters, ProgramCode, Ranking, Report, Seed, StandardDeviation, Student,
        Summary, Trials, WaitlistNumbers,
    };

    fn new_run(seed: u64) -> NewRun {
        NewRun {
            created_at: UNIX_EPOCH,
            student: Student {
                attributes: BTreeSet::new(),
                ranking: Ranking::new(vec![ProgramCode::parse("K1").expect("valid")])
                    .expect("valid"),
                lottery_number: None,
                talent_test_percentile: None,
                talent_test_sd: StandardDeviation::ZERO,
            },
            report: Report {
                parameters: Parameters {
                    trials: Trials::new(1).expect("valid"),
                    seed: Seed::new(seed),
                    demand_sd: StandardDeviation::ZERO,
                    attrition: Attrition::ONE,
                    waitlist_numbers: WaitlistNumbers::Redrawn,
                },
                lottery_fraction: None,
                rows: vec![],
                summary: Summary {
                    p_initial_offer: 0.0,
                    p_any_offer: 0.0,
                    p_no_offer: 1.0,
                    p_lottery_offer: None,
                    p_screened_offer: None,
                },
            },
        }
    }

    fn capacity(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).expect("non-zero")
    }

    #[tokio::test]
    async fn insert_assigns_increasing_ids_and_find_returns_the_run() {
        let repo = InMemoryRunRepository::new(capacity(10));
        let first = repo.insert(new_run(1)).await.expect("stores");
        let second = repo.insert(new_run(2)).await.expect("stores");
        assert_eq!(first.get(), 1);
        assert_eq!(second.get(), 2);
        let found = repo.find(second).await.expect("reads").expect("present");
        assert_eq!(found.report.parameters.seed.get(), 2);
        let missing = RunId::new(NonZeroU64::new(99).expect("non-zero"));
        assert_eq!(repo.find(missing).await.expect("reads"), None);
    }

    #[tokio::test]
    async fn recent_lists_newest_first_up_to_the_limit() {
        let repo = InMemoryRunRepository::new(capacity(10));
        for seed in 1..=5 {
            repo.insert(new_run(seed)).await.expect("stores");
        }
        let recent = repo.recent(3).await.expect("reads");
        let seeds: Vec<u64> = recent
            .iter()
            .map(|r| r.report.parameters.seed.get())
            .collect();
        assert_eq!(seeds, vec![5, 4, 3]);
    }

    #[tokio::test]
    async fn the_store_drops_the_oldest_run_when_full() {
        let repo = InMemoryRunRepository::new(capacity(2));
        let first = repo.insert(new_run(1)).await.expect("stores");
        repo.insert(new_run(2)).await.expect("stores");
        repo.insert(new_run(3)).await.expect("stores");
        assert_eq!(repo.find(first).await.expect("reads"), None);
        assert_eq!(repo.recent(10).await.expect("reads").len(), 2);
    }

    #[tokio::test]
    async fn clones_share_one_store() {
        let repo = InMemoryRunRepository::new(capacity(2));
        let clone = repo.clone();
        let id = clone.insert(new_run(1)).await.expect("stores");
        assert!(repo.find(id).await.expect("reads").is_some());
    }
}
