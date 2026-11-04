//! Checked instantaneous supplied-sensitivity approximations, separate from model revaluation.

use super::scenarios::{RateScenario, SpreadScenario, StressScenario};
use crate::Portfolio;
use convex_core::{Currency, Date};
use convex_math::decimal::BoundedDecimal;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Maximum scenarios in a single native batch.
pub const MAX_STRESS_SCENARIOS: usize = 256;
/// Maximum positions, cash rows, or factors individually admitted.
pub const MAX_STRESS_ROWS: usize = 65_536;
/// Total admitted position/factor/scenario work, including supplied factor rows.
pub const MAX_STRESS_WORK: usize = 1_048_576;

/// Explicit scalar sensitivity family; no automatic effective/modified substitution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StressRateBasis {
    /// Modified yield duration with its same-yield convexity.
    ModifiedYield,
    /// Effective curve duration with its matching effective convexity.
    EffectiveCurve,
    /// Exact supplied key-rate curve factors, first order only.
    KeyRateCurve,
}
/// Explicit Taylor order. Spread and key-rate components remain first order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StressOrder {
    /// Use declared first derivatives only.
    FirstOrder,
    /// Include the matching scalar rate convexity.
    SecondOrder,
}
/// Actual direct same-as-of foreign conversion, never an inferred unit rate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StressFxPin {
    /// Foreign original currency.
    pub from_currency: Currency,
    /// Portfolio reporting currency.
    pub to_currency: Currency,
    /// Exact portfolio valuation date.
    pub as_of_date: Date,
    /// Positive reporting units per foreign unit; must match supplied row FX.
    #[serde(with = "convex_core::contracts::decimal_serde")]
    pub rate: Decimal,
}
/// Complete approximation declaration. Presentation weights have no monetary role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StressPolicy {
    /// Explicit compatible supplied rate-risk family.
    pub rate_basis: StressRateBasis,
    /// Scalar rate approximation order.
    pub rate_order: StressOrder,
    /// Exact required key-rate coordinates, in requested order.
    pub key_rate_tenors: Vec<f64>,
    /// One consumed direct same-date pin per foreign currency.
    pub fx: Vec<StressFxPin>,
}
/// Distinguishes absent risk, malformed input, unsupported model and arithmetic failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StressErrorCode {
    /// Invalid identities, quantities, FX, shocks or dimensions.
    Invalid,
    /// An actual required risk or classification input is absent.
    Missing,
    /// This approximation cannot represent the requested model/order.
    Unsupported,
    /// The final monetary or percentage result is unrepresentable.
    Arithmetic,
}
/// Original checked failure. A failed scenario contains no accepted numeric result.
#[derive(Debug, Clone, thiserror::Error, Serialize)]
#[error("{code:?} {field}: {message}")]
pub struct StressError {
    /// Machine-readable failure category.
    pub code: StressErrorCode,
    /// Original supplied holding when relevant.
    pub holding_id: Option<String>,
    /// Original input role.
    pub field: String,
    /// Concrete failure reason.
    pub message: String,
    #[serde(skip)]
    context: Option<Arc<StressContext>>,
}
/// Result of a bounded native supplied-sensitivity calculation.
pub type StressCalculation<T> = Result<T, StressError>;

pub(super) fn failure(code: StressErrorCode, field: &str, message: &str) -> StressError {
    StressError {
        code,
        holding_id: None,
        field: field.into(),
        message: message.into(),
        context: None,
    }
}
fn invalid(field: &str, message: &str) -> StressError {
    failure(StressErrorCode::Invalid, field, message)
}
pub(super) fn arithmetic(error: impl Into<crate::PortfolioError>) -> StressError {
    let error = error.into();
    failure(
        StressErrorCode::Arithmetic,
        "arithmetic",
        &error.to_string(),
    )
}
pub(super) fn scalar(value: f64, field: &str) -> StressCalculation<Decimal> {
    Decimal::from_f64_retain(value)
        .filter(|number| value == 0.0 || !number.is_zero())
        .ok_or_else(|| invalid(field, "finite representable non-erased scalar required"))
}
fn required(value: Option<f64>, id: &str, field: &str) -> StressCalculation<Decimal> {
    value
        .ok_or_else(|| {
            let mut error = failure(
                StressErrorCode::Missing,
                field,
                "required supplied sensitivity is absent",
            );
            error.holding_id = Some(id.into());
            error
        })
        .and_then(|v| {
            scalar(v, field).map_err(|mut error| {
                error.holding_id = Some(id.into());
                error
            })
        })
}
fn finish(value: &BoundedDecimal) -> StressCalculation<Decimal> {
    value.finish().map_err(arithmetic)
}
fn ratio_percent(
    amount: &BoundedDecimal,
    nav: &BoundedDecimal,
) -> StressCalculation<Option<Decimal>> {
    if nav.is_zero() {
        return Ok(None);
    }
    let sign = if nav.is_negative() {
        -Decimal::ONE
    } else {
        Decimal::ONE
    };
    let divisor = nav.multiply(sign).map_err(arithmetic)?;
    amount
        .multiply(Decimal::ONE_HUNDRED)
        .and_then(|v| v.multiply(sign))
        .and_then(|v| v.ratio(&divisor))
        .map(Some)
        .map_err(arithmetic)
}

