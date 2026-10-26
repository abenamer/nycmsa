//! Askama page templates and their view models. Values are formatted here
//! so the templates stay free of logic.

use askama::Template;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::adapters::cli::join_attributes;
use crate::adapters::cli::render::{pct0, pct1, thousands};
use crate::adapters::dto::StudentDto;
use crate::application::Run;
use crate::domain::{ProgramDefinitions, Reach, ScanRequest, ScanRow};

use super::{ScanQuery, SimulateForm};

/// `2026-10-26T19:40:00Z`, or the raw seconds when formatting fails.
pub fn format_time(time: std::time::SystemTime) -> String {
    OffsetDateTime::from(time)
        .format(&Rfc3339)
        .unwrap_or_else(|_| format!("{}s", crate::adapters::dto::unix_seconds(time)))
}

fn percent_input(fraction: Option<f64>) -> String {
    fraction
        .map(|f| format!("{}", (f * 100.0 * 1000.0).round() / 1000.0))
        .unwrap_or_default()
}

/// Values shown in the simulate form.
#[derive(Debug, Clone)]
pub struct FormValues {
    pub attributes: String,
    pub ranking: String,
    pub lottery_number: String,
    pub talent_test_percentile: String,
    pub talent_test_sd: String,
    pub trials: String,
    pub seed: String,
    pub demand_sd: String,
    pub attrition: String,
    pub waitlist_reused: bool,
}

impl FormValues {
    pub fn from_student(student: &StudentDto) -> Self {
        Self {
            attributes: student.attributes.join(", "),
            ranking: student.ranking.join(", "),
            lottery_number: student.lottery_number.clone().unwrap_or_default(),
            talent_test_percentile: percent_input(student.talent_test_percentile),
            talent_test_sd: percent_input(Some(student.talent_test_sd)),
            trials: crate::adapters::dto::DEFAULT_TRIALS.to_string(),
            seed: "0".to_owned(),
            demand_sd: crate::adapters::dto::DEFAULT_DEMAND_SD.to_string(),
            attrition: crate::adapters::dto::DEFAULT_ATTRITION.to_string(),
            waitlist_reused: false,
        }
    }

    pub fn from_form(form: &SimulateForm) -> Self {
        Self {
            attributes: form.attributes.clone(),
            ranking: form.ranking.clone(),
            lottery_number: form.lottery_number.clone(),
            talent_test_percentile: form.talent_test_percentile.clone(),
            talent_test_sd: form.talent_test_sd.clone(),
            trials: form.trials.clone(),
            seed: form.seed.clone(),
            demand_sd: form.demand_sd.clone(),
            attrition: form.attrition.clone(),
            waitlist_reused: form.waitlist_numbers.trim() == "reused",
        }
    }
}

pub struct ProgramOption {
    pub code: String,
    pub name: String,
    pub method: String,
    pub kind: String,
}

pub struct RecentRun {
    pub id: u64,
    pub created_at: String,
    pub ranking: String,
    pub lottery: String,
    pub p_any: String,
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
                    method: p.admissions_method().unwrap_or("").to_owned(),
                    kind: p.selection().label().to_owned(),
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
                        .map_or_else(|| "random".to_owned(), |l| l.to_string()),
                    p_any: pct1(run.report.summary.p_any_offer),
                })
                .collect(),
            error,
        }
    }
}

pub struct RowView {
    pub rank: usize,
    pub code: String,
    pub name: String,
    pub selection: String,
    pub group: Option<String>,
    pub init_rate: String,
    pub wl_rate: String,
    pub p_initial: String,
    pub p_waitlist: String,
    pub p_any: String,
}

pub struct SummaryLine {
    pub label: String,
    pub value: String,
}

#[derive(Template)]
#[template(path = "run.html")]
pub struct RunPage {
    pub id: u64,
    pub created_at: String,
    pub attributes: String,
    pub lottery_line: Option<String>,
    pub talent_line: Option<String>,
    pub parameters_line: String,
    pub rows: Vec<RowView>,
    pub summary: Vec<SummaryLine>,
}

