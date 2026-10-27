//! Askama page templates and their view models. Values are formatted and
//! worded here so the templates stay free of logic.

use askama::Template;

use crate::adapters::cli::render::{pct0, thousands};
use crate::domain::{
    Borough, DistrictCode, ProgramDefinitions, ProgramFocus, ProgramRow, Reach, Report,
    ScanRequest, ScanRow, SelectionKind, Student, WaitlistNumbers, TAG_THRESHOLD,
};

use super::choices::{attribute_label, OutlookChoice, TalentTestChoice, WaitlistChoice};
use super::{AppState, ScanQuery, SimulateForm};

/// A whole-percent chance; tiny chances read as "under 1%".
pub fn chance(p: f64) -> String {
    if p > 0.0 && p < 0.005 {
        "under 1%".to_owned()
    } else {
        pct0(p)
    }
}

/// One selectable chip.
pub struct Chip {
    pub value: String,
    pub label: String,
    pub checked: bool,
}

fn chips<T: Copy + PartialEq>(
    all: &[T],
    value: fn(T) -> &'static str,
    label: fn(T) -> &'static str,
    selected: Option<T>,
) -> Vec<Chip> {
    all.iter()
        .map(|&c| Chip {
            value: value(c).to_owned(),
            label: label(c).to_owned(),
            checked: selected == Some(c),
        })
        .collect()
}

/// A district chip: `District 15`, grouped by borough on the page.
pub struct DistrictChip {
    pub value: String,
    pub label: String,
    pub borough: &'static str,
    pub checked: bool,
}

fn district_chips(districts: &[DistrictCode], selected: &[String]) -> Vec<DistrictChip> {
    districts
        .iter()
        .map(|d| {
            let value = d.resident_attribute().as_str().to_owned();
            DistrictChip {
                label: format!("District {d}"),
                borough: d.borough().map_or("Other", Borough::name),
                checked: selected.contains(&value),
                value,
            }
        })
        .collect()
}

/// Values shown in the simulate form.
pub struct FormValues {
    pub ranking: String,
    pub lottery_number: String,
    pub district_chips: Vec<DistrictChip>,
    pub frl: bool,
    pub sibling_codes: String,
    pub zoned_codes: String,
    pub continuing_codes: String,
    pub talent_chips: Vec<Chip>,
    pub waitlist_chips: Vec<Chip>,
    pub outlook_chips: Vec<Chip>,
}

impl FormValues {
    #[allow(clippy::too_many_arguments)]
    fn build(
        districts: &[DistrictCode],
        ranking: String,
        lottery_number: String,
        attributes: &[String],
        code_fields: (String, String, String),
        talent: Option<TalentTestChoice>,
        waitlist: Option<WaitlistChoice>,
        outlook: Option<OutlookChoice>,
    ) -> Self {
        Self {
            ranking,
            lottery_number,
            district_chips: district_chips(districts, attributes),
            frl: attributes.iter().any(|a| a == "frl"),
            sibling_codes: code_fields.0,
            zoned_codes: code_fields.1,
            continuing_codes: code_fields.2,
            talent_chips: chips(
                &TalentTestChoice::ALL,
                TalentTestChoice::value,
                TalentTestChoice::label,
                talent,
            ),
            waitlist_chips: chips(
                &WaitlistChoice::ALL,
                WaitlistChoice::value,
                WaitlistChoice::label,
                waitlist,
            ),
            outlook_chips: chips(
                &OutlookChoice::ALL,
                OutlookChoice::value,
                OutlookChoice::label,
                outlook,
            ),
        }
    }

    /// No answer prefilled. The model-assumption chips take their documented
    /// defaults (the DOE's waitlist rule, last year's numbers as published);
    /// everything about the child is left for the parent to enter.
    pub fn blank(districts: &[DistrictCode]) -> Self {
        Self::build(
            districts,
            String::new(),
            String::new(),
            &[],
            (String::new(), String::new(), String::new()),
            Some(TalentTestChoice::Unknown),
            Some(WaitlistChoice::Redrawn),
            Some(OutlookChoice::AsPublished),
        )
    }