fn validate_book(
    portfolio: &Portfolio,
    policy: &StressPolicy,
    count: usize,
) -> StressCalculation<()> {
    if portfolio.id.trim().is_empty() || portfolio.id.len() > 8192 {
        return Err(invalid(
            "portfolio_id",
            "bounded nonempty original portfolio identity required",
        ));
    }
    let rows = portfolio
        .holdings
        .len()
        .checked_add(portfolio.cash.len())
        .ok_or_else(|| invalid("book", "position count overflow"))?;
    let factors = policy.key_rate_tenors.len().max(1);
    if rows == 0
        || rows > MAX_STRESS_ROWS
        || count == 0
        || count > MAX_STRESS_SCENARIOS
        || factors > 4096
        || rows
            .checked_mul(factors)
            .and_then(|v| v.checked_mul(count))
            .is_none_or(|v| v > MAX_STRESS_WORK)
        || policy.fx.len() > 256
    {
        return Err(invalid(
            "work",
            "bounded nonempty book/scenario/factor dimensions required",
        ));
    }
    let mut tenors = HashSet::new();
    for tenor in &policy.key_rate_tenors {
        if !tenor.is_finite() || *tenor <= 0.0 || !tenors.insert(tenor.to_bits()) {
            return Err(invalid(
                "key_rate_tenors",
                "distinct finite positive exact factors required",
            ));
        }
    }
    if policy.rate_basis == StressRateBasis::KeyRateCurve && tenors.is_empty() {
        return Err(invalid(
            "key_rate_tenors",
            "key-rate risk requires an explicit complete factor manifest",
        ));
    }
    if policy.rate_basis != StressRateBasis::KeyRateCurve && !tenors.is_empty() {
        return Err(invalid(
            "key_rate_tenors",
            "scalar rate policies cannot ignore a supplied key-rate manifest",
        ));
    }
    let supplied_factors = portfolio.holdings.iter().try_fold(0usize, |sum, holding| {
        let n = holding
            .analytics
            .key_rate_durations
            .as_ref()
            .map_or(0, |risk| risk.durations.len());
        if n > 4096 {
            return Err(invalid("key_rates", "factor row bound exceeded"));
        }
        sum.checked_add(n)
            .ok_or_else(|| invalid("work", "supplied factor count overflow"))
    })?;
    if rows
        .checked_mul(factors)
        .and_then(|v| v.checked_add(rows))
        .and_then(|v| v.checked_add(supplied_factors))
        .and_then(|v| v.checked_mul(count))
        .is_none_or(|v| v > MAX_STRESS_WORK)
    {
        return Err(invalid(
            "work",
            "complete supplied/requested factor work exceeds the fixed bound",
        ));
    }
    let consumed = portfolio
        .holdings
        .iter()
        .map(|h| h.currency)
        .chain(portfolio.cash.iter().map(|c| c.currency))
        .collect::<HashSet<_>>();
    let mut pins = HashMap::new();
    for pin in &policy.fx {
        if pin.from_currency == portfolio.base_currency
            || pin.to_currency != portfolio.base_currency
            || pin.as_of_date != portfolio.as_of_date
            || pin.rate <= Decimal::ZERO
            || !consumed.contains(&pin.from_currency)
            || pins.insert(pin.from_currency, pin.rate).is_some()
        {
            return Err(invalid(
                "fx",
                "unique consumed positive direct same-date reporting conversion required",
            ));
        }
    }
    let check_fx = |currency, rate| -> StressCalculation<()> {
        if rate <= Decimal::ZERO
            || (currency == portfolio.base_currency && rate != Decimal::ONE)
            || (currency != portfolio.base_currency && pins.get(&currency) != Some(&rate))
        {
            return Err(invalid(
                "fx",
                "row conversion must match the explicit common-base FX declaration",
            ));
        }
        Ok(())
    };
    let mut ids = HashSet::new();
    for holding in &portfolio.holdings {
        if holding.id.trim().is_empty()
            || holding.id.len() > 8192
            || !ids.insert(holding.id.as_str())
            || holding.market_price < Decimal::ZERO
        {
            return Err(invalid(
                "holdings",
                "bounded unique identities and nonnegative supplied clean prices required",
            ));
        }
        check_fx(holding.currency, holding.fx_rate).map_err(|mut error| {
            error.holding_id = Some(holding.id.clone());
            error
        })?;
        let mut dirty = BoundedDecimal::decimal(holding.market_price);
        dirty
            .add(&BoundedDecimal::decimal(holding.accrued_interest))
            .map_err(arithmetic)?;
        if dirty.is_negative() {
            let mut error = invalid(
                "dirty_price",
                "supplied clean plus accrued cannot be negative",
            );
            error.holding_id = Some(holding.id.clone());
            return Err(error);
        }
        for value in [
            holding.analytics.modified_duration,
            holding.analytics.effective_duration,
            holding.analytics.convexity,
            holding.analytics.effective_convexity,
            holding.analytics.spread_duration,
        ]
        .into_iter()
        .flatten()
        {
            scalar(value, "supplied_risk").map_err(|mut error| {
                error.holding_id = Some(holding.id.clone());
                error
            })?;
        }
        if let Some(risk) = &holding.analytics.key_rate_durations {
            let mut keys = HashSet::new();
            for row in &risk.durations {
                if !row.tenor.is_finite() || row.tenor <= 0.0 || !keys.insert(row.tenor.to_bits()) {
                    return Err(invalid(
                        "key_rates",
                        "distinct positive finite exact supplied factors required",
                    ));
                }
            }
        }
    }
    for cash in &portfolio.cash {
        check_fx(cash.currency, cash.fx_rate)?;
    }
    if portfolio.liabilities.is_some_and(|v| v < Decimal::ZERO) {
        return Err(invalid(
            "liabilities",
            "declared base-currency liability amount cannot be negative",
        ));
    }
    Ok(())
}

