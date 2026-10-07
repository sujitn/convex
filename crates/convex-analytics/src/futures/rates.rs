//! Actual overnight/term windows, publication-aware projection and frozen risk.

use super::*;
use convex_core::{
    cashflows::{OvernightConvention, OvernightSchedule},
    contracts::{IndexId, SourceId, UtcTimestamp, Version},
    fixings::{
        FixingContext, FixingObservation, FixingRequirement, FixingSeries, RateOrigin,
        UnpublishedFixingPolicy,
    },
};
use convex_curves::contracts::{CurvePoint, CurveQuery, CurveQueryValue, CurveRole};
use std::collections::BTreeMap;

/// Contract-specific fixing revision treatment; no inferred administrator default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FuturesFixingSelection {
    /// Latest eligible revision at the supplied information cutoff.
    LatestEligible,
    /// First publication, as specified for CME final SOFR settlement.
    /// Revision1 must be supplied; a correction cannot stand in for it.
    FirstPublished,
}

/// Immutable checked history. The full supplied history is validated before
/// selecting first revisions; no global fixing date or mutable index registry.
#[derive(Debug, Clone)]
pub struct FuturesFixings {
    series: FixingSeries,
    selection: FuturesFixingSelection,
}
impl FuturesFixings {
    /// Preflight full requirements with this history's clearing revision policy.
    /// This never invokes a projection and preserves complete missing lists.
    pub fn resolve_requirements(
        &self,
        requirements: &[FixingRequirement],
        context: FixingContext,
    ) -> Result<convex_core::fixings::FixingResolution, CashflowError> {
        self.series.resolve(requirements, context)
    }

    /// Validate all revisions and construct the explicitly selected history.
    pub fn new(
        observations: Vec<FixingObservation>,
        selection: FuturesFixingSelection,
    ) -> FuturesResult<Self> {
        if observations.len() > convex_core::fixings::MAX_FIXING_OBSERVATIONS {
            return Err(CashflowError::BoundExceeded("futures fixing history").into());
        }
        let complete = FixingSeries::new(observations.clone())?;
        let series = if selection == FuturesFixingSelection::FirstPublished {
            let mut first = BTreeMap::<(IndexId, Date), FixingObservation>::new();
            for observation in observations {
                let key = (observation.index_id.clone(), observation.observation_date);
                if first
                    .get(&key)
                    .is_none_or(|old| observation.revision < old.revision)
                {
                    first.insert(key, observation);
                }
            }
            if first.values().any(|o| o.revision.get() != 1) {
                return Err(invalid(
                    "fixing_revision",
                    "first publication requires original revision1",
                ));
            }
            FixingSeries::new(first.into_values().collect())?
        } else {
            complete
        };
        Ok(Self { series, selection })
    }
    /// Actual immutable revision policy retained by every valuation.
    pub const fn selection(&self) -> FuturesFixingSelection {
        self.selection
    }
}

/// Complete resolved inputs; a fully fixed contract needs no projection curve
/// or convexity assumption. Forecasts require both explicit admission and source.
pub struct RateFuturesInputs<'a> {
    /// Actual dated exchange contract and exact observation/value window.
    pub contract: &'a FuturesContract,
    /// Exact fixing/value calendar and quality.
    pub calendar: FuturesCalendar<'a>,
    /// Explicit final-rate rounding and normal-futures clearing rules.
    pub clearing: &'a FuturesClearingRules,
    /// Original or latest fixing revisions, as explicitly selected.
    pub fixings: &'a FuturesFixings,
    /// Actual history-manifest source/quality, including synthetic example history.
    pub fixing_stamp: &'a ReferenceStamp,
    /// Exact index/currency projection dependency; never substituted from discount.
    pub projection: Option<FuturesCurve<'a>>,
    /// Mandatory matching source stamp for Supplied convexity when projecting.
    pub convexity_model: Option<&'a ReferenceStamp>,
    /// Economic instant and information cutoff.
    pub context: EligibilityContext,
    /// Explicit treatment before expected publication; due missing history fails.
    pub unpublished: UnpublishedFixingPolicy,
    /// Whether preliminary published rates are inadmissible.
    pub final_only: bool,
}

