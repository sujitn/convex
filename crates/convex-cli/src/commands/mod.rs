//! CLI command implementations.

pub mod analyze;
pub mod bootstrap;
pub mod config;
pub mod curve;
pub mod price;
pub mod spread;

// Re-export submodules for convenience
pub use analyze::AnalyzeArgs;
pub use bootstrap::BootstrapArgs;
pub use config::ConfigArgs;
pub use curve::CurveArgs;
pub use price::PriceArgs;
pub use spread::SpreadArgs;

use chrono::{Datelike, NaiveDate};
use convex_core::types::Date;

use crate::error::{CliError, CliResult};

/// Parses a date string in YYYY-MM-DD format.
pub fn parse_date(s: &str) -> CliResult<Date> {
    let naive = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| CliError::InvalidDate(s.to_string()))?;

    Date::from_ymd(naive.year(), naive.month(), naive.day())
        .map_err(|_| CliError::InvalidDate(s.to_string()))
}

/// Validates a coupon rate.
pub fn validate_coupon(coupon: f64) -> CliResult<f64> {
    if !(0.0..=100.0).contains(&coupon) {
        return Err(CliError::InvalidCoupon(coupon));
    }
    Ok(coupon)
}

/// Validates a yield value.
pub fn validate_yield(yield_value: f64) -> CliResult<f64> {
    if !(-10.0..=100.0).contains(&yield_value) {
        return Err(CliError::InvalidYield(yield_value));
    }
    Ok(yield_value)
}

/// Validates a price value.
pub fn validate_price(price: f64) -> CliResult<f64> {
    if price <= 0.0 {
        return Err(CliError::InvalidPrice(price));
    }
    Ok(price)
}
