//! Curve builder - constructs curves from market data.

/// Delegate supplied credit construction to the pure product composition.
pub use convex_analytics::credit_calibration::build_credit_curve_set as build_supplied_credit_curves;
/// Supplied product calibration delegates directly to the pure composition API.
/// Callers retain exact immutable results; this entry point uses no live lookup,
/// cache publication, timestamp, tenor approximation or numerical duplication.
pub use convex_analytics::swap_calibration::build_swap_curve_set as build_supplied_swap_curves;

use std::sync::Arc;

use dashmap::DashMap;
use tracing::info;

use crate::ports::market_data::MarketDataProvider;
use convex_core::daycounts::DayCountConvention;
use convex_core::ids::CurveId;
use convex_core::Date;
use convex_curves::{
    Compounding, CurveError, CurveResult, DiscreteCurve, ExtrapolationMethod, InterpolationMethod,
    RateCurveDyn, ValueType,
};

use crate::calc_graph::{CalculationGraph, NodeId};
use crate::error::EngineError;

/// Validated numerical owner; complete recipes are never converted to nodes.
#[derive(Debug, Clone)]
pub enum BuiltCurveModel {
    /// Existing explicit ACT/365F continuous-zero representation.
    Discrete(DiscreteCurve),
    /// Exact full native recipe and dependency graph.
    Recipe(crate::curve_replay::RecipeRateCurve),
}

/// Built curve (cached in memory).
///
/// Implements `RateCurveDyn` to be compatible with spread calculations.
#[derive(Debug, Clone)]
pub struct BuiltCurve {
    /// Curve identifier
    pub curve_id: CurveId,
    /// Reference date
    pub reference_date: Date,
    /// Curve points (tenor years -> zero rate as decimal)
    pub points: Vec<(f64, f64)>,
    /// Build timestamp
    pub built_at: i64,
    /// Hash of inputs (for change detection)
    pub inputs_hash: String,

    /// Long-end extrapolation used when (re)building [`BuiltCurve::inner`].
    pub extrapolation: ExtrapolationMethod,

    /// The proper math curve underlying this
    pub inner: Option<Arc<BuiltCurveModel>>,
}

impl BuiltCurve {
    /// Original continuous-zero query. Invalid/unbuilt values return NaN;
    /// numerical callers should use the checked RateCurveDyn queries.
    pub fn interpolate_rate(&self, tenor_years: f64) -> f64 {
        match &self.inner {
            Some(model) => match model.as_ref() {
                BuiltCurveModel::Discrete(curve) => {
                    curve.checked_value_at(tenor_years).unwrap_or(f64::NAN)
                }
                BuiltCurveModel::Recipe(curve) => curve
                    .zero_rate(tenor_years, Compounding::Continuous)
                    .unwrap_or(f64::NAN),
            },
            None => f64::NAN,
        }
    }

    /// Rebuild original positive ACT/365F continuous-zero pillars. Invalid,
    /// duplicate or unsupported inputs clear the old representation and fail;
    /// no partial/last-good curve is attached to changed inputs.
    pub fn rebuild_inner(&mut self) -> CurveResult<()> {
        self.inner = None;
        let mut sorted = self.points.clone();
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
        if sorted.len() < 2
            || sorted.len() > convex_ports::snapshot::MAX_SNAPSHOT_PILLARS
            || sorted
                .iter()
                .any(|(t, r)| !t.is_finite() || *t <= 0.0 || !r.is_finite())
            || sorted.windows(2).any(|p| p[0].0 >= p[1].0)
        {
            return Err(CurveError::invalid_value(
                "bounded distinct positive finite curve pillars required",
            ));
        }
        let curve = DiscreteCurve::with_extrapolation(
            self.reference_date,
            sorted.iter().map(|p| p.0).collect(),
            sorted.iter().map(|p| p.1).collect(),
            ValueType::continuous_zero(DayCountConvention::Act365Fixed),
            InterpolationMethod::MonotoneConvex,
            self.extrapolation,
        )?;
        self.inner = Some(Arc::new(BuiltCurveModel::Discrete(curve)));
        Ok(())
    }

    /// Actual native horizon; an unavailable representation has no valid horizon.
    pub fn max_tenor(&self) -> f64 {
        match self.inner.as_deref() {
            Some(BuiltCurveModel::Recipe(curve)) => curve.native().domain().maximum_tenor,
            Some(BuiltCurveModel::Discrete(_)) => self
                .points
                .iter()
                .map(|p| p.0)
                .max_by(f64::total_cmp)
                .unwrap_or(f64::NAN),
            None => f64::NAN,
        }
    }

