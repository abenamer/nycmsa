//! Outbound adapter: the MySchools catalog JSON written by `nycmsa fetch`
//! (or the Python `fetch_myschools.py`).
//!
//! MySchools publishes last year's demand per priority group as prose, for
//! example "There were about 230 additional applicants in this group and
//! about 140 received offers last year." This module turns that prose into
//! [`GroupDemandText`] and the group name into a [`PriorityCriterion`]. The
//! domain then applies the eligibility and reachability rules.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

use crate::domain::catalog::InvalidDistrictCode;
use crate::domain::{
    Borough, CatalogGroup, CatalogProgram, DemandPair, DistrictCode, GeoPoint, GroupDemandText,
    InvalidProgramCode, PriorityCriterion, ProgramCode, ProgramProfile,
};

#[derive(Debug, thiserror::Error)]
pub enum CatalogLoadError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("catalog is not valid MySchools JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("school {school:?} has no district code")]
    MissingDistrict { school: String },
    #[error("school {school:?}: {source}")]
    InvalidDistrict {
        school: String,
        #[source]
        source: InvalidDistrictCode,
    },
    #[error("school {school:?}: {source}")]
    InvalidProgramCode {
        school: String,
        #[source]
        source: InvalidProgramCode,
    },
}

#[derive(Debug, Deserialize)]
struct SchoolEntryDto {
    school: SchoolDto,
    #[serde(default)]
    programs: Vec<ProgramEntryDto>,
}

#[derive(Debug, Deserialize)]
struct SchoolDto {
    name: String,
    #[serde(default)]
    dbn: String,
    district: Option<DistrictDto>,
    #[serde(default)]
    address: Option<AddressDto>,
    #[serde(default)]
    full_address: Option<String>,
}

/// The geocoded address; MySchools gives the coordinates as strings.
#[derive(Debug, Deserialize)]
struct AddressDto {
    latitude: Option<String>,
    longitude: Option<String>,
}

