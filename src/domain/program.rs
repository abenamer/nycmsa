//! Programs, priority groups, and last year's demand figures.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A MySchools program code such as `K442S`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProgramCode(String);

/// Error from [`ProgramCode::parse`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidProgramCode {
    #[error("program code is empty")]
    Empty,
    #[error("program code must be ASCII letters and digits, got {0:?}")]
    NotAlphanumeric(String),
}

impl ProgramCode {
    pub fn parse(raw: &str) -> Result<Self, InvalidProgramCode> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(InvalidProgramCode::Empty);
        }
        if !trimmed.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(InvalidProgramCode::NotAlphanumeric(raw.to_owned()));
        }
        Ok(Self(trimmed.to_ascii_uppercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProgramCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&self.0)
    }
}

/// An eligibility attribute that a student holds, such as `d15`, `frl`,
/// `sibling_K442S`, or `mark_twain_test`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Attribute(String);

/// Error from [`Attribute::parse`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidAttribute {
    #[error("attribute is empty")]
    Empty,
    #[error("attribute must not contain whitespace, got {0:?}")]
    ContainsWhitespace(String),
}

impl Attribute {
    pub fn parse(raw: &str) -> Result<Self, InvalidAttribute> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(InvalidAttribute::Empty);
        }
        if trimmed.chars().any(char::is_whitespace) {
            return Err(InvalidAttribute::ContainsWhitespace(raw.to_owned()));
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&self.0)
    }
}

/// Last year's figures for one priority group.
///
/// Invariant: `initial_offers <= offers <= applicants`. Published offers
/// include waitlist offers, so `offers - initial_offers` is the estimated
/// number of waitlist offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupDemand {
    applicants: u32,
    offers: u32,
    initial_offers: u32,
}

/// Error from [`GroupDemand::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidGroupDemand {
    #[error("offers ({offers}) exceed applicants ({applicants})")]
    OffersExceedApplicants { applicants: u32, offers: u32 },
    #[error("initial offers ({initial_offers}) exceed offers ({offers})")]
    InitialOffersExceedOffers { offers: u32, initial_offers: u32 },
}

impl GroupDemand {
    /// Builds the figures. When `initial_offers` is `None`, every offer
    /// counts as an initial-round offer.
    pub fn new(
        applicants: u32,
        offers: u32,
        initial_offers: Option<u32>,
    ) -> Result<Self, InvalidGroupDemand> {
        if offers > applicants {
            return Err(InvalidGroupDemand::OffersExceedApplicants { applicants, offers });
        }
        let initial_offers = initial_offers.unwrap_or(offers);
        if initial_offers > offers {
            return Err(InvalidGroupDemand::InitialOffersExceedOffers {
                offers,
                initial_offers,
            });
        }
        Ok(Self {
            applicants,
            offers,
            initial_offers,
        })
    }

    pub fn applicants(self) -> u32 {
        self.applicants
    }

    pub fn offers(self) -> u32 {
        self.offers
    }

    pub fn initial_offers(self) -> u32 {
        self.initial_offers
    }

    /// Waitlist offers: published offers minus initial-round offers.
    pub fn waitlist_offers(self) -> u32 {
        self.offers - self.initial_offers
    }

    /// Applicants who did not receive an initial-round offer.
    pub fn remaining_applicants(self) -> u32 {
        self.applicants - self.initial_offers
    }

    /// Share of the group matched in the initial round. Zero when the group
    /// had no applicants.
    pub fn initial_rate(self) -> f64 {
        if self.applicants == 0 {
            return 0.0;
        }
        f64::from(self.initial_offers) / f64::from(self.applicants)
    }

    /// Share of the remaining group reached by the waitlist.
    pub fn waitlist_rate(self) -> f64 {
        f64::from(self.waitlist_offers()) / f64::from(self.remaining_applicants().max(1))
    }
}

/// One priority group of a program. A student falls into the first group
/// whose requirements they satisfy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorityGroup {
    pub name: String,
    pub requires: Vec<Attribute>,
    pub demand: GroupDemand,
}

impl PriorityGroup {
    /// True when the student holds every required attribute.
    pub fn admits(&self, attributes: &BTreeSet<Attribute>) -> bool {
        self.requires.iter().all(|r| attributes.contains(r))
    }
}

