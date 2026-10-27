//! Scan of the MySchools catalog: which programs the student is eligible
//! for, and how reachable each one is for their lottery number.

use std::collections::BTreeSet;
use std::fmt;

use super::lottery::LotteryPrefix;
use super::program::{Attribute, GroupDemand, PriorityGroup, Program, ProgramCode, SelectionKind};

/// A DOE community school district code, such as `15`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DistrictCode(String);

/// Error from [`DistrictCode::parse`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("district code is empty")]
pub struct InvalidDistrictCode;

impl DistrictCode {
    /// Parses a code. MySchools writes `09` for a school and `District 9`
    /// in group names, so an all-digit code drops its leading zeros.
    pub fn parse(raw: &str) -> Result<Self, InvalidDistrictCode> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(InvalidDistrictCode);
        }
        if trimmed.chars().all(|c| c.is_ascii_digit()) {
            let digits = trimmed.trim_start_matches('0');
            return Ok(Self(if digits.is_empty() {
                "0".to_owned()
            } else {
                digits.to_owned()
            }));
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// The borough of a community school district.
    pub fn borough(&self) -> Option<Borough> {
        self.0.parse::<u32>().ok().and_then(Borough::of_district)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The attribute a resident of this district holds, such as `d15`.
    pub fn resident_attribute(&self) -> Attribute {
        Attribute::parse(&format!("d{}", self.0)).expect("a district code has no whitespace")
    }
}

impl fmt::Display for DistrictCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&self.0)
    }
}

/// A NYC borough. Community school districts 1–6 are Manhattan, 7–12 the
/// Bronx, 13–23 and 32 Brooklyn, 24–30 Queens, and 31 Staten Island.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Borough {
    Manhattan,
    Bronx,
    Brooklyn,
    Queens,
    StatenIsland,
}

impl Borough {
    pub const ALL: [Borough; 5] = [
        Borough::Manhattan,
        Borough::Bronx,
        Borough::Brooklyn,
        Borough::Queens,
        Borough::StatenIsland,
    ];

    pub fn of_district(district: u32) -> Option<Borough> {
        match district {
            1..=6 => Some(Borough::Manhattan),
            7..=12 => Some(Borough::Bronx),
            13..=23 | 32 => Some(Borough::Brooklyn),
            24..=30 => Some(Borough::Queens),
            31 => Some(Borough::StatenIsland),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Borough::Manhattan => "Manhattan",
            Borough::Bronx => "Bronx",
            Borough::Brooklyn => "Brooklyn",
            Borough::Queens => "Queens",
            Borough::StatenIsland => "Staten Island",
        }
    }

    /// The attribute a resident of this borough holds, such as
    /// `borough_brooklyn`.
    pub fn resident_attribute(self) -> Attribute {
        let slug = match self {
            Borough::Manhattan => "manhattan",
            Borough::Bronx => "bronx",
            Borough::Brooklyn => "brooklyn",
            Borough::Queens => "queens",
            Borough::StatenIsland => "staten_island",
        };
        Attribute::parse(&format!("borough_{slug}")).expect("a borough slug has no whitespace")
    }
}

/// Adds the borough attribute for every district attribute the student
/// holds, so a borough priority group can admit them.
pub fn with_boroughs(attributes: BTreeSet<Attribute>) -> BTreeSet<Attribute> {
    let boroughs: Vec<Attribute> = attributes
        .iter()
        .filter_map(|a| a.as_str().strip_prefix('d'))
        .filter_map(|d| d.parse::<u32>().ok())
        .filter_map(Borough::of_district)
        .map(Borough::resident_attribute)
        .collect();
    let mut all = attributes;
    all.extend(boroughs);
    all
}

/// The eligibility rule behind a MySchools priority-group name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PriorityCriterion {
    Sibling,
    Continuing,
    Zoned,
    CurrentStudent,
    District(DistrictCode),
    Borough(Borough),
    AnyNycResident,
    /// The name did not match a known rule. No student is eligible.
    Unrecognized,
}

impl PriorityCriterion {
    /// True when the student's attributes satisfy the rule. Borough rules
    /// read the district attributes.
    pub fn admits(&self, attributes: &BTreeSet<Attribute>) -> bool {
        let has = |name: &str| attributes.iter().any(|a| a.as_str() == name);
        match self {
            PriorityCriterion::Sibling => has("sibling"),
            PriorityCriterion::Continuing => has("continuing"),
            PriorityCriterion::Zoned => has("zoned"),
            PriorityCriterion::CurrentStudent => has("current_student"),
            PriorityCriterion::District(code) => attributes.contains(&code.resident_attribute()),
            PriorityCriterion::Borough(borough) => {
                with_boroughs(attributes.clone()).contains(&borough.resident_attribute())
            }
            PriorityCriterion::AnyNycResident => true,
            PriorityCriterion::Unrecognized => false,
        }
    }

    /// The attributes a simulation program group requires for this rule.
    /// Per-program rules (sibling, zone, continuing, attending) take the
    /// program code so they apply to that program only.
    pub fn required_attributes(&self, program: &ProgramCode) -> Vec<Attribute> {
        let one = |raw: String| vec![Attribute::parse(&raw).expect("no whitespace")];
        match self {
            PriorityCriterion::Sibling => one(format!("sibling_{program}")),
            PriorityCriterion::Continuing => one(format!("continuing_{program}")),
            PriorityCriterion::Zoned => one(format!("zoned_{program}")),
            PriorityCriterion::CurrentStudent => one(format!("attends_{program}")),
            PriorityCriterion::District(code) => vec![code.resident_attribute()],
            PriorityCriterion::Borough(borough) => vec![borough.resident_attribute()],
            PriorityCriterion::AnyNycResident => vec![],
            PriorityCriterion::Unrecognized => one("unrecognized_priority_group".to_owned()),
        }
    }
}

/// Applicants and offers for one slice of a priority group. `offers` is
/// `None` when the source text gives applicants but no offer count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DemandPair {
    pub applicants: u32,
    pub offers: Option<u32>,
}

impl DemandPair {
    /// Offers divided by applicants, when both are known and applicants is
    /// not zero.
    pub fn share(self) -> Option<f64> {
        let offers = self.offers?;
        if self.applicants == 0 {
            return None;
        }
        Some(f64::from(offers) / f64::from(self.applicants))
    }
}

/// The figures parsed from a group's description, split by free or reduced
/// price lunch (FRL) eligibility where the text gives that split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GroupDemandText {
    pub frl: Option<DemandPair>,
    pub non_frl: Option<DemandPair>,
    pub all: Option<DemandPair>,
}

