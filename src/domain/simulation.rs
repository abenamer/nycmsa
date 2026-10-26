//! The Monte Carlo model of one applicant's chances.
//!
//! NYC uses deferred acceptance with a single random lottery number per
//! student. From one applicant's point of view that reduces to: each program
//! has a cutoff within the student's priority group, and the student is
//! matched to the highest-ranked program whose cutoff their number beats.
//!
//! Per trial:
//!
//! 1. Draw one lottery number `L ~ U(0, 1)`. Lower is better. It is shared
//!    across every lottery program.
//! 2. Draw each program's cutoff for the student's group from
//!    `Beta(offers + 1, applicants - offers + 1)`, the posterior on last
//!    year's offer rate, then scale it by a log-normal demand factor and the
//!    attrition multiplier.
//! 3. Screened programs rank by talent test, not lottery. The student's test
//!    percentile `T` is drawn from the student configuration and compared in
//!    the same way.
//! 4. The initial match is the first program in the ranking that the student
//!    clears.
//! 5. Waitlist round. Each waitlist draws a fresh random number, so for every
//!    lottery program ranked above the initial match the student draws a new
//!    `U(0, 1)` and gets a waitlist offer when it beats
//!    `(offers - initial_offers) / (applicants - initial_offers)`. Screened
//!    programs rank their waitlist by test score, so there is no re-draw.
//!    That is the DOE's published policy. In July 2022 the DOE confirmed
//!    that numbers were not redrawn for a subset of schools (Marian, FAccT
//!    2023), so [`WaitlistNumbers::Reused`] models the alternative: the
//!    student keeps their number, and their position among the applicants
//!    who missed the initial round is `(L - cutoff) / (1 - cutoff)`.
//!
//! The random stream is a seeded ChaCha generator, so one seed reproduces one
//! report (REAS-TEST-001). The Python original used NumPy's generator; the
//! two produce the same distributions, not the same numbers.

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Beta, Distribution, Normal};

use super::program::{GroupDemand, ProgramCode, ProgramDefinitions, SelectionKind};
use super::student::Student;

/// Upper bound on trials per run. It limits the CPU time one request can
/// consume (REAS-SEC-001).
pub const MAX_TRIALS: u32 = 5_000_000;

/// Number of Monte Carlo trials, `1..=MAX_TRIALS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trials(u32);

/// Error from [`Trials::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidTrials {
    #[error("trials must be at least 1")]
    Zero,
    #[error("trials must not exceed {MAX_TRIALS}, got {0}")]
    TooMany(u32),
}

impl Trials {
    pub fn new(count: u32) -> Result<Self, InvalidTrials> {
        if count == 0 {
            return Err(InvalidTrials::Zero);
        }
        if count > MAX_TRIALS {
            return Err(InvalidTrials::TooMany(count));
        }
        Ok(Self(count))
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

/// Seed of the random stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Seed(u64);

impl Seed {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

/// A finite, non-negative standard deviation.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct StandardDeviation(f64);

/// Error from [`StandardDeviation::new`].
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum InvalidStandardDeviation {
    #[error("standard deviation must be a finite number, got {0}")]
    NotFinite(f64),
    #[error("standard deviation must not be negative, got {0}")]
    Negative(f64),
}

impl StandardDeviation {
    pub fn new(value: f64) -> Result<Self, InvalidStandardDeviation> {
        if !value.is_finite() {
            return Err(InvalidStandardDeviation::NotFinite(value));
        }
        if value < 0.0 {
            return Err(InvalidStandardDeviation::Negative(value));
        }
        Ok(Self(value))
    }

    pub const ZERO: StandardDeviation = StandardDeviation(0.0);

    pub fn get(self) -> f64 {
        self.0
    }
}

/// Multiplier on published offer rates. `1.0` uses the rates as published;
/// higher values are optimistic.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Attrition(f64);

/// Error from [`Attrition::new`].
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum InvalidAttrition {
    #[error("attrition must be a finite number, got {0}")]
    NotFinite(f64),
    #[error("attrition must be greater than 0, got {0}")]
    NotPositive(f64),
}

impl Attrition {
    pub fn new(value: f64) -> Result<Self, InvalidAttrition> {
        if !value.is_finite() {
            return Err(InvalidAttrition::NotFinite(value));
        }
        if value <= 0.0 {
            return Err(InvalidAttrition::NotPositive(value));
        }
        Ok(Self(value))
    }

    pub const ONE: Attrition = Attrition(1.0);

    pub fn get(self) -> f64 {
        self.0
    }
}

/// How lottery programs order their waitlists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WaitlistNumbers {
    /// Each waitlist draws a new random number (DOE published policy).
    #[default]
    Redrawn,
    /// The waitlist reuses the application lottery number (observed for some
    /// schools in 2022).
    Reused,
}

impl WaitlistNumbers {
    pub fn label(self) -> &'static str {
        match self {
            WaitlistNumbers::Redrawn => "redrawn",
            WaitlistNumbers::Reused => "reused",
        }
    }
}

/// Run parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parameters {
    pub trials: Trials,
    pub seed: Seed,
    /// Log-normal standard deviation on next year's offer rate.
    pub demand_sd: StandardDeviation,
    pub attrition: Attrition,
    pub waitlist_numbers: WaitlistNumbers,
}

