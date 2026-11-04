//! Complete supplied mixed rate calibration and dependency rebuild composition.
//! Full economics remain above curves; no provider, storage, clock or registry.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod build;
mod fit;
mod helpers;
mod inputs;
mod reprice;

pub use build::build_rate_curve_set;
pub use inputs::*;
pub use reprice::{reprice_rate_helpers, RateHelperRepricing};

use convex_core::{
    cashflows::CashflowError,
    contracts::{IndexId, UtcTimestamp},
    fixings::FixingRequirement,
    Date,
};
use convex_curves::contracts::{CurveContractError, CurveKey};
use std::collections::BTreeMap;

/// Total original helpers and parameters in one atomic graph.
pub const MAX_RATE_FIT_HELPERS: usize = 128;
/// Unique frozen administrator observations in one complete graph.
pub const MAX_RATE_FIT_OBSERVATIONS: usize = 32_768;
/// Total actual helper valuations, including trial/preflight/final repricing.
pub const MAX_RATE_PRICE_EVALUATIONS: usize = 16_384;

type Publications = BTreeMap<(IndexId, Date), FixingRequirement>;
fn invalid(field: &str, reason: impl Into<String>) -> RateCalibrationError {
    CurveContractError::InvalidInput {
        field: field.into(),
        reason: reason.into(),
    }
    .into()
}
fn unsupported(reason: &str) -> RateCalibrationError {
    CurveContractError::Unsupported {
        capability: "mixed rate calibration".into(),
        reason: reason.into(),
    }
    .into()
}
fn publication(
    map: &Publications,
    index: &IndexId,
    at: Date,
) -> Result<UtcTimestamp, CashflowError> {
    map.get(&(index.clone(), at))
        .map(|r| r.expected_publication)
        .ok_or_else(|| CashflowError::InvalidTerms {
            field: "publication",
            reason: "missing frozen index/date publication".into(),
        })
}
fn freeze<P>(
    map: &mut Publications,
    index: &IndexId,
    at: Date,
    callback: &mut P,
) -> Result<UtcTimestamp, CashflowError>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    if let Some(r) = map.get(&(index.clone(), at)) {
        return Ok(r.expected_publication);
    }
    if map.len() >= MAX_RATE_FIT_OBSERVATIONS {
        return Err(CashflowError::BoundExceeded(
            "rate calibration publications",
        ));
    }
    let t = callback(index, at)?;
    map.insert(
        (index.clone(), at),
        FixingRequirement {
            index_id: index.clone(),
            observation_date: at,
            expected_publication: t,
        },
    );
    Ok(t)
}
fn same_output(key: &CurveKey, recipe: &RateCurveRecipe) -> bool {
    key == &recipe.curve.key
        || recipe
            .overnight_projection
            .as_ref()
            .is_some_and(|v| key == &v.key)
}