/// The school's coordinates, or `None` when the catalog has none, has an
/// unparsable value, or has the `0, 0` placeholder an ungeocoded address
/// carries. A location is optional data for a cosmetic column, so a bad
/// value drops the point rather than failing the catalog load.
/// `500 19 STREET, BROOKLYN, NY 11215` as MySchools prints it becomes
/// `500 19 Street, Brooklyn, NY 11215`: the state and ZIP keep their case.
fn address_case(raw: &str) -> String {
    let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
    let n = parts.len();
    parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            if i + 1 == n && n > 1 {
                (*part).to_owned()
            } else {
                // Ordinals such as 86TH keep a lowercase suffix.
                title_case(part)
                    .split(' ')
                    .map(|w| {
                        if w.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                            w.to_lowercase()
                        } else {
                            w.to_owned()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn location_of(address: Option<&AddressDto>) -> Option<GeoPoint> {
    let address = address?;
    let latitude: f64 = address.latitude.as_deref()?.trim().parse().ok()?;
    let longitude: f64 = address.longitude.as_deref()?.trim().parse().ok()?;
    if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
        return None;
    }
    if latitude == 0.0 && longitude == 0.0 {
        return None;
    }
    Some(GeoPoint {
        latitude,
        longitude,
    })
}

#[derive(Debug, Deserialize)]
struct DistrictDto {
    code: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ProgramEntryDto {
    program: ProgramRefDto,
    name: String,
    admissions_method: Option<MethodDto>,
    demand_last_year: Option<DemandDto>,
    #[serde(default)]
    program_priority_groups: Option<Vec<GroupEntryDto>>,
}

#[derive(Debug, Deserialize)]
struct ProgramRefDto {
    code: String,
}

#[derive(Debug, Deserialize)]
struct MethodDto {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DemandDto {
    general_education: Option<GeneralEducationDto>,
}

#[derive(Debug, Deserialize)]
struct GeneralEducationDto {
    seats: Option<u32>,
    applicants: Option<u32>,
    all_seats_filled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct GroupEntryDto {
    order: u32,
    name: String,
    ge_priority_group_description: Option<String>,
}

/// Loads a catalog file.
pub fn load_catalog(path: &Path) -> Result<Vec<CatalogProgram>, CatalogLoadError> {
    let text = std::fs::read_to_string(path).map_err(|source| CatalogLoadError::Io {
        path: path.to_owned(),
        source,
    })?;
    parse_catalog(&text)
}

/// Parses the MySchools JSON array of schools.
pub fn parse_catalog(text: &str) -> Result<Vec<CatalogProgram>, CatalogLoadError> {
    let schools: Vec<SchoolEntryDto> = serde_json::from_str(text)?;
    catalog_from_entries(schools)
}

/// Builds the catalog from the stored school documents.
pub fn catalog_from_documents(
    documents: &[serde_json::Value],
) -> Result<Vec<CatalogProgram>, CatalogLoadError> {
    let schools = documents
        .iter()
        .map(|d| serde_json::from_value::<SchoolEntryDto>(d.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    catalog_from_entries(schools)
}

fn catalog_from_entries(
    schools: Vec<SchoolEntryDto>,
) -> Result<Vec<CatalogProgram>, CatalogLoadError> {
    let mut programs = Vec::new();
    for school in schools {
        let school_name = school.school.name;
        let dbn = school.school.dbn.trim().to_uppercase();
        let location = location_of(school.school.address.as_ref());
        let address = school.school.full_address.as_deref().map(address_case);
        let raw_code = school
            .school
            .district
            .and_then(|d| d.code)
            .map(|code| match code {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            })
            .ok_or_else(|| CatalogLoadError::MissingDistrict {
                school: school_name.clone(),
            })?;
        let district =
            DistrictCode::parse(&raw_code).map_err(|source| CatalogLoadError::InvalidDistrict {
                school: school_name.clone(),
                source,
            })?;
        for entry in school.programs {
            let code = ProgramCode::parse(&entry.program.code).map_err(|source| {
                CatalogLoadError::InvalidProgramCode {
                    school: school_name.clone(),
                    source,
                }
            })?;
            let general = entry.demand_last_year.and_then(|d| d.general_education);
            let groups = entry
                .program_priority_groups
                .unwrap_or_default()
                .into_iter()
                .map(|g| CatalogGroup {
                    order: g.order,
                    criterion: classify_group_name(&g.name),
                    demand: parse_group_description(g.ge_priority_group_description.as_deref()),
                    name: g.name,
                })
                .collect();
            programs.push(
                CatalogProgram::new(
                    district.clone(),
                    dbn.clone(),
                    title_case(before_parenthesis(&school_name)),
                    code,
                    before_parenthesis(&entry.name).to_owned(),
                    entry
                        .admissions_method
                        .and_then(|m| m.name)
                        .unwrap_or_default(),
                    general.as_ref().and_then(|g| g.seats),
                    general.as_ref().and_then(|g| g.applicants),
                    general.as_ref().and_then(|g| g.all_seats_filled),
                    groups,
                )
                .with_location(location)
                .with_address(address.clone()),
            );
        }
    }
    Ok(programs)
}

/// The text before the first ` (`, which MySchools uses for the code suffix.
fn before_parenthesis(name: &str) -> &str {
    name.split(" (").next().unwrap_or(name)
}

/// Python `str.title()`: each letter that follows a non-letter is uppercased
/// and every other letter is lowercased.
pub fn title_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_is_letter = false;
    for c in text.chars() {
        if c.is_alphabetic() {
            if previous_is_letter {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
            previous_is_letter = true;
        } else {
            out.push(c);
            previous_is_letter = false;
        }
    }
    out
}

static QUANTITY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"about (\d+)|fewer than 10").expect("valid regex"));
static OFFERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(about \d+|fewer than 10) received offers").expect("valid regex")
});
static SENTENCE_BREAK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.\s+").expect("valid regex"));
static DISTRICT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bdistrict (\d+)|\bd(\d+)\b").expect("valid regex"));

/// The first quantity phrase in lowercase text: "about N" gives `N`, "fewer
/// than 10" gives `5`.
pub fn quantity(text: &str) -> Option<u32> {
    let captures = QUANTITY.captures(text)?;
    match captures.get(1) {
        Some(digits) => digits.as_str().parse().ok(),
        None => Some(5),
    }
}

/// Applicants and offers from one sentence; `None` when the sentence
/// reports no applicants.
pub fn parse_pair(sentence: &str) -> Option<DemandPair> {
    let text = sentence.to_lowercase();
    if text.contains("no applicants") || text.contains("were no ") {
        return None;
    }
    if text.contains("all seats") && !text.contains("applicant") {
        return None;
    }
    let applicants = quantity(&text)?;
    let offers = if text.contains("almost all received") {
        Some((f64::from(applicants) * 0.95).round_ties_even() as u32)
    } else if text.contains("all received") {
        Some(applicants)
    } else if text.contains("none received") || text.contains("no offers") {
        Some(0)
    } else {
        OFFERS
            .captures(&text)
            .and_then(|c| c.get(1))
            .and_then(|m| quantity(m.as_str()))
    };
    Some(DemandPair { applicants, offers })
}

/// Splits a group description into sentences and files each one under the
/// FRL slice, the non-FRL ("additional applicants") slice, or the whole
/// group.
pub fn parse_group_description(description: Option<&str>) -> GroupDemandText {
    let mut out = GroupDemandText::default();
    let Some(description) = description else {
        return out;
    };
    for sentence in SENTENCE_BREAK
        .split(description)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let lower = sentence.to_lowercase();
        if lower.contains("free or reduced") {
            out.frl = parse_pair(sentence);
        } else if lower.contains("additional applicants") {
            out.non_frl = parse_pair(sentence);
        } else {
            out.all = parse_pair(sentence);
        }
    }
    out
}

/// Maps a MySchools priority-group name to its eligibility rule.
pub fn classify_group_name(name: &str) -> PriorityCriterion {
    let n = name.to_lowercase();
    if n.contains("sibling") {
        return PriorityCriterion::Sibling;
    }
    if n.contains("continuing") {
        return PriorityCriterion::Continuing;
    }
    if n.contains("zoned") || n.contains("zone") {
        return PriorityCriterion::Zoned;
    }
    if n.contains("currently attend") || (n.contains("students in") && !n.contains("district")) {
        return PriorityCriterion::CurrentStudent;
    }
    if let Some(code) = DISTRICT
        .captures(&n)
        .and_then(|c| c.get(1).or_else(|| c.get(2)))
        .and_then(|m| DistrictCode::parse(m.as_str()).ok())
    {
        return PriorityCriterion::District(code);
    }
    if n.contains("district") {
        return PriorityCriterion::Unrecognized;
    }
    let boroughs = [
        ("staten island", Borough::StatenIsland),
        ("manhattan", Borough::Manhattan),
        ("bronx", Borough::Bronx),
        ("brooklyn", Borough::Brooklyn),
        ("queens", Borough::Queens),
    ];
    if let Some((_, borough)) = boroughs.iter().find(|(needle, _)| n.contains(needle)) {
        return PriorityCriterion::Borough(*borough);
    }
    if n.contains("new york city") || n.contains("nyc") || n.contains("open to") {
        return PriorityCriterion::AnyNycResident;
    }
    PriorityCriterion::Unrecognized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(applicants: u32, offers: Option<u32>) -> Option<DemandPair> {
        Some(DemandPair { applicants, offers })
    }

    #[test]
    fn quantity_reads_about_and_fewer_than_ten() {
        assert_eq!(
            quantity("there were about 230 additional applicants"),
            Some(230)
        );
        assert_eq!(quantity("there were fewer than 10 applicants"), Some(5));
        assert_eq!(quantity("there were no applicants"), None);
    }

    #[test]
    fn parse_pair_matches_the_python_rules() {
        assert_eq!(
            parse_pair("There were about 230 additional applicants in this group and about 140 received offers last year."),
            pair(230, Some(140))
        );
        assert_eq!(
            parse_pair(
                "There were about 70 applicants in this group and all received offers last year."
            ),
            pair(70, Some(70))
        );
        assert_eq!(
            parse_pair("There were about 40 applicants eligible for free or reduced price lunch in this group and almost all received offers last year."),
            pair(40, Some(38))
        );
        // Python round() is round-half-to-even: 230 * 0.95 = 218.5 -> 218.
        assert_eq!(
            parse_pair("There were about 230 applicants in this group and almost all received offers last year."),
            pair(230, Some(218))
        );
        assert_eq!(
            parse_pair(
                "There were about 40 applicants in this group and none received offers last year."
            ),
            pair(40, Some(0))
        );
        assert_eq!(
            parse_pair("There were fewer than 10 additional applicants in this group and fewer than 10 received offers last year."),
            pair(5, Some(5))
        );
        assert_eq!(parse_pair("There were no applicants in this group."), None);
        assert_eq!(parse_pair("All seats were filled."), None);
        assert_eq!(
            parse_pair("There were about 12 applicants in this group."),
            pair(12, None)
        );
    }

    #[test]
    fn group_description_splits_frl_and_non_frl_sentences() {
        let text = "There were about 50 applicants eligible for free or reduced price lunch in this group and about 40 received offers last year. There were about 370 additional applicants in this group and about 40 received offers last year.";
        let parsed = parse_group_description(Some(text));
        assert_eq!(parsed.frl, pair(50, Some(40)));
        assert_eq!(parsed.non_frl, pair(370, Some(40)));
        assert_eq!(parsed.all, None);
        let whole = parse_group_description(Some(
            "There were about 680 applicants in this group and about 90 received offers last year.",
        ));
        assert_eq!(whole.all, pair(680, Some(90)));
        assert_eq!(parse_group_description(None), GroupDemandText::default());
    }

    #[test]
    fn group_names_map_to_criteria() {
        assert_eq!(
            classify_group_name(
                "Priority to applicants whose sibling(s) will be enrolled in this school next year"
            ),
            PriorityCriterion::Sibling
        );
        assert_eq!(
            classify_group_name("Then to continuing students"),
            PriorityCriterion::Continuing
        );
        assert_eq!(
            classify_group_name("Then to students residing in the zone"),
            PriorityCriterion::Zoned
        );
        assert_eq!(
            classify_group_name("Then to students and residents of District 15"),
            PriorityCriterion::District(DistrictCode::parse("15").expect("valid"))
        );
        assert_eq!(
            classify_group_name("Priority to District 20 students and residents"),
            PriorityCriterion::District(DistrictCode::parse("20").expect("valid"))
        );
        assert_eq!(
            classify_group_name("Then to New York City residents"),
            PriorityCriterion::AnyNycResident
        );
        assert_eq!(
            classify_group_name("Priority to students who currently attend this school"),
            PriorityCriterion::CurrentStudent
        );
        assert_eq!(
            classify_group_name("Then to students in this school"),
            PriorityCriterion::CurrentStudent
        );
        assert_eq!(
            classify_group_name("Then to students in District 15 schools"),
            PriorityCriterion::District(DistrictCode::parse("15").expect("valid"))
        );
        assert_eq!(
            classify_group_name("Then to Brooklyn students"),
            PriorityCriterion::Borough(Borough::Brooklyn)
        );
    }

    #[test]
    fn title_case_matches_python() {
        assert_eq!(
            title_case("THE CHRISTA MCAULIFFE SCHOOL\\I.S. 187"),
            "The Christa Mcauliffe School\\I.S. 187"
        );
        assert_eq!(
            title_case("P.S./I.S. 104 THE FORT HAMILTON SCHOOL"),
            "P.S./I.S. 104 The Fort Hamilton School"
        );
        assert_eq!(title_case("o'dewey 1st"), "O'Dewey 1St");
    }

    #[test]
    fn parse_catalog_reads_a_minimal_document() {
        let json = r#"[{
          "school": {"name": "J.H.S. 062 DITMAS (20K062)", "dbn": "20K062", "district": {"code": "20"}},
          "programs": [{
            "program": {"code": "K062Z"},
            "name": "Zoned Program (K062Z)",
            "admissions_method": {"name": "Zone Priority"},
            "demand_last_year": {"general_education": {"seats": 209, "applicants": 99, "all_seats_filled": false}},
            "program_priority_groups": [
              {"order": 2, "name": "Then to students residing in the zone", "ge_priority_group_description": "There were about 70 applicants in this group and all received offers last year."},
              {"order": 1, "name": "Priority to applicants whose sibling(s) will be enrolled", "ge_priority_group_description": "There were about 10 applicants in this group and all received offers last year."}
            ]
          }]
        }]"#;
        let catalog = parse_catalog(json).expect("parses");
        assert_eq!(catalog.len(), 1);
        let program = &catalog[0];
        assert_eq!(program.school_name, "J.H.S. 062 Ditmas");
        assert_eq!(program.dbn, "20K062");
        assert_eq!(program.program_name, "Zoned Program");
        assert_eq!(program.method, "Zone Priority");
        assert_eq!(program.seats, Some(209));
        assert_eq!(program.groups()[0].order, 1);
        assert_eq!(program.groups()[1].criterion, PriorityCriterion::Zoned);
        assert_eq!(program.groups()[1].demand.all, pair(70, Some(70)));
    }

    #[test]
    fn parse_catalog_rejects_a_school_without_a_district() {
        let json = r#"[{"school": {"name": "X"}, "programs": []}]"#;
        let err = parse_catalog(json).expect_err("missing district");
        assert!(
            matches!(err, CatalogLoadError::MissingDistrict { .. }),
            "{err}"
        );
    }

    #[test]
    fn detail_fixture_gives_labelled_texts_per_program() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/myschools-detail-20K187.json");
        let document: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("reads")).expect("json");
        let details = program_details_from_document(&document).expect("parses");
        assert_eq!(details.len(), 6);
        let k187a = details
            .iter()
            .find(|d| d.code.as_str() == "K187A")
            .expect("present");
        assert_eq!(k187a.dbn, "20K187");
        assert_eq!(k187a.program_name, "Academy for Scientific Research");
        assert_eq!(k187a.method, "Screened");
        assert!(k187a
            .overview
            .starts_with("Our school is a place of wonder"));
        assert!(k187a.art_courses.contains("Chorus"));
        let k187c = details
            .iter()
            .find(|d| d.code.as_str() == "K187C")
            .expect("present");
        assert!(k187c.description.starts_with("Pending approval"));
    }

    #[test]
    fn the_fixture_catalog_loads() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/myschools_d15_d20.json");
        let d15_d20 = load_catalog(&path).expect("loads");
        assert!(d15_d20.len() > 50, "{}", d15_d20.len());
        let located = d15_d20.iter().filter(|p| p.location.is_some()).count();
        assert_eq!(located, d15_d20.len(), "every fixture school is geocoded");
        let ms442 = d15_d20
            .iter()
            .find(|p| p.dbn == "15K442")
            .expect("M.S. 442 is in the fixture")
            .location
            .expect("geocoded");
        assert!((ms442.latitude - 40.6562).abs() < 0.001, "{ms442:?}");
        let address = d15_d20
            .iter()
            .find(|p| p.dbn == "15K442")
            .and_then(|p| p.address.as_deref())
            .expect("M.S. 442 has an address");
        assert!(address.contains("Brooklyn, NY"), "{address}");
        assert!((ms442.longitude + 73.9826).abs() < 0.001, "{ms442:?}");
        let text = std::fs::read_to_string(&path).expect("reads");
        let documents: Vec<serde_json::Value> = serde_json::from_str(&text).expect("json");
        assert_eq!(catalog_from_documents(&documents).expect("parses"), d15_d20);
    }

    #[test]
    fn a_location_needs_two_real_coordinates() {
        let point = |lat: &str, lon: &str| {
            location_of(Some(&AddressDto {
                latitude: Some(lat.to_owned()),
                longitude: Some(lon.to_owned()),
            }))
        };
        assert!(point("40.6562", "-73.982625").is_some());
        assert_eq!(point("0", "0"), None, "the ungeocoded placeholder");
        assert_eq!(point("91", "-73.9"), None, "out of range");
        assert_eq!(point("forty", "-73.9"), None, "not a number");
        assert_eq!(point("NaN", "-73.9"), None);
        assert_eq!(location_of(None), None);
    }

    #[test]
    fn addresses_are_title_cased_except_the_state_and_zip() {
        assert_eq!(
            address_case("500 19 STREET, BROOKLYN, NY 11215"),
            "500 19 Street, Brooklyn, NY 11215"
        );
        assert_eq!(address_case("610 HENRY STREET"), "610 Henry Street");
        assert_eq!(
            address_case("650 86TH STREET, BROOKLYN, NY 11228"),
            "650 86th Street, Brooklyn, NY 11228"
        );
    }
}