impl GroupDemandText {
    /// The slice that applies to the student: the FRL or non-FRL slice when
    /// given, else the whole group.
    pub fn slice_for(&self, frl: bool) -> Option<DemandPair> {
        if frl {
            self.frl.or(self.all)
        } else {
            self.non_frl.or(self.all)
        }
    }
}

/// One priority group of a catalog program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogGroup {
    pub order: u32,
    pub name: String,
    pub criterion: PriorityCriterion,
    pub demand: GroupDemandText,
}

/// State test results for one school, as the share of students at
/// proficiency (levels 3 and 4) in the latest published year.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SchoolScores {
    pub year: u32,
    pub ela_proficient_pct: Option<f64>,
    pub math_proficient_pct: Option<f64>,
}

/// Test results keyed by school DBN, such as `15K442`.
pub type TestResults = std::collections::BTreeMap<String, SchoolScores>;

/// One subject's proficiency share per DBN for one test year. `None`
/// marks a figure the DOE suppressed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SubjectResults {
    pub year: u32,
    pub by_dbn: std::collections::BTreeMap<String, Option<f64>>,
}

/// Combines the ELA and Math subjects. Each school takes the later of the
/// two years; a subject from an earlier year reads as unknown.
pub fn combine_subjects(ela: &SubjectResults, math: &SubjectResults) -> TestResults {
    let year = ela.year.max(math.year);
    let dbns: BTreeSet<&String> = ela.by_dbn.keys().chain(math.by_dbn.keys()).collect();
    dbns.into_iter()
        .map(|dbn| {
            let pick = |subject: &SubjectResults| {
                if subject.year == year {
                    subject.by_dbn.get(dbn).copied().flatten()
                } else {
                    None
                }
            };
            (
                dbn.clone(),
                SchoolScores {
                    year,
                    ela_proficient_pct: pick(ela),
                    math_proficient_pct: pick(math),
                },
            )
        })
        .collect()
}

/// A point on the map, WGS 84 degrees, as the DOE's geocoder reports a
/// school's address.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoPoint {
    pub latitude: f64,
    pub longitude: f64,
}

/// A program as listed in the MySchools catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogProgram {
    pub district: DistrictCode,
    /// The school's DBN, such as `15K442`.
    pub dbn: String,
    /// Where the school is, when the catalog says.
    pub location: Option<GeoPoint>,
    /// The school's street address as MySchools prints it.
    pub address: Option<String>,
    pub school_name: String,
    pub code: ProgramCode,
    pub program_name: String,
    pub method: String,
    pub seats: Option<u32>,
    pub applicants: Option<u32>,
    pub all_seats_filled: Option<bool>,
    groups: Vec<CatalogGroup>,
}

impl CatalogProgram {
    /// Builds the program and sorts its groups by `order`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        district: DistrictCode,
        dbn: String,
        school_name: String,
        code: ProgramCode,
        program_name: String,
        method: String,
        seats: Option<u32>,
        applicants: Option<u32>,
        all_seats_filled: Option<bool>,
        mut groups: Vec<CatalogGroup>,
    ) -> Self {
        groups.sort_by_key(|g| g.order);
        Self {
            district,
            dbn,
            location: None,
            address: None,
            school_name,
            code,
            program_name,
            method,
            seats,
            applicants,
            all_seats_filled,
            groups,
        }
    }

    pub fn with_location(mut self, location: Option<GeoPoint>) -> Self {
        self.location = location;
        self
    }

    pub fn with_address(mut self, address: Option<String>) -> Self {
        self.address = address
            .map(|a| a.trim().to_owned())
            .filter(|a| !a.is_empty());
        self
    }

    pub fn groups(&self) -> &[CatalogGroup] {
        &self.groups
    }

    /// The first group that admits the student.
    pub fn eligible_group(&self, attributes: &BTreeSet<Attribute>) -> Option<&CatalogGroup> {
        self.groups.iter().find(|g| g.criterion.admits(attributes))
    }

    /// True when MySchools published no seats and no applicants.
    pub fn has_no_demand_figures(&self) -> bool {
        self.seats.unwrap_or(0) == 0 && self.applicants.unwrap_or(0) == 0
    }

    /// Open and zone-priority programs rank by lottery number. Every other
    /// method ranks by a screen, audition, test, or language criterion.
    pub fn ranks_by_lottery(&self) -> bool {
        let method = self.method.to_lowercase();
        method == "open" || method == "zone priority"
    }
}

/// A program's focus, guessed from its name and admissions method because
/// MySchools publishes no interest areas for middle school programs. A
/// program can carry several.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProgramFocus {
    Stem,
    Arts,
    Language,
    Humanities,
    Careers,
    Honors,
    Zoned,
    General,
}

impl ProgramFocus {
    pub const ALL: [ProgramFocus; 8] = [
        ProgramFocus::Stem,
        ProgramFocus::Arts,
        ProgramFocus::Language,
        ProgramFocus::Humanities,
        ProgramFocus::Careers,
        ProgramFocus::Honors,
        ProgramFocus::Zoned,
        ProgramFocus::General,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            ProgramFocus::Stem => "stem",
            ProgramFocus::Arts => "arts",
            ProgramFocus::Language => "language",
            ProgramFocus::Humanities => "humanities",
            ProgramFocus::Careers => "careers",
            ProgramFocus::Honors => "honors",
            ProgramFocus::Zoned => "zoned",
            ProgramFocus::General => "general",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ProgramFocus::Stem => "STEM",
            ProgramFocus::Arts => "Arts",
            ProgramFocus::Language => "Dual language",
            ProgramFocus::Humanities => "Humanities & leadership",
            ProgramFocus::Careers => "Business & careers",
            ProgramFocus::Honors => "Honors & accelerated",
            ProgramFocus::Zoned => "Zoned",
            ProgramFocus::General => "General",
        }
    }
}

