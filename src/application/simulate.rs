//! Use case: simulate one student's chances. Nothing is stored: the
//! server keeps no record of what a visitor enters (ADR 0005).

use crate::domain::{simulate, Parameters, ProgramDefinitions, Report, SimulationError, Student};

/// Input to [`SimulateStudent::execute`]. `programs` overrides the default
/// definitions for this request only.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulationRequest {
    pub student: Student,
    pub parameters: Parameters,
    pub programs: Option<ProgramDefinitions>,
}

/// Runs the Monte Carlo. The work is pure CPU; an async inbound adapter
/// runs it off the executor thread (REAS-RUST-007).
pub struct SimulateStudent {
    defaults: ProgramDefinitions,
}

impl SimulateStudent {
    pub fn new(defaults: ProgramDefinitions) -> Self {
        Self { defaults }
    }

    /// The program definitions used when a request supplies none.
    pub fn default_programs(&self) -> &ProgramDefinitions {
        &self.defaults
    }

    /// Validates the ranking against the programs and runs the trials.
    pub fn execute(&self, request: &SimulationRequest) -> Result<Report, SimulationError> {
        let programs = request.programs.as_ref().unwrap_or(&self.defaults);
        simulate(programs, &request.student, &request.parameters)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::{
        Attribute, Attrition, GroupDemand, PriorityGroup, Program, ProgramCode, Ranking, Seed,
        SelectionKind, StandardDeviation, Trials, WaitlistNumbers,
    };

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

    #[test]
    fn execute_returns_one_row_per_ranked_program() {
        let use_case = SimulateStudent::new(definitions());
        let report = use_case.execute(&request("K1")).expect("runs");
        assert_eq!(report.rows.len(), 1);
    }

    #[test]
    fn execute_reports_an_unknown_program() {
        let use_case = SimulateStudent::new(definitions());
        let err = use_case.execute(&request("K9")).expect_err("unknown code");
        assert_eq!(
            err,
            SimulationError::UnknownProgram(ProgramCode::parse("K9").expect("valid"))
        );
    }

    #[test]
    fn a_request_can_override_the_program_definitions() {
        let use_case = SimulateStudent::new(ProgramDefinitions::default());
        let mut custom = request("K1");
        custom.programs = Some(definitions());
        assert!(use_case.execute(&custom).is_ok());
    }
}