/// Retained publication or explicit forecast meaning, without lossy rate JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FuturesRateOrigin {
    /// Exact administrator/source revision actually consumed.
    Published {
        /// Original immutable revision.
        revision: Version,
        /// Actual producing source.
        source: SourceId,
        /// Administrator publication instant.
        published_at: UtcTimestamp,
        /// Receipt/information availability instant.
        known_at: UtcTimestamp,
    },
    /// Observation after economic date.
    FutureProjection {},
    /// Past/today observation not yet due, under explicit forecast policy.
    AwaitingPublication {},
}
impl From<RateOrigin> for FuturesRateOrigin {
    fn from(origin: RateOrigin) -> Self {
        match origin {
            RateOrigin::Published(o) => Self::Published {
                revision: o.revision,
                source: o.source_id,
                published_at: o.published_at,
                known_at: o.known_at,
            },
            RateOrigin::Projected(convex_core::fixings::ProjectionReason::FutureObservation) => {
                Self::FutureProjection {}
            }
            RateOrigin::Projected(convex_core::fixings::ProjectionReason::AwaitingPublication) => {
                Self::AwaitingPublication {}
            }
        }
    }
}

/// One actual weighted rate. Holiday tails and clipped month starts stay exact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FuturesRateObservation {
    /// Exact observation and expected publication time.
    pub requirement: FixingRequirement,
    /// Actual full rate quotation interval start.
    pub rate_start: Date,
    /// Actual full rate quotation interval end, including weekends/holidays.
    pub rate_end: Date,
    /// Actual included accrual weight start, which may be inside a holiday.
    pub weight_start: Date,
    /// Exclusive actual included accrual weight end.
    pub weight_end: Date,
    /// Actual ACT/360 or ACT/365F weight.
    pub year_fraction: ExactDecimal,
    /// Annualized decimal rate, unchanged for published observations under risk.
    pub decimal_rate: ExactDecimal,
    /// Exact publication revision or permitted projection reason.
    pub origin: FuturesRateOrigin,
}

/// Complete indicative rate quote; model price and possible final settlement
/// are distinct, and no discounted FRA PV is substituted for a futures mark.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateFuturesValuation {
    /// Exact actual contract revision.
    pub contract: InstrumentPin,
    /// Arithmetic/compound window annualized equivalent before adjustment.
    pub curve_equivalent_decimal_rate: ExactDecimal,
    /// Futures rate after the declared futures-to-forward adjustment is inverted.
    pub quoted_decimal_rate: ExactDecimal,
    /// Unrounded model index points; order ticks and final precision are separate.
    pub model_price_points: ExactDecimal,
    /// Final-rate-rounded price when all observations are published, otherwise None.
    /// This expected exchange result is not an actual posted settlement.
    pub expected_final_price_points: Option<ExactDecimal>,
    /// Published calendar-day weights (all-or-none for a term fixing).
    pub known_days: i64,
    /// Remaining projected weights, distinct from elapsed calendar days.
    pub projected_days: i64,
    /// Contractual annualization period.
    pub period_year_fraction: ExactDecimal,
    /// Product of published compound factors; 1 for arithmetic/term aggregation.
    pub known_factor: ExactDecimal,
    /// Published arithmetic interest sum; 0 for the compounded aggregation.
    pub known_interest: ExactDecimal,
    /// Actual quote-level additive futures-to-forward adjustment applied; frozen
    /// under risk and zero once fully fixed. No automatic time proration is inferred.
    pub applied_forward_adjustment: ExactDecimal,
    /// Original explicit policy, including NotAdmitted for a fully fixed payoff.
    pub convexity: FuturesConvexityPolicy,
    /// Actual administrator revision-selection policy.
    pub fixing_selection: FuturesFixingSelection,
    /// Exact rates, windows and original known/forecast origins.
    pub observations: Vec<FuturesRateObservation>,
    /// Actually consumed forward queries, retaining curve versions/conventions.
    pub projection_queries: Vec<CurveQueryValue>,
    /// Exact source data admission; result is always indicative.
    pub inputs: FuturesInputReport,
}

