//! Overnight compounding from explicit observation weights and eligible fixings.

use super::{validate_span, CashflowError, MAX_DATE_OFFSET, MAX_SCHEDULE_PERIODS};
use crate::{
    calendars::{checked, Calendar},
    contracts::{IndexId, UtcTimestamp},
    daycounts::DayCountConvention,
    fixings::{FixingContext, FixingRequirement, FixingSeries, RateOrigin},
    Date,
};
use rust_decimal::Decimal;
use std::collections::BTreeMap;

/// Explicit contracted overnight observation mechanics. No universal SOFR,
/// SONIA or EUR rule is inferred from an index name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OvernightConvention {
    /// Look back this many business days on the supplied observation calendar.
    pub lookback_business_days: u32,
    /// Weight by shifted observation days when true, accrual days when false.
    pub observation_shift: bool,
    /// Freeze the final N observation business days at the last preceding rate.
    pub lockout_business_days: u32,
}

/// One explicit term of the compound product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OvernightObservation {
    /// Index observation date after lookback/lockout.
    pub observation_date: Date,
    /// End of the full overnight rate period, used by the projection curve.
    pub rate_end: Date,
    /// Start of the weighted interval (may be inside a weekend).
    pub weight_start: Date,
    /// End of the weighted interval; the last interval may be clipped.
    pub weight_end: Date,
}

/// Immutable validated weights, separated from history and projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OvernightSchedule {
    start: Date,
    end: Date,
    day_count: DayCountConvention,
    observations: Vec<OvernightObservation>,
}

/// One rate actually consumed in a compounded result, with its day weighting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedOvernightObservation {
    /// Validated observation and weight dates.
    pub dates: OvernightObservation,
    /// Day-count weight in the product.
    pub year_fraction: Decimal,
    /// Annualized decimal rate.
    pub rate: Decimal,
    /// Exact fixing revision or explicit forecast reason.
    pub origin: RateOrigin,
}

/// Checked contractual compound product. This result does not record a payment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundedCoupon {
    /// Product of (1 + rate * weight), including the initial unit investment.
    pub period_factor: Decimal,
    /// Unshifted interest-period year fraction, for spread/gearing composition.
    pub period_year_fraction: Decimal,
    /// (period_factor - 1) / period_year_fraction, in decimal annualized units.
    pub annualized_rate: Decimal,
    /// Every known/projected observation, including reused lockout rates.
    pub observations: Vec<ResolvedOvernightObservation>,
}

impl OvernightSchedule {
    /// Generate actual business-day weights, including a partial initial weekend.
    /// Observation shift requires business-day endpoints; nonbusiness shifted
    /// endpoints need a separately reviewed contract rather than implicit rounding.
    pub fn generate(
        start: Date,
        end: Date,
        day_count: DayCountConvention,
        calendar: &dyn Calendar,
        convention: OvernightConvention,
    ) -> Result<Self, CashflowError> {
        validate_span(start, end)?;
        if !matches!(
            day_count,
            DayCountConvention::Act360 | DayCountConvention::Act365Fixed
        ) {
            return Err(CashflowError::terms(
                "overnight_day_count",
                "supports ACT/360 or ACT/365 Fixed",
            ));
        }
        if convention.lookback_business_days > MAX_DATE_OFFSET as u32
            || convention.lockout_business_days > MAX_DATE_OFFSET as u32
        {
            return Err(CashflowError::BoundExceeded("overnight convention"));
        }
        let lookback = convention.lookback_business_days as i32;
        let (weight_start, weight_end) = if convention.observation_shift {
            if !calendar.is_business_day(start) || !calendar.is_business_day(end) {
                return Err(CashflowError::terms(
                    "observation_shift",
                    "requires business-day endpoints",
                ));
            }
            (
                super::DateOffset::BusinessDays(-lookback).apply(start, calendar)?,
                super::DateOffset::BusinessDays(-lookback).apply(end, calendar)?,
            )
        } else {
            (start, end)
        };
        validate_span(weight_start, weight_end)?;
        let mut current = weight_start;
        let mut rate_date = checked::business_day_on_or_before(weight_start, calendar)?;
        super::validate_date(rate_date)?;
        let mut observations = Vec::new();
        for _ in 0..MAX_SCHEDULE_PERIODS {
            let next = super::DateOffset::BusinessDays(1).apply(rate_date, calendar)?;
            let interval_end = next.min(weight_end);
            let observation_date = if convention.observation_shift {
                rate_date
            } else {
                super::DateOffset::BusinessDays(-lookback).apply(rate_date, calendar)?
            };
            let rate_end = super::DateOffset::BusinessDays(1).apply(observation_date, calendar)?;
            observations.push(OvernightObservation {
                observation_date,
                rate_end,
                weight_start: current,
                weight_end: interval_end,
            });
            current = interval_end;
            if current == weight_end {
                break;
            }
            rate_date = next;
        }
        if current != weight_end {
            return Err(CashflowError::BoundExceeded("overnight observations"));
        }
        let lockout = convention.lockout_business_days as usize;
        if lockout > 0 {
            if lockout >= observations.len() {
                return Err(CashflowError::terms(
                    "lockout",
                    "requires an observation preceding the freeze",
                ));
            }
            let freeze_position = observations.len() - lockout - 1;
            let frozen_date = observations[freeze_position].observation_date;
            let frozen_end = observations[freeze_position].rate_end;
            for observation in &mut observations[(freeze_position + 1)..] {
                observation.observation_date = frozen_date;
                observation.rate_end = frozen_end;
            }
        }
        Ok(Self {
            start,
            end,
            day_count,
            observations,
        })
    }