/// How the program ranks applicants within a priority group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    /// Ranked by the shared random lottery number.
    Lottery,
    /// Ranked by a talent test; modelled as an independent percentile draw.
    Screened,
}

impl SelectionKind {
    pub fn label(self) -> &'static str {
        match self {
            SelectionKind::Lottery => "lottery",
            SelectionKind::Screened => "screened",
        }
    }
}

/// A middle school program with its priority groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    code: ProgramCode,
    name: String,
    seats: u32,
    applicants: u32,
    selection: SelectionKind,
    groups: Vec<PriorityGroup>,
    admissions_method: Option<String>,
    note: Option<String>,
}

/// Error from [`Program::new`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidProgram {
    #[error("program {0} has an empty name")]
    EmptyName(ProgramCode),
    #[error("program {code} has a priority group with an empty name")]
    EmptyGroupName { code: ProgramCode },
}

impl Program {
    /// Builds a program. The eight named inputs mirror one entry of
    /// `programs.json`; a builder would only hide them.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        code: ProgramCode,
        name: String,
        seats: u32,
        applicants: u32,
        selection: SelectionKind,
        groups: Vec<PriorityGroup>,
        admissions_method: Option<String>,
        note: Option<String>,
    ) -> Result<Self, InvalidProgram> {
        if name.trim().is_empty() {
            return Err(InvalidProgram::EmptyName(code));
        }
        if groups.iter().any(|g| g.name.trim().is_empty()) {
            return Err(InvalidProgram::EmptyGroupName { code });
        }
        Ok(Self {
            code,
            name,
            seats,
            applicants,
            selection,
            groups,
            admissions_method,
            note,
        })
    }

    pub fn code(&self) -> &ProgramCode {
        &self.code
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn seats(&self) -> u32 {
        self.seats
    }

    pub fn applicants(&self) -> u32 {
        self.applicants
    }

    pub fn selection(&self) -> SelectionKind {
        self.selection
    }

    pub fn groups(&self) -> &[PriorityGroup] {
        &self.groups
    }

    pub fn admissions_method(&self) -> Option<&str> {
        self.admissions_method.as_deref()
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// The first priority group that admits the student, if any.
    pub fn pick_group(&self, attributes: &BTreeSet<Attribute>) -> Option<&PriorityGroup> {
        self.groups.iter().find(|g| g.admits(attributes))
    }
}

/// The set of programs a student can rank, keyed by code.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgramDefinitions {
    programs: BTreeMap<ProgramCode, Program>,
}

/// Error from [`ProgramDefinitions::new`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("program code {0} is defined more than once")]
pub struct DuplicateProgramCode(pub ProgramCode);

impl ProgramDefinitions {
    pub fn new(programs: Vec<Program>) -> Result<Self, DuplicateProgramCode> {
        let mut map = BTreeMap::new();
        for program in programs {
            let code = program.code.clone();
            if map.insert(code.clone(), program).is_some() {
                return Err(DuplicateProgramCode(code));
            }
        }
        Ok(Self { programs: map })
    }