fn accrual(contract: &FuturesContract) -> FuturesResult<(&RateAccrual, &FuturesConvexityPolicy)> {
    let FuturesSettlement::Rate {
        accrual, convexity, ..
    } = &contract.settlement
    else {
        return Err(invalid(
            "rate_future",
            "bond futures cannot use rate observation analytics",
        ));
    };
    Ok((accrual, convexity))
}
fn schedule(
    accrual: &RateAccrual,
    calendar: &dyn Calendar,
) -> FuturesResult<Option<OvernightSchedule>> {
    match accrual {
        RateAccrual::Overnight {
            start,
            end,
            day_count,
            ..
        } => Ok(Some(OvernightSchedule::generate(
            start.date(),
            end.date(),
            *day_count,
            calendar,
            OvernightConvention {
                lookback_business_days: 0,
                observation_shift: false,
                lockout_business_days: 0,
            },
        )?)),
        RateAccrual::Term {
            fixing, start, end, ..
        } => {
            if !calendar.is_business_day(fixing.date())
                || !calendar.is_business_day(start.date())
                || !calendar.is_business_day(end.date())
            {
                return Err(invalid(
                    "term_dates",
                    "actual fixing/value business dates required",
                ));
            }
            Ok(None)
        }
    }
}

/// Discover every actual contracted requirement before supplying a forecast.
pub fn rate_futures_requirements<P>(
    inputs: &RateFuturesInputs<'_>,
    mut publication: P,
) -> FuturesResult<Vec<FixingRequirement>>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    let mut report = contract_report(inputs.contract, None, inputs.context)?;
    let (accrual, _) = accrual(inputs.contract)?;
    let (index, pin) = match accrual {
        RateAccrual::Overnight {
            index, calendar, ..
        }
        | RateAccrual::Term {
            index, calendar, ..
        } => (index, calendar),
    };
    inputs.calendar.check(pin, &mut report)?;
    if let Some(plan) = schedule(accrual, inputs.calendar.calendar)? {
        Ok(plan.requirements(index, |d| publication(index, d))?)
    } else if let RateAccrual::Term { fixing, .. } = accrual {
        Ok(vec![FixingRequirement {
            index_id: index.clone(),
            observation_date: fixing.date(),
            expected_publication: publication(index, fixing.date())?,
        }])
    } else {
        Err(invalid("rate_plan", "inconsistent accrual family"))
    }
}

/// Price the exact known/unfixed window, applying the explicit quote adjustment
/// only while forecasts remain. Missing due history fails before curve access.
pub fn price_rate_future<P>(
    inputs: &RateFuturesInputs<'_>,
    publication: P,
) -> FuturesResult<RateFuturesValuation>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    price_rate_future_bumped(inputs, publication, Decimal::ZERO)
}

