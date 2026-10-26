//! Lottery numbers and unit-interval values.

use std::fmt;

/// A finite number in the closed interval `[0, 1]`.
///
/// The field is private; [`UnitInterval::new`] is the only constructor
/// (REAS-RUST-001).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct UnitInterval(f64);

/// Error from [`UnitInterval::new`].
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum InvalidUnitInterval {
    #[error("value must be a finite number, got {0}")]
    NotFinite(f64),
    #[error("value must be between 0 and 1 inclusive, got {0}")]
    OutOfRange(f64),
}

impl UnitInterval {
    pub fn new(value: f64) -> Result<Self, InvalidUnitInterval> {
        if !value.is_finite() {
            return Err(InvalidUnitInterval::NotFinite(value));
        }
        if !(0.0..=1.0).contains(&value) {
            return Err(InvalidUnitInterval::OutOfRange(value));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

/// The known leading hexadecimal characters of a DOE lottery number.
///
/// NYC lottery numbers are 32-character hexadecimal strings compared
/// lexicographically, so the leading characters give the percentile. The
/// stored form is lowercase, without hyphens, and has 1 to 32 characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LotteryPrefix(String);

/// Error from [`LotteryPrefix::parse`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidLotteryPrefix {
    #[error("lottery number prefix is empty")]
    Empty,
    #[error("lottery number prefix is longer than 32 hexadecimal characters: {0:?}")]
    TooLong(String),
    #[error("lottery number must be hexadecimal, got {0:?}")]
    NotHexadecimal(String),
}

impl LotteryPrefix {
    /// Normalizes and validates a prefix such as `"B0"` or `"b0-3f"`.
    pub fn parse(raw: &str) -> Result<Self, InvalidLotteryPrefix> {
        let normalized: String = raw
            .trim()
            .chars()
            .filter(|c| *c != '-')
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if normalized.is_empty() {
            return Err(InvalidLotteryPrefix::Empty);
        }
        if normalized.len() > 32 {
            return Err(InvalidLotteryPrefix::TooLong(raw.to_owned()));
        }
        if !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(InvalidLotteryPrefix::NotHexadecimal(raw.to_owned()));
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The fraction of the lottery space that sorts before this prefix.
    ///
    /// `"B0"` gives `0xB0 / 0x100 = 0.6875`: about 69% of applicants hold a
    /// better (lower) number.
    pub fn fraction(&self) -> UnitInterval {
        let digits = self.0.len() as i32;
        let value = u128::from_str_radix(&self.0, 16).expect("prefix holds 1 to 32 hex digits");
        let fraction = value as f64 / 2f64.powi(4 * digits);
        UnitInterval::new(fraction).expect("a prefix fraction is within [0, 1)")
    }
}

impl fmt::Display for LotteryPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&self.0.to_ascii_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_interval_accepts_bounds() {
        assert_eq!(UnitInterval::new(0.0).map(UnitInterval::get), Ok(0.0));
        assert_eq!(UnitInterval::new(1.0).map(UnitInterval::get), Ok(1.0));
    }

    #[test]
    fn unit_interval_rejects_out_of_range_and_nan() {
        assert_eq!(
            UnitInterval::new(1.5),
            Err(InvalidUnitInterval::OutOfRange(1.5))
        );
        assert_eq!(
            UnitInterval::new(-0.1),
            Err(InvalidUnitInterval::OutOfRange(-0.1))
        );
        assert!(matches!(
            UnitInterval::new(f64::NAN),
            Err(InvalidUnitInterval::NotFinite(_))
        ));
    }

    #[test]
    fn prefix_b0_is_the_documented_fraction() {
        let prefix = LotteryPrefix::parse("B0").expect("valid");
        assert_eq!(prefix.fraction().get(), 0.6875);
        assert_eq!(prefix.as_str(), "b0");
        assert_eq!(prefix.to_string(), "B0");
    }

    #[test]
    fn prefix_ignores_hyphens_and_case() {
        let prefix = LotteryPrefix::parse(" b0-3F ").expect("valid");
        assert_eq!(prefix.as_str(), "b03f");
    }

    #[test]
    fn prefix_rejects_empty_and_non_hex() {
        assert_eq!(LotteryPrefix::parse(""), Err(InvalidLotteryPrefix::Empty));
        assert_eq!(
            LotteryPrefix::parse("zz"),
            Err(InvalidLotteryPrefix::NotHexadecimal("zz".to_owned()))
        );
    }

    #[test]
    fn prefix_rejects_more_than_32_digits() {
        let raw = "f".repeat(33);
        assert_eq!(
            LotteryPrefix::parse(&raw),
            Err(InvalidLotteryPrefix::TooLong(raw.clone()))
        );
    }

    #[test]
    fn full_length_prefix_stays_below_one() {
        let prefix = LotteryPrefix::parse(&"f".repeat(32)).expect("valid");
        assert!(prefix.fraction().get() <= 1.0);
    }
}