    /// Older graph day/zero format is only usable for exact whole-day legacy
    /// pillars. Full recipes and fractional pillars require immutable snapshots.
    pub fn to_points(&self) -> Result<Vec<(u32, f64)>, EngineError> {
        let Some(BuiltCurveModel::Discrete(model)) = self.inner.as_deref() else {
            return Err(EngineError::CurveBuildError(
                "complete curve recipes require immutable snapshot graph publication".into(),
            ));
        };
        if self.points.len() != model.tenors().len() {
            return Err(EngineError::CurveBuildError(
                "curve points differ from validated representation".into(),
            ));
        }
        let mut points = self.points.clone();
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        if !points
            .iter()
            .zip(model.tenors().iter().zip(model.values()))
            .all(|((t, r), (built_t, built_r))| t == built_t && r == built_r)
        {
            return Err(EngineError::CurveBuildError(
                "curve points differ from validated representation".into(),
            ));
        }
        points
            .iter()
            .map(|(t, r)| {
                // Multiplication can round an original integer day away from
                // the integer. Only the exact original day/365 roundtrip admits it.
                let days = (t * 365.0).round();
                if !days.is_finite() || days < 1.0 || days > u32::MAX as f64 || days / 365.0 != *t {
                    return Err(EngineError::CurveBuildError(
                        "older graph cannot represent fractional curve pillars".into(),
                    ));
                }
                Ok((days as u32, *r))
            })
            .collect()
    }

    /// Retain the original full checked curve without a fabricated sampled grid.
    pub fn from_recipe(
        record: &convex_ports::snapshot::SnapshotCurveRecipe,
        curve: Arc<convex_curves::contracts::BuiltCurve>,
    ) -> Result<Self, convex_ports::TraitError> {
        record.validate()?;
        let supplied: convex_curves::contracts::CurveRecipe =
            serde_json::from_value(record.recipe.clone()).map_err(|_| {
                convex_ports::TraitError::InvalidInput("invalid native curve recipe payload")
            })?;
        if &supplied != curve.recipe() {
            return Err(convex_ports::TraitError::Conflict(
                "curve differs from captured original recipe",
            ));
        }
        let model = crate::curve_replay::RecipeRateCurve::new(curve).map_err(|_| {
            convex_ports::TraitError::InvalidInput(
                "unsupported snapshot rate pricing representation",
            )
        })?;
        let bytes = serde_json::to_vec(record)
            .map_err(|_| convex_ports::TraitError::InvalidInput("curve serialization failed"))?;
        Ok(Self {
            curve_id: record.curve_id.clone().into(),
            reference_date: supplied.reference_date,
            points: vec![],
            built_at: record.recorded_at.datetime().timestamp_millis(),
            inputs_hash: convex_ports::snapshot::content_digest(
                b"convex.supplied-curve-recipe.v1\0",
                &bytes,
            ),
            extrapolation: supplied.extrapolation,
            inner: Some(Arc::new(BuiltCurveModel::Recipe(model))),
        })
    }

    /// Reconstruct the exact supplied reference date, continuous-zero ACT/365F values and policies.
    /// This path validates before construction, reads no clock and never falls back after failure.
    pub fn from_snapshot(
        record: &convex_ports::snapshot::SnapshotCurve,
    ) -> Result<Self, convex_ports::TraitError> {
        use convex_ports::snapshot::{ZeroExtrapolation, ZeroInterpolation};
        record.validate()?;
        let interpolation = match record.interpolation {
            ZeroInterpolation::Linear => InterpolationMethod::Linear,
            ZeroInterpolation::MonotoneConvex => InterpolationMethod::MonotoneConvex,
        };
        let extrapolation = match record.extrapolation {
            ZeroExtrapolation::FlatZero => ExtrapolationMethod::Flat,
            ZeroExtrapolation::FlatForward => {
                return Err(convex_ports::TraitError::InvalidInput(
                    "flat-forward snapshot reconstruction requires a complete curve-method contract",
                ));
            }
        };
        let points: Vec<_> = record
            .points
            .iter()
            .map(|p| (p.tenor_years.get(), p.continuous_zero.get()))
            .collect();
        let inner = DiscreteCurve::with_extrapolation(
            record.reference_date.date(),
            points.iter().map(|p| p.0).collect(),
            points.iter().map(|p| p.1).collect(),
            ValueType::continuous_zero(DayCountConvention::Act365Fixed),
            interpolation,
            extrapolation,
        )
        .map_err(|_| {
            convex_ports::TraitError::InvalidInput("supplied curve construction failed")
        })?;
        let bytes = serde_json::to_vec(record)
            .map_err(|_| convex_ports::TraitError::InvalidInput("curve serialization failed"))?;
        Ok(Self {
            curve_id: record.curve_id.clone().into(),
            reference_date: record.reference_date.date(),
            points,
            built_at: record.recorded_at.datetime().timestamp_millis(),
            inputs_hash: convex_ports::snapshot::content_digest(
                b"convex.supplied-curve.v1\0",
                &bytes,
            ),
            extrapolation,
            inner: Some(Arc::new(BuiltCurveModel::Discrete(inner))),
        })
    }
}