fn price_rate_future_bumped<P>(
    inputs: &RateFuturesInputs<'_>,
    publication: P,
    bump: Decimal,
) -> FuturesResult<RateFuturesValuation>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    let mut report = contract_report(inputs.contract, None, inputs.context)?;
    inputs.clearing.check(inputs.contract, &mut report)?;
    if inputs.clearing.fixing_selection != Some(inputs.fixings.selection) {
        return Err(invalid(
            "fixing_selection",
            "history must use the exact declared clearing revision rule",
        ));
    }
    report.add(inputs.fixing_stamp)?;
    let (terms, convexity) = accrual(inputs.contract)?;
    let (index, start, end, pin, day_count, averaging) = match terms {
        RateAccrual::Overnight {
            index,
            start,
            end,
            calendar,
            day_count,
            averaging,
        } => (
            index,
            start.date(),
            end.date(),
            calendar,
            *day_count,
            *averaging,
        ),
        RateAccrual::Term {
            index,
            start,
            end,
            calendar,
            day_count,
            ..
        } => (
            index,
            start.date(),
            end.date(),
            calendar,
            *day_count,
            RateAveraging::Arithmetic,
        ),
    };
    inputs.calendar.check(pin, &mut report)?;
    let requirements = rate_futures_requirements(inputs, publication)?;
    let context = FixingContext {
        valuation_date: date(inputs.context),
        known_at: inputs.context.known_at,
        unpublished: inputs.unpublished,
        final_only: inputs.final_only,
    };
    let plan = schedule(terms, inputs.calendar.calendar)?;
    let intervals: BTreeMap<_, _> = if let Some(plan) = &plan {
        plan.observations()
            .iter()
            .map(|o| (o.observation_date, (o.observation_date, o.rate_end)))
            .collect()
    } else {
        requirements
            .iter()
            .map(|r| (r.observation_date, (start, end)))
            .collect()
    };
    let mut queries = Vec::new();
    let resolved = inputs.fixings.series.resolve(&requirements,context)?.project(|requirement| {
        let mut project = || -> FuturesResult<Decimal> {
            let curve = inputs.projection.ok_or_else(|| invalid("projection", "missing exact index projection source"))?;
            curve.check(&mut report)?;
            if curve.curve.recipe().currency != inputs.contract.currency
                || !matches!(&curve.curve.recipe().role, CurveRole::Projection {index_id} if index_id == index.as_str()) {
                return Err(invalid("projection", "actual currency/index projection role required"));
            }
            let (from,to) = intervals.get(&requirement.observation_date).ok_or_else(|| invalid("projection", "missing exact quotation interval"))?;
            let query = curve.curve.query(&CurveQuery::ForwardRate {start:CurvePoint::Date(*from),end:CurvePoint::Date(*to),compounding:convex_core::types::Compounding::Simple,day_count})?;
            let forecast = arithmetic(decimal(query.value)?.checked_add(bump),"projection_bump")?;
            rate(forecast,"projection_rate")?;
            queries.push(query); Ok(forecast)
        };
        project().map_err(|e| CashflowError::InvalidTerms {field:"futures_projection",reason:e.to_string()})
    })?;
    let rates: BTreeMap<_, _> = resolved
        .into_iter()
        .map(|r| (r.requirement.observation_date, r))
        .collect();
    let mut observations = Vec::new();
    let weights: Vec<_> = if let Some(plan) = plan {
        plan.observations()
            .iter()
            .map(|o| (o.observation_date, o.rate_end, o.weight_start, o.weight_end))
            .collect()
    } else {
        requirements
            .iter()
            .map(|r| (r.observation_date, end, start, end))
            .collect()
    };
    for (observation, rate_end, weight_start, weight_end) in weights {
        let r = rates
            .get(&observation)
            .ok_or_else(|| invalid("rate", "missing resolved observation"))?;
        if matches!(&r.origin, RateOrigin::Published(o) if o.source_id != inputs.fixing_stamp.source)
        {
            return Err(invalid(
                "fixing_source",
                "published rate source differs from the supplied history manifest",
            ));
        }
        rate(r.rate, "fixing_rate")?;
        let rate_start = if matches!(terms, RateAccrual::Term { .. }) {
            start
        } else {
            observation
        };
        observations.push(FuturesRateObservation {
            requirement: r.requirement.clone(),
            rate_start,
            rate_end,
            weight_start,
            weight_end,
            year_fraction: ExactDecimal::new(
                day_count
                    .to_day_count()
                    .year_fraction(weight_start, weight_end),
            ),
            decimal_rate: ExactDecimal::new(r.rate),
            origin: r.origin.clone().into(),
        });
    }
    let fraction = day_count.to_day_count().year_fraction(start, end);
    let mut factor = Decimal::ONE;
    let mut interest = Decimal::ZERO;
    let mut known_factor = Decimal::ONE;
    let mut known_interest = Decimal::ZERO;
    let mut known_days = 0;
    let mut projected_days = 0;
    for observation in &observations {
        let cash = arithmetic(
            observation
                .decimal_rate
                .decimal()
                .checked_mul(observation.year_fraction.decimal()),
            "weighted_rate",
        )?;
        let term = arithmetic(Decimal::ONE.checked_add(cash), "compound_factor")?;
        positive(term, "compound_factor")?;
        let published = matches!(observation.origin, FuturesRateOrigin::Published { .. });
        let days = observation
            .weight_start
            .days_between(&observation.weight_end);
        if published {
            known_days += days;
        } else {
            projected_days += days;
        }
        match averaging {
            RateAveraging::Compounded => {
                factor = arithmetic(factor.checked_mul(term), "compound_product")?;
                if published {
                    known_factor = arithmetic(known_factor.checked_mul(term), "known_factor")?;
                }
            }
            RateAveraging::Arithmetic => {
                interest = arithmetic(interest.checked_add(cash), "arithmetic_average")?;
                if published {
                    known_interest =
                        arithmetic(known_interest.checked_add(cash), "known_interest")?;
                }
            }
        }
    }
    let equivalent = arithmetic(
        (if averaging == RateAveraging::Compounded {
            factor - Decimal::ONE
        } else {
            interest
        })
        .checked_div(fraction),
        "annualized_rate",
    )?;
    let adjustment = if projected_days == 0 {
        Decimal::ZERO
    } else {
        match convexity {
            FuturesConvexityPolicy::NotAdmitted => {
                return Err(invalid(
                    "convexity",
                    "forecast rate future requires an explicit adjustment policy",
                ))
            }
            FuturesConvexityPolicy::ZeroAssumption { .. } => Decimal::ZERO,
            FuturesConvexityPolicy::Supplied {
                decimal_rate,
                model,
                ..
            } => {
                let stamp = inputs.convexity_model.ok_or_else(|| {
                    invalid("convexity", "missing actual adjustment model source")
                })?;
                if stamp.id != model.id || stamp.version != model.version {
                    return Err(invalid(
                        "convexity",
                        "exact adjustment model version required",
                    ));
                }
                report.add(stamp)?;
                decimal_rate.decimal()
            }
        }
    };
    let quoted = arithmetic(equivalent.checked_sub(adjustment), "futures_rate")?;
    rate(quoted, "futures_rate")?;
    let price = arithmetic(
        quoted
            .checked_mul(Decimal::from(100))
            .and_then(|r| Decimal::from(100).checked_sub(r)),
        "rate_price",
    )?;
    let final_price = if projected_days == 0 {
        Some(ExactDecimal::new(inputs.clearing.final_price(quoted)?))
    } else {
        None
    };
    report.finish();
    Ok(RateFuturesValuation {
        contract: InstrumentPin {
            id: inputs.contract.instrument_id.clone(),
            version: inputs.contract.stamp.version,
        },
        curve_equivalent_decimal_rate: ExactDecimal::new(equivalent),
        quoted_decimal_rate: ExactDecimal::new(quoted),
        model_price_points: ExactDecimal::new(price),
        expected_final_price_points: final_price,
        known_days,
        projected_days,
        period_year_fraction: ExactDecimal::new(fraction),
        known_factor: ExactDecimal::new(known_factor),
        known_interest: ExactDecimal::new(known_interest),
        applied_forward_adjustment: ExactDecimal::new(adjustment),
        convexity: convexity.clone(),
        fixing_selection: inputs.fixings.selection,
        observations,
        projection_queries: queries,
        inputs: report,
    })
}

