//! Credit rating types for fixed income securities.
//!
//! This module provides agency-agnostic credit rating representations:
//!
//! - [`CreditRating`]: Normalized rating scale (AAA to D)
//! - [`RatingBucket`]: Grouped rating categories for reporting

use serde::{Deserialize, Serialize};

/// Normalized credit rating for analytics (agency-agnostic).
///
/// Maps to the standard S&P-style notation but works with any agency's ratings.
/// The ordering is from highest quality (AAA) to lowest (D).
///
/// # Examples
///
/// ```
/// use convex_bonds::types::CreditRating;
///
/// let rating = CreditRating::parse("Aa1").unwrap(); // Moody's notation
/// assert_eq!(rating, CreditRating::AAPlus);
/// assert!(rating.is_investment_grade());
/// ```
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum CreditRating {
    /// Highest quality
    AAA = 1,
    /// AA+
    AAPlus = 2,
    /// AA
    AA = 3,
    /// AA-
    AAMinus = 4,
    /// A+
    APlus = 5,
    /// A
    A = 6,
    /// A-
    AMinus = 7,
    /// BBB+
    BBBPlus = 8,
    /// BBB
    BBB = 9,
    /// BBB- (lowest investment grade)
    BBBMinus = 10,
    /// BB+ (highest high yield)
    BBPlus = 11,
    /// BB
    BB = 12,
    /// BB-
    BBMinus = 13,
    /// B+
    BPlus = 14,
    /// B
    B = 15,
    /// B-
    BMinus = 16,
    /// CCC+
    CCCPlus = 17,
    /// CCC
    CCC = 18,
    /// CCC-
    CCCMinus = 19,
    /// CC
    CC = 20,
    /// C
    C = 21,
    /// Default
    D = 22,
    /// Not rated
    #[default]
    NotRated = 99,
}

impl CreditRating {
    /// Returns the numeric score (1 = AAA, 22 = D, 99 = NR).
    #[must_use]
    pub fn score(&self) -> u8 {
        *self as u8
    }

    /// Returns true if this is investment grade (BBB- or better).
    #[must_use]
    pub fn is_investment_grade(&self) -> bool {
        *self <= CreditRating::BBBMinus && *self != CreditRating::NotRated
    }

    /// Returns true if this is high yield (BB+ or below, excluding NR and D).
    #[must_use]
    pub fn is_high_yield(&self) -> bool {
        *self >= CreditRating::BBPlus && *self <= CreditRating::C
    }

    /// Returns the rating bucket for this rating.
    #[must_use]
    pub fn bucket(&self) -> RatingBucket {
        match self {
            Self::AAA => RatingBucket::AAA,
            Self::AAPlus | Self::AA | Self::AAMinus => RatingBucket::AA,
            Self::APlus | Self::A | Self::AMinus => RatingBucket::A,
            Self::BBBPlus | Self::BBB | Self::BBBMinus => RatingBucket::BBB,
            Self::BBPlus | Self::BB | Self::BBMinus => RatingBucket::BB,
            Self::BPlus | Self::B | Self::BMinus => RatingBucket::B,
            Self::CCCPlus | Self::CCC | Self::CCCMinus | Self::CC | Self::C => RatingBucket::CCC,
            Self::D => RatingBucket::Default,
            Self::NotRated => RatingBucket::NotRated,
        }
    }