impl RunPage {
    pub fn new(run: &Run) -> Self {
        let report = &run.report;
        let student = &run.student;
        let p = &report.parameters;
        let lottery_line = match (&student.lottery_number, report.lottery_fraction) {
            (Some(prefix), Some(fraction)) => Some(format!(
                "Lottery number {prefix}: about {} of applicants have a better number.",
                pct0(fraction)
            )),
            _ => Some("Lottery number unknown: drawn at random in each trial.".to_owned()),
        };
        let talent_line = student.talent_test_percentile.map(|pct| {
            format!(
                "Talent test: expected at the {} mark among test-takers (0% = best), spread ±{}.",
                pct0(pct.get()),
                pct0(student.talent_test_sd.get())
            )
        });
        let mut summary = vec![
            SummaryLine {
                label: "P(initial-round offer from this list)".to_owned(),
                value: pct1(report.summary.p_initial_offer),
            },
            SummaryLine {
                label: "P(any offer incl. waitlist from this list)".to_owned(),
                value: pct1(report.summary.p_any_offer),
            },
            SummaryLine {
                label: "P(no offer at all, placed by DOE elsewhere)".to_owned(),
                value: pct1(report.summary.p_no_offer),
            },
        ];
        if let Some(v) = report.summary.p_lottery_offer {
            summary.push(SummaryLine {
                label: "P(any offer from a lottery program)".to_owned(),
                value: pct1(v),
            });
        }
        if let Some(v) = report.summary.p_screened_offer {
            summary.push(SummaryLine {
                label: "P(any offer from a screened program)".to_owned(),
                value: pct1(v),
            });
        }
        Self {
            id: run.id.get(),
            created_at: format_time(run.created_at),
            attributes: join_attributes(&student.attributes),
            lottery_line,
            talent_line,
            parameters_line: format!(
                "Trials {} · seed {} · demand sd {} · attrition {} · waitlist numbers {}",
                thousands(u64::from(p.trials.get())),
                p.seed.get(),
                p.demand_sd.get(),
                p.attrition.get(),
                p.waitlist_numbers.label()
            ),
            rows: report
                .rows
                .iter()
                .map(|r| RowView {
                    rank: r.rank,
                    code: r.code.to_string(),
                    name: r.name.clone(),
                    selection: r.selection.label().to_owned(),
                    group: r.group_name.clone(),
                    init_rate: pct0(r.initial_rate),
                    wl_rate: pct0(r.waitlist_rate),
                    p_initial: pct1(r.p_initial),
                    p_waitlist: pct1(r.p_waitlist),
                    p_any: pct1(r.p_any),
                })
                .collect(),
            summary,
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
    pub applied: String,
    pub offered: String,
    pub share: String,
    pub reach: String,
    pub reach_class: &'static str,
}

fn reach_class(reach: Reach) -> &'static str {
    match reach {
        Reach::AllOffered | Reach::SeatsOpen => "good",
        Reach::Likely => "likely",
        Reach::WaitlistMaybe => "maybe",
        Reach::No => "no",
        Reach::ByScreen | Reach::Unknown => "neutral",
    }
}

fn opt(value: Option<u32>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

#[derive(Template)]
#[template(path = "scan.html")]
pub struct ScanPage {
    pub attrs: String,
    pub frl: bool,
    pub lottery: String,
    pub lottery_line: Option<String>,
    pub catalog_size: usize,
    pub rows: Vec<ScanRowView>,
    pub csv_url: String,
    pub error: Option<String>,
}

impl ScanPage {
    pub fn new(
        request: &ScanRequest,
        rows: &[ScanRow],
        catalog_size: usize,
        error: Option<String>,
    ) -> Self {
        let attrs = join_attributes(&request.attributes);
        let lottery = request.lottery.to_string();
        Self {
            csv_url: format!(
                "/api/scan.csv?attrs={}&frl={}&lottery={}",
                attrs.replace(", ", ","),
                u8::from(request.frl),
                lottery
            ),
            lottery_line: Some(format!(
                "Lottery {lottery}: about {} of applicants rank ahead.",
                pct0(request.lottery.fraction().get())
            )),
            attrs,
            frl: request.frl,
            lottery,
            catalog_size,
            rows: rows
                .iter()
                .map(|r| ScanRowView {
                    district: r.district.to_string(),
                    code: r.code.to_string(),
                    school: r.school_name.clone(),
                    program: r.program_name.clone(),
                    method: r.method.clone(),
                    seats: opt(r.seats),
                    applicants: opt(r.applicants),
                    group: r.group_name.clone(),
                    applied: opt(r.group_applied),
                    offered: opt(r.group_offered),
                    share: r.group_share.map(pct0).unwrap_or_else(|| "n/a".to_owned()),
                    reach: r.reach.label().to_owned(),
                    reach_class: reach_class(r.reach),
                })
                .collect(),
            error,
        }
    }

    pub fn invalid(query: &ScanQuery, catalog_size: usize, error: String) -> Self {
        Self {
            attrs: query.attrs().to_owned(),
            frl: query.frl(),
            lottery: query.lottery().to_owned(),
            lottery_line: None,
            catalog_size,
            rows: vec![],
            csv_url: String::new(),
            error: Some(error),
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
    pub kind: String,
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
                    kind: p.selection().label().to_owned(),
                    seats: p.seats(),
                    applicants: p.applicants(),
                    note: p.note().map(str::to_owned),
                    groups: p
                        .groups()
                        .iter()
                        .map(|g| GroupView {
                            name: g.name.clone(),
                            requires: if g.requires.is_empty() {
                                "(everyone)".to_owned()
                            } else {
                                join_attributes(&g.requires)
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