/// Signed expected mark change for +1bp in every remaining simple forward,
/// with published rates and quote-level convexity held fixed. This is a raw
/// projection diagnostic, not recalibrated market-quote PV01 or discount risk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateFuturesRisk {
    /// Base original price/fixings/versions.
    pub base: RateFuturesValuation,
    /// Signed position in integer contracts.
    pub contracts: i64,
    /// Actual central simple-forward bump in decimal annualized rate.
    pub bump_decimal_rate: ExactDecimal,
    /// Signed currency change normalized to a +1bp simple-forward move.
    pub currency_per_forward_bp: Money,
    /// Repriced unrounded index points for +bump.
    pub up_price_points: ExactDecimal,
    /// Repriced unrounded index points for -bump.
    pub down_price_points: ExactDecimal,
    /// `remaining_simple_forwards_frozen_fixings_and_convexity`.
    pub basis: String,
}
/// Actual bump/reprice on the shared valuation, including all unfixed holiday
/// weights. Fully fixed contracts have zero remaining projection exposure.
pub fn rate_futures_risk<P>(
    inputs: &RateFuturesInputs<'_>,
    publication: P,
    contracts: i64,
    bump_decimal_rate: ExactDecimal,
) -> FuturesResult<RateFuturesRisk>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError> + Clone,
{
    let n = count(contracts, true)?;
    let bump = bump_decimal_rate.decimal();
    if bump < Decimal::new(1, 8) || bump > Decimal::new(5, 2) {
        return Err(invalid("risk_bump", "decimal bump must be in[1e-8,0.05]"));
    }
    let base = price_rate_future(inputs, publication.clone())?;
    let up = price_rate_future_bumped(inputs, publication.clone(), bump)?;
    let down = price_rate_future_bumped(inputs, publication, -bump)?;
    if up.observations.len() != base.observations.len()
        || down.observations.len() != base.observations.len()
        || up
            .observations
            .iter()
            .chain(&down.observations)
            .zip(base.observations.iter().cycle())
            .any(|(u, b)| {
                u.requirement != b.requirement
                    || u.origin != b.origin
                    || (matches!(b.origin, FuturesRateOrigin::Published { .. }) && u != b)
            })
    {
        return Err(invalid(
            "risk_history",
            "publication callback changed frozen history across risk scenarios",
        ));
    }
    let value = arithmetic(
        up.model_price_points
            .decimal()
            .checked_sub(down.model_price_points.decimal())
            .and_then(|v| v.checked_mul(inputs.contract.currency_per_price_point.decimal()))
            .and_then(|v| v.checked_mul(n))
            .and_then(|v| v.checked_mul(Decimal::new(1, 4)))
            .and_then(|v| v.checked_div(Decimal::from(2) * bump)),
        "forward_risk",
    )?;
    Ok(RateFuturesRisk {
        base,
        contracts,
        bump_decimal_rate,
        currency_per_forward_bp: money(value, inputs.contract.currency),
        up_price_points: up.model_price_points,
        down_price_points: down.model_price_points,
        basis: "remaining_simple_forwards_frozen_fixings_and_convexity".into(),
    })
}