    /// Returns the S&P-style notation.
    #[must_use]
    pub fn sp_notation(&self) -> &'static str {
        match self {
            Self::AAA => "AAA",
            Self::AAPlus => "AA+",
            Self::AA => "AA",
            Self::AAMinus => "AA-",
            Self::APlus => "A+",
            Self::A => "A",
            Self::AMinus => "A-",
            Self::BBBPlus => "BBB+",
            Self::BBB => "BBB",
            Self::BBBMinus => "BBB-",
            Self::BBPlus => "BB+",
            Self::BB => "BB",
            Self::BBMinus => "BB-",
            Self::BPlus => "B+",
            Self::B => "B",
            Self::BMinus => "B-",
            Self::CCCPlus => "CCC+",
            Self::CCC => "CCC",
            Self::CCCMinus => "CCC-",
            Self::CC => "CC",
            Self::C => "C",
            Self::D => "D",
            Self::NotRated => "NR",
        }
    }

    /// Returns the Moody's-style notation.
    #[must_use]
    pub fn moodys_notation(&self) -> &'static str {
        match self {
            Self::AAA => "Aaa",
            Self::AAPlus => "Aa1",
            Self::AA => "Aa2",
            Self::AAMinus => "Aa3",
            Self::APlus => "A1",
            Self::A => "A2",
            Self::AMinus => "A3",
            Self::BBBPlus => "Baa1",
            Self::BBB => "Baa2",
            Self::BBBMinus => "Baa3",
            Self::BBPlus => "Ba1",
            Self::BB => "Ba2",
            Self::BBMinus => "Ba3",
            Self::BPlus => "B1",
            Self::B => "B2",
            Self::BMinus => "B3",
            Self::CCCPlus => "Caa1",
            Self::CCC => "Caa2",
            Self::CCCMinus => "Caa3",
            Self::CC => "Ca",
            Self::C => "C",
            Self::D => "D",
            Self::NotRated => "NR",
        }
    }

    /// Parses a rating from S&P or Moody's notation.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        // Try exact match first (case-sensitive for Moody's)
        match s {
            "AAA" | "Aaa" => return Some(Self::AAA),
            "AA+" | "Aa1" => return Some(Self::AAPlus),
            "AA" | "Aa2" => return Some(Self::AA),
            "AA-" | "Aa3" => return Some(Self::AAMinus),
            "A+" | "A1" => return Some(Self::APlus),
            "A" | "A2" => return Some(Self::A),
            "A-" | "A3" => return Some(Self::AMinus),
            "BBB+" | "Baa1" => return Some(Self::BBBPlus),
            "BBB" | "Baa2" => return Some(Self::BBB),
            "BBB-" | "Baa3" => return Some(Self::BBBMinus),
            "BB+" | "Ba1" => return Some(Self::BBPlus),
            "BB" | "Ba2" => return Some(Self::BB),
            "BB-" | "Ba3" => return Some(Self::BBMinus),
            "B+" | "B1" => return Some(Self::BPlus),
            "B" | "B2" => return Some(Self::B),
            "B-" | "B3" => return Some(Self::BMinus),
            "CCC+" | "Caa1" => return Some(Self::CCCPlus),
            "CCC" | "Caa2" => return Some(Self::CCC),
            "CCC-" | "Caa3" => return Some(Self::CCCMinus),
            "CC" | "Ca" => return Some(Self::CC),
            "C" => return Some(Self::C),
            "D" => return Some(Self::D),
            "NR" => return Some(Self::NotRated),
            _ => {}
        }

        // Try uppercase for S&P-style ratings
        match s.to_uppercase().as_str() {
            "AAA" => Some(Self::AAA),
            "AA+" => Some(Self::AAPlus),
            "AA" => Some(Self::AA),
            "AA-" => Some(Self::AAMinus),
            "A+" => Some(Self::APlus),
            "A" => Some(Self::A),
            "A-" => Some(Self::AMinus),
            "BBB+" => Some(Self::BBBPlus),
            "BBB" => Some(Self::BBB),
            "BBB-" => Some(Self::BBBMinus),
            "BB+" => Some(Self::BBPlus),
            "BB" => Some(Self::BB),
            "BB-" => Some(Self::BBMinus),
            "B+" => Some(Self::BPlus),
            "B" => Some(Self::B),
            "B-" => Some(Self::BMinus),
            "CCC+" => Some(Self::CCCPlus),
            "CCC" => Some(Self::CCC),
            "CCC-" => Some(Self::CCCMinus),
            "CC" => Some(Self::CC),
            "C" => Some(Self::C),
            "D" => Some(Self::D),
            "NR" | "NOT RATED" | "NOTRATED" => Some(Self::NotRated),
            _ => None,
        }
    }
}

impl std::fmt::Display for CreditRating {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.sp_notation())
    }
}

/// Rating bucket for summary reporting.
///
/// Groups individual ratings into broader categories for portfolio analysis.
///
/// # Examples
///
/// ```
/// use convex_bonds::types::{CreditRating, RatingBucket};
///
/// let rating = CreditRating::AAPlus;
/// assert_eq!(rating.bucket(), RatingBucket::AA);
/// assert!(RatingBucket::AA.is_investment_grade());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RatingBucket {
    /// AAA
    AAA,
    /// AA+, AA, AA-
    AA,
    /// A+, A, A-
    A,
    /// BBB+, BBB, BBB-
    BBB,
    /// BB+, BB, BB-
    BB,
    /// B+, B, B-
    B,
    /// CCC+, CCC, CCC-, CC, C
    CCC,
    /// D
    Default,
    /// Not Rated
    NotRated,
}