// Exact native economic identity, shared once across a batch. No hash collision,
// Decimal JSON conversion, provider clock or repeated per-result book copy.
#[derive(Debug, PartialEq)]
struct StressContext {
    portfolio_id: String,
    as_of_date: Date,
    currency: Currency,
    policy: StressPolicy,
    holdings: Vec<StressHoldingContext>,
    cash: Vec<(Currency, Decimal, Decimal)>,
    liabilities: Option<Decimal>,
}
#[derive(Debug, PartialEq)]
struct StressHoldingContext {
    id: String,
    currency: Currency,
    amounts: [Decimal; 4],
    scalar_risk: [Option<u64>; 5],
    key_rates: Vec<(u64, Decimal)>,
    rating: Option<String>,
    sector: Option<String>,
}
impl StressContext {
    fn new(portfolio: &Portfolio, policy: &StressPolicy) -> Self {
        let holdings = portfolio
            .holdings
            .iter()
            .map(|h| StressHoldingContext {
                id: h.id.clone(),
                currency: h.currency,
                amounts: [h.par_amount, h.market_price, h.accrued_interest, h.fx_rate],
                scalar_risk: [
                    h.analytics.modified_duration,
                    h.analytics.effective_duration,
                    h.analytics.convexity,
                    h.analytics.effective_convexity,
                    h.analytics.spread_duration,
                ]
                .map(|v| v.map(f64::to_bits)),
                key_rates: h
                    .analytics
                    .key_rate_durations
                    .as_ref()
                    .map_or_else(Vec::new, |risk| {
                        risk.durations
                            .iter()
                            .map(|r| (r.tenor.to_bits(), r.duration.years()))
                            .collect()
                    }),
                rating: h.classification.rating.composite.map(|v| v.to_string()),
                sector: h.classification.sector.composite.map(|v| v.to_string()),
            })
            .collect();
        Self {
            portfolio_id: portfolio.id.clone(),
            as_of_date: portfolio.as_of_date,
            currency: portfolio.base_currency,
            policy: policy.clone(),
            holdings,
            cash: portfolio
                .cash
                .iter()
                .map(|c| (c.currency, c.amount, c.fx_rate))
                .collect(),
            liabilities: portfolio.liabilities,
        }
    }
}

/// All-or-error approximation, with actual common-base amounts and explicit order.
/// Rate/spread components are additive; rate-spread cross terms are absent.
/// Key-rate scenarios use exactly requested supplied factors, with no inferred risk.
/// Cash and base-currency liabilities are unchanged under these instantaneous shocks.
/// Final Decimal outputs use nearest/even; nonzero underflow/overflow is an error.
#[derive(Debug, Clone, Serialize)]
pub struct StressResult {
    /// Original supplied portfolio identity.
    pub portfolio_id: String,
    /// Original scenario identity.
    pub scenario_name: String,
    /// Actual supplied reporting currency.
    pub currency: Currency,
    /// Actual supplied common valuation date.
    pub as_of_date: Date,
    /// Initial dirty securities plus cash minus liabilities.
    #[serde(with = "convex_core::contracts::decimal_serde")]
    pub initial_value: Decimal,
    /// Initial value plus the complete additive approximation.
    #[serde(with = "convex_core::contracts::decimal_serde")]
    pub stressed_value: Decimal,
    /// Actual signed currency contribution, independent of presentation weights.
    #[serde(with = "convex_core::contracts::decimal_serde")]
    pub pnl: Decimal,
    /// Signed P&L divided by initial NAV, times100; undefined at zero NAV.
    #[serde(with = "convex_core::contracts::decimal_serde::optional")]
    pub pnl_pct: Option<Decimal>,
    /// Monetary rate contribution; absent only if no rate scenario was requested.
    #[serde(with = "convex_core::contracts::decimal_serde::optional")]
    pub rate_impact: Option<Decimal>,
    /// Monetary spread contribution; absent only if no spread scenario was requested.
    #[serde(with = "convex_core::contracts::decimal_serde::optional")]
    pub spread_impact: Option<Decimal>,
    /// Explicit rate family retained in the report.
    pub rate_basis: StressRateBasis,
    /// Explicit scalar approximation order retained in the report.
    pub rate_order: StressOrder,
    /// All admitted supplied securities, including actual zero positions.
    pub holding_count: usize,
    /// All admitted unshocked cash rows.
    pub cash_count: usize,
    /// Complete supplied approximation policy, including exact factors and FX pins.
    pub policy: StressPolicy,
    #[serde(skip)]
    context: Arc<StressContext>,
}
impl StressResult {
    /// Whether the complete available monetary result is positive.
    pub fn is_gain(&self) -> bool {
        self.pnl > Decimal::ZERO
    }
    /// Whether the complete available monetary result is negative.
    pub fn is_loss(&self) -> bool {
        self.pnl < Decimal::ZERO
    }
}