    pub fn from_form(form: &SimulateForm, districts: &[DistrictCode]) -> Self {
        Self::build(
            districts,
            form.ranking.clone(),
            form.lottery_number.clone(),
            &form.attributes,
            (
                form.sibling_codes.clone(),
                form.zoned_codes.clone(),
                form.continuing_codes.clone(),
            ),
            TalentTestChoice::parse(&form.talent_test),
            WaitlistChoice::parse(&form.waitlist),
            OutlookChoice::parse(&form.outlook),
        )
    }
}

/// A link that filters a list by district.
pub struct FilterLink {
    pub label: String,
    pub href: String,
    pub active: bool,
}

/// Builds one link per district, toggling it in the current selection.
fn district_links(path: &str, districts: &[DistrictCode], selected: &[String]) -> Vec<FilterLink> {
    districts
        .iter()
        .map(|d| {
            let code = d.as_str().to_owned();
            let active = selected.contains(&code);
            let mut next: Vec<&String> = selected.iter().filter(|s| **s != code).collect();
            if !active {
                next.push(&code);
            }
            let query: Vec<String> = next.iter().map(|c| format!("district={c}")).collect();
            FilterLink {
                label: format!("District {code}"),
                href: if query.is_empty() {
                    format!("{path}?district=")
                } else {
                    format!("{path}?{}", query.join("&"))
                },
                active,
            }
        })
        .collect()
}

/// How a program picks students, in plain words.
fn how_it_admits(kind: SelectionKind) -> &'static str {
    match kind {
        SelectionKind::Lottery => "lottery number",
        SelectionKind::Screened => "screened test",
    }
}

#[derive(Template)]
#[template(path = "home.html")]
pub struct HomePage {
    pub form: FormValues,
    pub error: Option<String>,
}

impl HomePage {
    pub fn new(form: FormValues, error: Option<String>) -> Self {
        Self { form, error }
    }
}

/// One row of the program picker.
pub struct PickRow {
    pub code: String,
    pub district: String,
    pub borough: &'static str,
    pub school: String,
    pub program: String,
    pub picks_by: &'static str,
    pub screened: bool,
    /// Space-separated focus slugs for filtering.
    pub focus_slugs: String,
    pub focus_labels: String,
    pub seats: String,
    pub applicants: String,
    pub ela: ScoreCell,
    pub math: ScoreCell,
    /// The school's coordinates for the browser's distance column; empty
    /// when the catalog has none.
    pub lat: String,
    pub lon: String,
}

#[derive(Template)]
#[template(path = "pick.html")]
pub struct PickerPage {
    pub district_chips: Vec<DistrictChip>,
    pub focus_chips: Vec<Chip>,
    pub rows: Vec<PickRow>,
    pub scores_year: Option<u32>,
    pub profiled: usize,
}

impl PickerPage {
    pub fn new(state: &AppState) -> Self {
        let definitions = state.simulate.default_programs();
        let scores_year = state.scan.scores_year();
        let rows = state
            .scan
            .programs()
            .iter()
            .filter_map(|p| {
                let program = definitions.get(&p.code)?;
                let scores = state.scan.scores_for(&p.dbn);
                let focus = state.profiles.tags(p.code.as_str());
                Some(PickRow {
                    code: p.code.to_string(),
                    district: p.district.to_string(),
                    borough: p.district.borough().map_or("Other", Borough::name),
                    school: p.school_name.clone(),
                    program: p.program_name.clone(),
                    picks_by: how_it_admits(program.selection()),
                    screened: program.selection() == SelectionKind::Screened,
                    focus_slugs: focus.iter().map(|f| f.slug()).collect::<Vec<_>>().join(" "),
                    focus_labels: focus
                        .iter()
                        .map(|f| f.label())
                        .collect::<Vec<_>>()
                        .join(", "),
                    seats: opt(p.seats),
                    applicants: opt(p.applicants),
                    ela: ScoreCell::new(scores.and_then(|s| s.ela_proficient_pct)),
                    math: ScoreCell::new(scores.and_then(|s| s.math_proficient_pct)),
                    lat: coordinate(p.location.map(|l| l.latitude)),
                    lon: coordinate(p.location.map(|l| l.longitude)),
                })
            })
            .collect();
        Self {
            district_chips: district_chips(&state.scan.districts(), &[]),
            focus_chips: ProgramFocus::ALL
                .iter()
                .map(|f| Chip {
                    value: f.slug().to_owned(),
                    label: f.label().to_owned(),
                    checked: false,
                })
                .collect(),
            rows,
            scores_year,
            profiled: state.profiles.len(),
        }
    }
}