/// How a keyword matches the word tokens of a text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keyword {
    /// A whole word.
    Word(&'static str),
    /// The start of a word: `scien` matches science and scientific.
    Prefix(&'static str),
    /// Two consecutive words.
    Phrase(&'static str, &'static str),
}

use Keyword::{Phrase, Prefix, Word};

const STEM_KEYWORDS: [Keyword; 9] = [
    Word("stem"),
    Word("steam"),
    Prefix("scien"),
    Prefix("math"),
    Prefix("technolog"),
    Prefix("engineer"),
    Prefix("comput"),
    Prefix("robot"),
    Word("coding"),
];
const ARTS_KEYWORDS: [Keyword; 15] = [
    Word("art"),
    Word("arts"),
    Prefix("music"),
    Prefix("danc"),
    Prefix("theat"),
    Word("drama"),
    Prefix("perform"),
    Word("visual"),
    Word("creative"),
    Word("design"),
    Word("film"),
    Word("media"),
    Word("chorus"),
    Word("orchestra"),
    Word("choir"),
];
const LANGUAGE_KEYWORDS: [Keyword; 15] = [
    Phrase("dual", "language"),
    Word("bilingual"),
    Word("spanish"),
    Word("french"),
    Word("mandarin"),
    Word("chinese"),
    Word("italian"),
    Word("arabic"),
    Word("russian"),
    Word("bengali"),
    Word("haitian"),
    Word("creole"),
    Word("korean"),
    Word("hebrew"),
    Word("japanese"),
];
const HUMANITIES_KEYWORDS: [Keyword; 14] = [
    Word("humanities"),
    Word("leadership"),
    Word("law"),
    Word("justice"),
    Word("civic"),
    Word("civics"),
    Word("global"),
    Word("writing"),
    Word("literature"),
    Word("history"),
    Word("international"),
    Word("journalism"),
    Word("debate"),
    Word("government"),
];
const CAREERS_KEYWORDS: [Keyword; 7] = [
    Word("business"),
    Word("career"),
    Word("careers"),
    Prefix("entrepreneur"),
    Word("finance"),
    Word("vocational"),
    Word("technical"),
];
const HONORS_KEYWORDS: [Keyword; 7] = [
    Word("honors"),
    Word("gifted"),
    Word("superintendent"),
    Word("scholars"),
    Word("accelerated"),
    Word("talented"),
    Word("algebra"),
];
const ZONED_KEYWORDS: [Keyword; 1] = [Word("zoned")];

/// Subject names that contain a keyword without meaning the kind:
/// "English Language Arts" is not an arts focus. They are removed before
/// matching.
const SUBJECT_PHRASES: [&str; 2] = ["language arts", "social studies"];

/// Lowercase word tokens: runs of letters and digits, after the subject
/// phrases are removed.
fn tokens(text: &str) -> Vec<String> {
    let mut lower = text.to_lowercase();
    for phrase in SUBJECT_PHRASES {
        lower = lower.replace(phrase, " ");
    }
    lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

impl Keyword {
    fn matches(self, tokens: &[String]) -> bool {
        match self {
            Word(w) => tokens.iter().any(|t| t == w),
            Prefix(p) => tokens.iter().any(|t| t.starts_with(p)),
            Phrase(a, b) => tokens.windows(2).any(|w| w[0] == a && w[1] == b),
        }
    }

    fn text(self) -> String {
        match self {
            Word(w) | Prefix(w) => w.to_owned(),
            Phrase(a, b) => format!("{a} {b}"),
        }
    }
}

/// The weight of a match by the text it was found in. Texts about the
/// program itself count most; the school overview describes the whole
/// school and counts least.
pub fn source_weight(label: &str) -> f64 {
    match label {
        "admissions method" => 0.7,
        "program name" => 0.6,
        "program description" => 0.45,
        "school name" => 0.5,
        "academic opportunities" => 0.25,
        "school overview" => 0.15,
        _ => 0.1,
    }
}

/// A kind becomes a tag at this confidence.
pub const TAG_THRESHOLD: f64 = 0.5;

/// One piece of text about a program, with where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileText {
    /// `program name`, `program description`, `school name`,
    /// `school overview`, or `academic opportunities`.
    pub label: String,
    pub text: String,
}

/// One keyword match: the keyword, the text it was found in, and the
/// weight that text carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Evidence {
    pub focus: ProgramFocus,
    pub keyword: String,
    pub label: String,
    pub weight: f64,
}

/// A kind with its confidence, `0` to `1`: one minus the product of
/// `1 - weight` over its distinct matches. One strong match or several
/// weak ones both raise it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusScore {
    pub focus: ProgramFocus,
    pub confidence: f64,
}

/// What a program is about, as read from MySchools and classified by rule.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgramProfile {
    pub code: ProgramCode,
    pub dbn: String,
    pub program_name: String,
    pub school_name: String,
    pub method: String,
    /// The program's own description, when MySchools gives one.
    pub description: String,
    /// The school's overview text.
    pub overview: String,
    pub academic_opportunities: String,
    /// Course lists from the school page, shown but not matched: nearly
    /// every school lists art courses.
    pub art_courses: String,
    pub language_classes: String,
    /// Every kind with evidence, highest confidence first.
    pub scores: Vec<FocusScore>,
    pub evidence: Vec<Evidence>,
}

impl ProgramProfile {
    /// The kinds at or above [`TAG_THRESHOLD`], or `General` when none is.
    pub fn tags(&self) -> Vec<ProgramFocus> {
        tags_of(&self.scores)
    }

    /// The texts the classification reads, rebuilt from the stored fields.
    pub fn texts(&self) -> Vec<ProfileText> {
        let text = |label: &str, text: &str| ProfileText {
            label: label.to_owned(),
            text: text.to_owned(),
        };
        vec![
            text("program name", &self.program_name),
            text("program description", &self.description),
            text("school name", &self.school_name),
            text("school overview", &self.overview),
            text("academic opportunities", &self.academic_opportunities),
        ]
    }

    /// Re-runs the classification on the stored texts.
    pub fn reclassified(mut self) -> Self {
        let (scores, evidence) = classify_profile(&self.texts(), &self.method);
        self.scores = scores;
        self.evidence = evidence;
        self
    }
}

/// Scores every kind from the texts MySchools gives about a program. The
/// admissions method counts as a text labelled `admissions method`.
pub fn classify_profile(texts: &[ProfileText], method: &str) -> (Vec<FocusScore>, Vec<Evidence>) {
    let rules: [(ProgramFocus, &[Keyword]); 7] = [
        (ProgramFocus::Stem, &STEM_KEYWORDS),
        (ProgramFocus::Arts, &ARTS_KEYWORDS),
        (ProgramFocus::Language, &LANGUAGE_KEYWORDS),
        (ProgramFocus::Humanities, &HUMANITIES_KEYWORDS),
        (ProgramFocus::Careers, &CAREERS_KEYWORDS),
        (ProgramFocus::Honors, &HONORS_KEYWORDS),
        (ProgramFocus::Zoned, &ZONED_KEYWORDS),
    ];
    let mut evidence = Vec::new();
    for text in texts {
        let words = tokens(&text.text);
        let weight = source_weight(&text.label);
        for (focus, keywords) in rules {
            for keyword in keywords {
                if keyword.matches(&words) {
                    evidence.push(Evidence {
                        focus,
                        keyword: keyword.text(),
                        label: text.label.clone(),
                        weight,
                    });
                }
            }
        }
    }
    let method_lower = method.to_lowercase();
    let method_focus = match method_lower.as_str() {
        "audition" | "talent test" => Some(ProgramFocus::Arts),
        "language criteria" => Some(ProgramFocus::Language),
        "zone priority" => Some(ProgramFocus::Zoned),
        _ => None,
    };
    if let Some(focus) = method_focus {
        evidence.push(Evidence {
            focus,
            keyword: method_lower,
            label: "admissions method".to_owned(),
            weight: source_weight("admissions method"),
        });
    }
    let mut scores: Vec<FocusScore> = ProgramFocus::ALL
        .iter()
        .filter_map(|&focus| {
            let miss: f64 = evidence
                .iter()
                .filter(|e| e.focus == focus)
                .map(|e| 1.0 - e.weight)
                .product();
            let matched = evidence.iter().any(|e| e.focus == focus);
            matched.then_some(FocusScore {
                focus,
                confidence: 1.0 - miss,
            })
        })
        .collect();
    scores.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    (scores, evidence)
}