/// Invert a full quoted price into the equivalent SIMPLE accumulation rate over
/// only the unfixed weights. For compounding this is the residual product rate,
/// not an assumed constant daily forward. The result is absent if already fixed.
pub fn remaining_rate_from_price(
    contract: &FuturesContract,
    valuation: &RateFuturesValuation,
    price_points: ExactDecimal,
) -> FuturesResult<Option<ExactDecimal>> {
    contract.validate()?;
    if valuation.contract.id != contract.instrument_id
        || valuation.contract.version != contract.stamp.version
    {
        return Err(invalid(
            "remaining_rate",
            "exact original contract valuation required",
        ));
    }
    let (terms, _) = accrual(contract)?;
    validate_snapshot(contract, valuation)?;
    let target = arithmetic(
        contract
            .rate_from_price(price_points)?
            .decimal()
            .checked_add(valuation.applied_forward_adjustment.decimal()),
        "remaining_rate",
    )?;
    if valuation.projected_days == 0 {
        if (target - valuation.curve_equivalent_decimal_rate.decimal()).abs() > Decimal::new(1, 12)
        {
            return Err(invalid(
                "remaining_rate",
                "fully fixed payoff cannot fit a different unrounded quote",
            ));
        }
        return Ok(None);
    }
    let remaining = valuation
        .observations
        .iter()
        .filter(|o| !matches!(o.origin, FuturesRateOrigin::Published { .. }))
        .try_fold(Decimal::ZERO, |v, o| {
            arithmetic(
                v.checked_add(o.year_fraction.decimal()),
                "remaining_fraction",
            )
        })?;
    positive(remaining, "remaining_fraction")?;
    let interest = arithmetic(
        target.checked_mul(valuation.period_year_fraction.decimal()),
        "implied_interest",
    )?;
    let residual = if matches!(
        terms,
        RateAccrual::Overnight {
            averaging: RateAveraging::Compounded,
            ..
        }
    ) {
        let total = arithmetic(Decimal::ONE.checked_add(interest), "implied_factor")?;
        positive(total, "implied_factor")?;
        arithmetic(
            total
                .checked_div(valuation.known_factor.decimal())
                .and_then(|v| v.checked_sub(Decimal::ONE)),
            "remaining_factor",
        )?
    } else {
        arithmetic(
            interest.checked_sub(valuation.known_interest.decimal()),
            "remaining_interest",
        )?
    };
    Ok(Some(ExactDecimal::new(arithmetic(
        residual.checked_div(remaining),
        "remaining_rate",
    )?)))
}

