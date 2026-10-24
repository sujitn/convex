//! Thin explicit native cash-projection and checked horizon-policy dispatch.
use super::*;
use convex_bonds::pricing::{reinvestment as native, ShortDateCalculator};
use convex_core::contracts::ExactDecimal;
use serde_json::json;

/// Supplied numerical model operations; no market convention is inferred.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReinvestmentOperation {
    /// Complete native account projection using actual original cash units.
    Project {
        /// Original dates, both rates, frequency, initial cash and reporting basis.
        terms: native::ProjectionTerms,
        /// Original ordered signed transfers; actual terminal cash is required at close.
        cash: Vec<native::ProjectionCash>,
    },
    /// Explicit periodic ACT/365F growth; dates/rate/domain are all validated.
    Growth {
        /// Supplied annual decimal model rate.
        annual_rate: ExactDecimal,
        /// Positive compound frequency.
        frequency: u32,
        /// Actual inclusive interval start.
        start: Date,
        /// Actual end; an equal date has genuine unit growth.
        end: Date,
    },
    /// Checked numeric regime classification, separate from instrument pricing.
    Classify {
        /// Actual original lower threshold, in years.
        money_market_threshold: f64,
        /// Actual original upper threshold, in years.
        short_date_threshold: f64,
        /// Explicitly supplied classification flag.
        enabled: bool,
        /// Positive finite remaining ACT/365F years.
        remaining_years: f64,
    },
    /// Remaining ACT/365F time with actual ordered native dates.
    RemainingYears {
        /// Original settlement/start date.
        settlement: Date,
        /// Actual strictly later maturity date.
        maturity: Date,
    },
}
fn error(error: convex_bonds::BondError) -> LibraryError {
    LibraryError {
        code: if matches!(error, convex_bonds::BondError::MissingField { .. }) {
            LibraryErrorCode::MissingInput
        } else {
            LibraryErrorCode::InvalidInput
        },
        message: error.to_string(),
        details: json!({"native_kind":format!("{error:?}")}),
    }
}
pub(super) fn execute(action: &ReinvestmentOperation) -> LibraryResult<Value> {
    match action {
        ReinvestmentOperation::Project {terms,cash}=>native::project(cash,terms)
            .map(|value|json!({"cash_unit":"input_cash","return_unit":"annual_decimal_rate","projection":value})).map_err(error),
        ReinvestmentOperation::Growth {annual_rate,frequency,start,end}=>
            native::periodic_growth(annual_rate.decimal(),*frequency,*start,*end)
                .map(|value|json!({"unit":"compound_factor","growth_factor":ExactDecimal::new(value)})).map_err(error),
        ReinvestmentOperation::Classify {money_market_threshold,short_date_threshold,enabled,remaining_years}=> {
            let policy=ShortDateCalculator::new(*money_market_threshold,*short_date_threshold)
                .map_err(error)?.with_enabled(*enabled);
            Ok(json!({"money_market_threshold":policy.money_market_threshold(),
                "short_date_threshold":policy.short_date_threshold(),"enabled":policy.enabled(),
                "money_market_selected":policy.use_money_market_below(*remaining_years).map_err(error)?,
                "short_date_selected":policy.is_short_dated(*remaining_years).map_err(error)?}))
        }
        ReinvestmentOperation::RemainingYears {settlement,maturity}=>
            ShortDateCalculator::years_to_maturity(*settlement,*maturity)
                .map(|value|json!({"unit":"actual365f_years","remaining_years":value})).map_err(error),
    }
}