/// One program on the profiles page.
pub struct ProfileView {
    pub code: String,
    pub district: String,
    pub school: String,
    pub program: String,
    pub method: String,
    pub focus: String,
    pub description: String,
    pub overview: String,
    pub academic_opportunities: String,
    pub art_courses: String,
    pub language_classes: String,
    /// `STEM (66%)`, the kinds at or above the threshold.
    pub tags: Vec<String>,
    /// `Arts (28%)`, the kinds below it.
    pub weaker: Vec<String>,
    /// `STEM: "scien" in program name (+60%)`, one per match.
    pub evidence: Vec<String>,
}

#[derive(Template)]
#[template(path = "profiles.html")]
pub struct ProfilesPage {
    pub district_links: Vec<FilterLink>,
    pub shown_districts: String,
    pub profiles: Vec<ProfileView>,
    pub total: usize,
}

impl ProfilesPage {
    pub fn new(state: &AppState, requested_districts: &[String]) -> Self {
        let districts = state.scan.districts();
        // No district chosen shows nothing: every profile at once is a very
        // long page, and the picker links here with a district.
        let selected = requested_districts.to_vec();
        let district_of: std::collections::BTreeMap<&str, String> = state
            .scan
            .programs()
            .iter()
            .map(|p| (p.code.as_str(), p.district.to_string()))
            .collect();
        let profiles = state
            .profiles
            .all()
            .filter_map(|p| {
                let district = district_of.get(p.code.as_str())?;
                if !selected.contains(district) {
                    return None;
                }
                Some(ProfileView {
                    code: p.code.to_string(),
                    district: district.clone(),
                    school: p.school_name.clone(),
                    program: p.program_name.clone(),
                    method: p.method.clone(),
                    focus: p
                        .tags()
                        .iter()
                        .map(|f| f.label())
                        .collect::<Vec<_>>()
                        .join(", "),
                    description: p.description.clone(),
                    overview: p.overview.clone(),
                    academic_opportunities: p.academic_opportunities.clone(),
                    art_courses: p.art_courses.clone(),
                    language_classes: p.language_classes.clone(),
                    tags: p
                        .scores
                        .iter()
                        .filter(|s| s.confidence >= TAG_THRESHOLD)
                        .map(|s| format!("{} ({})", s.focus.label(), pct0(s.confidence)))
                        .collect(),
                    weaker: p
                        .scores
                        .iter()
                        .filter(|s| s.confidence < TAG_THRESHOLD)
                        .map(|s| format!("{} ({})", s.focus.label(), pct0(s.confidence)))
                        .collect(),
                    evidence: p
                        .evidence
                        .iter()
                        .map(|e| {
                            format!(
                                "{}: \"{}\" in {} (+{})",
                                e.focus.label(),
                                e.keyword,
                                e.label,
                                pct0(e.weight)
                            )
                        })
                        .collect(),
                })
            })
            .collect();
        Self {
            district_links: district_links("/profiles", &districts, &selected),
            shown_districts: selected.join(", "),
            profiles,
            total: state.profiles.len(),
        }
    }
}

