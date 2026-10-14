//! Immutable supplied fixing history with publication, receipt and revision cutoffs.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::{
    cashflows::{validate_date, CashflowError},
    contracts::{IndexId, SourceId, UtcTimestamp, Version},
    Date,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum supplied fixing revisions, across all indices and dates.
pub const MAX_FIXING_OBSERVATIONS: usize = 65_536;
/// Maximum fixing requirements in one resolution request.
pub const MAX_FIXING_REQUIREMENTS: usize = 4096;

/// Administrator status supplied by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum FixingStatus {
    /// Available for policies allowing provisional values.
    Preliminary,
    /// Final published value, including final corrections.
    Final,
}

/// One supplied revision. Rates are annualized decimals, never percent quotes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FixingObservation {
    /// Provider-neutral index identity.
    pub index_id: IndexId,
    /// Value/observation date, distinct from publication date.
    pub observation_date: Date,
    /// Annualized decimal rate; negative rates are valid within ±10.
    #[serde(with = "crate::contracts::decimal_serde")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "crate::contracts::decimal_serde::schema")
    )]
    pub rate: Decimal,
    /// Actual administrator publication instant.
    pub published_at: UtcTimestamp,
    /// Instant the information became available to this information set.
    pub known_at: UtcTimestamp,
    /// Positive revision, strictly increasing per index/date.
    pub revision: Version,
    /// Preliminary or final administrator status.
    pub status: FixingStatus,
    /// Selected source; resolving competing sources is an application decision.
    pub source_id: SourceId,
}

/// A required observation and its contractual expected publication instant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FixingRequirement {
    /// Exact required index.
    pub index_id: IndexId,
    /// Observation date, not publication date.
    pub observation_date: Date,
    /// Supplied publication calendar/time expectation. No timezone is inferred.
    pub expected_publication: UtcTimestamp,
}

/// Explicit treatment of a past observation that is not yet due for publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum UnpublishedFixingPolicy {
    /// Forecast, retaining an AwaitingPublication origin distinct from future dates.
    Forecast,
    /// Return the requirement as missing; do not invoke a curve.
    RequirePublished,
}

/// All time and admissibility policy supplied by the valuation caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FixingContext {
    /// Future observation dates are projected, even in a restated history.
    pub valuation_date: Date,
    /// Both publication and receipt must be on or before this instant.
    pub known_at: UtcTimestamp,
    /// Explicit unpublished-observation policy.
    pub unpublished: UnpublishedFixingPolicy,
    /// Reject preliminary observations when true.
    pub final_only: bool,
}

/// Reason a curve may be used. Missing already-published history is never one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ProjectionReason {
    /// The observation date is after valuation date.
    FutureObservation,
    /// A past observation is not yet due for publication under explicit policy.
    AwaitingPublication,
}

/// Provenance of a consumed or projected rate.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum RateOrigin {
    /// Exact published revision retained in an immutable result.
    Published(FixingObservation),
    /// Explicit permitted forecast.
    Projected(ProjectionReason),
}

/// One resolved rate, preserving requirement and origin.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ResolvedRate {
    /// Original required observation and due time.
    pub requirement: FixingRequirement,
    /// Annualized decimal rate.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub rate: Decimal,
    /// Known revision or reason for forecast.
    pub origin: RateOrigin,
}

#[derive(Debug, Clone)]
enum Selection {
    Published(FixingObservation),
    Project(ProjectionReason),
    Missing,
}

/// Resolution before valuation; inspect missing inputs before invoking projection.
#[derive(Debug, Clone)]
pub struct FixingResolution {
    selected: Vec<(FixingRequirement, Selection)>,
    missing: Vec<FixingRequirement>,
}

impl FixingResolution {
    /// Complete list of absent published/required observations, in request order.
    pub fn missing(&self) -> &[FixingRequirement] {
        &self.missing
    }
    /// Permitted forecasts, without creating placeholder rate values. This
    /// inspects eligibility only; missing history must still be supplied.
    pub fn projected_requirements(&self) -> impl Iterator<Item = &FixingRequirement> {
        self.selected.iter().filter_map(|(requirement, selection)| {
            matches!(selection, Selection::Project(_)).then_some(requirement)
        })
    }
    /// Forecast only permitted observations. All missing history fails before any
    /// callback runs. Projection errors propagate without rate-to-zero fallback.
    pub fn project<F>(&self, mut forecast: F) -> Result<Vec<ResolvedRate>, CashflowError>
    where
        F: FnMut(&FixingRequirement) -> Result<Decimal, CashflowError>,
    {
        if !self.missing.is_empty() {
            return Err(CashflowError::MissingFixings(self.missing.clone()));
        }
        self.selected
            .iter()
            .map(|(requirement, selection)| {
                let (rate, origin) = match selection {
                    Selection::Published(observation) => {
                        (observation.rate, RateOrigin::Published(observation.clone()))
                    }
                    Selection::Project(reason) => {
                        (forecast(requirement)?, RateOrigin::Projected(*reason))
                    }
                    Selection::Missing => {
                        return Err(CashflowError::MissingFixings(vec![requirement.clone()]))
                    }
                };
                validate_rate(rate)?;
                Ok(ResolvedRate {
                    requirement: requirement.clone(),
                    rate,
                    origin,
                })
            })
            .collect()
    }
}

