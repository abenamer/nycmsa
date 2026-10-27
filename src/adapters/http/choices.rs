//! The choices the web form offers instead of numeric parameters. Each
//! chip maps to fixed model inputs; the mapping is recorded in
//! `docs/api.md`.

use crate::adapters::dto::WaitlistNumbersDto;
use crate::domain::ProgramDefinitions;

/// The answer to "How high do you think your child will place on a
/// screened test?"
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TalentTestChoice {
    NotTaken,
    Unknown,
    TopQuarter,
    SecondQuarter,
    ThirdQuarter,
    BottomQuarter,
}

impl TalentTestChoice {
    pub const ALL: [TalentTestChoice; 6] = [
        TalentTestChoice::NotTaken,
        TalentTestChoice::Unknown,
        TalentTestChoice::TopQuarter,
        TalentTestChoice::SecondQuarter,
        TalentTestChoice::ThirdQuarter,
        TalentTestChoice::BottomQuarter,
    ];

    pub fn value(self) -> &'static str {
        match self {
            TalentTestChoice::NotTaken => "not_taken",
            TalentTestChoice::Unknown => "unknown",
            TalentTestChoice::TopQuarter => "top_quarter",
            TalentTestChoice::SecondQuarter => "second_quarter",
            TalentTestChoice::ThirdQuarter => "third_quarter",
            TalentTestChoice::BottomQuarter => "bottom_quarter",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TalentTestChoice::NotTaken => "Didn't take a screened test",
            TalentTestChoice::Unknown => "Don't know",
            TalentTestChoice::TopQuarter => "Top 25%",
            TalentTestChoice::SecondQuarter => "25% to 50%",
            TalentTestChoice::ThirdQuarter => "50% to 75%",
            TalentTestChoice::BottomQuarter => "Bottom 25%",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.value() == raw.trim())
    }

    /// True when the student holds the `mark_twain_test` attribute.
    pub fn took_test(self) -> bool {
        self != TalentTestChoice::NotTaken
    }

    /// Expected percentile (`0` best) and spread, as fractions: the middle
    /// of the chosen quarter with a spread that keeps most draws inside it.
    pub fn percentile(self) -> Option<(f64, f64)> {
        match self {
            TalentTestChoice::NotTaken | TalentTestChoice::Unknown => None,
            TalentTestChoice::TopQuarter => Some((0.125, 0.10)),
            TalentTestChoice::SecondQuarter => Some((0.375, 0.10)),
            TalentTestChoice::ThirdQuarter => Some((0.625, 0.10)),
            TalentTestChoice::BottomQuarter => Some((0.875, 0.10)),
        }
    }

    /// The chip that best describes a stored student.
    pub fn from_student(took_test: bool, percentile: Option<f64>) -> Self {
        if !took_test {
            return TalentTestChoice::NotTaken;
        }
        match percentile {
            None => TalentTestChoice::Unknown,
            Some(p) if p <= 0.25 => TalentTestChoice::TopQuarter,
            Some(p) if p <= 0.50 => TalentTestChoice::SecondQuarter,
            Some(p) if p <= 0.75 => TalentTestChoice::ThirdQuarter,
            Some(_) => TalentTestChoice::BottomQuarter,
        }
    }
}

/// How waitlists order students.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitlistChoice {
    Redrawn,
    Reused,
}

impl WaitlistChoice {
    pub const ALL: [WaitlistChoice; 2] = [WaitlistChoice::Redrawn, WaitlistChoice::Reused];

    pub fn value(self) -> &'static str {
        match self {
            WaitlistChoice::Redrawn => "redrawn",
            WaitlistChoice::Reused => "reused",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WaitlistChoice::Redrawn => "Each waitlist draws a new number (what the DOE says)",
            WaitlistChoice::Reused => {
                "Waitlists keep the same number (what some schools did in 2022)"
            }
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.value() == raw.trim())
    }

    pub fn dto(self) -> WaitlistNumbersDto {
        match self {
            WaitlistChoice::Redrawn => WaitlistNumbersDto::Redrawn,
            WaitlistChoice::Reused => WaitlistNumbersDto::Reused,
        }
    }
}

