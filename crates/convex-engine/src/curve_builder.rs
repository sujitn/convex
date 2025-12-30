//! Curve builder - constructs curves from market data.

use std::sync::Arc;

use dashmap::DashMap;
use tracing::info;

use convex_core::Date;
use convex_curves::{Compounding, CurveError, CurveResult, RateCurveDyn};
use convex_traits::ids::CurveId;
use convex_traits::market_data::MarketDataProvider;

use crate::calc_graph::{CalculationGraph, NodeId};
use crate::error::EngineError;

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
}

impl BuiltCurve {
    /// Get zero rate for a tenor (in years) using linear interpolation.
    pub fn interpolate_rate(&self, tenor_years: f64) -> f64 {
        if self.points.is_empty() {
            return 0.0;
        }

        // Handle edge cases
        if tenor_years <= self.points[0].0 {
            return self.points[0].1;
        }
        if tenor_years >= self.points.last().unwrap().0 {
            return self.points.last().unwrap().1;
        }

        // Find surrounding points and interpolate
        for i in 1..self.points.len() {
            if self.points[i].0 >= tenor_years {
                let (t0, r0) = self.points[i - 1];
                let (t1, r1) = self.points[i];
                let weight = (tenor_years - t0) / (t1 - t0);
                return r0 + weight * (r1 - r0);
            }
        }

        self.points.last().unwrap().1
    }

    /// Get max tenor in years.
    pub fn max_tenor(&self) -> f64 {
        self.points.last().map(|(t, _)| *t).unwrap_or(30.0)
    }

    /// Convert to points format for NodeValue::Curve.
    ///
    /// Returns (tenor_days, zero_rate) pairs.
    pub fn to_points(&self) -> Vec<(u32, f64)> {
        self.points
            .iter()
            .map(|(tenor_years, rate)| {
                let tenor_days = (*tenor_years * 365.0) as u32;
                (tenor_days, *rate)
            })
            .collect()
    }

    /// Create a BuiltCurve from cached points.
    ///
    /// Used to reconstruct a curve from NodeValue::Curve data.
    pub fn from_points(curve_id: &str, points: Vec<(u32, f64)>) -> Self {
        let curve_points: Vec<(f64, f64)> = points
            .into_iter()
            .map(|(tenor_days, rate)| {
                let tenor_years = tenor_days as f64 / 365.0;
                (tenor_years, rate)
            })
            .collect();

        Self {
            curve_id: CurveId::new(curve_id),
            reference_date: Date::today(),
            points: curve_points,
            built_at: chrono::Utc::now().timestamp(),
            inputs_hash: String::new(),
        }
    }
}

/// Implement RateCurveDyn for spread calculations
impl RateCurveDyn for BuiltCurve {
    fn discount_factor(&self, t: f64) -> CurveResult<f64> {
        if t < 0.0 {
            return Err(CurveError::tenor_out_of_range(t, 0.0, self.max_tenor()));
        }
        let rate = self.interpolate_rate(t);
        // Continuous compounding: DF = exp(-r * t)
        Ok((-rate * t).exp())
    }

    fn zero_rate(&self, t: f64, compounding: Compounding) -> CurveResult<f64> {
        if t < 0.0 {
            return Err(CurveError::tenor_out_of_range(t, 0.0, self.max_tenor()));
        }
        if t == 0.0 {
            // Return overnight rate
            return Ok(self.interpolate_rate(1.0 / 365.0));
        }

        let cc_rate = self.interpolate_rate(t);

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
        Ok((r2 * t2 - r1 * t1) / (t2 - t1))
    }

    fn instantaneous_forward(&self, t: f64) -> CurveResult<f64> {
        // Approximate with a small delta
        let dt = 0.001;
        self.forward_rate(t, t + dt)
    }

    fn reference_date(&self) -> Date {
        self.reference_date
    }

    fn max_date(&self) -> Date {
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
}

impl CurveBuilder {
    /// Create a new curve builder.
    pub fn new(market_data: Arc<MarketDataProvider>, calc_graph: Arc<CalculationGraph>) -> Self {
        Self {
            market_data,
            calc_graph,
            curves: DashMap::new(),
        }
    }

    /// Build a curve from current market data.
    pub async fn build(&self, curve_id: &CurveId, ref_date: Date) -> Result<BuiltCurve, EngineError> {
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
                let rate: f64 = input.rate.try_into().unwrap_or(0.0);
                (tenor_years, rate)
            })
            .collect();

        // Sort by tenor
        points.sort_by(|(t1, _), (t2, _)| t1.partial_cmp(t2).unwrap());

        // Create hash of inputs for change detection
        let inputs_hash = format!(
            "{:x}",
            inputs
                .iter()
                .map(|i| format!("{}-{}", i.tenor, i.rate))
                .collect::<Vec<_>>()
                .join(",")
                .len()
        );

        let built = BuiltCurve {
            curve_id: curve_id.clone(),
            reference_date: ref_date,
            points,
            built_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            inputs_hash,
        };

        // Update cache
        self.curves.insert(curve_id.clone(), built.clone());

        // Invalidate calc graph
        self.calc_graph.invalidate(&NodeId::Curve {
            curve_id: curve_id.clone(),
        });

        info!("Curve {} built with {} points", curve_id, built.points.len());
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
        if points.is_empty() {
            return Err(EngineError::CurveBuildError(
                "Cannot create curve with no points".to_string(),
            ));
        }

        // Sort by tenor
        points.sort_by(|(t1, _), (t2, _)| t1.partial_cmp(t2).unwrap());

        // Create hash of inputs for change detection
        let inputs_hash = format!(
            "{:x}",
            points
                .iter()
                .map(|(t, r)| format!("{:.6}-{:.6}", t, r))
                .collect::<Vec<_>>()
                .join(",")
                .len()
        );

        let built = BuiltCurve {
            curve_id: curve_id.clone(),
            reference_date,
            points,
            built_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            inputs_hash,
        };

        // Update cache
        self.curves.insert(curve_id.clone(), built.clone());

        // Invalidate calc graph
        self.calc_graph.invalidate(&NodeId::Curve {
            curve_id: curve_id.clone(),
        });

        info!(
            "Curve {} created with {} points",
            curve_id,
            built.points.len()
        );
        Ok(built)
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
