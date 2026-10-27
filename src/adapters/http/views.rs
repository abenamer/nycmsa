//! Askama page templates and their view models. Values are formatted and
//! worded here so the templates stay free of logic.

use askama::Template;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::adapters::cli::render::{pct0, thousands};
use crate::adapters::dto::StudentDto;
use crate::application::Run;
use crate::domain::{
    DistrictCode, ProgramDefinitions, ProgramRow, Reach, ScanRequest, ScanRow, SelectionKind,
    WaitlistNumbers,
};

use super::choices::{
    attribute_chips, attribute_label, OutlookChoice, TalentTestChoice, WaitlistChoice,
};
use super::{ScanQuery, SimulateForm};

/// `2026-10-26T19:40:00Z`, or the raw seconds when formatting fails.
pub fn format_time(time: std::time::SystemTime) -> String {
    OffsetDateTime::from(time)
        .format(&Rfc3339)
        .unwrap_or_else(|_| format!("{}s", crate::adapters::dto::unix_seconds(time)))
}

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

/// Values shown in the simulate form.
pub struct FormValues {
    pub ranking: String,
    pub lottery_number: String,
    pub attribute_chips: Vec<Chip>,
    pub talent_chips: Vec<Chip>,
    pub waitlist_chips: Vec<Chip>,
    pub outlook_chips: Vec<Chip>,
}

impl FormValues {
    fn build(
        definitions: &ProgramDefinitions,
        ranking: String,
        lottery_number: String,
        selected_attributes: &[String],
        talent: Option<TalentTestChoice>,
        waitlist: Option<WaitlistChoice>,
        outlook: Option<OutlookChoice>,
    ) -> Self {
        Self {
            ranking,
            lottery_number,
            attribute_chips: attribute_chips(definitions)
                .into_iter()
                .map(|a| Chip {
                    label: attribute_label(&a, definitions),
                    checked: selected_attributes.contains(&a),
                    value: a,
                })
                .collect(),
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

    pub fn from_student(student: &StudentDto, definitions: &ProgramDefinitions) -> Self {
        let took_test = student.attributes.iter().any(|a| a == "mark_twain_test");
        Self::build(
            definitions,
            student.ranking.join(", "),
            student.lottery_number.clone().unwrap_or_default(),
            &student.attributes,
            Some(TalentTestChoice::from_student(
                took_test,
                student.talent_test_percentile,
            )),
            Some(WaitlistChoice::Redrawn),
            Some(OutlookChoice::AsPublished),
        )
    }

    pub fn from_form(form: &SimulateForm, definitions: &ProgramDefinitions) -> Self {
        Self::build(
            definitions,
            form.ranking.clone(),
            form.lottery_number.clone(),
            &form.attributes,
            TalentTestChoice::parse(&form.talent_test),
            WaitlistChoice::parse(&form.waitlist),
            OutlookChoice::parse(&form.outlook),
        )
    }
}

pub struct ProgramOption {
    pub code: String,
    pub name: String,
    pub how: &'static str,
}

/// How a program picks students, in plain words.
fn how_it_admits(kind: SelectionKind) -> &'static str {
    match kind {
        SelectionKind::Lottery => "lottery number",
        SelectionKind::Screened => "screened test",
    }
}

pub struct RecentRun {
    pub id: u64,
    pub created_at: String,
    pub ranking: String,
    pub lottery: String,
    pub chance: String,
}

#[derive(Template)]
#[template(path = "home.html")]
pub struct HomePage {
    pub form: FormValues,
    pub programs: Vec<ProgramOption>,
    pub recent: Vec<RecentRun>,
    pub error: Option<String>,
}

impl HomePage {
    pub fn new(
        form: FormValues,
        definitions: &ProgramDefinitions,
        recent: &[Run],
        error: Option<String>,
    ) -> Self {
        Self {
            form,
            programs: definitions
                .iter()
                .map(|p| ProgramOption {
                    code: p.code().to_string(),
                    name: p.name().to_owned(),
                    how: how_it_admits(p.selection()),
                })
                .collect(),
            recent: recent
                .iter()
                .map(|run| RecentRun {
                    id: run.id.get(),
                    created_at: format_time(run.created_at),
                    ranking: run
                        .student
                        .ranking
                        .codes()
                        .iter()
                        .map(|c| c.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    lottery: run
                        .student
                        .lottery_number
                        .as_ref()
                        .map_or_else(|| "not given".to_owned(), |l| l.to_string()),
                    chance: chance(run.report.summary.p_any_offer),
                })
                .collect(),
            error,
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
#[template(path = "run.html")]
pub struct RunPage {
    pub id: u64,
    pub created_at: String,
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

impl RunPage {
    pub fn new(run: &Run) -> Self {
        let report = &run.report;
        let student = &run.student;
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
            id: run.id.get(),
            created_at: format_time(run.created_at),
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
    pub outlook: &'static str,
    pub outlook_class: &'static str,
}

fn outlook(reach: Reach) -> (&'static str, &'static str) {
    match reach {
        Reach::AllOffered => ("Everyone in your group got in", "good"),
        Reach::SeatsOpen => ("Seats were left open", "good"),
        Reach::Likely => ("Good odds for your number", "likely"),
        Reach::WaitlistMaybe => ("Maybe, via the waitlist", "maybe"),
        Reach::No => ("Unlikely with your number", "no"),
        Reach::ByScreen => ("Depends on the screen, not the lottery", "neutral"),
        Reach::Unknown => ("No figures published", "neutral"),
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
    pub district_chips: Vec<Chip>,
    pub role_chips: Vec<Chip>,
    pub lottery_line: Option<String>,
    pub catalog_size: usize,
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
        error: Option<String>,
    ) -> Self {
        let definitions = ProgramDefinitions::default();
        let selected: Vec<String> = query
            .attributes
            .iter()
            .flat_map(|a| a.split(',').map(|s| s.trim().to_owned()))
            .collect();
        let district_chips = districts
            .iter()
            .map(|d| {
                let value = d.resident_attribute().as_str().to_owned();
                Chip {
                    label: attribute_label(&value, &definitions),
                    checked: selected.contains(&value),
                    value,
                }
            })
            .collect();
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
            rows,
            csv_url,
            error,
        }
    }

    fn row(r: &ScanRow) -> ScanRowView {
        let (outlook, outlook_class) = outlook(r.reach);
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
            outlook,
            outlook_class,
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
    pub programs: Vec<ProgramView>,
}

impl ProgramsPage {
    pub fn new(definitions: &ProgramDefinitions) -> Self {
        Self {
            programs: definitions
                .iter()
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