    pub fn get(&self, code: &ProgramCode) -> Option<&Program> {
        self.programs.get(code)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Program> {
        self.programs.values()
    }

    pub fn len(&self) -> usize {
        self.programs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(list: &[&str]) -> BTreeSet<Attribute> {
        list.iter()
            .map(|a| Attribute::parse(a).expect("valid"))
            .collect()
    }

    fn group(name: &str, requires: &[&str], demand: (u32, u32, u32)) -> PriorityGroup {
        PriorityGroup {
            name: name.to_owned(),
            requires: requires
                .iter()
                .map(|r| Attribute::parse(r).expect("valid"))
                .collect(),
            demand: GroupDemand::new(demand.0, demand.1, Some(demand.2)).expect("valid"),
        }
    }

    #[test]
    fn program_code_is_trimmed_and_uppercased() {
        let code = ProgramCode::parse(" k442s ").expect("valid");
        assert_eq!(code.as_str(), "K442S");
    }

    #[test]
    fn program_code_rejects_empty_and_punctuation() {
        assert_eq!(ProgramCode::parse("  "), Err(InvalidProgramCode::Empty));
        assert_eq!(
            ProgramCode::parse("K-442"),
            Err(InvalidProgramCode::NotAlphanumeric("K-442".to_owned()))
        );
    }

    #[test]
    fn attribute_rejects_whitespace() {
        assert_eq!(Attribute::parse(""), Err(InvalidAttribute::Empty));
        assert_eq!(
            Attribute::parse("mark twain"),
            Err(InvalidAttribute::ContainsWhitespace(
                "mark twain".to_owned()
            ))
        );
    }

    #[test]
    fn demand_defaults_initial_offers_to_offers() {
        let demand = GroupDemand::new(100, 40, None).expect("valid");
        assert_eq!(demand.initial_offers(), 40);
        assert_eq!(demand.waitlist_offers(), 0);
    }

    #[test]
    fn demand_rejects_inverted_figures() {
        assert_eq!(
            GroupDemand::new(10, 20, None),
            Err(InvalidGroupDemand::OffersExceedApplicants {
                applicants: 10,
                offers: 20
            })
        );
        assert_eq!(
            GroupDemand::new(100, 20, Some(30)),
            Err(InvalidGroupDemand::InitialOffersExceedOffers {
                offers: 20,
                initial_offers: 30
            })
        );
    }

    #[test]
    fn demand_rates_match_the_python_formulas() {
        let demand = GroupDemand::new(230, 140, Some(94)).expect("valid");
        assert!((demand.initial_rate() - 94.0 / 230.0).abs() < 1e-12);
        assert!((demand.waitlist_rate() - 46.0 / 136.0).abs() < 1e-12);
        let empty = GroupDemand::new(0, 0, None).expect("valid");
        assert_eq!(empty.initial_rate(), 0.0);
        assert_eq!(empty.waitlist_rate(), 0.0);
    }

    #[test]
    fn pick_group_returns_the_first_match() {
        let program = Program::new(
            ProgramCode::parse("K442S").expect("valid"),
            "M.S. 442".to_owned(),
            81,
            494,
            SelectionKind::Lottery,
            vec![
                group("siblings, FRL", &["sibling_K442S", "frl"], (5, 5, 5)),
                group("District 15, FRL", &["d15", "frl"], (50, 40, 37)),
                group("District 15, non-FRL", &["d15"], (370, 40, 34)),
                group("NYC residents", &[], (60, 0, 0)),
            ],
            Some("Open".to_owned()),
            None,
        )
        .expect("valid");
        let picked = program
            .pick_group(&attrs(&["d15", "d20"]))
            .expect("eligible");
        assert_eq!(picked.name, "District 15, non-FRL");
        let frl = program
            .pick_group(&attrs(&["frl", "d15"]))
            .expect("eligible");
        assert_eq!(frl.name, "District 15, FRL");
        let other = program.pick_group(&attrs(&["d20"])).expect("eligible");
        assert_eq!(other.name, "NYC residents");
    }

    #[test]
    fn pick_group_is_none_without_an_open_group() {
        let program = Program::new(
            ProgramCode::parse("K239SC").expect("valid"),
            "Mark Twain".to_owned(),
            27,
            191,
            SelectionKind::Screened,
            vec![group("talent test", &["mark_twain_test"], (190, 30, 27))],
            None,
            None,
        )
        .expect("valid");
        assert!(program.pick_group(&attrs(&["d15"])).is_none());
    }

    #[test]
    fn program_rejects_empty_names() {
        let code = ProgramCode::parse("K1").expect("valid");
        let err = Program::new(
            code.clone(),
            "  ".to_owned(),
            1,
            1,
            SelectionKind::Lottery,
            vec![],
            None,
            None,
        );
        assert_eq!(err, Err(InvalidProgram::EmptyName(code)));
    }

    #[test]
    fn definitions_reject_duplicate_codes() {
        let make = || {
            Program::new(
                ProgramCode::parse("K1").expect("valid"),
                "One".to_owned(),
                1,
                1,
                SelectionKind::Lottery,
                vec![],
                None,
                None,
            )
            .expect("valid")
        };
        let err = ProgramDefinitions::new(vec![make(), make()]);
        assert_eq!(
            err,
            Err(DuplicateProgramCode(
                ProgramCode::parse("K1").expect("valid")
            ))
        );
    }
}
