//! Complete supplied curve reconstruction. Ports retain versioned wire payloads;
//! this boundary decodes native terms and builds the entire immutable graph.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use convex_curves::{
    contracts::{
        build_curve_set, BuiltCurve, CurveBuildOutput, CurvePoint, CurveQuery, CurveRecipe,
        CurveRepresentation, CurveRole, CurveSet, TenorRounding,
    },
    Compounding, CurveError, CurveResult, RateCurveDyn,
};
use convex_ports::{snapshot::*, TraitError};
use std::{collections::BTreeSet, sync::Arc};

/// Reconstruction errors retain original native fields, dependency versions,
/// cycles, residuals and unsupported-model reasons for numerical callers.
#[derive(Debug, thiserror::Error)]
pub enum CurveReplayError {
    /// Invalid or oversized captured metadata.
    #[error(transparent)]
    Record(#[from] TraitError),
    /// Strict native payload decoding failure.
    #[error(transparent)]
    Payload(#[from] serde_json::Error),
    /// Original native build failure, without a partial accepted graph.
    #[error(transparent)]
    Native(#[from] convex_curves::contracts::CurveContractError),
    /// Original complete product-calibration admission failure.
    #[error(transparent)]
    Calibration(Box<crate::calibration_replay::CalibrationReplayError>),
}
impl From<CurveReplayError> for TraitError {
    fn from(value: CurveReplayError) -> Self {
        match value {
            CurveReplayError::Record(error) => error,
            CurveReplayError::Calibration(error) => (*error).into(),
            CurveReplayError::Native(
                convex_curves::contracts::CurveContractError::BoundExceeded(_),
            ) => Self::CapacityExceeded,
            CurveReplayError::Native(
                convex_curves::contracts::CurveContractError::Unsupported { .. },
            ) => Self::Unsupported(
                "complete curve recipe requires its native model/calibration owner",
            ),
            _ => Self::InvalidInput("complete native curve graph reconstruction failed"),
        }
    }
}

/// Decode complete original recipes. Unknown terms, mixed record collisions,
/// aggregate resource excess, cycles and missing exact versions return no set.
/// Product-helper graph calibration needs its own full supplied composition;
/// helper IDs/settings are never reinterpreted as direct node curves here.
pub fn rebuild_curve_recipes(
    records: &[SnapshotCurveRecipe],
) -> Result<CurveBuildOutput, CurveReplayError> {
    if records.len() > MAX_SNAPSHOT_CURVES
        || records
            .iter()
            .map(SnapshotCurveRecipe::input_count)
            .sum::<usize>()
            > MAX_SNAPSHOT_PILLARS
    {
        return Err(TraitError::CapacityExceeded.into());
    }
    let mut ids = BTreeSet::new();
    let recipes = records
        .iter()
        .map(|record| {
            record.validate()?;
            if !ids.insert(&record.curve_id) {
                return Err(TraitError::InvalidInput("duplicate complete curve identity").into());
            }
            serde_json::from_value::<CurveRecipe>(record.recipe.clone())
                .map_err(CurveReplayError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if recipes.is_empty() {
        return Ok(CurveBuildOutput {
            curves: CurveSet::new(),
            build_order: vec![],
        });
    }
    build_curve_set(&recipes, &CurveSet::new()).map_err(CurveReplayError::from)
}

/// Verify the content identity and every retained recipe before numerical use.
/// Legacy zero records do not substitute for a missing full native dependency.
pub fn replay_snapshot_curves(
    snapshot: &MarketSnapshot,
) -> Result<CurveBuildOutput, CurveReplayError> {
    snapshot.verify()?;
    for record in &snapshot.content().curves {
        crate::BuiltCurve::from_snapshot(record)?;
    }
    if snapshot.content().calibration.is_none() {
        return rebuild_curve_recipes(&snapshot.content().curve_recipes);
    }
    crate::calibration_replay::rebuild_captured_curves(
        &snapshot.content().curve_recipes,
        snapshot.content().calibration.as_ref(),
        snapshot.content().as_of,
        snapshot.content().known_at,
    )
    .map_err(|e| CurveReplayError::Calibration(Box::new(e)))
}

/// Thin bridge to older object-safe rate pricing. Full queries still expose
/// original sides, units, method diagnostics and actual extension consumption.
#[derive(Debug, Clone)]
pub struct RecipeRateCurve {
    curve: Arc<BuiltCurve>,
    maximum_date: convex_core::Date,
}

impl RecipeRateCurve {
    /// Only rate roles and actual DF/zero representations can enter rate pricing.
    /// The last calendar date on/before the native horizon uses the original
    /// time basis and an explicit AtOrBefore convention, without a sampled grid.
    pub fn new(curve: Arc<BuiltCurve>) -> CurveResult<Self> {
        let recipe = curve.recipe();
        if !matches!(
            recipe.role,
            CurveRole::Discount { .. }
                | CurveRole::Projection { .. }
                | CurveRole::Government { .. }
                | CurveRole::Repo { .. }
        ) || !matches!(
            recipe.representation,
            CurveRepresentation::DiscountFactors | CurveRepresentation::ZeroRates { .. }
        ) {
            return Err(CurveError::invalid_value(
                "rate pricing requires a native rate role and DF/zero representation",
            ));
        }
        let maximum_date = curve
            .tenor_to_date(curve.domain().maximum_tenor, TenorRounding::AtOrBefore)
            .map_err(|e| CurveError::invalid_value(e.to_string()))?;
        Ok(Self {
            curve,
            maximum_date,
        })
    }
    /// Original checked curve, with the complete recipe and method-specific queries.
    pub fn native(&self) -> &Arc<BuiltCurve> {
        &self.curve
    }
    fn value(&self, query: CurveQuery) -> CurveResult<f64> {
        self.curve
            .query(&query)
            .map(|v| v.value)
            .map_err(|e| CurveError::invalid_value(e.to_string()))
    }
}
impl RateCurveDyn for RecipeRateCurve {
    fn discount_factor(&self, t: f64) -> CurveResult<f64> {
        self.value(CurveQuery::DiscountFactor {
            point: CurvePoint::Tenor(t),
        })
    }
    fn zero_rate(&self, t: f64, compounding: Compounding) -> CurveResult<f64> {
        self.value(CurveQuery::ZeroRate {
            point: CurvePoint::Tenor(t),
            compounding,
            day_count: self.curve.recipe().time_basis,
        })
    }
    fn forward_rate(&self, t1: f64, t2: f64) -> CurveResult<f64> {
        self.value(CurveQuery::ForwardRate {
            start: CurvePoint::Tenor(t1),
            end: CurvePoint::Tenor(t2),
            compounding: Compounding::Continuous,
            day_count: self.curve.recipe().time_basis,
        })
    }
    fn instantaneous_forward(&self, t: f64) -> CurveResult<f64> {
        self.value(CurveQuery::InstantaneousForward {
            point: CurvePoint::Tenor(t),
        })
    }
    fn reference_date(&self) -> convex_core::Date {
        self.curve.recipe().reference_date
    }
    fn max_date(&self) -> convex_core::Date {
        self.maximum_date
    }
    fn date_to_tenor(&self, date: convex_core::Date) -> f64 {
        self.curve.date_to_tenor(date).unwrap_or(f64::NAN)
    }
}
