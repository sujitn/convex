//! Market conventions for government bonds.
//!
//! This module re-exports [`MarketConvention`] from `convex-core` and provides
//! helper functions for curve construction.

use convex_core::Date;

// Re-export MarketConvention from convex-core as the canonical implementation
pub use convex_core::types::MarketConvention;

/// Calculates the year fraction between two dates using the specified convention.
///
/// This is a convenience wrapper around `MarketConvention::year_fraction()`.
///
/// # Arguments
///
/// * `start` - Start date
/// * `end` - End date
/// * `convention` - Market convention determining day count
///
/// # Returns
///
/// The year fraction (e.g., 0.5 for 6 months).
#[must_use]
pub fn day_count_factor(start: Date, end: Date, convention: MarketConvention) -> f64 {
    convention.year_fraction(start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_convention_day_counts() {
        assert_eq!(MarketConvention::USTreasury.day_count_name(), "ACT/ACT");
        assert_eq!(MarketConvention::UKGilt.day_count_name(), "ACT/365F");
        assert_eq!(
            MarketConvention::GermanBund.day_count_name(),
            "ACT/ACT ICMA"
        );
        assert_eq!(MarketConvention::Generic360.year_basis(), 360.0);
    }

    #[test]
    fn test_market_convention_frequency() {
        assert_eq!(MarketConvention::USTreasury.coupons_per_year(), 2);
        assert_eq!(MarketConvention::GermanBund.coupons_per_year(), 1);
    }

    #[test]
    fn test_day_count_factor() {
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 7, 1).unwrap();

        // 181 days using ACT/365
        let dcf = day_count_factor(start, end, MarketConvention::UKGilt);
        assert!((dcf - 181.0 / 365.0).abs() < 1e-10);

        // 181 days using ACT/360
        let dcf360 = day_count_factor(start, end, MarketConvention::Generic360);
        assert!((dcf360 - 181.0 / 360.0).abs() < 1e-10);
    }
}
