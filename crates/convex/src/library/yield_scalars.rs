//! Thin supplied scalar arithmetic, with explicit input/output money and rate units.
use super::*;
use convex_bonds::pricing as b;
use convex_core::contracts::ExactDecimal;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Scalar measures only; no dated instrument or market convention is manufactured.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum YieldScalarOperation {
    /// Annual coupon cash and positive price in the same basis; annual decimal rate.
    CurrentBinary64 {
        /// Actual annual coupon cash, including explicit signed/zero amounts.
        annual_coupon: f64,
        /// Strictly positive price in the same money basis.
        clean_price: f64,
    },
    /// Per100 arithmetic simple-yield estimate; annual decimal rate.
    SimpleBinary64 {
        /// Actual annual coupon cash per100.
        annual_coupon: f64,
        /// Actual positive clean price per100.
        clean_price: f64,
        /// Explicit positive years; no day count is inferred.
        years_to_maturity: f64,
    },
    /// Actual Decimal coupon/price ratio, reported as an annual binary64 rate.
    CurrentDecimalRate {
        /// Actual annual coupon cash.
        annual_coupon: ExactDecimal,
        /// Positive price in the same money basis.
        clean_price: ExactDecimal,
    },
    /// Annual coupon rate and per100 price; Decimal percentage-point output.
    CurrentPercentFromRate {
        /// Actual annual decimal coupon rate.
        annual_rate: ExactDecimal,
        /// Positive clean price per100.
        clean_price_per100: ExactDecimal,
    },
    /// Coupon cash and price per100; Decimal percentage-point output.
    CurrentPercentFromAmount {
        /// Actual annual coupon cash per100.
        annual_coupon_per100: ExactDecimal,
        /// Positive clean price per100.
        clean_price_per100: ExactDecimal,
    },
    /// Same-basis Decimal arithmetic estimate; percentage-point output.
    SimpleDecimalPercent {
        /// Actual annual coupon cash in the price's money basis.
        annual_coupon: ExactDecimal,
        /// Actual positive clean price.
        clean_price: ExactDecimal,
        /// Actual positive par value in the same basis.
        par_value: ExactDecimal,
        /// Explicit positive years, without a guessed day count.
        years_to_maturity: ExactDecimal,
    },
    /// Constant coupon rate, original positive face and per100 price; annual rate.
    FixedCurrentDecimalRate {
        /// Supplied annual decimal coupon rate; no instrument is constructed.
        annual_rate: ExactDecimal,
        /// Actual positive original face; it cannot change per100 normalization.
        original_face: ExactDecimal,
        /// Positive clean price per100.
        clean_price_per100: ExactDecimal,
    },
    /// Same exact face/per100 binding; Decimal percentage-point output.
    FixedCurrentPercent {
        /// Actual supplied annual decimal coupon rate.
        annual_rate: ExactDecimal,
        /// Actual original positive face.
        original_face: ExactDecimal,
        /// Positive clean price per100.
        clean_price_per100: ExactDecimal,
    },
    /// Checked annual decimal rate to binary64 percentage points.
    ReportPercent {
        /// Actual finite annual decimal rate, not a fabricated solver result.
        yield_value: f64,
    },
    /// Retain supplied binary64 in Decimal; annual decimal-rate units survive.
    ReportDecimal {
        /// Actual binary64 annual decimal rate.
        yield_value: f64,
    },
}
pub(super) fn execute(action: &YieldScalarOperation) -> LibraryResult<Value> {
    use YieldScalarOperation::*;
    let binary = |unit: &str, result: convex_bonds::BondResult<f64>| {
        result.map(|number| json!({"unit":unit, "representation":"binary64", "value":number}))
    };
    let decimal = |unit: &str, result: convex_bonds::BondResult<rust_decimal::Decimal>| {
        result.map(|amount| json!({"unit":unit, "representation":"decimal", "value":ExactDecimal::new(amount)}))
    };
    let result = match action {
        CurrentBinary64 {
            annual_coupon,
            clean_price,
        } => binary(
            "decimal_rate",
            b::current_yield_simple(*annual_coupon, *clean_price),
        ),
        SimpleBinary64 {
            annual_coupon,
            clean_price,
            years_to_maturity,
        } => binary(
            "decimal_rate",
            b::simple_yield(*annual_coupon, *clean_price, *years_to_maturity),
        ),
        CurrentDecimalRate {
            annual_coupon,
            clean_price,
        } => binary(
            "decimal_rate",
            b::current_yield(annual_coupon.decimal(), clean_price.decimal()),
        ),
        CurrentPercentFromRate {
            annual_rate,
            clean_price_per100,
        } => decimal(
            "percent",
            b::yield_scalars::current_percent_from_rate(
                annual_rate.decimal(),
                clean_price_per100.decimal(),
            ),
        ),
        CurrentPercentFromAmount {
            annual_coupon_per100,
            clean_price_per100,
        } => decimal(
            "percent",
            b::yield_scalars::current_percent_from_amount(
                annual_coupon_per100.decimal(),
                clean_price_per100.decimal(),
            ),
        ),
        SimpleDecimalPercent {
            annual_coupon,
            clean_price,
            par_value,
            years_to_maturity,
        } => decimal(
            "percent",
            b::yield_scalars::simple_decimal_percent(
                annual_coupon.decimal(),
                clean_price.decimal(),
                par_value.decimal(),
                years_to_maturity.decimal(),
            ),
        ),
        FixedCurrentDecimalRate {
            annual_rate,
            original_face,
            clean_price_per100,
        } => binary(
            "decimal_rate",
            b::yield_scalars::fixed_current_decimal_rate(
                annual_rate.decimal(),
                original_face.decimal(),
                clean_price_per100.decimal(),
            ),
        ),
        FixedCurrentPercent {
            annual_rate,
            original_face,
            clean_price_per100,
        } => decimal(
            "percent",
            b::yield_scalars::fixed_current_percent(
                annual_rate.decimal(),
                original_face.decimal(),
                clean_price_per100.decimal(),
            ),
        ),
        ReportPercent { yield_value } => {
            binary("percent", b::yield_scalars::to_percent(*yield_value))
        }
        ReportDecimal { yield_value } => {
            decimal("decimal_rate", b::yield_scalars::to_decimal(*yield_value))
        }
    };
    result.map_err(|error| LibraryError {
        code: LibraryErrorCode::InvalidInput,
        message: error.to_string(),
        details: json!({"native_kind":format!("{error:?}")}),
    })
}