/// One program on the results page.
pub struct ProgramChance {
    pub rank: usize,
    pub code: String,
    pub name: String,
    pub group: Option<String>,
    pub chance: String,
    /// Bar width in percent.
    pub bar: u8,
    pub verdict: &'static str,
    pub verdict_class: &'static str,
    pub note: Option<String>,
}

fn verdict(p: f64, eligible: bool) -> (&'static str, &'static str) {
    if !eligible {
        ("Not eligible", "neutral")
    } else if p >= 0.9 {
        ("Very likely", "good")
    } else if p >= 0.6 {
        ("Good chance", "good")
    } else if p >= 0.3 {
        ("Possible", "likely")
    } else if p >= 0.1 {
        ("Long shot", "maybe")
    } else if p >= 0.01 {
        ("Very unlikely", "no")
    } else {
        ("Practically none", "no")
    }
}

fn program_chance(row: &ProgramRow) -> ProgramChance {
    let eligible = row.group_name.is_some();
    let (verdict, verdict_class) = verdict(row.p_any, eligible);
    let note = if !eligible {
        Some("None of this program's priority groups includes your child.".to_owned())
    } else if row.p_any >= 0.01 && row.p_waitlist > row.p_initial {
        Some("Mostly through the waitlist, after the first round.".to_owned())
    } else {
        None
    };
    ProgramChance {
        rank: row.rank,
        code: row.code.to_string(),
        name: row.name.clone(),
        group: row.group_name.clone(),
        chance: chance(row.p_any),
        bar: (row.p_any * 100.0).round().clamp(0.0, 100.0) as u8,
        verdict,
        verdict_class,
        note,
    }
}

/// One program inside a stage section.
pub struct StageRow {
    pub name: String,
    pub code: String,
    pub chance: String,
    pub bar: u8,
    pub last_year: String,
}

#[derive(Template)]
#[template(path = "result.html")]
pub struct ResultPage {
    /// The submitted answers as JSON, for the browser to remember.
    pub answers_json: String,
    /// One line for the browser's list of earlier results.
    pub programs_line: String,
    pub lottery_line: String,
    pub chance_short: String,
    pub headline: String,
    pub no_offer_line: String,
    pub first_offer_line: Option<String>,
    pub assumptions: Vec<String>,
    pub programs: Vec<ProgramChance>,
    pub lottery_rows: Vec<StageRow>,
    pub screened_rows: Vec<StageRow>,
    pub waitlist_rows: Vec<StageRow>,
    pub waitlist_rule: String,
}