// Results are serializable diagnostics, so an inverse must check its economic
// aggregates again rather than trust arbitrary deserialized cached fields.
fn validate_snapshot(contract: &FuturesContract, v: &RateFuturesValuation) -> FuturesResult<()> {
    let (terms, policy) = accrual(contract)?;
    let (index, start, end, dc, compounded) = match terms {
        RateAccrual::Overnight {
            index,
            start,
            end,
            day_count,
            averaging,
            ..
        } => (
            index,
            start.date(),
            end.date(),
            *day_count,
            *averaging == RateAveraging::Compounded,
        ),
        RateAccrual::Term {
            index,
            start,
            end,
            day_count,
            ..
        } => (index, start.date(), end.date(), *day_count, false),
    };
    if v.observations.is_empty()
        || v.observations.len() > convex_core::cashflows::MAX_SCHEDULE_PERIODS
        || &v.convexity != policy
    {
        return Err(invalid(
            "remaining_rate",
            "bounded original observations and convexity policy required",
        ));
    }
    let mut cursor = start;
    let mut factor = Decimal::ONE;
    let mut interest = Decimal::ZERO;
    let mut known_factor = Decimal::ONE;
    let mut known_interest = Decimal::ZERO;
    let mut known_days = 0;
    let mut projected_days = 0;
    for o in &v.observations {
        if o.weight_start != cursor
            || o.weight_end <= cursor
            || o.weight_end > end
            || o.requirement.index_id != *index
            || o.rate_start > o.weight_start
            || o.rate_end < o.weight_end
            || o.year_fraction.decimal()
                != dc
                    .to_day_count()
                    .year_fraction(o.weight_start, o.weight_end)
        {
            return Err(invalid(
                "remaining_rate",
                "original contiguous weighted intervals required",
            ));
        }
        match terms {
            RateAccrual::Term { fixing, .. }
                if v.observations.len() != 1
                    || o.requirement.observation_date != fixing.date()
                    || o.rate_start != start
                    || o.rate_end != end =>
            {
                return Err(invalid(
                    "remaining_rate",
                    "original term fixing/value interval required",
                ))
            }
            RateAccrual::Overnight { .. } if o.rate_start != o.requirement.observation_date => {
                return Err(invalid(
                    "remaining_rate",
                    "original overnight quotation interval required",
                ))
            }
            _ => (),
        }
        cursor = o.weight_end;
        rate(o.decimal_rate.decimal(), "remaining_rate")?;
        let cash = arithmetic(
            o.decimal_rate
                .decimal()
                .checked_mul(o.year_fraction.decimal()),
            "remaining_rate",
        )?;
        let term = arithmetic(Decimal::ONE.checked_add(cash), "remaining_rate")?;
        positive(term, "remaining_rate")?;
        let published = matches!(o.origin, FuturesRateOrigin::Published { .. });
        if let FuturesRateOrigin::Published {
            revision,
            published_at,
            known_at,
            ..
        } = &o.origin
        {
            if *known_at < *published_at
                || *known_at > v.inputs.context.known_at
                || o.requirement.observation_date > date(v.inputs.context)
                || (v.fixing_selection == FuturesFixingSelection::FirstPublished
                    && revision.get() != 1)
            {
                return Err(invalid("remaining_rate", "ineligible original publication"));
            }
        }
        let days = o.weight_start.days_between(&o.weight_end);
        if published {
            known_days += days;
        } else {
            projected_days += days;
        }
        if compounded {
            factor = arithmetic(factor.checked_mul(term), "remaining_rate")?;
            if published {
                known_factor = arithmetic(known_factor.checked_mul(term), "remaining_rate")?;
            }
        } else {
            interest = arithmetic(interest.checked_add(cash), "remaining_rate")?;
            if published {
                known_interest = arithmetic(known_interest.checked_add(cash), "remaining_rate")?;
            }
        }
    }
    let fraction = dc.to_day_count().year_fraction(start, end);
    let equivalent = arithmetic(
        (if compounded {
            factor - Decimal::ONE
        } else {
            interest
        })
        .checked_div(fraction),
        "remaining_rate",
    )?;
    let adjustment = if projected_days == 0 {
        Decimal::ZERO
    } else {
        match policy {
            FuturesConvexityPolicy::Supplied { decimal_rate, .. } => decimal_rate.decimal(),
            FuturesConvexityPolicy::ZeroAssumption { .. } => Decimal::ZERO,
            FuturesConvexityPolicy::NotAdmitted => {
                return Err(invalid("remaining_rate", "forecast not admitted"))
            }
        }
    };
    let quoted = arithmetic(equivalent.checked_sub(adjustment), "remaining_rate")?;
    let price = arithmetic(
        quoted
            .checked_mul(Decimal::from(100))
            .and_then(|r| Decimal::from(100).checked_sub(r)),
        "remaining_rate",
    )?;
    if cursor != end
        || known_days != v.known_days
        || projected_days != v.projected_days
        || fraction != v.period_year_fraction.decimal()
        || known_factor != v.known_factor.decimal()
        || known_interest != v.known_interest.decimal()
        || equivalent != v.curve_equivalent_decimal_rate.decimal()
        || quoted != v.quoted_decimal_rate.decimal()
        || price != v.model_price_points.decimal()
        || adjustment != v.applied_forward_adjustment.decimal()
    {
        return Err(invalid(
            "remaining_rate",
            "inconsistent cached valuation aggregates",
        ));
    }
    Ok(())
}