/// Implement RateCurveDyn for spread calculations
impl RateCurveDyn for BuiltCurve {
    fn discount_factor(&self, t: f64) -> CurveResult<f64> {
        let inner = self
            .inner
            .as_deref()
            .ok_or_else(|| CurveError::invalid_value("missing validated curve representation"))?;
        if let BuiltCurveModel::Recipe(curve) = inner {
            return curve.discount_factor(t);
        }

        if t < 0.0 {
            return Err(CurveError::tenor_out_of_range(t, 0.0, self.max_tenor()));
        }
        let rate = self.interpolate_rate(t);
        if !rate.is_finite() {
            return Err(CurveError::invalid_value("curve query unavailable"));
        }
        // Continuous compounding: DF = exp(-r * t)
        Ok((-rate * t).exp())
    }

    fn zero_rate(&self, t: f64, compounding: Compounding) -> CurveResult<f64> {
        let inner = self
            .inner
            .as_deref()
            .ok_or_else(|| CurveError::invalid_value("missing validated curve representation"))?;
        if let BuiltCurveModel::Recipe(curve) = inner {
            return curve.zero_rate(t, compounding);
        }

        if t < 0.0 {
            return Err(CurveError::tenor_out_of_range(t, 0.0, self.max_tenor()));
        }
        if t == 0.0 {
            // Return overnight rate
            return Ok(self.interpolate_rate(1.0 / 365.0));
        }

        let cc_rate = self.interpolate_rate(t);
        if !cc_rate.is_finite() {
            return Err(CurveError::invalid_value("curve query unavailable"));
        }

        // Convert from continuous to requested compounding
        match compounding {
            Compounding::Continuous => Ok(cc_rate),
            Compounding::Simple => {
                // r_simple = (exp(r_cc * t) - 1) / t
                Ok((cc_rate * t).exp_m1() / t)
            }
            Compounding::Annual => {
                // r_annual = exp(r_cc) - 1
                Ok(cc_rate.exp() - 1.0)
            }
            Compounding::SemiAnnual => {
                // r_semi = 2 * (exp(r_cc / 2) - 1)
                Ok(2.0 * ((cc_rate / 2.0).exp() - 1.0))
            }
            Compounding::Quarterly => {
                // r_quarterly = 4 * (exp(r_cc / 4) - 1)
                Ok(4.0 * ((cc_rate / 4.0).exp() - 1.0))
            }
            Compounding::Monthly => {
                // r_monthly = 12 * (exp(r_cc / 12) - 1)
                Ok(12.0 * ((cc_rate / 12.0).exp() - 1.0))
            }
            Compounding::Daily => {
                // r_daily = 365 * (exp(r_cc / 365) - 1)
                Ok(365.0 * ((cc_rate / 365.0).exp() - 1.0))
            }
        }
    }

    fn forward_rate(&self, t1: f64, t2: f64) -> CurveResult<f64> {
        let inner = self
            .inner
            .as_deref()
            .ok_or_else(|| CurveError::invalid_value("missing validated curve representation"))?;
        if let BuiltCurveModel::Recipe(curve) = inner {
            return curve.forward_rate(t1, t2);
        }

        if t1 < 0.0 || t2 < 0.0 {
            let bad_t = t1.min(t2);
            return Err(CurveError::tenor_out_of_range(bad_t, 0.0, self.max_tenor()));
        }
        if t2 <= t1 {
            return Err(CurveError::invalid_value(format!(
                "t2 ({}) must be greater than t1 ({})",
                t2, t1
            )));
        }

        // Forward rate from t1 to t2: F(t1,t2) = (r2*t2 - r1*t1) / (t2 - t1)
        let r1 = self.interpolate_rate(t1);
        let r2 = self.interpolate_rate(t2);
        if !r1.is_finite() || !r2.is_finite() {
            return Err(CurveError::invalid_value("curve query unavailable"));
        }
        Ok((r2 * t2 - r1 * t1) / (t2 - t1))
    }