impl ResultPage {
    pub fn new(form: &SimulateForm, student: &Student, report: &Report) -> Self {
        let p = &report.parameters;
        let took_test = student
            .attributes
            .iter()
            .any(|a| a.as_str() == "mark_twain_test");

        let mut assumptions = Vec::new();
        assumptions.push(match (&student.lottery_number, report.lottery_fraction) {
            (Some(prefix), Some(fraction)) => format!(
                "Lottery number starting {prefix}: about {} of applicants drew a better number.",
                pct0(fraction)
            ),
            _ => "Lottery number not given: every possible number was tried.".to_owned(),
        });
        assumptions.push(if !took_test {
            "Did not take a screened test, so screened programs are out of reach.".to_owned()
        } else {
            match TalentTestChoice::from_student(
                true,
                student.talent_test_percentile.map(|v| v.get()),
            ) {
                TalentTestChoice::Unknown | TalentTestChoice::NotTaken => {
                    "Screened test result unknown: every placement was tried.".to_owned()
                }
                choice => format!(
                    "Screened test: expected to place in the {}.",
                    choice.label().to_lowercase()
                ),
            }
        });
        let waitlist_rule = match p.waitlist_numbers {
            WaitlistNumbers::Redrawn => {
                "Each waitlist draws a new random number, as the DOE describes.".to_owned()
            }
            WaitlistNumbers::Reused => {
                "Waitlists keep the application lottery number, as some schools did in 2022."
                    .to_owned()
            }
        };
        assumptions.push(waitlist_rule.clone());
        assumptions.push(match OutlookChoice::from_attrition(p.attrition.get()) {
            OutlookChoice::AsPublished => "Last year's offer numbers taken as they are.".to_owned(),
            OutlookChoice::Hopeful => format!(
                "Schools assumed to reach {} further down their lists than last year.",
                pct0(p.attrition.get() - 1.0)
            ),
        });
        assumptions.push(format!(
            "Based on {} simulated admissions rounds with last year's seats and applicants.",
            thousands(u64::from(p.trials.get()))
        ));

        let first_offer_line = report
            .rows
            .iter()
            .filter(|r| r.p_initial > 0.0)
            .max_by(|a, b| a.p_initial.total_cmp(&b.p_initial))
            .map(|r| {
                format!(
                    "The first-round offer, when there is one, most often comes from {} ({} of the time).",
                    r.name,
                    chance(r.p_initial)
                )
            });

        let stage_row = |r: &ProgramRow, p: f64, last_year: String| StageRow {
            name: r.name.clone(),
            code: r.code.to_string(),
            chance: chance(p),
            bar: (p * 100.0).round().clamp(0.0, 100.0) as u8,
            last_year,
        };
        let eligible = |r: &&ProgramRow| r.group_name.is_some();
        let lottery_rows = report
            .rows
            .iter()
            .filter(eligible)
            .filter(|r| r.selection == SelectionKind::Lottery)
            .map(|r| {
                stage_row(
                    r,
                    r.p_initial,
                    format!(
                        "Last year {} of your group got a first-round offer.",
                        pct0(r.initial_rate)
                    ),
                )
            })
            .collect();
        let screened_rows = report
            .rows
            .iter()
            .filter(eligible)
            .filter(|r| r.selection == SelectionKind::Screened)
            .map(|r| {
                stage_row(
                    r,
                    r.p_initial,
                    format!(
                        "Last year {} of test-takers got a first-round offer.",
                        pct0(r.initial_rate)
                    ),
                )
            })
            .collect();
        let waitlist_rows = report
            .rows
            .iter()
            .filter(eligible)
            .map(|r| {
                stage_row(
                    r,
                    r.p_waitlist,
                    format!(
                        "Last year the waitlist reached {} of those still waiting.",
                        pct0(r.waitlist_rate)
                    ),
                )
            })
            .collect();

        Self {
            answers_json: serde_json::to_string(form).unwrap_or_else(|_| "{}".to_owned()),
            programs_line: student
                .ranking
                .codes()
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            lottery_line: student
                .lottery_number
                .as_ref()
                .map_or_else(|| "not given".to_owned(), |l| l.to_string()),
            chance_short: chance(report.summary.p_any_offer),
            headline: format!(
                "About a {} chance of an offer from one of these programs.",
                chance(report.summary.p_any_offer)
            ),
            no_offer_line: format!(
                "About a {} chance of no offer from this list, in which case the DOE assigns a school with open seats.",
                chance(report.summary.p_no_offer)
            ),
            first_offer_line,
            assumptions,
            programs: report.rows.iter().map(program_chance).collect(),
            lottery_rows,
            screened_rows,
            waitlist_rows,
            waitlist_rule,
        }
    }
}

pub struct ScanRowView {
    pub district: String,
    pub code: String,
    pub school: String,
    pub program: String,
    pub method: String,
    pub seats: String,
    pub applicants: String,
    pub group: String,
    pub last_year: String,
    /// Offer share as a number for sorting, empty when unknown.
    pub share_sort: String,
    pub outlook: &'static str,
    pub outlook_class: &'static str,
    /// Rank of the outlook for sorting: best first.
    pub outlook_sort: u8,
    pub ela: ScoreCell,
    pub math: ScoreCell,
    pub lat: String,
    pub lon: String,
}

