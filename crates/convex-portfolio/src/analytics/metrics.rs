//! Private checked owner for supplied metric presentation and actual currency sums.
use crate::{AnalyticsConfig, Holding, PortfolioError, PortfolioResult};
use convex_math::decimal::BoundedDecimal;
use rust_decimal::{prelude::ToPrimitive, Decimal};
use std::collections::HashSet;

pub(crate) fn scalar(value: f64, id: &str, field: &str) -> PortfolioResult<Decimal> {
    Decimal::from_f64_retain(value)
        .filter(|v| value == 0.0 || !v.is_zero())
        .ok_or_else(|| {
            PortfolioError::invalid_holding(
                id,
                format!("{field}: finite representable non-erased supplied scalar required"),
            )
        })
}
pub(crate) fn identities(holdings: &[Holding]) -> PortfolioResult<()> {
    if holdings.len() > 65_536 {
        return Err(PortfolioError::invalid_portfolio(
            "supplied metric holding bound exceeded",
        ));
    }
    let mut ids = HashSet::new();
    for holding in holdings {
        if holding.id.trim().is_empty()
            || holding.id.len() > 8192
            || !ids.insert(holding.id.as_str())
        {
            return Err(PortfolioError::invalid_holding(
                &holding.id,
                "bounded unique nonempty identity required",
            ));
        }
    }
    Ok(())
}
pub(crate) fn signed_ratio(
    numerator: &BoundedDecimal,
    denominator: &BoundedDecimal,
) -> PortfolioResult<Option<Decimal>> {
    if denominator.is_zero() {
        return Ok(None);
    }
    let sign = if denominator.is_negative() {
        -Decimal::ONE
    } else {
        Decimal::ONE
    };
    numerator
        .multiply(sign)?
        .ratio(&denominator.multiply(sign)?)
        .map(Some)
        .map_err(Into::into)
}

pub(crate) struct MetricBook<'a> {
    holdings: Vec<&'a Holding>,
    weights: Vec<BoundedDecimal>,
}
impl<'a> MetricBook<'a> {
    pub(crate) fn new(holdings: &'a [Holding], config: &AnalyticsConfig) -> PortfolioResult<Self> {
        Self::from_rows(holdings.iter(), config)
    }
    pub(crate) fn from_rows(
        rows: impl IntoIterator<Item = &'a Holding>,
        config: &AnalyticsConfig,
    ) -> PortfolioResult<Self> {
        let holdings = amount_rows(rows)?;
        let weights = holdings
            .iter()
            .map(|h| h.exact_weight(config.weighting))
            .collect::<PortfolioResult<Vec<_>>>()?;
        Ok(Self { holdings, weights })
    }
    pub(crate) fn weighted(
        &self,
        field: &str,
        get: impl Fn(&Holding) -> Option<f64>,
    ) -> PortfolioResult<Option<f64>> {
        let mut numerator = BoundedDecimal::zero();
        let mut denominator = BoundedDecimal::zero();
        let mut complete = true;
        for (holding, weight) in self.holdings.iter().zip(&self.weights) {
            if let Some(value) = get(holding) {
                numerator.add(&weight.multiply(scalar(value, &holding.id, field)?)?)?;
            } else {
                complete = false;
            }
            denominator.add(weight)?;
        }
        if !complete {
            return Ok(None);
        }
        ratio_as_f64(&numerator, &denominator)
    }
}
pub(crate) fn currency_exact(
    holdings: &[Holding],
    field: &str,
    get: impl Fn(&Holding) -> Option<f64>,
) -> PortfolioResult<Option<BoundedDecimal>> {
    currency_rows(holdings.iter(), field, get)
}
pub(crate) fn currency_rows<'a>(
    rows: impl IntoIterator<Item = &'a Holding>,
    field: &str,
    get: impl Fn(&Holding) -> Option<f64>,
) -> PortfolioResult<Option<BoundedDecimal>> {
    let holdings = amount_rows(rows)?;
    let mut sum = BoundedDecimal::zero();
    let mut complete = true;
    for holding in holdings {
        if let Some(value) = get(holding) {
            let term = BoundedDecimal::decimal(scalar(value, &holding.id, field)?)
                .multiply(holding.par_amount)?
                .multiply(holding.fx_rate)?
                .divide_power_of_ten(2)?;
            sum.add(&term)?;
        } else {
            complete = false;
        }
    }
    if complete {
        Ok(Some(sum))
    } else {
        Ok(None)
    }
}