    fn instantaneous_forward(&self, t: f64) -> CurveResult<f64> {
        let inner = self
            .inner
            .as_deref()
            .ok_or_else(|| CurveError::invalid_value("missing validated curve representation"))?;
        if let BuiltCurveModel::Recipe(curve) = inner {
            return curve.instantaneous_forward(t);
        }

        // Approximate with a small delta
        let dt = 0.001;
        self.forward_rate(t, t + dt)
    }

    fn reference_date(&self) -> Date {
        if let Some(BuiltCurveModel::Recipe(curve)) = self.inner.as_deref() {
            return curve.reference_date();
        }
        self.reference_date
    }
    fn date_to_tenor(&self, date: Date) -> f64 {
        match self.inner.as_deref() {
            Some(BuiltCurveModel::Recipe(curve)) => curve.date_to_tenor(date),
            _ => self.reference_date.days_between(&date) as f64 / 365.0,
        }
    }

    fn max_date(&self) -> Date {
        if let Some(BuiltCurveModel::Recipe(curve)) = self.inner.as_deref() {
            return curve.max_date();
        }
        let max_years = self.max_tenor();
        let days = (max_years * 365.0) as i64;
        self.reference_date.add_days(days)
    }
}

/// Curve builder - manages curve construction from market data.
pub struct CurveBuilder {
    /// Market data source
    market_data: Arc<MarketDataProvider>,

    /// Calculation graph (to invalidate dependents)
    calc_graph: Arc<CalculationGraph>,

    /// Built curves cache
    curves: DashMap<CurveId, BuiltCurve>,

    /// Explicit raw-zero tail policy. The default holds the boundary zero.
    /// Exact flat-forward sources use the pure complete curve-method contract;
    /// this older monotone-convex builder cannot supply that tail.
    extrapolation: ExtrapolationMethod,
}

impl CurveBuilder {
    /// Create a new curve builder.
    pub fn new(market_data: Arc<MarketDataProvider>, calc_graph: Arc<CalculationGraph>) -> Self {
        Self {
            market_data,
            calc_graph,
            curves: DashMap::new(),
            extrapolation: ExtrapolationMethod::Flat,
        }
    }

    /// Sets the raw-zero extrapolation policy. Unsupported flat-forward policy
    /// fails at construction before reading market data or publishing a curve.
    #[must_use]
    pub fn with_extrapolation(mut self, extrapolation: ExtrapolationMethod) -> Self {
        self.extrapolation = extrapolation;
        self
    }

    /// Build a curve from current market data.
    pub async fn build(
        &self,
        curve_id: &CurveId,
        ref_date: Date,
    ) -> Result<BuiltCurve, EngineError> {
        self.validate_extrapolation()?;
        info!("Building curve: {}", curve_id);

        // Fetch market data inputs
        let inputs = self
            .market_data
            .curve_inputs
            .get_curve_inputs(curve_id)
            .await
            .map_err(|e| EngineError::MarketDataError(e.to_string()))?;

        if inputs.is_empty() {
            return Err(EngineError::CurveBuildError(format!(
                "No inputs found for curve {}",
                curve_id
            )));
        }

        // Convert to (tenor_years, rate) pairs
        let mut points: Vec<(f64, f64)> = inputs
            .iter()
            .map(|input| {
                let tenor_years = input.tenor.to_days() as f64 / 365.0;
                let rate: f64 = input.rate.try_into().map_err(|_| {
                    EngineError::CurveBuildError("unrepresentable input rate".into())
                })?;
                Ok((tenor_years, rate))
            })
            .collect::<Result<_, EngineError>>()?;

        validate_points(&points)?;
        // Sort by tenor
        points.sort_by(|(t1, _), (t2, _)| t1.total_cmp(t2));
        if points.windows(2).any(|p| p[0].0 == p[1].0) {
            return Err(EngineError::CurveBuildError(
                "duplicate curve pillar".into(),
            ));
        }

        // Create hash of inputs for change detection
        let exact_inputs: Vec<_> = inputs
            .iter()
            .map(|i| {
                (
                    &i.instrument_type,
                    &i.tenor,
                    i.rate.to_string(),
                    i.timestamp,
                    &i.source,
                )
            })
            .collect();
        let bytes = serde_json::to_vec(&(
            curve_id,
            ref_date,
            exact_inputs,
            format!("{:?}", self.extrapolation),
        ))
        .map_err(|_| EngineError::CurveBuildError("curve identity serialization failed".into()))?;
        let inputs_hash =
            convex_ports::snapshot::content_digest(b"convex.curve-inputs.v1\0", &bytes);

        let mut built = BuiltCurve {
            curve_id: curve_id.clone(),
            reference_date: ref_date,
            points,
            built_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            inputs_hash,
            extrapolation: self.extrapolation,
            inner: None,
        };
        built
            .rebuild_inner()
            .map_err(|e| EngineError::CurveBuildError(e.to_string()))?;

        // Update cache
        self.curves.insert(curve_id.clone(), built.clone());

        // Invalidate calc graph
        let node = NodeId::Curve {
            curve_id: curve_id.clone(),
        };
        if self.calc_graph.node_spec(&node).is_some() {
            self.calc_graph
                .invalidate(&node)
                .map_err(|error| EngineError::CurveBuildError(error.to_string()))?;
        }

        info!(
            "Curve {} built with {} points",
            curve_id,
            built.points.len()
        );
        Ok(built)
    }