/// The kinds a program would be tagged with from the given texts: those at
/// or above [`TAG_THRESHOLD`], or `General`.
pub fn tags_of(scores: &[FocusScore]) -> Vec<ProgramFocus> {
    let tags: Vec<ProgramFocus> = scores
        .iter()
        .filter(|s| s.confidence >= TAG_THRESHOLD)
        .map(|s| s.focus)
        .collect();
    if tags.is_empty() {
        vec![ProgramFocus::General]
    } else {
        tags
    }
}

/// Guesses the focus of a program from its names and admissions method
/// alone, for programs without a stored profile.
pub fn classify_focus(program_name: &str, school_name: &str, method: &str) -> Vec<ProgramFocus> {
    let texts = [
        ProfileText {
            label: "program name".to_owned(),
            text: program_name.to_owned(),
        },
        ProfileText {
            label: "school name".to_owned(),
            text: school_name.to_owned(),
        },
    ];
    tags_of(&classify_profile(&texts, method).0)
}

/// Input to [`scan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanRequest {
    pub attributes: BTreeSet<Attribute>,
    /// The student is eligible for free or reduced price lunch, is an
    /// English language learner, or lives in temporary housing.
    pub frl: bool,
    pub lottery: LotteryPrefix,
}

/// How reachable a program is for the student's lottery number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Every applicant in the group received an offer.
    AllOffered,
    /// The group's offer share is at least the student's lottery fraction.
    Likely,
    /// The offer share is at least 40% but below the lottery fraction.
    WaitlistMaybe,
    No,
    /// The program does not rank by lottery number.
    ByScreen,
    /// No figures, and all seats were filled.
    Unknown,
    /// No figures, and seats stayed open.
    SeatsOpen,
}

impl Reach {
    pub fn label(self) -> &'static str {
        match self {
            Reach::AllOffered => "YES all offered",
            Reach::Likely => "likely (reach incl. waitlist)",
            Reach::WaitlistMaybe => "waitlist maybe",
            Reach::No => "no",
            Reach::ByScreen => "by screen",
            Reach::Unknown => "unknown",
            Reach::SeatsOpen => "seats open",
        }
    }
}

impl fmt::Display for Reach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.label())
    }
}

/// One eligible program in the scan result.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanRow {
    pub district: DistrictCode,
    pub dbn: String,
    pub location: Option<GeoPoint>,
    pub address: Option<String>,
    pub school_name: String,
    pub code: ProgramCode,
    pub program_name: String,
    pub method: String,
    pub seats: Option<u32>,
    pub applicants: Option<u32>,
    pub all_seats_filled: Option<bool>,
    /// The group name without its "Priority to" or "Then to" prefix.
    pub group_name: String,
    pub group_order: u32,
    pub group_count: usize,
    pub group_applied: Option<u32>,
    pub group_offered: Option<u32>,
    pub group_share: Option<f64>,
    pub reach: Reach,
    /// The school's latest state test results, when known.
    pub scores: Option<SchoolScores>,
}

impl ScanRow {
    /// Sort value: the offer share, else `1.0` when seats stayed open, else
    /// `-1.0`. Higher sorts first.
    fn sort_value(&self) -> f64 {
        match self.group_share {
            Some(share) => share,
            None if self.all_seats_filled == Some(false) => 1.0,
            None => -1.0,
        }
    }
}

fn strip_priority_prefix(name: &str) -> String {
    name.replace("Then to ", "").replace("Priority to ", "")
}

fn classify(
    program: &CatalogProgram,
    slice: Option<DemandPair>,
    share: Option<f64>,
    lottery_fraction: f64,
) -> Reach {
    let Some(share) = share else {
        return if program.all_seats_filled == Some(true) {
            Reach::Unknown
        } else {
            Reach::SeatsOpen
        };
    };
    if !program.ranks_by_lottery() {
        return Reach::ByScreen;
    }
    if slice.is_some_and(|pair| pair.offers == Some(pair.applicants)) {
        return Reach::AllOffered;
    }
    if share >= lottery_fraction {
        return Reach::Likely;
    }
    if share >= 0.4 {
        return Reach::WaitlistMaybe;
    }
    Reach::No
}

/// Lists every program the student is eligible for, most reachable first.
pub fn scan(
    catalog: &[CatalogProgram],
    results: &TestResults,
    request: &ScanRequest,
) -> Vec<ScanRow> {
    let lottery_fraction = request.lottery.fraction().get();
    let mut rows: Vec<ScanRow> = catalog
        .iter()
        .filter(|program| !program.has_no_demand_figures())
        .filter_map(|program| {
            let group = program.eligible_group(&request.attributes)?;
            let slice = group.demand.slice_for(request.frl);
            let share = slice.and_then(DemandPair::share);
            Some(ScanRow {
                district: program.district.clone(),
                dbn: program.dbn.clone(),
                location: program.location,
                address: program.address.clone(),
                school_name: program.school_name.clone(),
                code: program.code.clone(),
                program_name: program.program_name.clone(),
                method: program.method.clone(),
                seats: program.seats,
                applicants: program.applicants,
                all_seats_filled: program.all_seats_filled,
                group_name: strip_priority_prefix(&group.name),
                group_order: group.order,
                group_count: program.groups().len(),
                group_applied: slice.map(|p| p.applicants),
                group_offered: slice.and_then(|p| p.offers),
                group_share: share,
                reach: classify(program, slice, share, lottery_fraction),
                scores: results.get(&program.dbn).copied(),
            })
        })
        .collect();
    // Stable sort keeps the catalog order for equal values.
    rows.sort_by(|a, b| b.sort_value().total_cmp(&a.sort_value()));
    rows
}