/// The fields of a MySchools school detail document that describe its
/// programs.
#[derive(Debug, Deserialize)]
struct DetailDto {
    school: SchoolDto,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    academic_opportunities: Vec<NamedDto>,
    #[serde(default)]
    art_courses: Vec<NamedDto>,
    #[serde(default)]
    language_classes: Vec<NamedDto>,
    #[serde(default)]
    programs: Vec<DetailProgramDto>,
}

#[derive(Debug, Deserialize)]
struct NamedDto {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DetailProgramDto {
    program: ProgramRefDto,
    name: String,
    admissions_method: Option<MethodDto>,
    #[serde(default)]
    description: Option<String>,
}

fn names(items: &[NamedDto]) -> String {
    items
        .iter()
        .filter_map(|i| i.name.as_deref())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Turns a school detail document into one unscored [`ProgramProfile`] per
/// program. The use case scores it from the stored texts, so a fresh fetch
/// and a reclassification read the same texts.
pub fn program_details_from_document(
    document: &serde_json::Value,
) -> Result<Vec<ProgramProfile>, CatalogLoadError> {
    let detail: DetailDto = serde_json::from_value(document.clone())?;
    let school_name = title_case(before_parenthesis(&detail.school.name));
    let dbn = detail.school.dbn.trim().to_uppercase();
    let overview = detail.overview.unwrap_or_default().trim().to_owned();
    let academic_opportunities = names(&detail.academic_opportunities);
    let art_courses = names(&detail.art_courses);
    let language_classes = names(&detail.language_classes);
    detail
        .programs
        .into_iter()
        .map(|p| {
            let code = ProgramCode::parse(&p.program.code).map_err(|source| {
                CatalogLoadError::InvalidProgramCode {
                    school: school_name.clone(),
                    source,
                }
            })?;
            Ok(ProgramProfile {
                code,
                dbn: dbn.clone(),
                program_name: before_parenthesis(&p.name).to_owned(),
                school_name: school_name.clone(),
                method: p.admissions_method.and_then(|m| m.name).unwrap_or_default(),
                description: p.description.unwrap_or_default().trim().to_owned(),
                overview: overview.clone(),
                academic_opportunities: academic_opportunities.clone(),
                art_courses: art_courses.clone(),
                language_classes: language_classes.clone(),
                scores: Vec::new(),
                evidence: Vec::new(),
            })
        })
        .collect()
}
