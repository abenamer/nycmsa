//! The applicant: eligibility attributes, ranked list, and known draws.

use std::collections::BTreeSet;

use super::lottery::{LotteryPrefix, UnitInterval};
use super::program::{Attribute, ProgramCode};
use super::simulation::StandardDeviation;

/// The application list in preference order. It is non-empty and holds each
/// program code once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ranking(Vec<ProgramCode>);

/// Error from [`Ranking::new`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidRanking {
    #[error("the ranking is empty")]
    Empty,
    #[error("program {0} appears more than once in the ranking")]
    Duplicate(ProgramCode),
}

impl Ranking {
    pub fn new(codes: Vec<ProgramCode>) -> Result<Self, InvalidRanking> {
        if codes.is_empty() {
            return Err(InvalidRanking::Empty);
        }
        let mut seen = BTreeSet::new();
        for code in &codes {
            if !seen.insert(code) {
                return Err(InvalidRanking::Duplicate(code.clone()));
            }
        }
        Ok(Self(codes))
    }

    pub fn codes(&self) -> &[ProgramCode] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The student being simulated.
///
/// `lottery_number` is the known prefix of the DOE lottery number; `None`
/// draws a fresh number each trial. `talent_test_percentile` is the expected
/// rank among talent-test takers, `0` best; `None` draws it uniformly each
/// trial. `talent_test_sd` is trial-to-trial noise on that percentile.
#[derive(Debug, Clone, PartialEq)]
pub struct Student {
    pub attributes: BTreeSet<Attribute>,
    pub ranking: Ranking,
    pub lottery_number: Option<LotteryPrefix>,
    pub talent_test_percentile: Option<UnitInterval>,
    pub talent_test_sd: StandardDeviation,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(raw: &str) -> ProgramCode {
        ProgramCode::parse(raw).expect("valid")
    }

    #[test]
    fn ranking_rejects_empty_lists() {
        assert_eq!(Ranking::new(vec![]), Err(InvalidRanking::Empty));
    }

    #[test]
    fn ranking_rejects_duplicates() {
        let err = Ranking::new(vec![code("K1"), code("K2"), code("K1")]);
        assert_eq!(err, Err(InvalidRanking::Duplicate(code("K1"))));
    }

    #[test]
    fn ranking_keeps_order() {
        let ranking = Ranking::new(vec![code("K2"), code("K1")]).expect("valid");
        assert_eq!(ranking.codes(), &[code("K2"), code("K1")]);
        assert_eq!(ranking.len(), 2);
    }
}