/// Builds simulation programs from the catalog.
///
/// Rules:
///
/// - Programs without published seats or applicants are left out.
/// - Open and zone-priority programs rank by lottery; every other method
///   is modelled as a screened test.
/// - Each priority group becomes one simulation group, or two when
///   MySchools splits its figures by free or reduced price lunch: the FRL
///   slice first (it also requires `frl`), then the rest.
/// - Offers are capped at applicants, since both are rounded in the
///   source text.
/// - First-round offers are estimated by filling the seats in priority
///   order: each group takes `min(offers, seats still open)`. The remaining
///   offers count as waitlist offers.
pub fn programs_from_catalog(catalog: &[CatalogProgram]) -> Vec<Program> {
    catalog
        .iter()
        .filter(|p| !p.has_no_demand_figures())
        .filter_map(|p| {
            let seats = p.seats.unwrap_or(0);
            let mut remaining = seats;
            let mut groups = Vec::new();
            let frl = Attribute::parse("frl").expect("valid");
            for group in p.groups() {
                let base = group.criterion.required_attributes(&p.code);
                let mut slices: Vec<(String, Vec<Attribute>, DemandPair)> = Vec::new();
                match (group.demand.frl, group.demand.non_frl, group.demand.all) {
                    (Some(frl_pair), non_frl, _) => {
                        let mut requires = base.clone();
                        requires.push(frl.clone());
                        slices.push((format!("{}, FRL", group.name), requires, frl_pair));
                        if let Some(pair) = non_frl {
                            slices.push((format!("{}, non-FRL", group.name), base.clone(), pair));
                        }
                    }
                    (None, Some(pair), _) => {
                        slices.push((format!("{}, non-FRL", group.name), base.clone(), pair))
                    }
                    (None, None, Some(pair)) => slices.push((group.name.clone(), base.clone(), pair)),
                    (None, None, None) => {}
                }
                for (name, requires, pair) in slices {
                    let offers = pair.offers.unwrap_or(0).min(pair.applicants);
                    let initial = offers.min(remaining);
                    remaining -= initial;
                    let demand = GroupDemand::new(pair.applicants, offers, Some(initial))
                        .expect("initial <= offers <= applicants by construction");
                    groups.push(PriorityGroup {
                        name,
                        requires,
                        demand,
                    });
                }
            }
            let selection = if p.ranks_by_lottery() {
                SelectionKind::Lottery
            } else {
                SelectionKind::Screened
            };
            let name = if p.program_name == p.school_name {
                p.school_name.clone()
            } else {
                format!("{} · {}", p.school_name, p.program_name)
            };
            Program::new(
                p.code.clone(),
                name,
                seats,
                p.applicants.unwrap_or(0),
                selection,
                groups,
                Some(p.method.clone()),
                Some(format!(
                    "Derived from MySchools: {} program; first-round offers estimated by filling {} seats in priority order.",
                    p.method, seats
                )),
            )
            .ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(list: &[&str]) -> BTreeSet<Attribute> {
        list.iter()
            .map(|a| Attribute::parse(a).expect("valid"))
            .collect()
    }

    fn pair(applicants: u32, offers: Option<u32>) -> Option<DemandPair> {
        Some(DemandPair { applicants, offers })
    }

    fn group(
        order: u32,
        name: &str,
        criterion: PriorityCriterion,
        all: Option<DemandPair>,
    ) -> CatalogGroup {
        CatalogGroup {
            order,
            name: name.to_owned(),
            criterion,
            demand: GroupDemandText {
                all,
                ..GroupDemandText::default()
            },
        }
    }

    fn program(
        code: &str,
        method: &str,
        seats: Option<u32>,
        applicants: Option<u32>,
        filled: Option<bool>,
        groups: Vec<CatalogGroup>,
    ) -> CatalogProgram {
        CatalogProgram::new(
            DistrictCode::parse("15").expect("valid"),
            format!("15{code}"),
            "School".to_owned(),
            ProgramCode::parse(code).expect("valid"),
            "Program".to_owned(),
            method.to_owned(),
            seats,
            applicants,
            filled,
            groups,
        )
    }

    fn request(attributes: &[&str], frl: bool, lottery: &str) -> ScanRequest {
        ScanRequest {
            attributes: attrs(attributes),
            frl,
            lottery: LotteryPrefix::parse(lottery).expect("valid"),
        }
    }

    #[test]
    fn district_code_rejects_blank_and_builds_the_attribute() {
        assert_eq!(DistrictCode::parse(" "), Err(InvalidDistrictCode));
        let code = DistrictCode::parse("15").expect("valid");
        assert_eq!(code.resident_attribute().as_str(), "d15");
        assert_eq!(DistrictCode::parse("09").expect("valid").as_str(), "9");
        assert_eq!(DistrictCode::parse("00").expect("valid").as_str(), "0");
        assert_eq!(DistrictCode::parse("75").expect("valid").borough(), None);
        assert_eq!(code.borough(), Some(Borough::Brooklyn));
    }

    #[test]
    fn boroughs_follow_the_district_map() {
        assert_eq!(Borough::of_district(2), Some(Borough::Manhattan));
        assert_eq!(Borough::of_district(9), Some(Borough::Bronx));
        assert_eq!(Borough::of_district(32), Some(Borough::Brooklyn));
        assert_eq!(Borough::of_district(24), Some(Borough::Queens));
        assert_eq!(Borough::of_district(31), Some(Borough::StatenIsland));
        assert_eq!(Borough::of_district(33), None);
        let expanded = with_boroughs(attrs(&["d15", "frl"]));
        assert!(expanded.contains(&Borough::Brooklyn.resident_attribute()));
        assert!(PriorityCriterion::Borough(Borough::Brooklyn).admits(&attrs(&["d20"])));
        assert!(!PriorityCriterion::Borough(Borough::Queens).admits(&attrs(&["d20"])));
    }

    #[test]
    fn required_attributes_are_per_program_where_the_rule_is() {
        let code = ProgramCode::parse("K442S").expect("valid");
        let names = |c: PriorityCriterion| -> Vec<String> {
            c.required_attributes(&code)
                .iter()
                .map(|a| a.as_str().to_owned())
                .collect()
        };
        assert_eq!(names(PriorityCriterion::Sibling), vec!["sibling_K442S"]);
        assert_eq!(names(PriorityCriterion::Zoned), vec!["zoned_K442S"]);
        assert_eq!(
            names(PriorityCriterion::District(
                DistrictCode::parse("15").expect("valid")
            )),
            vec!["d15"]
        );
        assert_eq!(
            names(PriorityCriterion::Borough(Borough::Bronx)),
            vec!["borough_bronx"]
        );
        assert!(names(PriorityCriterion::AnyNycResident).is_empty());
        assert_eq!(
            names(PriorityCriterion::Unrecognized),
            vec!["unrecognized_priority_group"]
        );
    }

    #[test]
    fn programs_are_derived_with_seats_filled_in_priority_order() {
        let d15 = PriorityCriterion::District(DistrictCode::parse("15").expect("valid"));
        let catalog = vec![
            CatalogProgram::new(
                DistrictCode::parse("15").expect("valid"),
                "15K442".to_owned(),
                "M.S. 442".to_owned(),
                ProgramCode::parse("K442S").expect("valid"),
                "School for Innovation".to_owned(),
                "Open".to_owned(),
                Some(81),
                Some(494),
                Some(true),
                vec![
                    group(1, "Priority to siblings", PriorityCriterion::Sibling, None),
                    CatalogGroup {
                        order: 2,
                        name: "Then to District 15".to_owned(),
                        criterion: d15,
                        demand: GroupDemandText {
                            frl: pair(50, Some(40)),
                            non_frl: pair(370, Some(40)),
                            all: None,
                        },
                    },
                    group(
                        3,
                        "Then to NYC",
                        PriorityCriterion::AnyNycResident,
                        pair(60, Some(0)),
                    ),
                ],
            ),
            program("K000N", "Screened", None, None, None, vec![]),
            program(
                "K001T",
                "Talent Test",
                Some(27),
                Some(191),
                Some(true),
                vec![group(
                    1,
                    "Then to NYC",
                    PriorityCriterion::AnyNycResident,
                    pair(190, Some(200)),
                )],
            ),
        ];
        // The sibling group has FRL figures only in the first slice.
        let mut catalog = catalog;
        catalog[0] = CatalogProgram::new(
            DistrictCode::parse("15").expect("valid"),
            "15K442".to_owned(),
            "M.S. 442".to_owned(),
            ProgramCode::parse("K442S").expect("valid"),
            "School for Innovation".to_owned(),
            "Open".to_owned(),
            Some(81),
            Some(494),
            Some(true),
            vec![
                CatalogGroup {
                    order: 1,
                    name: "Priority to siblings".to_owned(),
                    criterion: PriorityCriterion::Sibling,
                    demand: GroupDemandText {
                        frl: pair(5, Some(5)),
                        non_frl: pair(5, Some(5)),
                        all: None,
                    },
                },
                catalog[0].groups()[1].clone(),
                catalog[0].groups()[2].clone(),
            ],
        );
        let programs = programs_from_catalog(&catalog);
        assert_eq!(programs.len(), 2, "the program without figures is skipped");
        let ms442 = &programs[0];
        assert_eq!(ms442.name(), "M.S. 442 · School for Innovation");
        assert_eq!(ms442.selection(), SelectionKind::Lottery);
        let groups: Vec<(String, Vec<String>, u32, u32, u32)> = ms442
            .groups()
            .iter()
            .map(|g| {
                (
                    g.name.clone(),
                    g.requires.iter().map(|a| a.as_str().to_owned()).collect(),
                    g.demand.applicants(),
                    g.demand.offers(),
                    g.demand.initial_offers(),
                )
            })
            .collect();
        assert_eq!(
            groups,
            vec![
                (
                    "Priority to siblings, FRL".to_owned(),
                    vec!["sibling_K442S".to_owned(), "frl".to_owned()],
                    5,
                    5,
                    5
                ),
                (
                    "Priority to siblings, non-FRL".to_owned(),
                    vec!["sibling_K442S".to_owned()],
                    5,
                    5,
                    5
                ),
                (
                    "Then to District 15, FRL".to_owned(),
                    vec!["d15".to_owned(), "frl".to_owned()],
                    50,
                    40,
                    40
                ),
                (
                    "Then to District 15, non-FRL".to_owned(),
                    vec!["d15".to_owned()],
                    370,
                    40,
                    31
                ),
                ("Then to NYC".to_owned(), vec![], 60, 0, 0),
            ]
        );
        let twain = &programs[1];
        assert_eq!(twain.selection(), SelectionKind::Screened);
        assert_eq!(
            twain.groups()[0].demand.offers(),
            190,
            "offers capped at applicants"
        );
        assert_eq!(
            twain.groups()[0].demand.initial_offers(),
            27,
            "capped at seats"
        );
    }

    #[test]
    fn criteria_admit_the_matching_attributes() {
        let d20 = PriorityCriterion::District(DistrictCode::parse("20").expect("valid"));
        assert!(d20.admits(&attrs(&["d15", "d20"])));
        assert!(!d20.admits(&attrs(&["d15"])));
        assert!(PriorityCriterion::Sibling.admits(&attrs(&["sibling"])));
        assert!(!PriorityCriterion::Sibling.admits(&attrs(&["sibling_K442S"])));
        assert!(PriorityCriterion::AnyNycResident.admits(&attrs(&[])));
        assert!(!PriorityCriterion::Unrecognized.admits(&attrs(&["d15", "zoned"])));
        assert!(PriorityCriterion::Zoned.admits(&attrs(&["zoned"])));
        assert!(PriorityCriterion::Continuing.admits(&attrs(&["continuing"])));
        assert!(PriorityCriterion::CurrentStudent.admits(&attrs(&["current_student"])));
    }

    #[test]
    fn demand_share_needs_offers_and_applicants() {
        assert_eq!(
            DemandPair {
                applicants: 230,
                offers: Some(140)
            }
            .share(),
            Some(140.0 / 230.0)
        );
        assert_eq!(
            DemandPair {
                applicants: 0,
                offers: Some(0)
            }
            .share(),
            None
        );
        assert_eq!(
            DemandPair {
                applicants: 10,
                offers: None
            }
            .share(),
            None
        );
    }

    #[test]
    fn slice_prefers_the_matching_split_then_the_whole_group() {
        let split = GroupDemandText {
            frl: pair(50, Some(40)),
            non_frl: pair(370, Some(40)),
            all: None,
        };
        assert_eq!(split.slice_for(true), pair(50, Some(40)));
        assert_eq!(split.slice_for(false), pair(370, Some(40)));
        let whole = GroupDemandText {
            all: pair(680, Some(90)),
            ..GroupDemandText::default()
        };
        assert_eq!(whole.slice_for(true), pair(680, Some(90)));
        assert_eq!(whole.slice_for(false), pair(680, Some(90)));
    }

    #[test]
    fn groups_are_sorted_by_order_and_the_first_eligible_wins() {
        let program = program(
            "K062Z",
            "Zone Priority",
            Some(209),
            Some(99),
            Some(false),
            vec![
                group(
                    3,
                    "Then to District 20 students and residents",
                    PriorityCriterion::District(DistrictCode::parse("20").expect("valid")),
                    pair(10, Some(10)),
                ),
                group(
                    1,
                    "Priority to applicants whose sibling(s) will be enrolled",
                    PriorityCriterion::Sibling,
                    pair(10, Some(10)),
                ),
                group(
                    2,
                    "Then to students residing in the zone",
                    PriorityCriterion::Zoned,
                    pair(70, Some(70)),
                ),
                group(
                    4,
                    "Then to New York City residents",
                    PriorityCriterion::AnyNycResident,
                    pair(10, Some(10)),
                ),
            ],
        );
        assert_eq!(program.groups()[0].order, 1);
        let picked = program.eligible_group(&attrs(&["d20"])).expect("eligible");
        assert_eq!(picked.order, 3);
        let zoned = program
            .eligible_group(&attrs(&["d20", "zoned"]))
            .expect("eligible");
        assert_eq!(zoned.order, 2);
    }

    #[test]
    fn scan_classifies_reach_like_the_python_scanner() {
        let d15 = || PriorityCriterion::District(DistrictCode::parse("15").expect("valid"));
        let catalog = vec![
            program(
                "K448S",
                "Open",
                Some(70),
                Some(35),
                Some(false),
                vec![group(
                    2,
                    "Then to District 15 students and residents",
                    d15(),
                    pair(10, Some(10)),
                )],
            ),
            program(
                "K447M",
                "Open",
                Some(126),
                Some(235),
                Some(true),
                vec![group(
                    2,
                    "Then to students and residents of District 15",
                    d15(),
                    pair(150, Some(120)),
                )],
            ),
            program(
                "K051M",
                "Open",
                Some(247),
                Some(454),
                Some(true),
                vec![group(
                    2,
                    "Then to students and residents of District 15",
                    d15(),
                    pair(260, Some(160)),
                )],
            ),
            program(
                "K442S",
                "Open",
                Some(81),
                Some(494),
                Some(true),
                vec![group(
                    2,
                    "Then to students and residents of District 15",
                    d15(),
                    pair(370, Some(40)),
                )],
            ),
            program(
                "K187A",
                "Screened",
                Some(90),
                Some(717),
                Some(true),
                vec![group(
                    1,
                    "Priority to students and residents of District 15",
                    d15(),
                    pair(680, Some(90)),
                )],
            ),
            program(
                "K136N",
                "Open",
                Some(23),
                Some(2),
                Some(false),
                vec![group(2, "Then to District 15", d15(), None)],
            ),
            program(
                "K000X",
                "Open",
                Some(20),
                Some(20),
                Some(true),
                vec![group(1, "Then to District 15", d15(), None)],
            ),
            program(
                "K999Z",
                "Open",
                None,
                None,
                None,
                vec![group(
                    1,
                    "Then to New York City residents",
                    PriorityCriterion::AnyNycResident,
                    pair(5, Some(5)),
                )],
            ),
            program(
                "K111S",
                "Open",
                Some(10),
                Some(10),
                Some(true),
                vec![group(
                    1,
                    "Then to applicants whose sibling",
                    PriorityCriterion::Sibling,
                    pair(5, Some(5)),
                )],
            ),
        ];
        let rows = scan(
            &catalog,
            &TestResults::new(),
            &request(&["d15", "d20"], false, "BD"),
        );
        let by_code: Vec<(&str, Reach)> = rows.iter().map(|r| (r.code.as_str(), r.reach)).collect();
        assert_eq!(
            by_code,
            vec![
                ("K448S", Reach::AllOffered),
                ("K136N", Reach::SeatsOpen),
                ("K447M", Reach::Likely),
                ("K051M", Reach::WaitlistMaybe),
                ("K187A", Reach::ByScreen),
                ("K442S", Reach::No),
                ("K000X", Reach::Unknown),
            ]
        );
        let first = &rows[0];
        assert_eq!(first.group_name, "District 15 students and residents");
        assert_eq!(first.group_applied, Some(10));
        assert_eq!(first.group_offered, Some(10));
        assert_eq!(first.group_share, Some(1.0));
        assert_eq!(first.group_count, 1);
    }

    #[test]
    fn scan_joins_test_results_by_dbn() {
        let d15 = PriorityCriterion::District(DistrictCode::parse("15").expect("valid"));
        let catalog = vec![program(
            "K442S",
            "Open",
            Some(81),
            Some(494),
            Some(true),
            vec![group(1, "Then to District 15", d15, pair(370, Some(40)))],
        )];
        let mut results = TestResults::new();
        results.insert(
            "15K442S".to_owned(),
            SchoolScores {
                year: 2025,
                ela_proficient_pct: Some(71.5),
                math_proficient_pct: None,
            },
        );
        let rows = scan(&catalog, &results, &request(&["d15"], false, "BD"));
        assert_eq!(rows[0].dbn, "15K442S");
        assert_eq!(
            rows[0].scores.map(|s| s.ela_proficient_pct),
            Some(Some(71.5))
        );
        let none = scan(
            &catalog,
            &TestResults::new(),
            &request(&["d15"], false, "BD"),
        );
        assert_eq!(none[0].scores, None);
    }

    #[test]
    fn combine_uses_the_later_year_and_blanks_the_other_subject() {
        let ela = SubjectResults {
            year: 2026,
            by_dbn: [
                ("15K442".to_owned(), Some(75.0)),
                ("20K062".to_owned(), None),
            ]
            .into_iter()
            .collect(),
        };
        let math = SubjectResults {
            year: 2025,
            by_dbn: [("15K442".to_owned(), Some(60.0))].into_iter().collect(),
        };
        let results = combine_subjects(&ela, &math);
        assert_eq!(results.len(), 2);
        assert_eq!(
            results["15K442"],
            SchoolScores {
                year: 2026,
                ela_proficient_pct: Some(75.0),
                math_proficient_pct: None
            }
        );
        let same_year = combine_subjects(&ela, &SubjectResults { year: 2026, ..math });
        assert_eq!(same_year["15K442"].math_proficient_pct, Some(60.0));
    }

    #[test]
    fn focus_is_guessed_from_names_and_method() {
        assert_eq!(
            classify_focus(
                "Academy for Scientific Research",
                "The Christa McAuliffe School",
                "Screened"
            ),
            vec![ProgramFocus::Stem]
        );
        assert_eq!(
            classify_focus("Mark Twain I.S. 239 (Science)", "Mark Twain", "Talent Test"),
            vec![ProgramFocus::Arts, ProgramFocus::Stem],
            "highest confidence first: talent test 0.7, then science 0.6"
        );
        assert_eq!(
            classify_focus("Spanish Dual Language Program", "I.S. 136", "Open"),
            vec![ProgramFocus::Language]
        );
        assert_eq!(
            classify_focus("Zoned Program", "J.H.S. 062 Ditmas", "Zone Priority"),
            vec![ProgramFocus::Zoned]
        );
        assert_eq!(
            classify_focus("Algebra Honors Program", "The Seeall Academy", "Screened"),
            vec![ProgramFocus::Honors]
        );
        assert_eq!(
            classify_focus(
                "Academy for College Preparation and Career Exploration",
                "x",
                "Open"
            ),
            vec![ProgramFocus::Careers]
        );
        assert_eq!(
            classify_focus("Park Slope Collegiate", "Park Slope Collegiate", "Open"),
            vec![ProgramFocus::General]
        );
    }

    #[test]
    fn keywords_match_whole_words_and_stems() {
        let words = tokens("Part of our smart start: Scientific designs, the ARTS!");
        assert!(
            !Word("art").matches(&words),
            "no match inside part, smart, start"
        );
        assert!(Word("arts").matches(&words));
        assert!(Prefix("scien").matches(&words));
        assert!(
            !Word("design").matches(&words),
            "designs is not the word design"
        );
        assert!(Phrase("smart", "start").matches(&words));
        assert!(!Phrase("start", "smart").matches(&words));
    }

    #[test]
    fn profile_classification_scores_by_source_weight() {
        let texts = [
            ProfileText {
                label: "program name".to_owned(),
                text: "Academy for Scientific Research".to_owned(),
            },
            ProfileText {
                label: "school overview".to_owned(),
                text: "Our robotics and chorus programs thrive; part of a creative community."
                    .to_owned(),
            },
        ];
        let (scores, evidence) = classify_profile(&texts, "Screened");
        // STEM: scien (0.6) and robot (0.15): 1 - 0.4 * 0.85 = 0.66.
        assert_eq!(scores[0].focus, ProgramFocus::Stem);
        assert!((scores[0].confidence - 0.66).abs() < 1e-9, "{scores:?}");
        // Arts: chorus (0.15) and creative (0.15): 1 - 0.85^2 = 0.2775.
        assert_eq!(scores[1].focus, ProgramFocus::Arts);
        assert!((scores[1].confidence - 0.2775).abs() < 1e-9, "{scores:?}");
        assert_eq!(tags_of(&scores), vec![ProgramFocus::Stem]);
        assert!(evidence.iter().any(|e| e.focus == ProgramFocus::Stem
            && e.keyword == "scien"
            && e.label == "program name"
            && e.weight == 0.6));
        let (none, evidence) = classify_profile(&[], "Open");
        assert!(none.is_empty());
        assert!(evidence.is_empty());
        assert_eq!(tags_of(&none), vec![ProgramFocus::General]);
        let (zoned, evidence) = classify_profile(&[], "Zone Priority");
        assert_eq!(tags_of(&zoned), vec![ProgramFocus::Zoned]);
        assert_eq!(evidence[0].label, "admissions method");
        assert!((zoned[0].confidence - 0.7).abs() < 1e-9);
    }

    #[test]
    fn subject_names_and_single_description_words_do_not_tag() {
        let description = ProfileText {
            label: "program description".to_owned(),
            text: "Students take English Language Arts, math, and social studies.".to_owned(),
        };
        let (scores, _) = classify_profile(&[description], "Open");
        assert!(
            !scores.iter().any(|s| s.focus == ProgramFocus::Arts),
            "language arts is a subject, not an arts focus: {scores:?}"
        );
        let stem = scores
            .iter()
            .find(|s| s.focus == ProgramFocus::Stem)
            .expect("math is a STEM word");
        assert!(
            stem.confidence < TAG_THRESHOLD,
            "one description word is not a tag"
        );
        assert_eq!(tags_of(&scores), vec![ProgramFocus::General]);
    }

    #[test]
    fn several_weak_mentions_can_reach_the_threshold() {
        let overview = ProfileText {
            label: "school overview".to_owned(),
            text: "Science labs, engineering projects, robotics, coding clubs and a math team."
                .to_owned(),
        };
        let (scores, _) = classify_profile(&[overview], "Open");
        // Five distinct STEM keywords at 0.15: 1 - 0.85^5 = 0.556.
        assert_eq!(scores[0].focus, ProgramFocus::Stem);
        assert!(scores[0].confidence > TAG_THRESHOLD, "{scores:?}");
        assert_eq!(tags_of(&scores), vec![ProgramFocus::Stem]);
    }

    #[test]
    fn a_profile_can_be_reclassified_from_its_stored_texts() {
        let profile = ProgramProfile {
            code: ProgramCode::parse("K1").expect("valid"),
            dbn: "15K001".to_owned(),
            program_name: "Spanish Dual Language".to_owned(),
            school_name: "School".to_owned(),
            method: "Open".to_owned(),
            description: String::new(),
            overview: "We love music.".to_owned(),
            academic_opportunities: String::new(),
            art_courses: "Chorus".to_owned(),
            language_classes: "Spanish".to_owned(),
            scores: vec![],
            evidence: vec![],
        };
        let again = profile.reclassified();
        assert_eq!(again.tags(), vec![ProgramFocus::Language]);
        assert!(again
            .scores
            .iter()
            .any(|s| s.focus == ProgramFocus::Arts && s.confidence < TAG_THRESHOLD));
        assert!(again
            .evidence
            .iter()
            .any(|e| e.keyword == "music" && e.weight == 0.15));
    }

    #[test]
    fn scan_uses_the_frl_slice_when_requested() {
        let d15 = PriorityCriterion::District(DistrictCode::parse("15").expect("valid"));
        let catalog = vec![program(
            "K442S",
            "Open",
            Some(81),
            Some(494),
            Some(true),
            vec![CatalogGroup {
                order: 2,
                name: "Then to students and residents of District 15".to_owned(),
                criterion: d15,
                demand: GroupDemandText {
                    frl: pair(50, Some(40)),
                    non_frl: pair(370, Some(40)),
                    all: None,
                },
            }],
        )];
        let frl = scan(
            &catalog,
            &TestResults::new(),
            &request(&["d15"], true, "BD"),
        );
        assert_eq!(frl[0].group_applied, Some(50));
        assert_eq!(frl[0].reach, Reach::Likely);
        let non_frl = scan(
            &catalog,
            &TestResults::new(),
            &request(&["d15"], false, "BD"),
        );
        assert_eq!(non_frl[0].group_applied, Some(370));
        assert_eq!(non_frl[0].reach, Reach::No);
    }
}