/// How far to trust last year's offer counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlookChoice {
    AsPublished,
    Hopeful,
}

impl OutlookChoice {
    pub const ALL: [OutlookChoice; 2] = [OutlookChoice::AsPublished, OutlookChoice::Hopeful];

    pub fn value(self) -> &'static str {
        match self {
            OutlookChoice::AsPublished => "as_published",
            OutlookChoice::Hopeful => "hopeful",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            OutlookChoice::AsPublished => "Take last year's numbers as they are",
            OutlookChoice::Hopeful => "Assume schools reach a bit further down their lists",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.value() == raw.trim())
    }

    /// The attrition multiplier behind the chip.
    pub fn attrition(self) -> f64 {
        match self {
            OutlookChoice::AsPublished => 1.0,
            OutlookChoice::Hopeful => 1.3,
        }
    }

    pub fn from_attrition(attrition: f64) -> Self {
        if attrition > 1.0 {
            OutlookChoice::Hopeful
        } else {
            OutlookChoice::AsPublished
        }
    }
}

fn program_name(code: &str, definitions: &ProgramDefinitions) -> String {
    crate::domain::ProgramCode::parse(code)
        .ok()
        .and_then(|c| definitions.get(&c).map(|p| p.name().to_owned()))
        .unwrap_or_else(|| code.to_owned())
}

/// A plain-language label for an eligibility attribute.
pub fn attribute_label(attribute: &str, definitions: &ProgramDefinitions) -> String {
    if let Some(district) = attribute
        .strip_prefix('d')
        .filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
    {
        return format!("Lives in District {district}");
    }
    if let Some(code) = attribute.strip_prefix("sibling_") {
        return format!("Sibling at {}", program_name(code, definitions));
    }
    if let Some(code) = attribute.strip_prefix("continuing_") {
        return format!("Already attends {}", program_name(code, definitions));
    }
    match attribute {
        "frl" => "Free or reduced-price lunch, English learner, or temporary housing".to_owned(),
        "mark_twain_test" => "Took the Mark Twain talent test".to_owned(),
        "sibling" => "Has a sibling at the school".to_owned(),
        "zoned" => "Lives in the school's zone".to_owned(),
        "continuing" => "Continuing student at the school".to_owned(),
        "current_student" => "Currently attends the school".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn talent_chip_round_trips_the_student_file() {
        assert_eq!(
            TalentTestChoice::from_student(true, Some(0.10)),
            TalentTestChoice::TopQuarter
        );
        assert_eq!(
            TalentTestChoice::from_student(true, Some(0.9)),
            TalentTestChoice::BottomQuarter
        );
        assert_eq!(
            TalentTestChoice::from_student(false, Some(0.10)),
            TalentTestChoice::NotTaken
        );
        assert_eq!(
            TalentTestChoice::from_student(true, None),
            TalentTestChoice::Unknown
        );
        assert_eq!(
            TalentTestChoice::parse("second_quarter"),
            Some(TalentTestChoice::SecondQuarter)
        );
        assert_eq!(TalentTestChoice::parse("nonsense"), None);
        assert_eq!(
            TalentTestChoice::TopQuarter.percentile(),
            Some((0.125, 0.10))
        );
        assert_eq!(TalentTestChoice::Unknown.percentile(), None);
        assert!(!TalentTestChoice::NotTaken.took_test());
    }

    #[test]
    fn outlook_maps_to_attrition_and_back() {
        assert_eq!(OutlookChoice::Hopeful.attrition(), 1.3);
        assert_eq!(OutlookChoice::from_attrition(1.3), OutlookChoice::Hopeful);
        assert_eq!(
            OutlookChoice::from_attrition(1.0),
            OutlookChoice::AsPublished
        );
    }

    #[test]
    fn attribute_labels_are_plain_language() {
        let definitions = ProgramDefinitions::default();
        assert_eq!(attribute_label("d15", &definitions), "Lives in District 15");
        assert_eq!(
            attribute_label("sibling_K442S", &definitions),
            "Sibling at K442S"
        );
        assert_eq!(
            attribute_label("zoned", &definitions),
            "Lives in the school's zone"
        );
        assert_eq!(attribute_label("custom", &definitions), "custom");
    }
}