/// Validated immutable history. Construction admits unordered input, but never
/// duplicate revisions, competing sources or backward revision timestamps.
#[derive(Debug, Clone, Default)]
pub struct FixingSeries {
    history: BTreeMap<(IndexId, Date), Vec<FixingObservation>>,
}

impl FixingSeries {
    /// Validate supplied observations once; no repository or mutable global state.
    pub fn new(observations: Vec<FixingObservation>) -> Result<Self, CashflowError> {
        if observations.len() > MAX_FIXING_OBSERVATIONS {
            return Err(CashflowError::BoundExceeded("fixing history"));
        }
        let mut history: BTreeMap<(IndexId, Date), Vec<FixingObservation>> = BTreeMap::new();
        for observation in observations {
            validate_date(observation.observation_date)?;
            validate_rate(observation.rate)?;
            if observation.known_at < observation.published_at
                || observation.published_at.datetime().date_naive()
                    < observation.observation_date.as_naive_date()
            {
                return Err(CashflowError::terms(
                    "fixing_times",
                    "receipt precedes publication or publication precedes observation date",
                ));
            }
            history
                .entry((observation.index_id.clone(), observation.observation_date))
                .or_default()
                .push(observation);
        }
        for revisions in history.values_mut() {
            revisions.sort_by_key(|observation| observation.revision);
            for pair in revisions.windows(2) {
                if pair[0].revision == pair[1].revision
                    || pair[0].source_id != pair[1].source_id
                    || pair[1].published_at < pair[0].published_at
                    || pair[1].known_at < pair[0].known_at
                    || (pair[0].status == FixingStatus::Final
                        && pair[1].status == FixingStatus::Preliminary)
                {
                    return Err(CashflowError::terms(
                        "fixing_revisions",
                        "duplicate, competing source or inconsistent revision order/status",
                    ));
                }
            }
        }
        Ok(Self { history })
    }

    /// Resolve a unique list with caller-supplied expected publication times.
    /// The highest eligible revision wins; future publications/receipts never leak.
    pub fn resolve(
        &self,
        requirements: &[FixingRequirement],
        context: FixingContext,
    ) -> Result<FixingResolution, CashflowError> {
        validate_date(context.valuation_date)?;
        if requirements.len() > MAX_FIXING_REQUIREMENTS {
            return Err(CashflowError::BoundExceeded("fixing requirements"));
        }
        let mut keys = BTreeSet::new();
        let mut selected = Vec::with_capacity(requirements.len());
        let mut missing = Vec::new();
        for requirement in requirements {
            validate_date(requirement.observation_date)?;
            if requirement.expected_publication.datetime().date_naive()
                < requirement.observation_date.as_naive_date()
            {
                return Err(CashflowError::terms(
                    "publication",
                    "expected publication before observation date",
                ));
            }
            let key = (requirement.index_id.clone(), requirement.observation_date);
            if !keys.insert(key.clone()) {
                return Err(CashflowError::terms("requirements", "duplicate index/date"));
            }
            let selection = if requirement.observation_date > context.valuation_date {
                Selection::Project(ProjectionReason::FutureObservation)
            } else if let Some(observation) = self.history.get(&key).and_then(|revisions| {
                revisions.iter().rev().find(|observation| {
                    observation.published_at <= context.known_at
                        && observation.known_at <= context.known_at
                        && (!context.final_only || observation.status == FixingStatus::Final)
                })
            }) {
                Selection::Published(observation.clone())
            } else if requirement.expected_publication > context.known_at
                && context.unpublished == UnpublishedFixingPolicy::Forecast
            {
                Selection::Project(ProjectionReason::AwaitingPublication)
            } else {
                missing.push(requirement.clone());
                Selection::Missing
            };
            selected.push((requirement.clone(), selection));
        }
        Ok(FixingResolution { selected, missing })
    }
}

pub(crate) fn validate_rate(rate: Decimal) -> Result<(), CashflowError> {
    if rate < Decimal::from(-10) || rate > Decimal::from(10) {
        return Err(CashflowError::terms(
            "rate",
            "annualized decimal rate outside ±10",
        ));
    }
    Ok(())
}
