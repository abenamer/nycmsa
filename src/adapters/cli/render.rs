//! Text rendering with the same layout as the Python scripts.

use crate::domain::{Report, ScanRequest, ScanRow, SelectionKind, Student};

use super::join_attributes;

/// `12345678` as `12,345,678`.
pub fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `0.1234` as `12.3%`.
pub fn pct1(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}

/// `0.1234` as `12%`.
pub fn pct0(value: f64) -> String {
    format!("{:.0}%", value * 100.0)
}

fn truncate(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

pub fn render_report(student: &Student, report: &Report) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Student attributes: {}\n",
        join_attributes(&student.attributes)
    ));
    if let (Some(prefix), Some(fraction)) = (&student.lottery_number, report.lottery_fraction) {
        out.push_str(&format!(
            "Lottery number {prefix}: about {} of applicants have a better number\n",
            pct0(fraction)
        ));
    }
    let p = &report.parameters;
    out.push_str(&format!(
        "Trials: {}   demand sd: {}   attrition: {}   waitlist numbers: {}\n\n",
        thousands(u64::from(p.trials.get())),
        p.demand_sd.get(),
        p.attrition.get(),
        p.waitlist_numbers.label()
    ));
    out.push_str(&format!(
        "{:>2} {:<7} {:<38} {:<32} {:>6} {:>6} {:>8} {:>7} {:>7}\n",
        "#", "Code", "Program", "Group", "Init%", "WL%", "P(init)", "P(wl)", "P(any)"
    ));
    for row in &report.rows {
        let group = row.group_name.as_deref().unwrap_or("(not eligible)");
        out.push_str(&format!(
            "{:>2} {:<7} {:<38} {:<32} {:>6} {:>6} {:>8} {:>7} {:>7}\n",
            row.rank,
            row.code,
            truncate(&row.name, 38),
            truncate(group, 32),
            pct0(row.initial_rate),
            pct0(row.waitlist_rate),
            pct1(row.p_initial),
            pct1(row.p_waitlist),
            pct1(row.p_any)
        ));
    }
    out.push_str("\nInit% = share of group matched in the initial round; WL% = share of the remaining group reached by the waitlist.\n\n");
    let s = &report.summary;
    out.push_str(&format!(
        "P(initial-round offer from this list):          {}\n",
        pct1(s.p_initial_offer)
    ));
    out.push_str(&format!(
        "P(any offer incl. waitlist from this list):     {}\n",
        pct1(s.p_any_offer)
    ));
    out.push_str(&format!(
        "P(no offer at all, placed by DOE elsewhere):    {}\n",
        pct1(s.p_no_offer)
    ));
    if let Some(p) = s.p_lottery_offer {
        out.push_str(&format!(
            "P(any offer from a lottery program):            {}\n",
            pct1(p)
        ));
    }
    if let Some(p) = s.p_screened_offer {
        out.push_str(&format!(
            "P(any offer from a screened program):           {}\n",
            pct1(p)
        ));
    }
    out
}

fn opt_u32(value: Option<u32>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

pub fn render_scan(request: &ScanRequest, rows: &[ScanRow]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Student: {}, {}, lottery {} (~{} of applicants rank ahead)\n",
        join_attributes(&request.attributes),
        if request.frl { "FRL" } else { "non-FRL" },
        request.lottery,
        pct0(request.lottery.fraction().get())
    ));
    out.push_str("Seats/Applicants = whole program. 'Your group' columns = the priority group you fall in: how many applied, how many got an offer (incl. waitlist), and the share.\n");
    out.push_str(&format!(
        "{:<4} {:<7} {:<58} {:<9} {:>5} {:>5} {:<34} {:>7} {:>7} {:>6}  Reach for your number\n",
        "Dist",
        "Code",
        "School / program",
        "Method",
        "Seats",
        "Appl",
        "Your group",
        "Applied",
        "Offered",
        "Share"
    ));
    for row in rows {
        let name = format!(
            "{} / {}",
            truncate(&row.school_name, 34),
            truncate(&row.program_name, 22)
        );
        let share = row
            .group_share
            .map(|s| format!("{:>5}", pct0(s)))
            .unwrap_or_else(|| "  n/a".to_owned());
        out.push_str(&format!(
            "{:<4} {:<7} {:<58} {:<9} {:>5} {:>5} {:<34} {:>7} {:>7} {:>6}  {}\n",
            row.district,
            row.code,
            name,
            truncate(&row.method, 9),
            row.seats.unwrap_or(0),
            row.applicants.unwrap_or(0),
            truncate(&row.group_name, 34),
            opt_u32(row.group_applied),
            opt_u32(row.group_offered),
            share,
            row.reach
        ));
    }
    out
}

/// Label used by the web report for a selection kind.
pub fn selection_label(kind: SelectionKind) -> &'static str {
    kind.label()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(200_000), "200,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn percent_formats() {
        assert_eq!(pct1(0.12345), "12.3%");
        assert_eq!(pct0(0.6875), "69%");
    }
}