    /// Explicit weights and dates for requirements discovery or audit.
    pub fn observations(&self) -> &[OvernightObservation] {
        &self.observations
    }

    /// Unique required observations, in chronological order. Publication instants
    /// come from a supplied administrator schedule, including holiday/DST rules.
    pub fn requirements<P>(
        &self,
        index_id: &IndexId,
        mut publication: P,
    ) -> Result<Vec<FixingRequirement>, CashflowError>
    where
        P: FnMut(Date) -> Result<UtcTimestamp, CashflowError>,
    {
        let mut dates = BTreeMap::new();
        for observation in &self.observations {
            dates.insert(observation.observation_date, ());
        }
        dates
            .keys()
            .map(|&date| {
                Ok(FixingRequirement {
                    index_id: index_id.clone(),
                    observation_date: date,
                    expected_publication: publication(date)?,
                })
            })
            .collect()
    }

    /// Resolve all required history before projecting any rate. A projected rate
    /// callback receives the full observation rate interval, not an accrual date.
    /// Errors propagate; missing published history is never forecast or zeroed.
    pub fn compound<P, F>(
        &self,
        index_id: &IndexId,
        history: &FixingSeries,
        context: FixingContext,
        publication: P,
        mut forecast: F,
    ) -> Result<CompoundedCoupon, CashflowError>
    where
        P: FnMut(Date) -> Result<UtcTimestamp, CashflowError>,
        F: FnMut(Date, Date) -> Result<Decimal, CashflowError>,
    {
        let requirements = self.requirements(index_id, publication)?;
        let intervals: BTreeMap<_, _> = self
            .observations
            .iter()
            .map(|o| (o.observation_date, o.rate_end))
            .collect();
        let rates = history
            .resolve(&requirements, context)?
            .project(|requirement| {
                let end = intervals
                    .get(&requirement.observation_date)
                    .ok_or_else(|| CashflowError::terms("observation", "missing rate interval"))?;
                forecast(requirement.observation_date, *end)
            })?;
        let rates: BTreeMap<_, _> = rates
            .into_iter()
            .map(|rate| (rate.requirement.observation_date, rate))
            .collect();
        let day_count = self.day_count.to_day_count();
        let mut factor = Decimal::ONE;
        let mut resolved = Vec::with_capacity(self.observations.len());
        for observation in &self.observations {
            let rate = rates
                .get(&observation.observation_date)
                .ok_or_else(|| CashflowError::terms("observation", "unresolved rate"))?;
            let fraction =
                day_count.year_fraction(observation.weight_start, observation.weight_end);
            let term = rate
                .rate
                .checked_mul(fraction)
                .and_then(|x| x.checked_add(Decimal::ONE))
                .ok_or(CashflowError::Arithmetic("overnight factor"))?;
            if term <= Decimal::ZERO {
                return Err(CashflowError::terms(
                    "overnight_factor",
                    "nonpositive compound factor",
                ));
            }
            factor = factor
                .checked_mul(term)
                .ok_or(CashflowError::Arithmetic("overnight product"))?;
            resolved.push(ResolvedOvernightObservation {
                dates: observation.clone(),
                year_fraction: fraction,
                rate: rate.rate,
                origin: rate.origin.clone(),
            });
        }
        let period_year_fraction = day_count.year_fraction(self.start, self.end);
        let annualized_rate = factor
            .checked_sub(Decimal::ONE)
            .and_then(|x| x.checked_div(period_year_fraction))
            .ok_or(CashflowError::Arithmetic("overnight annualization"))?;
        Ok(CompoundedCoupon {
            period_factor: factor,
            period_year_fraction,
            annualized_rate,
            observations: resolved,
        })
    }
}
