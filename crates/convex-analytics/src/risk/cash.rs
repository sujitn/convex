//! Checked adapters to the native complete-cash derivative owner.
use convex_bonds::pricing::dated_cash::{cash_yield_risk, DatedCashYieldRisk};
use convex_core::types::Compounding;
use rust_decimal::Decimal;

use crate::error::{AnalyticsError, AnalyticsResult};

pub(super) fn evaluate(
    times: &[f64],
    cash_flows: &[f64],
    annual_yield: f64,
    compounding: Compounding,
) -> AnalyticsResult<DatedCashYieldRisk> {
    if times.len() != cash_flows.len() || times.is_empty() || times.len() > 4096 {
        return Err(AnalyticsError::InvalidInput(
            "matching 1..4096 cash times and amounts required".into(),
        ));
    }
    let cash: Vec<_> = times
        .iter()
        .copied()
        .zip(cash_flows.iter().copied())
        .collect();
    cash_yield_risk(&cash, annual_yield, compounding).map_err(Into::into)
}

pub(super) fn decimal(value: f64) -> AnalyticsResult<Decimal> {
    let result = Decimal::from_f64_retain(value)
        .ok_or_else(|| AnalyticsError::CalculationFailed("unrepresentable risk measure".into()))?;
    if value != 0.0 && result.is_zero() {
        return Err(AnalyticsError::CalculationFailed(
            "risk measure underflow".into(),
        ));
    }
    Ok(result)
}

pub(super) fn deserialize_decimal<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: serde::Deserializer<'de>,
{
    convex_core::contracts::numeric_decimal::deserialize_named(deserializer, "risk")
}
