//! Maturity bucket classification.

use serde::{Deserialize, Serialize};

/// Standard maturity buckets for portfolio analysis.
///
/// These buckets are commonly used in fixed income indices and reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MaturityBucket {
    /// 0 to 1 year
    ZeroToOne,
    /// 1 to 3 years
    OneToThree,
    /// 3 to 5 years
    ThreeToFive,
    /// 5 to 7 years
    FiveToSeven,
    /// 7 to 10 years
    SevenToTen,
    /// 10 to 20 years
    TenToTwenty,
    /// 20 to 30 years
    TwentyToThirty,
    /// Over 30 years
    ThirtyPlus,
}

impl MaturityBucket {
    /// Classify a maturity in years into a bucket.
    #[must_use]
    pub fn from_years(years: f64) -> Self {
        if years <= 1.0 {
            Self::ZeroToOne
        } else if years <= 3.0 {
            Self::OneToThree
        } else if years <= 5.0 {
            Self::ThreeToFive
        } else if years <= 7.0 {
            Self::FiveToSeven
        } else if years <= 10.0 {
            Self::SevenToTen
        } else if years <= 20.0 {
            Self::TenToTwenty
        } else if years <= 30.0 {
            Self::TwentyToThirty
        } else {
            Self::ThirtyPlus
        }
    }

    /// Returns the label for this bucket.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::ZeroToOne => "0-1Y",
            Self::OneToThree => "1-3Y",
            Self::ThreeToFive => "3-5Y",
            Self::FiveToSeven => "5-7Y",
            Self::SevenToTen => "7-10Y",
            Self::TenToTwenty => "10-20Y",
            Self::TwentyToThirty => "20-30Y",
            Self::ThirtyPlus => "30Y+",
        }
    }

    /// Returns the midpoint of the bucket in years.
    #[must_use]
    pub fn midpoint(&self) -> f64 {
        match self {
            Self::ZeroToOne => 0.5,
            Self::OneToThree => 2.0,
            Self::ThreeToFive => 4.0,
            Self::FiveToSeven => 6.0,
            Self::SevenToTen => 8.5,
            Self::TenToTwenty => 15.0,
            Self::TwentyToThirty => 25.0,
            Self::ThirtyPlus => 35.0,
        }
    }

    /// Returns all buckets in order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::ZeroToOne,
            Self::OneToThree,
            Self::ThreeToFive,
            Self::FiveToSeven,
            Self::SevenToTen,
            Self::TenToTwenty,
            Self::TwentyToThirty,
            Self::ThirtyPlus,
        ]
    }
}

impl std::fmt::Display for MaturityBucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_years() {
        assert_eq!(MaturityBucket::from_years(0.5), MaturityBucket::ZeroToOne);
        assert_eq!(MaturityBucket::from_years(1.0), MaturityBucket::ZeroToOne);
        assert_eq!(MaturityBucket::from_years(1.1), MaturityBucket::OneToThree);
        assert_eq!(MaturityBucket::from_years(2.5), MaturityBucket::OneToThree);
        assert_eq!(MaturityBucket::from_years(4.0), MaturityBucket::ThreeToFive);
        assert_eq!(MaturityBucket::from_years(6.0), MaturityBucket::FiveToSeven);
        assert_eq!(MaturityBucket::from_years(8.0), MaturityBucket::SevenToTen);
        assert_eq!(
            MaturityBucket::from_years(15.0),
            MaturityBucket::TenToTwenty
        );
        assert_eq!(
            MaturityBucket::from_years(25.0),
            MaturityBucket::TwentyToThirty
        );
        assert_eq!(MaturityBucket::from_years(50.0), MaturityBucket::ThirtyPlus);
    }

    #[test]
    fn test_labels() {
        assert_eq!(MaturityBucket::ZeroToOne.label(), "0-1Y");
        assert_eq!(MaturityBucket::ThirtyPlus.label(), "30Y+");
    }

    #[test]
    fn test_all_buckets() {
        let all = MaturityBucket::all();
        assert_eq!(all.len(), 8);
        assert_eq!(all[0], MaturityBucket::ZeroToOne);
        assert_eq!(all[7], MaturityBucket::ThirtyPlus);
    }

    #[test]
    fn test_ordering() {
        assert!(MaturityBucket::ZeroToOne < MaturityBucket::OneToThree);
        assert!(MaturityBucket::TenToTwenty < MaturityBucket::ThirtyPlus);
    }
}