impl RatingBucket {
    /// Returns the label for this bucket.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::AAA => "AAA",
            Self::AA => "AA",
            Self::A => "A",
            Self::BBB => "BBB",
            Self::BB => "BB",
            Self::B => "B",
            Self::CCC => "CCC & Below",
            Self::Default => "Default",
            Self::NotRated => "Not Rated",
        }
    }

    /// Returns true if this bucket is investment grade.
    #[must_use]
    pub fn is_investment_grade(&self) -> bool {
        matches!(self, Self::AAA | Self::AA | Self::A | Self::BBB)
    }

    /// Returns all buckets in order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::AAA,
            Self::AA,
            Self::A,
            Self::BBB,
            Self::BB,
            Self::B,
            Self::CCC,
            Self::Default,
            Self::NotRated,
        ]
    }
}

impl std::fmt::Display for RatingBucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credit_rating_basics() {
        assert!(CreditRating::AAA.is_investment_grade());
        assert!(CreditRating::BBBMinus.is_investment_grade());
        assert!(!CreditRating::BBPlus.is_investment_grade());
        assert!(CreditRating::BBPlus.is_high_yield());
        assert!(!CreditRating::D.is_high_yield());
    }

    #[test]
    fn test_credit_rating_score() {
        assert_eq!(CreditRating::AAA.score(), 1);
        assert_eq!(CreditRating::D.score(), 22);
        assert_eq!(CreditRating::NotRated.score(), 99);
    }

    #[test]
    fn test_credit_rating_ordering() {
        assert!(CreditRating::AAA < CreditRating::AA);
        assert!(CreditRating::BBBMinus < CreditRating::BBPlus);
    }

    #[test]
    fn test_credit_rating_bucket() {
        assert_eq!(CreditRating::AAA.bucket(), RatingBucket::AAA);
        assert_eq!(CreditRating::AAPlus.bucket(), RatingBucket::AA);
        assert_eq!(CreditRating::AA.bucket(), RatingBucket::AA);
        assert_eq!(CreditRating::AAMinus.bucket(), RatingBucket::AA);
        assert_eq!(CreditRating::D.bucket(), RatingBucket::Default);
    }

    #[test]
    fn test_credit_rating_from_str() {
        assert_eq!(CreditRating::parse("AAA"), Some(CreditRating::AAA));
        assert_eq!(CreditRating::parse("Aa1"), Some(CreditRating::AAPlus));
        assert_eq!(CreditRating::parse("Baa2"), Some(CreditRating::BBB));
        assert_eq!(CreditRating::parse("XXX"), None);
    }

    #[test]
    fn test_credit_rating_notation() {
        assert_eq!(CreditRating::AAPlus.sp_notation(), "AA+");
        assert_eq!(CreditRating::AAPlus.moodys_notation(), "Aa1");
        assert_eq!(CreditRating::BBB.sp_notation(), "BBB");
        assert_eq!(CreditRating::BBB.moodys_notation(), "Baa2");
    }

    #[test]
    fn test_rating_bucket_basics() {
        assert!(RatingBucket::AAA.is_investment_grade());
        assert!(RatingBucket::BBB.is_investment_grade());
        assert!(!RatingBucket::BB.is_investment_grade());
        assert!(!RatingBucket::Default.is_investment_grade());
    }

    #[test]
    fn test_rating_bucket_all() {
        let all = RatingBucket::all();
        assert_eq!(all.len(), 9);
        assert_eq!(all[0], RatingBucket::AAA);
        assert_eq!(all[8], RatingBucket::NotRated);
    }

    #[test]
    fn test_serde() {
        let rating = CreditRating::BBBPlus;
        let json = serde_json::to_string(&rating).unwrap();
        let parsed: CreditRating = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, rating);

        let bucket = RatingBucket::AA;
        let json = serde_json::to_string(&bucket).unwrap();
        let parsed: RatingBucket = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, bucket);
    }
}
