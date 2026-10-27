//! Serde data-transfer types shared by the JSON files, the HTTP API, and the
//! CLI, with validated conversions to and from the domain.
//!
//! The file formats match the Python original: `programs.json` and
//! `student.json`. Field aliases keep the old names readable.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::domain::program::DuplicateProgramCode;
use crate::domain::{
    with_boroughs, Attribute, Attrition, GroupDemand, InvalidAttribute, InvalidAttrition,
    InvalidGroupDemand, InvalidLotteryPrefix, InvalidProgram, InvalidProgramCode, InvalidRanking,
    InvalidStandardDeviation, InvalidTrials, InvalidUnitInterval, LotteryPrefix, Parameters,
    PriorityGroup, Program, ProgramCode, ProgramDefinitions, ProgramRow, Ranking, Report, ScanRow,
    Seed, SelectionKind, StandardDeviation, Student, Summary, Trials, UnitInterval,
    WaitlistNumbers,
};

/// Validation error for external input. Each variant names the field or
/// program so the caller can report it.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum InvalidInput {
    #[error("program code: {0}")]
    ProgramCode(#[from] InvalidProgramCode),
    #[error("attribute: {0}")]
    Attribute(#[from] InvalidAttribute),
    #[error("program {code}, group {group:?}: {source}")]
    GroupDemand {
        code: String,
        group: String,
        source: InvalidGroupDemand,
    },
    #[error("program {code}: unknown type {kind:?}, expected \"lottery\" or \"screened\"")]
    UnknownSelectionKind { code: String, kind: String },
    #[error(transparent)]
    Program(#[from] InvalidProgram),
    #[error(transparent)]
    DuplicateProgram(#[from] DuplicateProgramCode),
    #[error("ranking: {0}")]
    Ranking(#[from] InvalidRanking),
    #[error("lottery number: {0}")]
    LotteryPrefix(#[from] InvalidLotteryPrefix),
    #[error("talent test percentile: {0}")]
    TalentTestPercentile(InvalidUnitInterval),
    #[error("talent test sd: {0}")]
    TalentTestSd(InvalidStandardDeviation),
    #[error("demand sd: {0}")]
    DemandSd(InvalidStandardDeviation),
    #[error("trials: {0}")]
    Trials(#[from] InvalidTrials),
    #[error("attrition: {0}")]
    Attrition(#[from] InvalidAttrition),
}

/// The `programs.json` document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgramsFileDto {
    #[serde(rename = "_notes", default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    pub programs: BTreeMap<String, ProgramDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgramDto {
    pub name: String,
    pub seats: u32,
    pub applicants: u32,
    /// `"lottery"` or `"screened"`.
    #[serde(rename = "type")]
    pub kind: String,
    pub groups: Vec<GroupDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admissions_method: Option<String>,
    #[serde(rename = "_note", default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupDto {
    pub name: String,
    #[serde(default)]
    pub requires: Vec<String>,
    pub applicants: u32,
    pub offers: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_offers: Option<u32>,
}

/// The `student.json` document and the JSON API student object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StudentDto {
    #[serde(rename = "_notes", default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    pub attributes: Vec<String>,
    pub ranking: Vec<String>,
    #[serde(default)]
    pub lottery_number: Option<String>,
    /// Expected rank among talent-test takers as a fraction, `0` best.
    #[serde(default, alias = "mark_twain_percentile")]
    pub talent_test_percentile: Option<f64>,
    /// Trial-to-trial noise on that fraction.
    #[serde(default, alias = "mark_twain_sd")]
    pub talent_test_sd: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WaitlistNumbersDto {
    #[default]
    Redrawn,
    Reused,
}

impl From<WaitlistNumbersDto> for WaitlistNumbers {
    fn from(dto: WaitlistNumbersDto) -> Self {
        match dto {
            WaitlistNumbersDto::Redrawn => WaitlistNumbers::Redrawn,
            WaitlistNumbersDto::Reused => WaitlistNumbers::Reused,
        }
    }
}

impl From<WaitlistNumbers> for WaitlistNumbersDto {
    fn from(value: WaitlistNumbers) -> Self {
        match value {
            WaitlistNumbers::Redrawn => WaitlistNumbersDto::Redrawn,
            WaitlistNumbers::Reused => WaitlistNumbersDto::Reused,
        }
    }
}

pub const DEFAULT_TRIALS: u32 = 200_000;
pub const DEFAULT_DEMAND_SD: f64 = 0.15;
pub const DEFAULT_ATTRITION: f64 = 1.0;

fn default_trials() -> u32 {
    DEFAULT_TRIALS
}

fn default_demand_sd() -> f64 {
    DEFAULT_DEMAND_SD
}

fn default_attrition() -> f64 {
    DEFAULT_ATTRITION
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ParametersDto {
    #[serde(default = "default_trials")]
    pub trials: u32,
    #[serde(default)]
    pub seed: u64,
    #[serde(default = "default_demand_sd")]
    pub demand_sd: f64,
    #[serde(default = "default_attrition")]
    pub attrition: f64,
    #[serde(default)]
    pub waitlist_numbers: WaitlistNumbersDto,
}

impl Default for ParametersDto {
    fn default() -> Self {
        Self {
            trials: DEFAULT_TRIALS,
            seed: 0,
            demand_sd: DEFAULT_DEMAND_SD,
            attrition: DEFAULT_ATTRITION,
            waitlist_numbers: WaitlistNumbersDto::Redrawn,
        }
    }
}

/// Body of `POST /api/simulate`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimulateRequestDto {
    pub student: StudentDto,
    #[serde(default)]
    pub parameters: ParametersDto,
    /// Program definitions for this run only. Omit to use the server's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub programs: Option<BTreeMap<String, ProgramDto>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgramRowDto {
    pub rank: usize,
    pub code: String,
    pub name: String,
    pub selection: String,
    pub group_name: Option<String>,
    pub initial_rate: f64,
    pub waitlist_rate: f64,
    pub p_initial: f64,
    pub p_waitlist: f64,
    pub p_any: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryDto {
    pub p_initial_offer: f64,
    pub p_any_offer: f64,
    pub p_no_offer: f64,
    pub p_lottery_offer: Option<f64>,
    pub p_screened_offer: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportDto {
    pub parameters: ParametersDto,
    pub lottery_fraction: Option<f64>,
    pub rows: Vec<ProgramRowDto>,
    pub summary: SummaryDto,
}

/// Body of `GET /api/scan`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanResponseDto {
    pub attributes: Vec<String>,
    pub frl: bool,
    pub lottery: String,
    pub lottery_fraction: f64,
    pub rows: Vec<ScanRowDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanRowDto {
    pub district: String,
    pub school: String,
    pub code: String,
    pub program: String,
    pub method: String,
    pub seats: Option<u32>,
    pub applicants: Option<u32>,
    pub filled: Option<bool>,
    pub group: String,
    pub group_order: u32,
    pub n_groups: usize,
    pub your_group_applied: Option<u32>,
    pub your_group_offered: Option<u32>,
    pub your_group_share: Option<f64>,
    pub reach: String,
    pub dbn: String,
    pub scores_year: Option<u32>,
    pub ela_proficient_pct: Option<f64>,
    pub math_proficient_pct: Option<f64>,
}

pub fn parse_attributes<'a>(
    raw: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeSet<Attribute>, InvalidInput> {
    raw.into_iter()
        .filter(|a| !a.trim().is_empty())
        .map(|a| Attribute::parse(a).map_err(InvalidInput::from))
        .collect()
}

pub fn parse_codes<'a>(
    raw: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<ProgramCode>, InvalidInput> {
    raw.into_iter()
        .filter(|c| !c.trim().is_empty())
        .map(|c| ProgramCode::parse(c).map_err(InvalidInput::from))
        .collect()
}

fn selection_kind(code: &str, kind: &str) -> Result<SelectionKind, InvalidInput> {
    match kind.trim().to_lowercase().as_str() {
        "lottery" => Ok(SelectionKind::Lottery),
        "screened" => Ok(SelectionKind::Screened),
        _ => Err(InvalidInput::UnknownSelectionKind {
            code: code.to_owned(),
            kind: kind.to_owned(),
        }),
    }
}

pub fn program_from_dto(code: &str, dto: ProgramDto) -> Result<Program, InvalidInput> {
    let program_code = ProgramCode::parse(code)?;
    let selection = selection_kind(code, &dto.kind)?;
    let groups =
        dto.groups
            .into_iter()
            .map(|g| {
                let demand = GroupDemand::new(g.applicants, g.offers, g.initial_offers).map_err(
                    |source| InvalidInput::GroupDemand {
                        code: code.to_owned(),
                        group: g.name.clone(),
                        source,
                    },
                )?;
                Ok(PriorityGroup {
                    name: g.name,
                    requires: parse_attributes(g.requires.iter().map(String::as_str))?
                        .into_iter()
                        .collect(),
                    demand,
                })
            })
            .collect::<Result<Vec<_>, InvalidInput>>()?;
    Ok(Program::new(
        program_code,
        dto.name,
        dto.seats,
        dto.applicants,
        selection,
        groups,
        dto.admissions_method,
        dto.note,
    )?)
}

pub fn program_definitions_from_dto(
    programs: BTreeMap<String, ProgramDto>,
) -> Result<ProgramDefinitions, InvalidInput> {
    let programs = programs
        .into_iter()
        .map(|(code, dto)| program_from_dto(&code, dto))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ProgramDefinitions::new(programs)?)
}

pub fn program_definitions_to_dto(definitions: &ProgramDefinitions) -> ProgramsFileDto {
    let programs = definitions
        .iter()
        .map(|p| {
            let dto = ProgramDto {
                name: p.name().to_owned(),
                seats: p.seats(),
                applicants: p.applicants(),
                kind: p.selection().label().to_owned(),
                groups: p
                    .groups()
                    .iter()
                    .map(|g| GroupDto {
                        name: g.name.clone(),
                        requires: g.requires.iter().map(|a| a.as_str().to_owned()).collect(),
                        applicants: g.demand.applicants(),
                        offers: g.demand.offers(),
                        initial_offers: Some(g.demand.initial_offers()),
                    })
                    .collect(),
                admissions_method: p.admissions_method().map(str::to_owned),
                note: p.note().map(str::to_owned),
            };
            (p.code().as_str().to_owned(), dto)
        })
        .collect();
    ProgramsFileDto {
        notes: vec![],
        programs,
    }
}

pub fn student_from_dto(dto: StudentDto) -> Result<Student, InvalidInput> {
    let attributes = with_boroughs(parse_attributes(dto.attributes.iter().map(String::as_str))?);
    let ranking = Ranking::new(parse_codes(dto.ranking.iter().map(String::as_str))?)?;
    let lottery_number = dto
        .lottery_number
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(LotteryPrefix::parse)
        .transpose()?;
    let talent_test_percentile = dto
        .talent_test_percentile
        .map(UnitInterval::new)
        .transpose()
        .map_err(InvalidInput::TalentTestPercentile)?;
    let talent_test_sd =
        StandardDeviation::new(dto.talent_test_sd).map_err(InvalidInput::TalentTestSd)?;
    Ok(Student {
        attributes,
        ranking,
        lottery_number,
        talent_test_percentile,
        talent_test_sd,
    })
}

pub fn student_to_dto(student: &Student) -> StudentDto {
    StudentDto {
        notes: vec![],
        attributes: student
            .attributes
            .iter()
            .map(|a| a.as_str().to_owned())
            .collect(),
        ranking: student
            .ranking
            .codes()
            .iter()
            .map(|c| c.as_str().to_owned())
            .collect(),
        lottery_number: student.lottery_number.as_ref().map(|l| l.to_string()),
        talent_test_percentile: student.talent_test_percentile.map(UnitInterval::get),
        talent_test_sd: student.talent_test_sd.get(),
    }
}

pub fn parameters_from_dto(dto: ParametersDto) -> Result<Parameters, InvalidInput> {
    Ok(Parameters {
        trials: Trials::new(dto.trials)?,
        seed: Seed::new(dto.seed),
        demand_sd: StandardDeviation::new(dto.demand_sd).map_err(InvalidInput::DemandSd)?,
        attrition: Attrition::new(dto.attrition)?,
        waitlist_numbers: dto.waitlist_numbers.into(),
    })
}

pub fn parameters_to_dto(parameters: &Parameters) -> ParametersDto {
    ParametersDto {
        trials: parameters.trials.get(),
        seed: parameters.seed.get(),
        demand_sd: parameters.demand_sd.get(),
        attrition: parameters.attrition.get(),
        waitlist_numbers: parameters.waitlist_numbers.into(),
    }
}

/// Rebuilds a report from its stored form, validating every value again
/// (REAS-DATA-001).
pub fn report_from_dto(dto: ReportDto) -> Result<Report, InvalidInput> {
    let parameters = parameters_from_dto(dto.parameters)?;
    let rows = dto
        .rows
        .into_iter()
        .map(|r| {
            Ok(ProgramRow {
                rank: r.rank,
                code: ProgramCode::parse(&r.code)?,
                selection: selection_kind(&r.code, &r.selection)?,
                name: r.name,
                group_name: r.group_name,
                initial_rate: r.initial_rate,
                waitlist_rate: r.waitlist_rate,
                p_initial: r.p_initial,
                p_waitlist: r.p_waitlist,
                p_any: r.p_any,
            })
        })
        .collect::<Result<Vec<_>, InvalidInput>>()?;
    Ok(Report {
        parameters,
        lottery_fraction: dto.lottery_fraction,
        rows,
        summary: Summary {
            p_initial_offer: dto.summary.p_initial_offer,
            p_any_offer: dto.summary.p_any_offer,
            p_no_offer: dto.summary.p_no_offer,
            p_lottery_offer: dto.summary.p_lottery_offer,
            p_screened_offer: dto.summary.p_screened_offer,
        },
    })
}

pub fn report_to_dto(report: &Report) -> ReportDto {
    ReportDto {
        parameters: parameters_to_dto(&report.parameters),
        lottery_fraction: report.lottery_fraction,
        rows: report
            .rows
            .iter()
            .map(|r| ProgramRowDto {
                rank: r.rank,
                code: r.code.as_str().to_owned(),
                name: r.name.clone(),
                selection: r.selection.label().to_owned(),
                group_name: r.group_name.clone(),
                initial_rate: r.initial_rate,
                waitlist_rate: r.waitlist_rate,
                p_initial: r.p_initial,
                p_waitlist: r.p_waitlist,
                p_any: r.p_any,
            })
            .collect(),
        summary: SummaryDto {
            p_initial_offer: report.summary.p_initial_offer,
            p_any_offer: report.summary.p_any_offer,
            p_no_offer: report.summary.p_no_offer,
            p_lottery_offer: report.summary.p_lottery_offer,
            p_screened_offer: report.summary.p_screened_offer,
        },
    }
}

pub fn scan_row_to_dto(row: &ScanRow) -> ScanRowDto {
    ScanRowDto {
        district: row.district.as_str().to_owned(),
        school: row.school_name.clone(),
        code: row.code.as_str().to_owned(),
        program: row.program_name.clone(),
        method: row.method.clone(),
        seats: row.seats,
        applicants: row.applicants,
        filled: row.all_seats_filled,
        group: row.group_name.clone(),
        group_order: row.group_order,
        n_groups: row.group_count,
        your_group_applied: row.group_applied,
        your_group_offered: row.group_offered,
        your_group_share: row.group_share,
        reach: row.reach.label().to_owned(),
        dbn: row.dbn.clone(),
        scores_year: row.scores.map(|s| s.year),
        ela_proficient_pct: row.scores.and_then(|s| s.ela_proficient_pct),
        math_proficient_pct: row.scores.and_then(|s| s.math_proficient_pct),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(
        name: &str,
        requires: &[&str],
        applicants: u32,
        offers: u32,
        initial: Option<u32>,
    ) -> GroupDto {
        GroupDto {
            name: name.to_owned(),
            requires: requires.iter().map(|r| (*r).to_owned()).collect(),
            applicants,
            offers,
            initial_offers: initial,
        }
    }

    fn program_dto(kind: &str) -> ProgramDto {
        ProgramDto {
            name: "M.S. 442".to_owned(),
            seats: 81,
            applicants: 494,
            kind: kind.to_owned(),
            groups: vec![
                group("siblings", &["sibling_K442S"], 5, 5, Some(5)),
                group("District 15", &["d15"], 370, 40, Some(34)),
            ],
            admissions_method: Some("Open".to_owned()),
            note: None,
        }
    }

    #[test]
    fn program_dto_converts_and_round_trips() {
        let mut map = BTreeMap::new();
        map.insert("K442S".to_owned(), program_dto("lottery"));
        let definitions = program_definitions_from_dto(map).expect("valid");
        let program = definitions
            .get(&ProgramCode::parse("K442S").expect("valid"))
            .expect("present");
        assert_eq!(program.selection(), SelectionKind::Lottery);
        assert_eq!(program.groups()[1].demand.initial_offers(), 34);
        let back = program_definitions_to_dto(&definitions);
        assert_eq!(back.programs["K442S"], program_dto("lottery"));
    }

    #[test]
    fn program_dto_rejects_an_unknown_type() {
        let err = program_from_dto("K442S", program_dto("audition")).expect_err("invalid");
        assert_eq!(
            err,
            InvalidInput::UnknownSelectionKind {
                code: "K442S".to_owned(),
                kind: "audition".to_owned()
            }
        );
    }

    #[test]
    fn program_dto_reports_the_group_with_bad_figures() {
        let mut dto = program_dto("lottery");
        dto.groups[1].offers = 400;
        let err = program_from_dto("K442S", dto).expect_err("invalid");
        assert!(matches!(
            err,
            InvalidInput::GroupDemand { ref code, ref group, .. } if code == "K442S" && group == "District 15"
        ));
    }

    #[test]
    fn student_json_from_the_python_project_parses() {
        let json = r#"{
          "_notes": ["x"],
          "lottery_number": "BD",
          "attributes": ["d20", "d15", "mark_twain_test"],
          "ranking": ["K187A", "K239SC"],
          "mark_twain_percentile": 0.10,
          "mark_twain_sd": 0.10
        }"#;
        let dto: StudentDto = serde_json::from_str(json).expect("parses");
        let student = student_from_dto(dto).expect("valid");
        assert_eq!(
            student.lottery_number.as_ref().map(|l| l.as_str()),
            Some("bd")
        );
        assert_eq!(
            student.talent_test_percentile.map(UnitInterval::get),
            Some(0.10)
        );
        assert_eq!(student.talent_test_sd.get(), 0.10);
        assert_eq!(student.ranking.len(), 2);
        let back = student_to_dto(&student);
        assert_eq!(back.lottery_number.as_deref(), Some("BD"));
        assert_eq!(
            back.attributes,
            vec!["borough_brooklyn", "d15", "d20", "mark_twain_test"]
        );
    }

    #[test]
    fn student_dto_treats_a_blank_lottery_number_as_unknown() {
        let dto = StudentDto {
            notes: vec![],
            attributes: vec!["d15".to_owned(), "".to_owned()],
            ranking: vec!["K1".to_owned()],
            lottery_number: Some("  ".to_owned()),
            talent_test_percentile: None,
            talent_test_sd: 0.0,
        };
        let student = student_from_dto(dto).expect("valid");
        assert_eq!(student.lottery_number, None);
        // d15 plus the derived borough attribute.
        assert_eq!(student.attributes.len(), 2);
    }

    #[test]
    fn student_dto_rejects_a_bad_percentile() {
        let dto = StudentDto {
            notes: vec![],
            attributes: vec![],
            ranking: vec!["K1".to_owned()],
            lottery_number: None,
            talent_test_percentile: Some(1.5),
            talent_test_sd: 0.0,
        };
        let err = student_from_dto(dto).expect_err("invalid");
        assert_eq!(
            err,
            InvalidInput::TalentTestPercentile(InvalidUnitInterval::OutOfRange(1.5))
        );
    }

    #[test]
    fn parameters_default_like_the_python_cli() {
        let dto: ParametersDto = serde_json::from_str("{}").expect("parses");
        assert_eq!(dto, ParametersDto::default());
        let parameters = parameters_from_dto(dto).expect("valid");
        assert_eq!(parameters.trials.get(), 200_000);
        assert_eq!(parameters.demand_sd.get(), 0.15);
        assert_eq!(parameters.waitlist_numbers, WaitlistNumbers::Redrawn);
        let reused: ParametersDto =
            serde_json::from_str(r#"{"waitlist_numbers": "reused", "trials": 10}"#)
                .expect("parses");
        assert_eq!(
            parameters_from_dto(reused).expect("valid").waitlist_numbers,
            WaitlistNumbers::Reused
        );
    }

    #[test]
    fn parameters_reject_zero_trials() {
        let dto = ParametersDto {
            trials: 0,
            ..ParametersDto::default()
        };
        assert_eq!(
            parameters_from_dto(dto),
            Err(InvalidInput::Trials(InvalidTrials::Zero))
        );
    }

    #[test]
    fn report_dto_round_trips() {
        let report = Report {
            parameters: parameters_from_dto(ParametersDto::default()).expect("valid"),
            lottery_fraction: Some(0.5),
            rows: vec![ProgramRow {
                rank: 1,
                code: ProgramCode::parse("K1").expect("valid"),
                name: "One".to_owned(),
                selection: SelectionKind::Screened,
                group_name: None,
                initial_rate: 0.1,
                waitlist_rate: 0.2,
                p_initial: 0.3,
                p_waitlist: 0.4,
                p_any: 0.7,
            }],
            summary: Summary {
                p_initial_offer: 0.3,
                p_any_offer: 0.7,
                p_no_offer: 0.3,
                p_lottery_offer: None,
                p_screened_offer: Some(0.7),
            },
        };
        let dto = report_to_dto(&report);
        let json = serde_json::to_string(&dto).expect("serializes");
        let back: ReportDto = serde_json::from_str(&json).expect("parses");
        assert_eq!(report_from_dto(back).expect("valid"), report);
    }
}