/// A coordinate as the template prints it, or empty when unknown.
fn coordinate(value: Option<f64>) -> String {
    value.map(|v| format!("{v:.6}")).unwrap_or_default()
}

fn outlook(reach: Reach) -> (&'static str, &'static str, u8) {
    match reach {
        Reach::AllOffered => ("Everyone in your group got in", "good", 0),
        Reach::SeatsOpen => ("Seats were left open", "good", 1),
        Reach::Likely => ("Good odds for your number", "likely", 2),
        Reach::WaitlistMaybe => ("Maybe, via the waitlist", "maybe", 3),
        Reach::ByScreen => ("Depends on the screen, not the lottery", "neutral", 4),
        Reach::No => ("Unlikely with your number", "no", 5),
        Reach::Unknown => ("No figures published", "neutral", 6),
    }
}

/// `71.5` as `72%`, or empty when unknown.
/// A test score as a table cell: the rounded text shown and the
/// unrounded value the sort script reads from `data-v`. Both are empty
/// when the figure is unknown, which the script sorts last.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScoreCell {
    pub shown: String,
    pub raw: String,
}

impl ScoreCell {
    pub fn new(value: Option<f64>) -> Self {
        match value {
            Some(v) => Self {
                shown: format!("{}%", v.round() as i64),
                raw: format!("{v}"),
            },
            None => Self::default(),
        }
    }
}