/// Error from [`simulate`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SimulationError {
    #[error("unknown program code {0} in the ranking")]
    UnknownProgram(ProgramCode),
}

/// Result for one ranked program.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgramRow {
    /// Position in the ranking, starting at 1.
    pub rank: usize,
    pub code: ProgramCode,
    pub name: String,
    pub selection: SelectionKind,
    /// The priority group the student falls into; `None` when not eligible.
    pub group_name: Option<String>,
    /// Share of the group matched in the initial round last year.
    pub initial_rate: f64,
    /// Share of the remaining group reached by the waitlist last year.
    pub waitlist_rate: f64,
    pub p_initial: f64,
    pub p_waitlist: f64,
    pub p_any: f64,
}

/// Probabilities across the whole ranking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub p_initial_offer: f64,
    pub p_any_offer: f64,
    pub p_no_offer: f64,
    /// `None` when the ranking holds no lottery program.
    pub p_lottery_offer: Option<f64>,
    /// `None` when the ranking holds no screened program.
    pub p_screened_offer: Option<f64>,
}

/// The output of one simulation.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub parameters: Parameters,
    /// Share of applicants with a better number than the known prefix.
    pub lottery_fraction: Option<f64>,
    pub rows: Vec<ProgramRow>,
    pub summary: Summary,
}

struct Outlook<'a> {
    code: &'a ProgramCode,
    name: &'a str,
    selection: SelectionKind,
    group_name: Option<&'a str>,
    demand: Option<GroupDemand>,
}

/// Draws a cutoff for a group: the Beta posterior on last year's offer rate,
/// scaled by demand noise and attrition, clipped to `[0, 1]`.
struct CutoffNoise {
    demand: Option<Normal<f64>>,
    attrition: f64,
}

impl CutoffNoise {
    fn new(parameters: &Parameters) -> Self {
        Self {
            demand: optional_normal(parameters.demand_sd),
            attrition: parameters.attrition.get(),
        }
    }

    fn rate<R: Rng>(&self, rng: &mut R, offers: u32, applicants: u32) -> f64 {
        if applicants == 0 {
            return 0.0;
        }
        let beta = Beta::new(
            f64::from(offers) + 1.0,
            f64::from(applicants - offers) + 1.0,
        )
        .expect("both Beta parameters are at least 1");
        let mut rate: f64 = beta.sample(rng);
        if let Some(demand) = &self.demand {
            rate *= demand.sample(rng).exp();
        }
        (rate * self.attrition).clamp(0.0, 1.0)
    }
}

/// `None` for a zero standard deviation, so the loop adds no noise.
fn optional_normal(sd: StandardDeviation) -> Option<Normal<f64>> {
    if sd.get() == 0.0 {
        return None;
    }
    Some(Normal::new(0.0, sd.get()).expect("standard deviation is finite"))
}