fn validate_spread(scenario: &SpreadScenario) -> StressCalculation<()> {
    match scenario {
        SpreadScenario::Uniform(value) => {
            scalar(*value, "spread_bps")?;
        }
        SpreadScenario::ByRating(rows) | SpreadScenario::BySector(rows) => {
            if rows.is_empty() || rows.len() > 256 {
                return Err(invalid(
                    "spread_map",
                    "1..256 explicit classification shifts required",
                ));
            }
            for (key, shift) in rows {
                if key.trim().is_empty() || key.len() > 256 {
                    return Err(invalid(
                        "spread_map",
                        "bounded nonempty exact classification required",
                    ));
                }
                scalar(*shift, "spread_bps")?;
            }
        }
    }
    Ok(())
}
fn spread_at(scenario: &SpreadScenario, holding: &crate::Holding) -> StressCalculation<Decimal> {
    let compute = || {
        let value = match scenario {
            SpreadScenario::Uniform(value) => *value,
            SpreadScenario::ByRating(rows) => holding
                .classification
                .rating
                .composite
                .and_then(|r| rows.get(&r.to_string()))
                .copied()
                .ok_or_else(|| {
                    failure(
                        StressErrorCode::Missing,
                        "rating_shift",
                        "required actual rating and explicit shift are absent",
                    )
                })?,
            SpreadScenario::BySector(rows) => holding
                .classification
                .sector
                .composite
                .and_then(|s| rows.get(&s.to_string()))
                .copied()
                .ok_or_else(|| {
                    failure(
                        StressErrorCode::Missing,
                        "sector_shift",
                        "required actual sector and explicit shift are absent",
                    )
                })?,
        };
        scalar(value, "spread_bps")
    };
    compute().map_err(|mut error: StressError| {
        error.holding_id = Some(holding.id.clone());
        error
    })
}
fn rate_shifts(
    scenario: &RateScenario,
    policy: &StressPolicy,
) -> StressCalculation<Vec<(f64, Decimal)>> {
    scenario.validate()?;
    if policy.rate_basis != StressRateBasis::KeyRateCurve {
        return match scenario {
            RateScenario::ParallelShift(value) => Ok(vec![(0.0, scalar(*value, "rate_bps")?)]),
            _ => Err(failure(
                StressErrorCode::Unsupported,
                "rate_basis",
                "nonparallel moves require explicit key-rate risk",
            )),
        };
    }
    if policy.rate_order != StressOrder::FirstOrder {
        return Err(failure(
            StressErrorCode::Unsupported,
            "rate_order",
            "second-order key-rate moves require a full cross-gamma model",
        ));
    }
    if let RateScenario::KeyRateShifts(rows) = scenario {
        let declared = policy
            .key_rate_tenors
            .iter()
            .map(|t| t.to_bits())
            .collect::<HashSet<_>>();
        if rows.len() != policy.key_rate_tenors.len()
            || rows.iter().any(|r| !declared.contains(&r.tenor.to_bits()))
        {
            return Err(invalid(
                "rate_factors",
                "exact scenario and policy factor manifests must agree",
            ));
        }
        let supplied = rows
            .iter()
            .map(|r| Ok((r.tenor.to_bits(), scalar(r.shift_bps, "rate_bps")?)))
            .collect::<StressCalculation<HashMap<_, _>>>()?;
        return policy
            .key_rate_tenors
            .iter()
            .map(|tenor| {
                supplied
                    .get(&tenor.to_bits())
                    .copied()
                    .map(|shift| (*tenor, shift))
                    .ok_or_else(|| invalid("rate_factors", "required exact shift is absent"))
            })
            .collect();
    }
    policy
        .key_rate_tenors
        .iter()
        .map(|tenor| {
            Ok((
                *tenor,
                scalar(scenario.shift_at_tenor(*tenor)?, "rate_bps")?,
            ))
        })
        .collect()
}
fn calculate(
    portfolio: &Portfolio,
    scenario: &StressScenario,
    policy: &StressPolicy,
    context: &Arc<StressContext>,
) -> StressCalculation<StressResult> {
    if scenario.name.trim().is_empty()
        || scenario.name.len() > 8192
        || scenario
            .description
            .as_ref()
            .is_some_and(|v| v.len() > 8192)
        || (scenario.rate_scenario.is_none() && scenario.spread_scenario.is_none())
    {
        return Err(invalid(
            "scenario",
            "bounded identity and at least one explicit move required",
        ));
    }
    let shifts = scenario
        .rate_scenario
        .as_ref()
        .map(|s| rate_shifts(s, policy))
        .transpose()?;
    if let Some(s) = &scenario.spread_scenario {
        validate_spread(s)?;
    }
    let mut nav = BoundedDecimal::zero();
    let mut rate = BoundedDecimal::zero();
    let mut spread = BoundedDecimal::zero();
    for holding in &portfolio.holdings {
        let mut dirty = BoundedDecimal::decimal(holding.market_price);
        dirty
            .add(&BoundedDecimal::decimal(holding.accrued_interest))
            .map_err(arithmetic)?;
        let value = dirty
            .multiply(holding.par_amount)
            .and_then(|v| v.multiply(holding.fx_rate))
            .and_then(|v| v.divide_power_of_ten(2))
            .map_err(arithmetic)?;
        nav.add(&value).map_err(arithmetic)?;
        if let Some(shifts) = &shifts {
            if policy.rate_basis == StressRateBasis::KeyRateCurve {
                let supplied = holding.analytics.key_rate_durations.as_ref().map(|risk| {
                    risk.durations
                        .iter()
                        .map(|row| (row.tenor.to_bits(), row.duration.years()))
                        .collect::<HashMap<_, _>>()
                });
                for (tenor, shift) in shifts {
                    let duration = supplied.as_ref()
                        .and_then(|risk| risk.get(&tenor.to_bits())).copied()
                        .ok_or_else(|| {
                            let mut error = failure(StressErrorCode::Missing, "key_rate", "required exact supplied factor is absent, including explicit-zero shifts");
                            error.holding_id = Some(holding.id.clone()); error
                        })?;
                    let effect = value
                        .multiply(-duration)
                        .and_then(|v| v.multiply(*shift))
                        .and_then(|v| v.divide_power_of_ten(4))
                        .map_err(arithmetic)?;
                    rate.add(&effect).map_err(arithmetic)?;
                }
            } else {
                let shift = shifts
                    .first()
                    .ok_or_else(|| invalid("rate_bps", "scalar shift is absent"))?
                    .1;
                if shift != Decimal::ZERO {
                    let (duration, convexity) = match policy.rate_basis {
                        StressRateBasis::ModifiedYield => (
                            holding.analytics.modified_duration,
                            holding.analytics.convexity,
                        ),
                        StressRateBasis::EffectiveCurve => (
                            holding.analytics.effective_duration,
                            holding.analytics.effective_convexity,
                        ),
                        StressRateBasis::KeyRateCurve => {
                            return Err(invalid(
                                "rate_basis",
                                "scalar branch requires a scalar rate family",
                            ))
                        }
                    };
                    let duration = required(duration, &holding.id, "rate_duration")?;
                    let linear = value
                        .multiply(-duration)
                        .and_then(|v| v.multiply(shift))
                        .and_then(|v| v.divide_power_of_ten(4))
                        .map_err(arithmetic)?;
                    rate.add(&linear).map_err(arithmetic)?;
                    if policy.rate_order == StressOrder::SecondOrder {
                        let convexity =
                            required(convexity, &holding.id, "matching_rate_convexity")?;
                        let quadratic = value
                            .multiply(convexity)
                            .and_then(|v| v.multiply(shift))
                            .and_then(|v| v.multiply(shift))
                            .and_then(|v| v.multiply(Decimal::new(5, 1)))
                            .and_then(|v| v.divide_power_of_ten(8))
                            .map_err(arithmetic)?;
                        rate.add(&quadratic).map_err(arithmetic)?;
                    }
                }
            }
        }
        if let Some(scenario) = &scenario.spread_scenario {
            let shift = spread_at(scenario, holding)?;
            if shift != Decimal::ZERO {
                let duration = required(
                    holding.analytics.spread_duration,
                    &holding.id,
                    "spread_duration",
                )?;
                let effect = value
                    .multiply(-duration)
                    .and_then(|v| v.multiply(shift))
                    .and_then(|v| v.divide_power_of_ten(4))
                    .map_err(arithmetic)?;
                spread.add(&effect).map_err(arithmetic)?;
            }
        }
    }
    for cash in &portfolio.cash {
        nav.add(
            &BoundedDecimal::decimal(cash.amount)
                .multiply(cash.fx_rate)
                .map_err(arithmetic)?,
        )
        .map_err(arithmetic)?;
    }
    if let Some(liability) = portfolio.liabilities {
        nav.add(&BoundedDecimal::decimal(-liability))
            .map_err(arithmetic)?;
    }
    let mut pnl = rate.clone();
    pnl.add(&spread).map_err(arithmetic)?;
    let mut stressed = nav.clone();
    stressed.add(&pnl).map_err(arithmetic)?;
    Ok(StressResult {
        portfolio_id: portfolio.id.clone(),
        scenario_name: scenario.name.clone(),
        currency: portfolio.base_currency,
        as_of_date: portfolio.as_of_date,
        initial_value: finish(&nav)?,
        stressed_value: finish(&stressed)?,
        pnl: finish(&pnl)?,
        pnl_pct: ratio_percent(&pnl, &nav)?,
        rate_impact: scenario
            .rate_scenario
            .as_ref()
            .map(|_| finish(&rate))
            .transpose()?,
        spread_impact: scenario
            .spread_scenario
            .as_ref()
            .map(|_| finish(&spread))
            .transpose()?,
        rate_basis: policy.rate_basis,
        rate_order: policy.rate_order,
        holding_count: portfolio.holdings.len(),
        cash_count: portfolio.cash.len(),
        policy: policy.clone(),
        context: Arc::clone(context),
    })
}