/// Check native Decimal presentation without a fabricated conversion fallback.
pub(crate) fn presentation(value: Decimal) -> PortfolioResult<f64> {
    value
        .to_f64()
        .filter(|v| v.is_finite() && (value.is_zero() || *v != 0.0))
        .ok_or_else(|| {
            PortfolioError::invalid_portfolio("finite non-erased metric presentation required")
        })
}
/// Exact sum of supplied bounded states, retaining cancellation until the final output.
pub(crate) fn sum(
    values: impl IntoIterator<Item = PortfolioResult<BoundedDecimal>>,
) -> PortfolioResult<BoundedDecimal> {
    let mut sum = BoundedDecimal::zero();
    for value in values {
        sum.add(&value?)?;
    }
    Ok(sum)
}

pub(crate) fn currency_sum(
    holdings: &[Holding],
    field: &str,
    get: impl Fn(&Holding) -> Option<f64>,
) -> PortfolioResult<Option<Decimal>> {
    currency_exact(holdings, field, get)?
        .map(|sum| sum.finish().map_err(Into::into))
        .transpose()
}
pub(crate) fn per_share_exact(
    value: Option<BoundedDecimal>,
    shares: Option<Decimal>,
) -> PortfolioResult<Option<Decimal>> {
    let Some(shares) = shares else {
        return Ok(None);
    };
    if shares < Decimal::ZERO {
        return Err(PortfolioError::invalid_portfolio(
            "shares cannot be negative",
        ));
    }
    value
        .map(|v| signed_ratio(&v, &BoundedDecimal::decimal(shares)))
        .transpose()
        .map(Option::flatten)
}

/// Validate supplied identities before an amount aggregation, including referenced buckets.
pub(crate) fn amount_rows<'a>(
    rows: impl IntoIterator<Item = &'a Holding>,
) -> PortfolioResult<Vec<&'a Holding>> {
    let mut values = Vec::new();
    let mut ids = HashSet::new();
    for h in rows {
        if values.len() >= 65_536
            || h.id.trim().is_empty()
            || h.id.len() > 8192
            || !ids.insert(h.id.as_str())
        {
            return Err(PortfolioError::invalid_holding(
                &h.id,
                "bounded unique nonempty identity required",
            ));
        }
        h.require_positive_fx()?;
        values.push(h);
    }
    Ok(values)
}
/// Exact sum of clean reporting amounts; intermediate products and sums are not rounded.
pub(crate) fn clean_exact<'a>(
    rows: impl IntoIterator<Item = &'a Holding>,
) -> PortfolioResult<BoundedDecimal> {
    let rows = amount_rows(rows)?;
    sum(rows
        .iter()
        .map(|h| h.exact_amount(h.exact_clean_price()?, true)))
}
pub(crate) fn clean_total<'a>(
    rows: impl IntoIterator<Item = &'a Holding>,
) -> PortfolioResult<Decimal> {
    clean_exact(rows)?.finish().map_err(Into::into)
}
pub(crate) fn par_total<'a>(
    rows: impl IntoIterator<Item = &'a Holding>,
) -> PortfolioResult<Decimal> {
    let rows = amount_rows(rows)?;
    sum(rows
        .iter()
        .map(|h| h.exact_weight(crate::WeightingMethod::ParValue)))?
    .finish()
    .map_err(Into::into)
}

pub(crate) fn ratio_as_f64(
    numerator: &BoundedDecimal,
    denominator: &BoundedDecimal,
) -> PortfolioResult<Option<f64>> {
    if denominator.is_zero() {
        return Ok(None);
    }
    let sign = if denominator.is_negative() {
        -Decimal::ONE
    } else {
        Decimal::ONE
    };
    numerator
        .multiply(sign)?
        .ratio_as_f64(&denominator.multiply(sign)?)
        .map(Some)
        .map_err(Into::into)
}