fn opt(value: Option<u32>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

#[derive(Template)]
#[template(path = "scan.html")]
pub struct ScanPage {
    pub lottery: String,
    pub frl: bool,
    pub district_chips: Vec<DistrictChip>,
    pub role_chips: Vec<Chip>,
    pub lottery_line: Option<String>,
    pub catalog_size: usize,
    pub scores_line: String,
    pub rows: Vec<ScanRowView>,
    pub csv_url: String,
    pub error: Option<String>,
}

const ROLE_ATTRIBUTES: [&str; 4] = ["sibling", "zoned", "continuing", "current_student"];

impl ScanPage {
    pub fn new(
        query: &ScanQuery,
        result: Option<(&ScanRequest, &[ScanRow])>,
        districts: &[DistrictCode],
        catalog_size: usize,
        schools_with_scores: usize,
        scores_year: Option<u32>,
        error: Option<String>,
    ) -> Self {
        let definitions = ProgramDefinitions::default();
        let selected: Vec<String> = query
            .attributes
            .iter()
            .flat_map(|a| a.split(',').map(|s| s.trim().to_owned()))
            .collect();
        let district_chips = district_chips(districts, &selected);
        let scores_line = match (schools_with_scores, scores_year) {
            (0, _) => "No state test results are loaded; run `nycmsa fetch-scores`.".to_owned(),
            (_, Some(year)) => format!(
                "ELA and Math show the share of the school's students at proficiency (levels 3 and 4) on the {year} New York State tests, as published by the NYC DOE. Blank means the DOE suppressed the figure or did not report it."
            ),
            (_, None) => "State test results are loaded but none of these schools has a figure.".to_owned(),
        };
        let role_chips = ROLE_ATTRIBUTES
            .iter()
            .map(|a| Chip {
                value: (*a).to_owned(),
                label: attribute_label(a, &definitions),
                checked: selected.contains(&(*a).to_owned()),
            })
            .collect();
        let (lottery_line, rows, csv_url) = match result {
            Some((request, rows)) => {
                let attrs: Vec<&str> = request.attributes.iter().map(|a| a.as_str()).collect();
                (
                    Some(format!(
                        "Lottery number starting {}: about {} of applicants drew a better number.",
                        request.lottery,
                        pct0(request.lottery.fraction().get())
                    )),
                    rows.iter().map(Self::row).collect(),
                    format!(
                        "/api/scan.csv?attrs={}&frl={}&lottery={}",
                        attrs.join(","),
                        u8::from(request.frl),
                        request.lottery
                    ),
                )
            }
            None => (None, vec![], String::new()),
        };
        Self {
            lottery: query.lottery.clone(),
            frl: query.frl,
            district_chips,
            role_chips,
            lottery_line,
            catalog_size,
            scores_line,
            rows,
            csv_url,
            error,
        }
    }

    fn row(r: &ScanRow) -> ScanRowView {
        let (outlook, outlook_class, outlook_sort) = outlook(r.reach);
        let last_year = match (r.group_applied, r.group_offered) {
            (Some(applied), Some(offered)) => format!("{offered} of {applied} got offers"),
            (Some(applied), None) => format!("{applied} applied; offers not published"),
            _ => "no figures".to_owned(),
        };
        ScanRowView {
            district: r.district.to_string(),
            code: r.code.to_string(),
            school: r.school_name.clone(),
            program: r.program_name.clone(),
            method: r.method.clone(),
            seats: opt(r.seats),
            applicants: opt(r.applicants),
            group: r.group_name.clone(),
            last_year,
            share_sort: r.group_share.map(|s| format!("{s}")).unwrap_or_default(),
            outlook,
            outlook_class,
            outlook_sort,
            ela: ScoreCell::new(r.scores.and_then(|s| s.ela_proficient_pct)),
            math: ScoreCell::new(r.scores.and_then(|s| s.math_proficient_pct)),
            lat: coordinate(r.location.map(|l| l.latitude)),
            lon: coordinate(r.location.map(|l| l.longitude)),
        }
    }
}

pub struct GroupView {
    pub name: String,
    pub requires: String,
    pub applicants: u32,
    pub offers: u32,
    pub initial_offers: u32,
}

pub struct ProgramView {
    pub code: String,
    pub name: String,
    pub method: String,
    pub how: &'static str,
    pub seats: u32,
    pub applicants: u32,
    pub note: Option<String>,
    pub groups: Vec<GroupView>,
}

#[derive(Template)]
#[template(path = "programs.html")]
pub struct ProgramsPage {
    pub district_links: Vec<FilterLink>,
    pub shown_districts: String,
    pub programs: Vec<ProgramView>,
}

impl ProgramsPage {
    pub fn new(state: &AppState, requested_districts: &[String]) -> Self {
        let districts = state.scan.districts();
        let selected = if requested_districts.is_empty() {
            districts.iter().map(|d| d.as_str().to_owned()).collect()
        } else {
            requested_districts.to_vec()
        };
        let definitions = state.simulate.default_programs();
        let in_district: std::collections::BTreeSet<&crate::domain::ProgramCode> = state
            .scan
            .programs()
            .iter()
            .filter(|p| selected.contains(&p.district.as_str().to_owned()))
            .map(|p| &p.code)
            .collect();
        Self {
            district_links: district_links("/programs", &districts, &selected),
            shown_districts: if requested_districts.is_empty() {
                "all districts".to_owned()
            } else {
                selected.join(", ")
            },
            programs: definitions
                .iter()
                .filter(|p| in_district.contains(p.code()))
                .map(|p| ProgramView {
                    code: p.code().to_string(),
                    name: p.name().to_owned(),
                    method: p.admissions_method().unwrap_or("").to_owned(),
                    how: how_it_admits(p.selection()),
                    seats: p.seats(),
                    applicants: p.applicants(),
                    note: p.note().map(str::to_owned),
                    groups: p
                        .groups()
                        .iter()
                        .map(|g| GroupView {
                            name: g.name.clone(),
                            requires: if g.requires.is_empty() {
                                "everyone".to_owned()
                            } else {
                                g.requires
                                    .iter()
                                    .map(|a| attribute_label(a.as_str(), definitions))
                                    .collect::<Vec<_>>()
                                    .join(" + ")
                            },
                            applicants: g.demand.applicants(),
                            offers: g.demand.offers(),
                            initial_offers: g.demand.initial_offers(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}
