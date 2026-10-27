//! School-level demand history from the DOE's reports under Local Law 72
//! of 2018 (formerly 1486-A): for each school and admissions year, the
//! grade 6 seats, the applicants who listed the school, the applicants who
//! were considered there, and the offers. Pure values; the adapters read
//! the workbooks.
//!
//! The DOE counts "applicants" two ways. The report's total applicants
//! are every child who listed the school anywhere on an application of up
//! to twelve choices. Its "true" applicants, published from fall 2023 on,
//! are the children who were considered for a seat: those who did not
//! match to a more preferred choice. MySchools shows the second figure per
//! program, so the catalog's applicants are already considered applicants.

use std::collections::BTreeMap;

/// Grade 6 figures for one school in one admissions year. `None` is a
/// figure the DOE suppressed or did not publish that year.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DemandFigures {
    pub seats: Option<u32>,
    /// Applicants who listed the school anywhere on their application.
    pub listed: Option<u32>,
    /// Applicants who were considered for a seat there: those not matched
    /// to a more preferred choice. Published from fall 2023 on.
    pub considered: Option<u32>,
    pub offers: Option<u32>,
}

impl DemandFigures {
    pub fn is_empty(&self) -> bool {
        self.seats.is_none()
            && self.listed.is_none()
            && self.considered.is_none()
            && self.offers.is_none()
    }

    /// Offers per considered applicant, capped at 1, when both are known
    /// and there was at least one considered applicant.
    pub fn offer_share(&self) -> Option<f64> {
        let considered = self.considered.filter(|c| *c > 0)?;
        let offers = self.offers?;
        Some((f64::from(offers) / f64::from(considered)).min(1.0))
    }

    /// Listed applicants per seat, when both are known and there is at
    /// least one seat.
    pub fn listed_per_seat(&self) -> Option<f64> {
        let seats = self.seats.filter(|s| *s > 0)?;
        let listed = self.listed?;
        Some(f64::from(listed) / f64::from(seats))
    }
}

/// One school's figures for one admissions year, overall and by the
/// applicants' residential district.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SchoolDemandYear {
    /// The fall of entry: 2025 for the cycle whose offers went out in the
    /// spring of 2025.
    pub year: u32,
    pub all: DemandFigures,
    /// Keyed by the residential district code without leading zeros, such
    /// as `15`. Seats are never split by district, so `seats` is `None`.
    pub by_residential_district: BTreeMap<String, DemandFigures>,
}

/// The latest year's overall figures, as a scan row carries them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatestDemand {
    pub year: u32,
    pub figures: DemandFigures,
}

/// Every year for every school, keyed by DBN, each school's years in
/// ascending order with each year once.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DemandHistory {
    by_dbn: BTreeMap<String, Vec<SchoolDemandYear>>,
}

impl DemandHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one school-year, replacing the same year when present.
    pub fn insert(&mut self, dbn: &str, year: SchoolDemandYear) {
        let years = self.by_dbn.entry(dbn.to_uppercase()).or_default();
        match years.binary_search_by_key(&year.year, |y| y.year) {
            Ok(index) => years[index] = year,
            Err(index) => years.insert(index, year),
        }
    }

    /// Adds every school-year of `other`, newer entries replacing older
    /// ones for the same school and year.
    pub fn extend(&mut self, other: DemandHistory) {
        for (dbn, years) in other.by_dbn {
            for year in years {
                self.insert(&dbn, year);
            }
        }
    }

    /// A school's years, ascending; empty for an unknown DBN.
    pub fn years_of(&self, dbn: &str) -> &[SchoolDemandYear] {
        self.by_dbn.get(dbn).map_or(&[], Vec::as_slice)
    }

    /// The latest year's figures for a school.
    pub fn latest(&self, dbn: &str) -> Option<LatestDemand> {
        self.years_of(dbn).last().map(|y| LatestDemand {
            year: y.year,
            figures: y.all,
        })
    }

    /// The latest year across every school.
    pub fn latest_year(&self) -> Option<u32> {
        self.by_dbn
            .values()
            .filter_map(|years| years.last().map(|y| y.year))
            .max()
    }

    /// Every year present, ascending, each once.
    pub fn years(&self) -> Vec<u32> {
        let mut years: Vec<u32> = self
            .by_dbn
            .values()
            .flat_map(|ys| ys.iter().map(|y| y.year))
            .collect();
        years.sort_unstable();
        years.dedup();
        years
    }

    /// Number of schools with at least one year.
    pub fn schools(&self) -> usize {
        self.by_dbn.len()
    }

    /// Number of school-years.
    pub fn len(&self) -> usize {
        self.by_dbn.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.by_dbn.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &SchoolDemandYear)> {
        self.by_dbn
            .iter()
            .flat_map(|(dbn, years)| years.iter().map(move |y| (dbn.as_str(), y)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn year(y: u32, listed: u32, considered: Option<u32>, offers: u32) -> SchoolDemandYear {
        SchoolDemandYear {
            year: y,
            all: DemandFigures {
                seats: Some(100),
                listed: Some(listed),
                considered,
                offers: Some(offers),
            },
            by_residential_district: BTreeMap::new(),
        }
    }

    #[test]
    fn years_stay_sorted_and_a_repeated_year_is_replaced() {
        let mut history = DemandHistory::new();
        history.insert("15K442", year(2025, 1585, Some(564), 138));
        history.insert("15k442", year(2019, 1400, None, 120));
        history.insert("15K442", year(2025, 1600, Some(570), 140));
        let years: Vec<u32> = history.years_of("15K442").iter().map(|y| y.year).collect();
        assert_eq!(years, vec![2019, 2025]);
        assert_eq!(
            history.latest("15K442").map(|l| l.figures.listed),
            Some(Some(1600))
        );
        assert_eq!(history.latest_year(), Some(2025));
        assert_eq!(history.years(), vec![2019, 2025]);
        assert_eq!(history.schools(), 1);
        assert_eq!(history.len(), 2);
        assert!(history.years_of("20K062").is_empty());
        assert_eq!(history.latest("20K062"), None);
    }

    #[test]
    fn extend_merges_years_from_another_history() {
        let mut a = DemandHistory::new();
        a.insert("15K442", year(2024, 1500, Some(550), 130));
        let mut b = DemandHistory::new();
        b.insert("15K442", year(2025, 1585, Some(564), 138));
        b.insert("20K062", year(2025, 300, Some(200), 180));
        a.extend(b);
        assert_eq!(a.years(), vec![2024, 2025]);
        assert_eq!(a.schools(), 2);
        assert_eq!(a.latest("15K442").map(|l| l.year), Some(2025));
    }

    #[test]
    fn shares_need_both_figures_and_are_capped() {
        let figures = DemandFigures {
            seats: Some(99),
            listed: Some(1489),
            considered: Some(598),
            offers: Some(115),
        };
        assert!((figures.offer_share().expect("share") - 115.0 / 598.0).abs() < 1e-12);
        assert!((figures.listed_per_seat().expect("per seat") - 1489.0 / 99.0).abs() < 1e-12);
        let over = DemandFigures {
            considered: Some(10),
            offers: Some(12),
            ..DemandFigures::default()
        };
        assert_eq!(over.offer_share(), Some(1.0));
        assert_eq!(over.listed_per_seat(), None);
        assert_eq!(
            DemandFigures {
                considered: Some(0),
                offers: Some(0),
                ..DemandFigures::default()
            }
            .offer_share(),
            None
        );
        assert!(DemandFigures::default().is_empty());
    }
}
