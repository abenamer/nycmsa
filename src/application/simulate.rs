//! Use case: simulate one student's chances and store the run.

use crate::domain::{simulate, Parameters, ProgramDefinitions, Report, SimulationError, Student};

use super::ports::{Clock, NewRun, RepositoryError, Run, RunRepository};

/// Input to [`SimulateStudent::execute`]. `programs` overrides the default
/// definitions for this run only.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulationRequest {
    pub student: Student,
    pub parameters: Parameters,
    pub programs: Option<ProgramDefinitions>,
}

/// Error from [`SimulateStudent::execute`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SimulateError {
    #[error(transparent)]
    Simulation(#[from] SimulationError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

/// Runs the Monte Carlo and records the result.
///
/// The use case has two steps so an async inbound adapter can run the CPU
/// work off the executor thread (REAS-RUST-007): [`SimulateStudent::compute`]
/// is pure CPU and belongs in `spawn_blocking`; [`SimulateStudent::record`]
/// awaits the repository. [`SimulateStudent::execute`] runs both in sequence
/// for callers that already run on a blocking-safe thread, such as tests.
pub struct SimulateStudent<R, C> {
    defaults: ProgramDefinitions,
    runs: R,
    clock: C,
}

impl<R: RunRepository, C: Clock> SimulateStudent<R, C> {
    pub fn new(defaults: ProgramDefinitions, runs: R, clock: C) -> Self {
        Self {
            defaults,
            runs,
            clock,
        }
    }

    /// The program definitions used when a request supplies none.
    pub fn default_programs(&self) -> &ProgramDefinitions {
        &self.defaults
    }

    /// CPU step: validates the ranking against the programs and runs the
    /// trials.
    pub fn compute(&self, request: &SimulationRequest) -> Result<Report, SimulationError> {
        let programs = request.programs.as_ref().unwrap_or(&self.defaults);
        simulate(programs, &request.student, &request.parameters)
    }

    /// I/O step: stamps the run with the clock and stores it.
    pub async fn record(&self, student: Student, report: Report) -> Result<Run, RepositoryError> {
        let created_at = self.clock.now();
        let new_run = NewRun {
            created_at,
            student: student.clone(),
            report: report.clone(),
        };
        let id = self.runs.insert(new_run).await?;
        Ok(Run {
            id,
            created_at,
            student,
            report,
        })
    }

    pub async fn execute(&self, request: SimulationRequest) -> Result<Run, SimulateError> {
        let report = self.compute(&request)?;
        Ok(self.record(request.student, report).await?)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::num::NonZeroU64;
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::application::ports::RunId;
    use crate::domain::{
        Attribute, Attrition, GroupDemand, PriorityGroup, Program, ProgramCode, Ranking, Seed,
        SelectionKind, StandardDeviation, Trials, WaitlistNumbers,
    };

    struct FixedClock(SystemTime);

    impl Clock for FixedClock {
        fn now(&self) -> SystemTime {
            self.0
        }
    }

    #[derive(Default)]
    struct RecordingRepository {
        inserted: Mutex<Vec<NewRun>>,
    }

    #[async_trait::async_trait]
    impl RunRepository for RecordingRepository {
        async fn insert(&self, run: NewRun) -> Result<RunId, RepositoryError> {
            self.inserted.lock().expect("not poisoned").push(run);
            Ok(RunId::new(NonZeroU64::new(42).expect("non-zero")))
        }

        async fn find(&self, _id: RunId) -> Result<Option<Run>, RepositoryError> {
            Ok(None)
        }

        async fn recent(&self, _limit: usize) -> Result<Vec<Run>, RepositoryError> {
            Ok(vec![])
        }
    }

    struct FailingRepository;

    #[async_trait::async_trait]
    impl RunRepository for FailingRepository {
        async fn insert(&self, _run: NewRun) -> Result<RunId, RepositoryError> {
            Err(RepositoryError::Unavailable("offline".to_owned()))
        }

        async fn find(&self, _id: RunId) -> Result<Option<Run>, RepositoryError> {
            Err(RepositoryError::Unavailable("offline".to_owned()))
        }

        async fn recent(&self, _limit: usize) -> Result<Vec<Run>, RepositoryError> {
            Err(RepositoryError::Unavailable("offline".to_owned()))
        }
    }

    fn definitions() -> ProgramDefinitions {
        ProgramDefinitions::new(vec![Program::new(
            ProgramCode::parse("K1").expect("valid"),
            "One".to_owned(),
            10,
            10,
            SelectionKind::Lottery,
            vec![PriorityGroup {
                name: "all".to_owned(),
                requires: vec![],
                demand: GroupDemand::new(10, 10, None).expect("valid"),
            }],
            None,
            None,
        )
        .expect("valid")])
        .expect("valid")
    }

    fn request(code: &str) -> SimulationRequest {
        SimulationRequest {
            student: Student {
                attributes: BTreeSet::<Attribute>::new(),
                ranking: Ranking::new(vec![ProgramCode::parse(code).expect("valid")])
                    .expect("valid"),
                lottery_number: None,
                talent_test_percentile: None,
                talent_test_sd: StandardDeviation::ZERO,
            },
            parameters: Parameters {
                trials: Trials::new(50).expect("valid"),
                seed: Seed::new(1),
                demand_sd: StandardDeviation::ZERO,
                attrition: Attrition::ONE,
                waitlist_numbers: WaitlistNumbers::Redrawn,
            },
            programs: None,
        }
    }

    #[tokio::test]
    async fn execute_stores_the_run_with_the_clock_time() {
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let use_case = SimulateStudent::new(
            definitions(),
            RecordingRepository::default(),
            FixedClock(now),
        );
        let run = use_case.execute(request("K1")).await.expect("runs");
        assert_eq!(run.id.get(), 42);
        assert_eq!(run.created_at, now);
        assert_eq!(run.report.rows.len(), 1);
        assert_eq!(
            use_case.runs.inserted.lock().expect("not poisoned").len(),
            1
        );
    }

    #[tokio::test]
    async fn execute_reports_an_unknown_program() {
        let use_case = SimulateStudent::new(
            definitions(),
            RecordingRepository::default(),
            FixedClock(UNIX_EPOCH),
        );
        let err = use_case
            .execute(request("K9"))
            .await
            .expect_err("unknown code");
        assert_eq!(
            err,
            SimulateError::Simulation(SimulationError::UnknownProgram(
                ProgramCode::parse("K9").expect("valid")
            ))
        );
        assert!(use_case
            .runs
            .inserted
            .lock()
            .expect("not poisoned")
            .is_empty());
    }

    #[tokio::test]
    async fn execute_propagates_repository_failure() {
        let use_case =
            SimulateStudent::new(definitions(), FailingRepository, FixedClock(UNIX_EPOCH));
        let err = use_case
            .execute(request("K1"))
            .await
            .expect_err("repository offline");
        assert_eq!(
            err,
            SimulateError::Repository(RepositoryError::Unavailable("offline".to_owned()))
        );
    }

    #[tokio::test]
    async fn a_request_can_override_the_program_definitions() {
        let use_case = SimulateStudent::new(
            ProgramDefinitions::default(),
            RecordingRepository::default(),
            FixedClock(UNIX_EPOCH),
        );
        let mut custom = request("K1");
        custom.programs = Some(definitions());
        assert!(use_case.execute(custom).await.is_ok());
    }
}