/// Runs the Monte Carlo and aggregates the trials into a [`Report`].
pub fn simulate(
    programs: &ProgramDefinitions,
    student: &Student,
    parameters: &Parameters,
) -> Result<Report, SimulationError> {
    let outlooks = student
        .ranking
        .codes()
        .iter()
        .map(|code| {
            let program = programs
                .get(code)
                .ok_or_else(|| SimulationError::UnknownProgram(code.clone()))?;
            let group = program.pick_group(&student.attributes);
            Ok(Outlook {
                code,
                name: program.name(),
                selection: program.selection(),
                group_name: group.map(|g| g.name.as_str()),
                demand: group.map(|g| g.demand),
            })
        })
        .collect::<Result<Vec<_>, SimulationError>>()?;

    let count = outlooks.len();
    let trials = parameters.trials.get();
    let mut rng = ChaCha8Rng::seed_from_u64(parameters.seed.get());
    let noise = CutoffNoise::new(parameters);
    let test_noise = optional_normal(student.talent_test_sd);
    let known_lottery = student.lottery_number.as_ref().map(|p| p.fraction().get());
    let known_test = student.talent_test_percentile.map(|p| p.get());

    let mut initial_hits = vec![0u64; count];
    let mut waitlist_hits = vec![0u64; count];
    let mut any_hits = vec![0u64; count];
    let mut cleared = vec![false; count];
    let mut waitlisted = vec![false; count];
    let mut matched_trials = 0u64;
    let mut any_offer_trials = 0u64;
    let mut lottery_offer_trials = 0u64;
    let mut screened_offer_trials = 0u64;

    for _ in 0..trials {
        let lottery = known_lottery.unwrap_or_else(|| rng.random::<f64>());
        let test_base = known_test.unwrap_or_else(|| rng.random::<f64>());

        for (i, outlook) in outlooks.iter().enumerate() {
            let Some(demand) = outlook.demand else {
                cleared[i] = false;
                waitlisted[i] = false;
                continue;
            };
            match outlook.selection {
                SelectionKind::Screened => {
                    let jitter = test_noise.as_ref().map_or(0.0, |n| n.sample(&mut rng));
                    let test = (test_base + jitter).clamp(0.0, 1.0);
                    let initial =
                        test < noise.rate(&mut rng, demand.initial_offers(), demand.applicants());
                    let waitlist = !initial
                        && test < noise.rate(&mut rng, demand.offers(), demand.applicants());
                    cleared[i] = initial;
                    waitlisted[i] = waitlist;
                }
                SelectionKind::Lottery => {
                    let cutoff = noise.rate(&mut rng, demand.initial_offers(), demand.applicants());
                    let initial = lottery < cutoff;
                    let waitlist_rate = noise.rate(
                        &mut rng,
                        demand.waitlist_offers(),
                        demand.remaining_applicants(),
                    );
                    let waitlist_number = match parameters.waitlist_numbers {
                        WaitlistNumbers::Redrawn => rng.random::<f64>(),
                        // Position among the applicants who missed the
                        // initial round; only evaluated when `lottery >=
                        // cutoff`, so the denominator is positive.
                        WaitlistNumbers::Reused => {
                            if cutoff >= 1.0 {
                                1.0
                            } else {
                                (lottery - cutoff) / (1.0 - cutoff)
                            }
                        }
                    };
                    cleared[i] = initial;
                    waitlisted[i] = !initial && waitlist_number < waitlist_rate;
                }
            }
        }

        let matched = cleared.iter().position(|&c| c);
        if matched.is_some() {
            matched_trials += 1;
        }
        let mut any_offer = false;
        let mut lottery_offer = false;
        let mut screened_offer = false;
        for (i, outlook) in outlooks.iter().enumerate() {
            // Waitlists exist only for programs ranked above the initial match.
            let waitlist = waitlisted[i] && matched.is_none_or(|m| m > i);
            if cleared[i] {
                initial_hits[i] += 1;
            }
            if waitlist {
                waitlist_hits[i] += 1;
            }
            if cleared[i] || waitlist {
                any_hits[i] += 1;
                any_offer = true;
                match outlook.selection {
                    SelectionKind::Lottery => lottery_offer = true,
                    SelectionKind::Screened => screened_offer = true,
                }
            }
        }
        if any_offer {
            any_offer_trials += 1;
        }
        if lottery_offer {
            lottery_offer_trials += 1;
        }
        if screened_offer {
            screened_offer_trials += 1;
        }
    }

    let share = |hits: u64| hits as f64 / f64::from(trials);
    let rows = outlooks
        .iter()
        .enumerate()
        .map(|(i, outlook)| ProgramRow {
            rank: i + 1,
            code: outlook.code.clone(),
            name: outlook.name.to_owned(),
            selection: outlook.selection,
            group_name: outlook.group_name.map(str::to_owned),
            initial_rate: outlook.demand.map_or(0.0, GroupDemand::initial_rate),
            waitlist_rate: outlook.demand.map_or(0.0, GroupDemand::waitlist_rate),
            p_initial: share(initial_hits[i]),
            p_waitlist: share(waitlist_hits[i]),
            p_any: share(any_hits[i]),
        })
        .collect();
    let has_lottery = outlooks
        .iter()
        .any(|o| o.selection == SelectionKind::Lottery);
    let has_screened = outlooks
        .iter()
        .any(|o| o.selection == SelectionKind::Screened);
    let p_any_offer = share(any_offer_trials);
    let summary = Summary {
        p_initial_offer: share(matched_trials),
        p_any_offer,
        p_no_offer: 1.0 - p_any_offer,
        p_lottery_offer: has_lottery.then(|| share(lottery_offer_trials)),
        p_screened_offer: has_screened.then(|| share(screened_offer_trials)),
    };
    Ok(Report {
        parameters: *parameters,
        lottery_fraction: known_lottery,
        rows,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::lottery::{LotteryPrefix, UnitInterval};
    use crate::domain::program::{Attribute, PriorityGroup, Program};
    use crate::domain::student::Ranking;

    fn code(raw: &str) -> ProgramCode {
        ProgramCode::parse(raw).expect("valid")
    }

    fn program(
        raw_code: &str,
        selection: SelectionKind,
        requires: &[&str],
        demand: (u32, u32, u32),
    ) -> Program {
        Program::new(
            code(raw_code),
            format!("Program {raw_code}"),
            demand.2,
            demand.0,
            selection,
            vec![PriorityGroup {
                name: "group".to_owned(),
                requires: requires
                    .iter()
                    .map(|r| Attribute::parse(r).expect("valid"))
                    .collect(),
                demand: GroupDemand::new(demand.0, demand.1, Some(demand.2)).expect("valid"),
            }],
            None,
            None,
        )
        .expect("valid")
    }

    fn student(ranking: &[&str], lottery: Option<&str>, test: Option<f64>) -> Student {
        Student {
            attributes: ["d15", "mark_twain_test"]
                .iter()
                .map(|a| Attribute::parse(a).expect("valid"))
                .collect::<BTreeSet<_>>(),
            ranking: Ranking::new(ranking.iter().map(|c| code(c)).collect()).expect("valid"),
            lottery_number: lottery.map(|l| LotteryPrefix::parse(l).expect("valid")),
            talent_test_percentile: test.map(|t| UnitInterval::new(t).expect("valid")),
            talent_test_sd: StandardDeviation::ZERO,
        }
    }

    fn parameters(trials: u32, seed: u64) -> Parameters {
        Parameters {
            trials: Trials::new(trials).expect("valid"),
            seed: Seed::new(seed),
            demand_sd: StandardDeviation::ZERO,
            attrition: Attrition::ONE,
            waitlist_numbers: WaitlistNumbers::Redrawn,
        }
    }

    #[test]
    fn trials_are_bounded() {
        assert_eq!(Trials::new(0), Err(InvalidTrials::Zero));
        assert_eq!(
            Trials::new(MAX_TRIALS + 1),
            Err(InvalidTrials::TooMany(MAX_TRIALS + 1))
        );
        assert_eq!(Trials::new(MAX_TRIALS).map(Trials::get), Ok(MAX_TRIALS));
    }

    #[test]
    fn standard_deviation_and_attrition_are_validated() {
        assert_eq!(
            StandardDeviation::new(-1.0),
            Err(InvalidStandardDeviation::Negative(-1.0))
        );
        assert!(matches!(
            StandardDeviation::new(f64::INFINITY),
            Err(InvalidStandardDeviation::NotFinite(_))
        ));
        assert_eq!(Attrition::new(0.0), Err(InvalidAttrition::NotPositive(0.0)));
        assert!(matches!(
            Attrition::new(f64::NAN),
            Err(InvalidAttrition::NotFinite(_))
        ));
        assert_eq!(Attrition::new(1.3).map(Attrition::get), Ok(1.3));
    }

    #[test]
    fn unknown_program_in_ranking_is_an_error() {
        let programs = ProgramDefinitions::new(vec![]).expect("valid");
        let err = simulate(&programs, &student(&["K1"], None, None), &parameters(10, 0));
        assert_eq!(err, Err(SimulationError::UnknownProgram(code("K1"))));
    }

    #[test]
    fn same_seed_gives_the_same_report() {
        let programs = ProgramDefinitions::new(vec![program(
            "K1",
            SelectionKind::Lottery,
            &["d15"],
            (230, 140, 94),
        )])
        .expect("valid");
        let noisy = Parameters {
            demand_sd: StandardDeviation::new(0.15).expect("valid"),
            ..parameters(2_000, 7)
        };
        let s = student(&["K1"], None, None);
        let first = simulate(&programs, &s, &noisy).expect("runs");
        let second = simulate(&programs, &s, &noisy).expect("runs");
        assert_eq!(first, second);
        let other_seed = Parameters {
            seed: Seed::new(8),
            ..noisy
        };
        let third = simulate(&programs, &s, &other_seed).expect("runs");
        assert_ne!(first.rows[0].p_initial, third.rows[0].p_initial);
    }

    #[test]
    fn a_best_number_always_clears_a_group_that_offered_everyone() {
        let programs = ProgramDefinitions::new(vec![program(
            "K1",
            SelectionKind::Lottery,
            &["d15"],
            (10, 10, 10),
        )])
        .expect("valid");
        let report = simulate(
            &programs,
            &student(&["K1"], Some("00"), None),
            &parameters(500, 1),
        )
        .expect("runs");
        assert_eq!(report.rows[0].p_initial, 1.0);
        assert_eq!(report.rows[0].p_waitlist, 0.0);
        assert_eq!(report.summary.p_initial_offer, 1.0);
        assert_eq!(report.summary.p_no_offer, 0.0);
        assert_eq!(report.summary.p_lottery_offer, Some(1.0));
        assert_eq!(report.summary.p_screened_offer, None);
        assert_eq!(report.lottery_fraction, Some(0.0));
    }

    #[test]
    fn an_ineligible_program_never_offers() {
        let programs = ProgramDefinitions::new(vec![program(
            "K1",
            SelectionKind::Lottery,
            &["sibling_K1"],
            (10, 10, 10),
        )])
        .expect("valid");
        let report = simulate(
            &programs,
            &student(&["K1"], Some("00"), None),
            &parameters(200, 1),
        )
        .expect("runs");
        assert_eq!(report.rows[0].group_name, None);
        assert_eq!(report.rows[0].initial_rate, 0.0);
        assert_eq!(report.rows[0].p_any, 0.0);
        assert_eq!(report.summary.p_no_offer, 1.0);
    }

    #[test]
    fn waitlists_exist_only_above_the_initial_match() {
        // With a median lottery number (prefix 80 = 0.5): K1 made no
        // initial-round offers, so its Beta(1, 101) cutoff never reaches 0.5,
        // but about half of its group got a waitlist offer. K2 offered every
        // one of 1000 applicants, so its Beta(1001, 1) cutoff is above 0.5.
        let programs = ProgramDefinitions::new(vec![
            program("K1", SelectionKind::Lottery, &["d15"], (100, 50, 0)),
            program("K2", SelectionKind::Lottery, &["d15"], (1000, 1000, 1000)),
        ])
        .expect("valid");
        let above = simulate(
            &programs,
            &student(&["K1", "K2"], Some("80"), None),
            &parameters(2_000, 3),
        )
        .expect("runs");
        assert_eq!(above.rows[0].p_initial, 0.0);
        assert_eq!(above.rows[1].p_initial, 1.0);
        assert!(above.rows[0].p_waitlist > 0.3);
        assert_eq!(above.summary.p_any_offer, 1.0);

        let below = simulate(
            &programs,
            &student(&["K2", "K1"], Some("80"), None),
            &parameters(2_000, 3),
        )
        .expect("runs");
        assert_eq!(below.rows[0].p_initial, 1.0);
        assert_eq!(below.rows[1].p_waitlist, 0.0);
    }

    #[test]
    fn a_reused_bad_number_stays_bad_on_the_waitlist() {
        // Prefix f0 = 0.9375. K1 made no initial offers (cutoff near 0.01)
        // and about half of its remaining group got a waitlist offer. A
        // redrawn number reaches the waitlist about half the time; the reused
        // number sits at position 0.93 among the remaining applicants, far
        // behind a Beta(51, 51) waitlist cutoff.
        let programs = ProgramDefinitions::new(vec![program(
            "K1",
            SelectionKind::Lottery,
            &["d15"],
            (100, 50, 0),
        )])
        .expect("valid");
        let s = student(&["K1"], Some("f0"), None);
        let redrawn = simulate(&programs, &s, &parameters(2_000, 4)).expect("runs");
        assert!(redrawn.rows[0].p_waitlist > 0.3, "{redrawn:?}");
        let reused = simulate(
            &programs,
            &s,
            &Parameters {
                waitlist_numbers: WaitlistNumbers::Reused,
                ..parameters(2_000, 4)
            },
        )
        .expect("runs");
        assert_eq!(reused.rows[0].p_initial, 0.0);
        assert!(reused.rows[0].p_waitlist < 0.01, "{reused:?}");
    }

    #[test]
    fn a_top_test_score_clears_a_screened_program() {
        let programs = ProgramDefinitions::new(vec![program(
            "K239SC",
            SelectionKind::Screened,
            &["mark_twain_test"],
            (190, 30, 27),
        )])
        .expect("valid");
        let report = simulate(
            &programs,
            &student(&["K239SC"], None, Some(0.0)),
            &parameters(300, 5),
        )
        .expect("runs");
        assert_eq!(report.rows[0].p_initial, 1.0);
        assert_eq!(report.summary.p_screened_offer, Some(1.0));
        assert_eq!(report.summary.p_lottery_offer, None);
    }

    #[test]
    fn probabilities_are_consistent() {
        let programs = ProgramDefinitions::new(vec![
            program("K1", SelectionKind::Lottery, &["d15"], (370, 40, 34)),
            program(
                "K2",
                SelectionKind::Screened,
                &["mark_twain_test"],
                (540, 110, 101),
            ),
            program("K3", SelectionKind::Lottery, &["d15"], (230, 140, 94)),
        ])
        .expect("valid");
        let noisy = Parameters {
            demand_sd: StandardDeviation::new(0.15).expect("valid"),
            attrition: Attrition::new(1.2).expect("valid"),
            ..parameters(5_000, 11)
        };
        let report =
            simulate(&programs, &student(&["K1", "K2", "K3"], None, None), &noisy).expect("runs");
        for row in &report.rows {
            assert!(row.p_any >= row.p_initial, "{row:?}");
            assert!(row.p_any >= row.p_waitlist, "{row:?}");
            assert!(
                row.p_any <= row.p_initial + row.p_waitlist + 1e-12,
                "{row:?}"
            );
        }
        let summary = report.summary;
        assert!((summary.p_no_offer - (1.0 - summary.p_any_offer)).abs() < 1e-12);
        assert!(summary.p_any_offer >= summary.p_initial_offer);
        assert!(summary.p_initial_offer > 0.0 && summary.p_initial_offer < 1.0);
        assert_eq!(report.lottery_fraction, None);
    }
}