    /// Create a curve from explicit points (tenor_years, zero_rate as decimal).
    ///
    /// This method allows direct curve creation without fetching from market data providers.
    /// Useful for API-driven curve creation.
    pub fn create_from_points(
        &self,
        curve_id: CurveId,
        reference_date: Date,
        mut points: Vec<(f64, f64)>,
    ) -> Result<BuiltCurve, EngineError> {
        self.validate_extrapolation()?;
        if points.is_empty() {
            return Err(EngineError::CurveBuildError(
                "Cannot create curve with no points".to_string(),
            ));
        }

        validate_points(&points)?;
        // Sort by tenor
        points.sort_by(|(t1, _), (t2, _)| t1.total_cmp(t2));
        if points.windows(2).any(|p| p[0].0 == p[1].0) {
            return Err(EngineError::CurveBuildError(
                "duplicate curve pillar".into(),
            ));
        }

        // Create hash of inputs for change detection
        let bytes = serde_json::to_vec(&(
            &curve_id,
            reference_date,
            &points,
            format!("{:?}", self.extrapolation),
        ))
        .map_err(|_| EngineError::CurveBuildError("curve identity serialization failed".into()))?;
        let inputs_hash =
            convex_ports::snapshot::content_digest(b"convex.curve-points.v1\0", &bytes);

        let mut built = BuiltCurve {
            curve_id: curve_id.clone(),
            reference_date,
            points,
            built_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            inputs_hash,
            extrapolation: self.extrapolation,
            inner: None,
        };
        built
            .rebuild_inner()
            .map_err(|e| EngineError::CurveBuildError(e.to_string()))?;

        // Update cache
        self.curves.insert(curve_id.clone(), built.clone());

        // Invalidate calc graph
        let node = NodeId::Curve {
            curve_id: curve_id.clone(),
        };
        if self.calc_graph.node_spec(&node).is_some() {
            self.calc_graph
                .invalidate(&node)
                .map_err(|error| EngineError::CurveBuildError(error.to_string()))?;
        }

        info!(
            "Curve {} created with {} points",
            curve_id,
            built.points.len()
        );
        Ok(built)
    }

    fn validate_extrapolation(&self) -> Result<(), EngineError> {
        if self.extrapolation == ExtrapolationMethod::FlatForward {
            return Err(EngineError::CurveBuildError(
                "flat-forward construction requires a complete curve-method contract".into(),
            ));
        }
        Ok(())
    }

    /// Delete a curve from the cache.
    pub fn delete(&self, curve_id: &CurveId) -> bool {
        self.curves.remove(curve_id).is_some()
    }

    /// Get a cached curve.
    pub fn get(&self, curve_id: &CurveId) -> Option<BuiltCurve> {
        self.curves.get(curve_id).map(|c| c.clone())
    }

    /// List all cached curves.
    pub fn list(&self) -> Vec<CurveId> {
        self.curves.iter().map(|r| r.key().clone()).collect()
    }

    /// Clear the curve cache.
    pub fn clear(&self) {
        self.curves.clear();
    }
}

fn validate_points(points: &[(f64, f64)]) -> Result<(), EngineError> {
    if points.is_empty()
        || points.len() > convex_ports::snapshot::MAX_SNAPSHOT_PILLARS
        || points
            .iter()
            .any(|(t, r)| !t.is_finite() || *t <= 0.0 || !r.is_finite())
    {
        return Err(EngineError::CurveBuildError(
            "invalid/bounded curve pillars required".into(),
        ));
    }
    Ok(())
}
