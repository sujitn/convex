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

fn exact_risk_text(text: &str) -> Result<rust_decimal::Decimal, &'static str> {
    if text.len() > 8192 {
        return Err("risk decimal text exceeds the admitted byte bound");
    }
    if let Some((base, _)) = text.split_once(['e', 'E']) {
        // The dependency's scientific parser first parses the mantissa with
        // rounding. Admit that mantissa exactly before delegating its exponent.
        rust_decimal::Decimal::from_str_exact(base)
            .map_err(|_| "risk decimal mantissa requires rounding or exceeds representation")?;
        rust_decimal::Decimal::from_scientific(text)
            .map_err(|_| "risk decimal exponent exceeds exact representation")
    } else {
        rust_decimal::Decimal::from_str_exact(text)
            .map_err(|_| "risk decimal requires rounding or exceeds exact representation")
    }
}

pub(super) fn deserialize_decimal<'de, D>(
    deserializer: D,
) -> Result<rust_decimal::Decimal, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct RiskDecimalVisitor;
    impl<'de> serde::de::Visitor<'de> for RiskDecimalVisitor {
        type Value = rust_decimal::Decimal;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a finite risk number or exact bounded decimal string")
        }

        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
            Ok(rust_decimal::Decimal::from(value))
        }

        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
            Ok(rust_decimal::Decimal::from(value))
        }

        fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
            if !value.is_finite() {
                return Err(E::custom("finite risk number required"));
            }
            // Preserve the established numeric wire interpretation; the new
            // exact parser cannot silently round a nonzero measure to zero.
            exact_risk_text(&value.to_string()).map_err(E::custom)
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
            exact_risk_text(value).map_err(E::custom)
        }
    }
    deserializer.deserialize_any(RiskDecimalVisitor)
}