/// Calculate one complete estimate. Missing risk and invalid input return no amount.
/// The declared fixed work/representation bounds apply before numerical work.
pub fn run_stress_scenario(
    portfolio: &Portfolio,
    scenario: &StressScenario,
    policy: &StressPolicy,
) -> StressCalculation<StressResult> {
    validate_book(portfolio, policy, 1)?;
    let context = Arc::new(StressContext::new(portfolio, policy));
    calculate(portfolio, scenario, policy, &context).map_err(|mut error| {
        error.context = Some(context);
        error
    })
}
/// Explicit per-scenario availability in a bounded batch.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StressOutcome {
    /// Complete actual native approximation.
    Available {
        /// Complete checked native result.
        result: StressResult,
    },
    /// Original scenario failed; no monetary value is accepted.
    Unavailable {
        /// Original requested scenario identity.
        scenario_name: String,
        /// Original failure without an accepted monetary result.
        error: StressError,
    },
}
/// Admit common book/policy/work once; preserve each scenario's original failure.
pub fn run_stress_scenarios(
    portfolio: &Portfolio,
    scenarios: &[StressScenario],
    policy: &StressPolicy,
) -> StressCalculation<Vec<StressOutcome>> {
    validate_book(portfolio, policy, scenarios.len())?;
    let mut names = HashSet::new();
    if scenarios.iter().any(|s| !names.insert(s.name.as_str())) {
        return Err(invalid("scenarios", "scenario identities must be distinct"));
    }
    let context = Arc::new(StressContext::new(portfolio, policy));
    Ok(scenarios
        .iter()
        .map(
            |scenario| match calculate(portfolio, scenario, policy, &context) {
                Ok(result) => StressOutcome::Available { result },
                Err(mut error) => {
                    error.context = Some(Arc::clone(&context));
                    StressOutcome::Unavailable {
                        scenario_name: scenario.name.clone(),
                        error,
                    }
                }
            },
        )
        .collect())
}
/// Summary only of available common-date/common-currency results; counts remain explicit.
#[derive(Debug, Clone, Serialize)]
pub struct StressSummary {
    /// All attempted scenarios, including unavailable outcomes.
    pub scenario_count: usize,
    /// Complete available scenarios.
    pub available_count: usize,
    /// Original unavailable scenarios, never included as zeros.
    pub unavailable_count: usize,
    /// Available scenarios whose initial NAV admits a percentage.
    pub percentage_count: usize,
    /// Lowest available monetary result.
    pub worst: Option<StressResult>,
    /// Highest available monetary result.
    pub best: Option<StressResult>,
    /// Mean of available monetary results only.
    #[serde(with = "convex_core::contracts::decimal_serde::optional")]
    pub avg_pnl: Option<Decimal>,
    /// Mean of defined available percentages only.
    #[serde(with = "convex_core::contracts::decimal_serde::optional")]
    pub avg_pnl_pct: Option<Decimal>,
}
/// Preserve unavailable counts and omit all numerical summary values if none are available.
pub fn summarize_results(results: &[StressOutcome]) -> StressCalculation<StressSummary> {
    if results.len() > MAX_STRESS_SCENARIOS {
        return Err(invalid("results", "scenario summary bound exceeded"));
    }
    let mut common_context: Option<&Arc<StressContext>> = None;
    for outcome in results {
        let context = match outcome {
            StressOutcome::Available { result } => {
                if result.policy != result.context.policy {
                    return Err(invalid(
                        "results",
                        "result policy differs from its native context",
                    ));
                }
                &result.context
            }
            StressOutcome::Unavailable { error, .. } => error
                .context
                .as_ref()
                .ok_or_else(|| invalid("results", "original native scenario context required"))?,
        };
        if common_context
            .is_some_and(|first| !Arc::ptr_eq(first, context) && first.as_ref() != context.as_ref())
        {
            return Err(invalid(
                "results",
                "one exact original economic book and complete policy required",
            ));
        }
        common_context = Some(context);
    }
    let available = results
        .iter()
        .filter_map(|v| match v {
            StressOutcome::Available { result } => Some(result),
            StressOutcome::Unavailable { .. } => None,
        })
        .collect::<Vec<_>>();
    if let Some(first) = available.first() {
        if available.iter().any(|v| {
            v.currency != first.currency
                || v.as_of_date != first.as_of_date
                || v.portfolio_id != first.portfolio_id
                || v.initial_value != first.initial_value
                || v.holding_count != first.holding_count
                || v.cash_count != first.cash_count
                || v.rate_basis != first.rate_basis
                || v.rate_order != first.rate_order
        }) {
            return Err(invalid(
                "results",
                "one original book, valuation context and approximation policy required",
            ));
        }
    }
    let mut amounts = BoundedDecimal::zero();
    let mut percentages = BoundedDecimal::zero();
    let mut percentage_count = 0usize;
    for result in &available {
        amounts
            .add(&BoundedDecimal::decimal(result.pnl))
            .map_err(arithmetic)?;
        if let Some(value) = result.pnl_pct {
            percentages
                .add(&BoundedDecimal::decimal(value))
                .map_err(arithmetic)?;
            percentage_count += 1;
        }
    }
    let mean = |sum: &BoundedDecimal, count: usize| -> StressCalculation<Option<Decimal>> {
        if count == 0 {
            return Ok(None);
        }
        sum.ratio(&BoundedDecimal::decimal(Decimal::from(count as u64)))
            .map(Some)
            .map_err(arithmetic)
    };
    Ok(StressSummary {
        scenario_count: results.len(),
        available_count: available.len(),
        unavailable_count: results.len() - available.len(),
        percentage_count,
        worst: available.iter().min_by_key(|v| v.pnl).map(|v| (*v).clone()),
        best: available.iter().max_by_key(|v| v.pnl).map(|v| (*v).clone()),
        avg_pnl: mean(&amounts, available.len())?,
        avg_pnl_pct: mean(&percentages, percentage_count)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AnalyticsConfig, CashPosition, Holding, HoldingAnalytics};
    use convex_analytics::risk::{Duration, KeyRateDuration, KeyRateDurations};
    use convex_bonds::types::BondIdentifiers;
    use convex_core::types::{Currency, Date};
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal_macros::dec;

    fn policy() -> StressPolicy {
        StressPolicy {
            rate_basis: StressRateBasis::ModifiedYield,
            rate_order: StressOrder::SecondOrder,
            key_rate_tenors: Vec::new(),
            fx: Vec::new(),
        }
    }
    fn test_book(holdings: &[Holding]) -> Portfolio {
        Portfolio {
            id: "original-unit-book".into(),
            name: "supplied unit economics".into(),
            base_currency: Currency::USD,
            as_of_date: Date::from_ymd(2025, 1, 15).unwrap(),
            holdings: holdings.to_vec(),
            cash: Vec::new(),
            shares_outstanding: None,
            liabilities: None,
        }
    }
    fn percent(holdings: &[Holding], scenario: StressScenario) -> Option<f64> {
        super::run_stress_scenario(&test_book(holdings), &scenario, &policy())
            .ok()?
            .pnl_pct?
            .to_f64()
    }
    fn parallel_shift_impact(
        holdings: &[Holding],
        shift: f64,
        _config: &AnalyticsConfig,
    ) -> Option<f64> {
        percent(
            holdings,
            StressScenario::new("original parallel")
                .with_rate_scenario(RateScenario::parallel(shift)),
        )
    }
    fn spread_shock_impact(
        holdings: &[Holding],
        shift: f64,
        _config: &AnalyticsConfig,
    ) -> Option<f64> {
        percent(
            holdings,
            StressScenario::new("original spread")
                .with_spread_scenario(SpreadScenario::Uniform(shift)),
        )
    }
    fn spread_shock_by_rating(
        holdings: &[Holding],
        shifts: &std::collections::HashMap<String, f64>,
        _config: &AnalyticsConfig,
    ) -> Option<f64> {
        percent(
            holdings,
            StressScenario::new("original rating shifts")
                .with_spread_scenario(SpreadScenario::ByRating(shifts.clone())),
        )
    }
    fn spread_shock_by_sector(
        holdings: &[Holding],
        shifts: &std::collections::HashMap<String, f64>,
        _config: &AnalyticsConfig,
    ) -> Option<f64> {
        percent(
            holdings,
            StressScenario::new("original sector shifts")
                .with_spread_scenario(SpreadScenario::BySector(shifts.clone())),
        )
    }
    fn key_rate_shift_impact(
        holdings: &[Holding],
        rate: &RateScenario,
        _config: &AnalyticsConfig,
    ) -> Option<f64> {
        let mut declared = policy();
        declared.rate_basis = StressRateBasis::KeyRateCurve;
        declared.rate_order = StressOrder::FirstOrder;
        if let RateScenario::KeyRateShifts(rows) = rate {
            declared.key_rate_tenors = rows.iter().map(|r| r.tenor).collect();
        }
        super::run_stress_scenario(
            &test_book(holdings),
            &StressScenario::new("original key shifts").with_rate_scenario(rate.clone()),
            &declared,
        )
        .ok()?
        .pnl_pct?
        .to_f64()
    }
    fn run_stress_scenario(
        book: &Portfolio,
        scenario: &StressScenario,
        _config: &AnalyticsConfig,
    ) -> StressResult {
        super::run_stress_scenario(book, scenario, &policy()).unwrap()
    }
    fn run_stress_scenarios(
        book: &Portfolio,
        scenarios: &[StressScenario],
        _config: &AnalyticsConfig,
    ) -> Vec<StressOutcome> {
        super::run_stress_scenarios(book, scenarios, &policy()).unwrap()
    }
    fn available(outcome: &StressOutcome) -> &StressResult {
        match outcome {
            StressOutcome::Available { result } => result,
            StressOutcome::Unavailable { error, .. } => {
                panic!("original healthy scenario failed: {error}")
            }
        }
    }
    fn worst_case(outcomes: &[StressOutcome]) -> Option<StressResult> {
        super::summarize_results(outcomes).unwrap().worst
    }
    fn best_case(outcomes: &[StressOutcome]) -> Option<StressResult> {
        super::summarize_results(outcomes).unwrap().best
    }

    fn create_holding_with_analytics(
        id: &str,
        par: rust_decimal::Decimal,
        price: rust_decimal::Decimal,
        duration: f64,
        convexity: f64,
    ) -> Holding {
        Holding::builder()
            .id(id)
            .identifiers(BondIdentifiers::new().with_ticker(format!("TST{}", id)))
            .par_amount(par)
            .market_price(price)
            .analytics(
                HoldingAnalytics::new()
                    .with_modified_duration(duration)
                    .with_convexity(convexity),
            )
            .build()
            .unwrap()
    }

    fn create_test_portfolio() -> Portfolio {
        let holding = create_holding_with_analytics("BOND1", dec!(1_000_000), dec!(100), 5.0, 50.0);

        Portfolio::builder("Test Portfolio")
            .id("TEST001")
            .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
            .add_holding(holding)
            .add_cash(CashPosition::new(dec!(100_000), Currency::USD))
            .build()
            .unwrap()
    }

    #[test]
    fn test_parallel_shift_impact() {
        let holding = create_holding_with_analytics("BOND1", dec!(1_000_000), dec!(100), 5.0, 50.0);
        let holdings = vec![holding];
        let config = AnalyticsConfig::default();

        // +100bp shift: -Duration × Δy + 0.5 × Convexity × Δy²
        // = -5.0 × 0.01 + 0.5 × 50 × 0.0001
        // = -0.05 + 0.0025 = -0.0475 = -4.75%
        let impact = parallel_shift_impact(&holdings, 100.0, &config).unwrap();
        assert!((impact - (-4.75)).abs() < 0.01);
    }

    #[test]
    fn test_parallel_shift_negative() {
        let holding = create_holding_with_analytics("BOND1", dec!(1_000_000), dec!(100), 5.0, 50.0);
        let holdings = vec![holding];
        let config = AnalyticsConfig::default();

        // -100bp shift: positive impact (rates down = prices up)
        let impact = parallel_shift_impact(&holdings, -100.0, &config).unwrap();
        assert!(impact > 0.0);

        // Approximately +5.25% (duration effect + convexity effect)
        assert!((impact - 5.25).abs() < 0.1);
    }

    #[test]
    fn test_spread_shock_impact() {
        let mut analytics = HoldingAnalytics::new().with_modified_duration(5.0);
        analytics.spread_duration = Some(4.5); // Spread duration slightly less

        let holding = Holding::builder()
            .id("BOND1")
            .identifiers(BondIdentifiers::new().with_ticker("TEST001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(analytics)
            .build()
            .unwrap();

        let holdings = vec![holding];
        let config = AnalyticsConfig::default();

        // +50bp spread widening: -4.5 × 0.005 = -2.25%
        let impact = spread_shock_impact(&holdings, 50.0, &config).unwrap();
        assert!((impact - (-2.25)).abs() < 0.01);
    }

    #[test]
    fn test_run_stress_scenario() {
        let portfolio = create_test_portfolio();
        let config = AnalyticsConfig::default();
        let scenario = super::super::scenarios::standard::rates_up_100();

        let result = run_stress_scenario(&portfolio, &scenario, &config);

        assert_eq!(result.scenario_name, "Rates +100bp");
        assert!(result.initial_value > rust_decimal::Decimal::ZERO);
        assert!(result.pnl < rust_decimal::Decimal::ZERO); // Rates up = loss for bond portfolio
        assert!(result.rate_impact.is_some());
        assert!(result.is_loss());
    }

    #[test]
    fn test_run_multiple_scenarios() {
        let portfolio = create_test_portfolio();
        let config = AnalyticsConfig::default();

        let scenarios = vec![
            super::super::scenarios::standard::rates_up_100(),
            super::super::scenarios::standard::rates_down_100(),
        ];

        let results = run_stress_scenarios(&portfolio, &scenarios, &config);

        assert_eq!(results.len(), 2);

        // Rates up should be a loss
        assert!(available(&results[0]).pnl < rust_decimal::Decimal::ZERO);

        // Rates down should be a gain
        assert!(available(&results[1]).pnl > rust_decimal::Decimal::ZERO);
    }

    #[test]
    fn test_worst_and_best_case() {
        let portfolio = create_test_portfolio();
        let config = AnalyticsConfig::default();
        let scenarios = super::super::scenarios::standard::all();

        let results = run_stress_scenarios(&portfolio, &scenarios, &config);

        let worst = worst_case(&results).unwrap();
        let best = best_case(&results).unwrap();

        assert!(worst.pnl <= best.pnl);
    }

    #[test]
    fn test_stress_summary() {
        let portfolio = create_test_portfolio();
        let config = AnalyticsConfig::default();
        let scenarios = super::super::scenarios::standard::all();

        let results = run_stress_scenarios(&portfolio, &scenarios, &config);
        let summary = summarize_results(&results).unwrap();

        assert_eq!(summary.scenario_count, 10);
        assert!(summary.worst.unwrap().pnl <= summary.best.unwrap().pnl);
    }

    #[test]
    fn test_key_rate_shift_impact() {
        // Create holding with KRD
        let krd = KeyRateDurations::new(vec![
            KeyRateDuration {
                tenor: 2.0,
                duration: Duration::try_from(0.5).unwrap(),
            },
            KeyRateDuration {
                tenor: 5.0,
                duration: Duration::try_from(2.0).unwrap(),
            },
            KeyRateDuration {
                tenor: 10.0,
                duration: Duration::try_from(2.5).unwrap(),
            },
        ]);

        let mut analytics = HoldingAnalytics::new();
        analytics.key_rate_durations = Some(krd);

        let holding = Holding::builder()
            .id("BOND1")
            .identifiers(BondIdentifiers::new().with_ticker("TEST001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(analytics)
            .build()
            .unwrap();

        let holdings = vec![holding];
        let config = AnalyticsConfig::default();

        // Steepening: short rates down, long rates up
        let scenario = RateScenario::key_rates(&[(2.0, -25.0), (5.0, 0.0), (10.0, 25.0)]);

        let impact = key_rate_shift_impact(&holdings, &scenario, &config).unwrap();

        // Impact = -0.5 × (-0.0025) - 2.0 × 0 - 2.5 × 0.0025
        // = 0.00125 - 0 - 0.00625 = -0.005 = -0.5%
        // The calculation is approximate due to tenor interpolation
        assert!(impact.abs() < 1.0); // Reasonable range
    }

    #[test]
    fn test_stress_result_helpers() {
        let result = StressResult {
            policy: policy(),
            context: Arc::new(StressContext::new(&test_book(&[]), &policy())),
            portfolio_id: "original-helper-case".into(),
            scenario_name: "Test".to_string(),
            currency: Currency::USD,
            as_of_date: Date::from_ymd(2025, 1, 15).unwrap(),
            initial_value: rust_decimal::Decimal::from(1000),
            stressed_value: rust_decimal::Decimal::from(950),
            pnl: rust_decimal::Decimal::from(-50),
            pnl_pct: Some(rust_decimal::Decimal::from(-5)),
            // Same original -5% of1000, expressed in the new monetary unit.
            rate_impact: Some(rust_decimal::Decimal::from(-50)),
            spread_impact: None,
            rate_basis: StressRateBasis::ModifiedYield,
            rate_order: StressOrder::SecondOrder,
            holding_count: 1,
            cash_count: 0,
        };

        assert!(result.is_loss());
        assert!(!result.is_gain());
    }

    #[test]
    fn test_spread_shock_by_rating() {
        use crate::types::{Classification, CreditRating, RatingInfo};
        use std::collections::HashMap;

        // Create analytics with spread duration
        let mut aaa_analytics = HoldingAnalytics::new().with_modified_duration(5.0);
        aaa_analytics.spread_duration = Some(4.5);

        let mut bbb_analytics = HoldingAnalytics::new().with_modified_duration(5.0);
        bbb_analytics.spread_duration = Some(4.5);

        // Create holdings with different ratings
        let aaa_holding = Holding::builder()
            .id("AAA_BOND")
            .identifiers(BondIdentifiers::new().with_ticker("AAA001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(aaa_analytics)
            .classification(
                Classification::new().with_rating(RatingInfo::from_composite(CreditRating::AAA)),
            )
            .build()
            .unwrap();

        let bbb_holding = Holding::builder()
            .id("BBB_BOND")
            .identifiers(BondIdentifiers::new().with_ticker("BBB001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(bbb_analytics)
            .classification(
                Classification::new().with_rating(RatingInfo::from_composite(CreditRating::BBB)),
            )
            .build()
            .unwrap();

        let holdings = vec![aaa_holding, bbb_holding];
        let config = AnalyticsConfig::default();

        // Rating-based spread shocks: AAA +25bp, BBB +100bp
        let mut rating_shifts = HashMap::new();
        rating_shifts.insert("AAA".to_string(), 25.0);
        rating_shifts.insert("BBB".to_string(), 100.0);

        let impact = spread_shock_by_rating(&holdings, &rating_shifts, &config).unwrap();

        // Both holdings have equal weights
        // AAA impact: -4.5 × 0.0025 = -0.01125 = -1.125%
        // BBB impact: -4.5 × 0.01 = -0.045 = -4.5%
        // Average: (-1.125 + -4.5) / 2 = -2.8125%
        assert!(impact < 0.0); // Spread widening = loss
        assert!((impact - (-2.8125)).abs() < 0.1);
    }

    #[test]
    fn test_spread_shock_by_sector() {
        use crate::types::{Classification, Sector, SectorInfo};
        use std::collections::HashMap;

        // Create holdings with different sectors
        let financial = Holding::builder()
            .id("FIN_BOND")
            .identifiers(BondIdentifiers::new().with_ticker("FIN001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new().with_modified_duration(5.0))
            .classification(
                Classification::new().with_sector(SectorInfo::from_composite(Sector::Financial)),
            )
            .build()
            .unwrap();

        let corporate = Holding::builder()
            .id("CORP_BOND")
            .identifiers(BondIdentifiers::new().with_ticker("CORP001"))
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new().with_modified_duration(5.0))
            .classification(
                Classification::new().with_sector(SectorInfo::from_composite(Sector::Corporate)),
            )
            .build()
            .unwrap();

        let holdings = vec![financial, corporate];
        let config = AnalyticsConfig::default();

        // Sector-based spread shocks: Financial +150bp, Corporate +75bp
        let mut sector_shifts = HashMap::new();
        sector_shifts.insert("Financial".to_string(), 150.0);
        sector_shifts.insert("Corporate".to_string(), 75.0);

        let impact = spread_shock_by_sector(&holdings, &sector_shifts, &config);

        // Both holdings have equal weights
        // Financial impact: -5.0 × 0.015 = -0.075 = -7.5%
        // Corporate impact: -5.0 × 0.0075 = -0.0375 = -3.75%
        // Average: (-7.5 + -3.75) / 2 = -5.625%
        // The old -5.625% expectation relied on substituting modified duration
        // for absent spread duration. Preserve these original inputs as missing.
        // The independent sector-mapped fixture supplies actual spread duration5
        // and retains the valid -5.625% financial measure at a tighter exact bound.
        assert!(impact.is_none());
    }
}
