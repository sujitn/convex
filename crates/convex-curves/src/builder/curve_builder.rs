//! Curve builder for fluent construction of term structures.
//!
//! The `CurveBuilder` provides a fluent API for constructing curves
//! with multiple segments, different interpolation methods, and
//! various data sources.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use convex_core::types::{Compounding, Date};

use crate::curves::{
    CurveSegment, DelegatedCurve, DelegationFallback, DerivedCurve, DiscreteCurve, SegmentSource,
    SegmentedCurve,
};
use crate::error::{CurveError, CurveResult};
use crate::term_structure::{CurveRef, TermStructure};
use crate::value_type::ValueType;
use crate::{ExtrapolationMethod, InterpolationMethod};

/// Type of curve being built.
#[derive(Debug, Clone)]
pub enum CurveFamily {
    /// Interest rate curve.
    Rate,
    /// Credit/survival curve.
    Credit {
        /// Recovery rate assumption.
        recovery: f64,
    },
    /// Inflation curve.
    Inflation {
        /// Base index value.
        base_index: f64,
    },
}

impl Default for CurveFamily {
    fn default() -> Self {
        CurveFamily::Rate
    }
}

/// Builder for constructing term structures.
///
/// Provides a fluent API for building curves with segments,
/// interpolation, and various data sources.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::builder::CurveBuilder;
/// use convex_core::types::{Date, Compounding};
///
/// let today = Date::from_ymd(2024, 1, 1).unwrap();
///
/// // Simple rate curve from discrete data
/// let curve = CurveBuilder::rate_curve(today)
///     .with_zeros(
///         vec![1.0, 2.0, 5.0, 10.0],
///         vec![0.04, 0.045, 0.05, 0.055],
///         Compounding::Continuous,
///     )
///     .interpolate(InterpolationMethod::MonotoneConvex)
///     .build()?;
///
/// // Multi-segment curve
/// let curve = CurveBuilder::rate_curve(today)
///     .segment(0.0..2.0)
///         .with_zeros(short_tenors, short_rates, Compounding::Continuous)
///         .interpolate(InterpolationMethod::Linear)
///     .segment(2.0..10.0)
///         .delegate(swap_curve)
///     .segment(10.0..)
///         .delegate(long_curve)
///         .extrapolate(ExtrapolationMethod::FlatForward)
///     .build()?;
/// ```
#[derive(Debug)]
pub struct CurveBuilder {
    /// Reference date for the curve.
    reference_date: Date,
    /// Type of curve being built.
    curve_family: CurveFamily,
    /// Segments being built.
    segments: Vec<SegmentConfig>,
    /// Default interpolation method.
    default_interpolation: InterpolationMethod,
    /// Default extrapolation method.
    default_extrapolation: ExtrapolationMethod,
}

/// Configuration for a single segment.
#[derive(Debug, Clone)]
struct SegmentConfig {
    /// Start tenor (inclusive).
    start: f64,
    /// End tenor (exclusive), None for unbounded.
    end: Option<f64>,
    /// Data source for this segment.
    source: SegmentDataSource,
    /// Interpolation method for this segment.
    interpolation: InterpolationMethod,
    /// Extrapolation method for this segment.
    extrapolation: ExtrapolationMethod,
}

/// Data source for a curve segment.
#[derive(Clone)]
enum SegmentDataSource {
    /// Discrete zero rates.
    DiscreteZeros {
        tenors: Vec<f64>,
        rates: Vec<f64>,
        compounding: Compounding,
    },
    /// Discrete discount factors.
    DiscreteDiscountFactors { tenors: Vec<f64>, dfs: Vec<f64> },
    /// Discrete forward rates.
    DiscreteForwards {
        tenors: Vec<f64>,
        forwards: Vec<f64>,
        tenor_length: f64,
    },
    /// Discrete survival probabilities (for credit curves).
    DiscreteSurvival {
        tenors: Vec<f64>,
        survival_probs: Vec<f64>,
    },
    /// Discrete hazard rates (for credit curves).
    DiscreteHazard {
        tenors: Vec<f64>,
        hazard_rates: Vec<f64>,
    },
    /// Delegate to another curve.
    Delegated {
        curve: CurveRef,
        fallback: DelegationFallback,
    },
    /// Spread over a base curve.
    SpreadOver { base: CurveRef, spread_bps: f64 },
    /// Parallel shift of a base curve.
    Shifted { base: CurveRef, shift_bps: f64 },
    /// Not yet configured.
    Empty,
}

impl fmt::Debug for SegmentDataSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SegmentDataSource::DiscreteZeros { tenors, .. } => f
                .debug_struct("DiscreteZeros")
                .field("n_points", &tenors.len())
                .finish(),
            SegmentDataSource::DiscreteDiscountFactors { tenors, .. } => f
                .debug_struct("DiscreteDiscountFactors")
                .field("n_points", &tenors.len())
                .finish(),
            SegmentDataSource::DiscreteForwards { tenors, .. } => f
                .debug_struct("DiscreteForwards")
                .field("n_points", &tenors.len())
                .finish(),
            SegmentDataSource::DiscreteSurvival { tenors, .. } => f
                .debug_struct("DiscreteSurvival")
                .field("n_points", &tenors.len())
                .finish(),
            SegmentDataSource::DiscreteHazard { tenors, .. } => f
                .debug_struct("DiscreteHazard")
                .field("n_points", &tenors.len())
                .finish(),
            SegmentDataSource::Delegated { fallback, .. } => f
                .debug_struct("Delegated")
                .field("fallback", fallback)
                .finish(),
            SegmentDataSource::SpreadOver { spread_bps, .. } => f
                .debug_struct("SpreadOver")
                .field("spread_bps", spread_bps)
                .finish(),
            SegmentDataSource::Shifted { shift_bps, .. } => f
                .debug_struct("Shifted")
                .field("shift_bps", shift_bps)
                .finish(),
            SegmentDataSource::Empty => write!(f, "Empty"),
        }
    }
}

impl Default for SegmentDataSource {
    fn default() -> Self {
        SegmentDataSource::Empty
    }
}

impl CurveBuilder {
    /// Creates a new curve builder for a rate curve.
    #[must_use]
    pub fn rate_curve(reference_date: Date) -> Self {
        Self {
            reference_date,
            curve_family: CurveFamily::Rate,
            segments: Vec::new(),
            default_interpolation: InterpolationMethod::MonotoneConvex,
            default_extrapolation: ExtrapolationMethod::Flat,
        }
    }

    /// Creates a new curve builder for a credit curve.
    #[must_use]
    pub fn credit_curve(reference_date: Date, recovery: f64) -> Self {
        Self {
            reference_date,
            curve_family: CurveFamily::Credit { recovery },
            segments: Vec::new(),
            default_interpolation: InterpolationMethod::PiecewiseConstant,
            default_extrapolation: ExtrapolationMethod::Flat,
        }
    }

    /// Creates a new curve builder for an inflation curve.
    #[must_use]
    pub fn inflation_curve(reference_date: Date, base_index: f64) -> Self {
        Self {
            reference_date,
            curve_family: CurveFamily::Inflation { base_index },
            segments: Vec::new(),
            default_interpolation: InterpolationMethod::Linear,
            default_extrapolation: ExtrapolationMethod::Linear,
        }
    }

    /// Sets the default interpolation method for all segments.
    #[must_use]
    pub fn default_interpolation(mut self, method: InterpolationMethod) -> Self {
        self.default_interpolation = method;
        self
    }

    /// Sets the default extrapolation method for all segments.
    #[must_use]
    pub fn default_extrapolation(mut self, method: ExtrapolationMethod) -> Self {
        self.default_extrapolation = method;
        self
    }

    // ========================================================================
    // Simple (single-segment) curve construction
    // ========================================================================

    /// Adds discrete zero rates as the curve data.
    ///
    /// This creates a single-segment curve from the provided data.
    #[must_use]
    pub fn with_zeros(
        mut self,
        tenors: Vec<f64>,
        rates: Vec<f64>,
        compounding: Compounding,
    ) -> Self {
        let start = tenors.first().copied().unwrap_or(0.0);
        let end = tenors.last().copied();

        self.segments.push(SegmentConfig {
            start,
            end,
            source: SegmentDataSource::DiscreteZeros {
                tenors,
                rates,
                compounding,
            },
            interpolation: self.default_interpolation,
            extrapolation: self.default_extrapolation,
        });
        self
    }

    /// Adds discrete discount factors as the curve data.
    #[must_use]
    pub fn with_discount_factors(mut self, tenors: Vec<f64>, dfs: Vec<f64>) -> Self {
        let start = tenors.first().copied().unwrap_or(0.0);
        let end = tenors.last().copied();

        self.segments.push(SegmentConfig {
            start,
            end,
            source: SegmentDataSource::DiscreteDiscountFactors { tenors, dfs },
            interpolation: self.default_interpolation,
            extrapolation: self.default_extrapolation,
        });
        self
    }

    /// Adds discrete forward rates as the curve data.
    #[must_use]
    pub fn with_forwards(
        mut self,
        tenors: Vec<f64>,
        forwards: Vec<f64>,
        tenor_length: f64,
    ) -> Self {
        let start = tenors.first().copied().unwrap_or(0.0);
        let end = tenors.last().copied();

        self.segments.push(SegmentConfig {
            start,
            end,
            source: SegmentDataSource::DiscreteForwards {
                tenors,
                forwards,
                tenor_length,
            },
            interpolation: self.default_interpolation,
            extrapolation: self.default_extrapolation,
        });
        self
    }

    /// Adds discrete survival probabilities (for credit curves).
    #[must_use]
    pub fn with_survival_probabilities(
        mut self,
        tenors: Vec<f64>,
        survival_probs: Vec<f64>,
    ) -> Self {
        let start = tenors.first().copied().unwrap_or(0.0);
        let end = tenors.last().copied();

        self.segments.push(SegmentConfig {
            start,
            end,
            source: SegmentDataSource::DiscreteSurvival {
                tenors,
                survival_probs,
            },
            interpolation: self.default_interpolation,
            extrapolation: self.default_extrapolation,
        });
        self
    }

    /// Adds discrete hazard rates (for credit curves).
    #[must_use]
    pub fn with_hazard_rates(mut self, tenors: Vec<f64>, hazard_rates: Vec<f64>) -> Self {
        let start = tenors.first().copied().unwrap_or(0.0);
        let end = tenors.last().copied();

        self.segments.push(SegmentConfig {
            start,
            end,
            source: SegmentDataSource::DiscreteHazard {
                tenors,
                hazard_rates,
            },
            interpolation: self.default_interpolation,
            extrapolation: self.default_extrapolation,
        });
        self
    }

    /// Delegates to another curve.
    #[must_use]
    pub fn delegate(mut self, curve: CurveRef) -> Self {
        let (min, max) = curve.tenor_bounds();

        self.segments.push(SegmentConfig {
            start: min,
            end: Some(max),
            source: SegmentDataSource::Delegated {
                curve,
                fallback: DelegationFallback::Trust,
            },
            interpolation: self.default_interpolation,
            extrapolation: self.default_extrapolation,
        });
        self
    }

    /// Creates a spread curve over a base curve.
    #[must_use]
    pub fn spread_over(mut self, base: CurveRef, spread_bps: f64) -> Self {
        let (min, max) = base.tenor_bounds();

        self.segments.push(SegmentConfig {
            start: min,
            end: Some(max),
            source: SegmentDataSource::SpreadOver { base, spread_bps },
            interpolation: self.default_interpolation,
            extrapolation: self.default_extrapolation,
        });
        self
    }

    /// Creates a shifted curve from a base curve.
    #[must_use]
    pub fn shift(mut self, base: CurveRef, shift_bps: f64) -> Self {
        let (min, max) = base.tenor_bounds();

        self.segments.push(SegmentConfig {
            start: min,
            end: Some(max),
            source: SegmentDataSource::Shifted { base, shift_bps },
            interpolation: self.default_interpolation,
            extrapolation: self.default_extrapolation,
        });
        self
    }

    /// Sets the interpolation method for the current segment.
    #[must_use]
    pub fn interpolate(mut self, method: InterpolationMethod) -> Self {
        if let Some(segment) = self.segments.last_mut() {
            segment.interpolation = method;
        }
        self
    }

    /// Sets the extrapolation method for the current segment.
    #[must_use]
    pub fn extrapolate(mut self, method: ExtrapolationMethod) -> Self {
        if let Some(segment) = self.segments.last_mut() {
            segment.extrapolation = method;
        }
        self
    }

    // ========================================================================
    // Multi-segment curve construction
    // ========================================================================

    /// Starts a new segment for a bounded range.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// builder.segment(0.0..2.0)  // 0 to 2 years
    /// ```
    #[must_use]
    pub fn segment(self, range: Range<f64>) -> SegmentBuilder {
        SegmentBuilder::new(self, range.start, Some(range.end))
    }

    /// Starts a new segment from a starting point to infinity.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// builder.segment_from(10.0)  // 10 years onwards
    /// ```
    #[must_use]
    pub fn segment_from(self, start: f64) -> SegmentBuilder {
        SegmentBuilder::new(self, start, None)
    }

    /// Starts a new segment from 0 to an ending point.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// builder.segment_to(2.0)  // 0 to 2 years
    /// ```
    #[must_use]
    pub fn segment_to(self, end: f64) -> SegmentBuilder {
        SegmentBuilder::new(self, 0.0, Some(end))
    }

    // ========================================================================
    // Build methods
    // ========================================================================

    /// Builds a simple discrete curve (single segment).
    pub fn build_discrete(self) -> CurveResult<DiscreteCurve> {
        if self.segments.is_empty() {
            return Err(CurveError::builder_error("No data provided for curve"));
        }

        if self.segments.len() > 1 {
            return Err(CurveError::builder_error(
                "Use build() for multi-segment curves",
            ));
        }

        let segment = &self.segments[0];
        self.build_discrete_from_config(segment)
    }

    /// Builds a segmented curve (single or multiple segments).
    pub fn build(self) -> CurveResult<SegmentedCurve> {
        if self.segments.is_empty() {
            return Err(CurveError::builder_error("No segments defined"));
        }

        // Sort segments by start tenor
        let mut configs = self.segments.clone();
        configs.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());

        // Build each segment
        let mut curve_segments = Vec::new();
        for config in &configs {
            let curve = self.build_curve_from_config(config)?;
            let source = self.build_segment_source(config);

            curve_segments.push(CurveSegment::new(config.start, config.end, source, curve));
        }

        // Determine value type
        let value_type = self.determine_value_type(&configs[0]);

        SegmentedCurve::new(self.reference_date, curve_segments, value_type)
    }

    /// Builds and wraps in a RateCurve.
    pub fn build_rate_curve(self) -> CurveResult<crate::wrappers::RateCurve<SegmentedCurve>> {
        let curve = self.build()?;
        Ok(crate::wrappers::RateCurve::new(curve))
    }

    /// Builds and wraps in a CreditCurve.
    pub fn build_credit_curve(self) -> CurveResult<crate::wrappers::CreditCurve<SegmentedCurve>> {
        let recovery = match &self.curve_family {
            CurveFamily::Credit { recovery } => *recovery,
            _ => 0.40, // Default recovery
        };
        let curve = self.build()?;
        Ok(crate::wrappers::CreditCurve::new(curve, recovery))
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    fn build_discrete_from_config(&self, config: &SegmentConfig) -> CurveResult<DiscreteCurve> {
        match &config.source {
            SegmentDataSource::DiscreteZeros {
                tenors,
                rates,
                compounding,
            } => DiscreteCurve::new(
                self.reference_date,
                tenors.clone(),
                rates.clone(),
                ValueType::zero_rate(*compounding),
                config.interpolation,
            ),
            SegmentDataSource::DiscreteDiscountFactors { tenors, dfs } => DiscreteCurve::new(
                self.reference_date,
                tenors.clone(),
                dfs.clone(),
                ValueType::DiscountFactor,
                config.interpolation,
            ),
            SegmentDataSource::DiscreteForwards {
                tenors,
                forwards,
                tenor_length,
            } => DiscreteCurve::new(
                self.reference_date,
                tenors.clone(),
                forwards.clone(),
                ValueType::forward_rate(*tenor_length),
                config.interpolation,
            ),
            SegmentDataSource::DiscreteSurvival {
                tenors,
                survival_probs,
            } => DiscreteCurve::new(
                self.reference_date,
                tenors.clone(),
                survival_probs.clone(),
                ValueType::SurvivalProbability,
                config.interpolation,
            ),
            SegmentDataSource::DiscreteHazard {
                tenors,
                hazard_rates,
            } => DiscreteCurve::new(
                self.reference_date,
                tenors.clone(),
                hazard_rates.clone(),
                ValueType::HazardRate,
                config.interpolation,
            ),
            _ => Err(CurveError::builder_error(
                "Cannot build discrete curve from this source type",
            )),
        }
    }

    fn build_curve_from_config(&self, config: &SegmentConfig) -> CurveResult<CurveRef> {
        match &config.source {
            SegmentDataSource::DiscreteZeros { .. }
            | SegmentDataSource::DiscreteDiscountFactors { .. }
            | SegmentDataSource::DiscreteForwards { .. }
            | SegmentDataSource::DiscreteSurvival { .. }
            | SegmentDataSource::DiscreteHazard { .. } => {
                let discrete = self.build_discrete_from_config(config)?;
                Ok(Arc::new(discrete))
            }
            SegmentDataSource::Delegated { curve, fallback } => {
                let delegated = DelegatedCurve::new(curve.clone(), *fallback);
                Ok(Arc::new(delegated))
            }
            SegmentDataSource::SpreadOver { base, spread_bps } => {
                let derived = DerivedCurve::with_spread(base.clone(), *spread_bps);
                Ok(Arc::new(derived))
            }
            SegmentDataSource::Shifted { base, shift_bps } => {
                let derived = DerivedCurve::with_shift(base.clone(), *shift_bps);
                Ok(Arc::new(derived))
            }
            SegmentDataSource::Empty => Err(CurveError::builder_error(
                "Segment has no data source configured",
            )),
        }
    }

    fn build_segment_source(&self, config: &SegmentConfig) -> SegmentSource {
        match &config.source {
            SegmentDataSource::DiscreteZeros { tenors, rates, .. } => SegmentSource::Discrete {
                tenors: tenors.clone(),
                values: rates.clone(),
            },
            SegmentDataSource::DiscreteDiscountFactors { tenors, dfs } => SegmentSource::Discrete {
                tenors: tenors.clone(),
                values: dfs.clone(),
            },
            SegmentDataSource::DiscreteForwards {
                tenors, forwards, ..
            } => SegmentSource::Discrete {
                tenors: tenors.clone(),
                values: forwards.clone(),
            },
            SegmentDataSource::DiscreteSurvival {
                tenors,
                survival_probs,
            } => SegmentSource::Discrete {
                tenors: tenors.clone(),
                values: survival_probs.clone(),
            },
            SegmentDataSource::DiscreteHazard {
                tenors,
                hazard_rates,
            } => SegmentSource::Discrete {
                tenors: tenors.clone(),
                values: hazard_rates.clone(),
            },
            SegmentDataSource::Delegated { curve, fallback } => SegmentSource::Delegated {
                curve: curve.clone(),
                fallback: *fallback,
            },
            SegmentDataSource::SpreadOver { base, spread_bps } => SegmentSource::Derived {
                base: base.clone(),
                transform: crate::curves::CurveTransform::spread(*spread_bps),
            },
            SegmentDataSource::Shifted { base, shift_bps } => SegmentSource::Derived {
                base: base.clone(),
                transform: crate::curves::CurveTransform::parallel_shift(*shift_bps),
            },
            SegmentDataSource::Empty => SegmentSource::Discrete {
                tenors: vec![],
                values: vec![],
            },
        }
    }

    fn determine_value_type(&self, config: &SegmentConfig) -> ValueType {
        match &config.source {
            SegmentDataSource::DiscreteZeros { compounding, .. } => {
                ValueType::zero_rate(*compounding)
            }
            SegmentDataSource::DiscreteDiscountFactors { .. } => ValueType::DiscountFactor,
            SegmentDataSource::DiscreteForwards { tenor_length, .. } => {
                ValueType::forward_rate(*tenor_length)
            }
            SegmentDataSource::DiscreteSurvival { .. } => ValueType::SurvivalProbability,
            SegmentDataSource::DiscreteHazard { .. } => ValueType::HazardRate,
            SegmentDataSource::Delegated { curve, .. } => curve.value_type(),
            SegmentDataSource::SpreadOver { base, .. } => base.value_type(),
            SegmentDataSource::Shifted { base, .. } => base.value_type(),
            SegmentDataSource::Empty => ValueType::DiscountFactor,
        }
    }

    /// Internal: adds a configured segment (used by SegmentBuilder).
    fn add_segment(&mut self, config: SegmentConfig) {
        self.segments.push(config);
    }
}

/// Builder for a single curve segment.
///
/// Created by calling `segment()` on a `CurveBuilder`.
#[derive(Debug)]
pub struct SegmentBuilder {
    /// Parent curve builder.
    parent: CurveBuilder,
    /// Start tenor.
    start: f64,
    /// End tenor (None for unbounded).
    end: Option<f64>,
    /// Data source.
    source: SegmentDataSource,
    /// Interpolation method.
    interpolation: InterpolationMethod,
    /// Extrapolation method.
    extrapolation: ExtrapolationMethod,
}

impl SegmentBuilder {
    fn new(parent: CurveBuilder, start: f64, end: Option<f64>) -> Self {
        Self {
            interpolation: parent.default_interpolation,
            extrapolation: parent.default_extrapolation,
            parent,
            start,
            end,
            source: SegmentDataSource::Empty,
        }
    }

    /// Adds discrete zero rates to this segment.
    #[must_use]
    pub fn with_zeros(
        mut self,
        tenors: Vec<f64>,
        rates: Vec<f64>,
        compounding: Compounding,
    ) -> Self {
        self.source = SegmentDataSource::DiscreteZeros {
            tenors,
            rates,
            compounding,
        };
        self
    }

    /// Adds discrete discount factors to this segment.
    #[must_use]
    pub fn with_discount_factors(mut self, tenors: Vec<f64>, dfs: Vec<f64>) -> Self {
        self.source = SegmentDataSource::DiscreteDiscountFactors { tenors, dfs };
        self
    }

    /// Adds discrete forward rates to this segment.
    #[must_use]
    pub fn with_forwards(
        mut self,
        tenors: Vec<f64>,
        forwards: Vec<f64>,
        tenor_length: f64,
    ) -> Self {
        self.source = SegmentDataSource::DiscreteForwards {
            tenors,
            forwards,
            tenor_length,
        };
        self
    }

    /// Adds discrete survival probabilities to this segment.
    #[must_use]
    pub fn with_survival_probabilities(
        mut self,
        tenors: Vec<f64>,
        survival_probs: Vec<f64>,
    ) -> Self {
        self.source = SegmentDataSource::DiscreteSurvival {
            tenors,
            survival_probs,
        };
        self
    }

    /// Adds discrete hazard rates to this segment.
    #[must_use]
    pub fn with_hazard_rates(mut self, tenors: Vec<f64>, hazard_rates: Vec<f64>) -> Self {
        self.source = SegmentDataSource::DiscreteHazard {
            tenors,
            hazard_rates,
        };
        self
    }

    /// Delegates to another curve for this segment.
    #[must_use]
    pub fn delegate(mut self, curve: CurveRef) -> Self {
        self.source = SegmentDataSource::Delegated {
            curve,
            fallback: DelegationFallback::Trust,
        };
        self
    }

    /// Delegates with strict bounds checking.
    #[must_use]
    pub fn delegate_strict(mut self, curve: CurveRef) -> Self {
        self.source = SegmentDataSource::Delegated {
            curve,
            fallback: DelegationFallback::Strict,
        };
        self
    }

    /// Delegates with clamping to bounds.
    #[must_use]
    pub fn delegate_clamped(mut self, curve: CurveRef) -> Self {
        self.source = SegmentDataSource::Delegated {
            curve,
            fallback: DelegationFallback::Clamp,
        };
        self
    }

    /// Creates a spread over a base curve.
    #[must_use]
    pub fn spread_over(mut self, base: CurveRef, spread_bps: f64) -> Self {
        self.source = SegmentDataSource::SpreadOver { base, spread_bps };
        self
    }

    /// Creates a shifted curve.
    #[must_use]
    pub fn shift(mut self, base: CurveRef, shift_bps: f64) -> Self {
        self.source = SegmentDataSource::Shifted { base, shift_bps };
        self
    }

    /// Sets the interpolation method for this segment.
    #[must_use]
    pub fn interpolate(mut self, method: InterpolationMethod) -> Self {
        self.interpolation = method;
        self
    }

    /// Sets the extrapolation method for this segment.
    #[must_use]
    pub fn extrapolate(mut self, method: ExtrapolationMethod) -> Self {
        self.extrapolation = method;
        self
    }

    /// Starts a new segment.
    #[must_use]
    pub fn segment(self, range: Range<f64>) -> SegmentBuilder {
        self.finish().segment(range)
    }

    /// Starts a new segment from a starting point.
    #[must_use]
    pub fn segment_from(self, start: f64) -> SegmentBuilder {
        self.finish().segment_from(start)
    }

    /// Builds the final curve.
    pub fn build(self) -> CurveResult<SegmentedCurve> {
        self.finish().build()
    }

    /// Builds and wraps in a RateCurve.
    pub fn build_rate_curve(self) -> CurveResult<crate::wrappers::RateCurve<SegmentedCurve>> {
        self.finish().build_rate_curve()
    }

    /// Builds and wraps in a CreditCurve.
    pub fn build_credit_curve(self) -> CurveResult<crate::wrappers::CreditCurve<SegmentedCurve>> {
        self.finish().build_credit_curve()
    }

    /// Finishes this segment and returns the parent builder.
    fn finish(mut self) -> CurveBuilder {
        self.parent.add_segment(SegmentConfig {
            start: self.start,
            end: self.end,
            source: self.source,
            interpolation: self.interpolation,
            extrapolation: self.extrapolation,
        });
        self.parent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn today() -> Date {
        Date::from_ymd(2024, 1, 1).unwrap()
    }

    #[test]
    fn test_simple_zero_curve() {
        let tenors = vec![1.0, 2.0, 5.0, 10.0];
        let rates = vec![0.04, 0.045, 0.05, 0.055];

        let curve = CurveBuilder::rate_curve(today())
            .with_zeros(tenors, rates, Compounding::Continuous)
            .interpolate(InterpolationMethod::Linear)
            .build()
            .unwrap();

        // Check value at 2Y
        let value = curve.value_at(2.0);
        assert_relative_eq!(value, 0.045, epsilon = 1e-10);
    }

    #[test]
    fn test_simple_df_curve() {
        let tenors = vec![1.0, 2.0, 5.0, 10.0];
        let dfs: Vec<f64> = tenors.iter().map(|&t| f64::exp(-0.05 * t)).collect();

        let curve = CurveBuilder::rate_curve(today())
            .with_discount_factors(tenors, dfs)
            .interpolate(InterpolationMethod::LogLinear)
            .build()
            .unwrap();

        // Check value at 2Y
        let value = curve.value_at(2.0);
        let expected = (-0.05 * 2.0_f64).exp();
        assert_relative_eq!(value, expected, epsilon = 1e-6);
    }

    #[test]
    fn test_multi_segment_curve() {
        let short_tenors = vec![0.25, 0.5, 1.0, 2.0];
        let short_rates = vec![0.03, 0.035, 0.04, 0.045];

        let long_tenors = vec![2.0, 5.0, 10.0, 30.0];
        let long_rates = vec![0.045, 0.05, 0.055, 0.06];

        let curve = CurveBuilder::rate_curve(today())
            .segment(0.0..2.0)
            .with_zeros(short_tenors, short_rates, Compounding::Continuous)
            .interpolate(InterpolationMethod::Linear)
            .segment_from(2.0)
            .with_zeros(long_tenors, long_rates, Compounding::Continuous)
            .interpolate(InterpolationMethod::MonotoneConvex)
            .build()
            .unwrap();

        // Check values from both segments
        let short_val = curve.value_at(1.0);
        assert_relative_eq!(short_val, 0.04, epsilon = 1e-10);

        let long_val = curve.value_at(10.0);
        assert_relative_eq!(long_val, 0.055, epsilon = 1e-10);
    }

    #[test]
    fn test_credit_curve_builder() {
        let tenors = vec![1.0, 2.0, 3.0, 5.0, 10.0];
        let survival: Vec<f64> = tenors.iter().map(|&t| f64::exp(-0.02 * t)).collect();

        let curve = CurveBuilder::credit_curve(today(), 0.40)
            .with_survival_probabilities(tenors, survival)
            .interpolate(InterpolationMethod::LogLinear)
            .build_credit_curve()
            .unwrap();

        // Check survival probability at 5Y
        let surv = curve.survival_probability_at_tenor(5.0).unwrap();
        let expected = (-0.02 * 5.0_f64).exp();
        assert_relative_eq!(surv, expected, epsilon = 1e-6);
    }

    #[test]
    fn test_spread_curve() {
        // Create base curve
        let base_tenors = vec![1.0, 2.0, 5.0, 10.0];
        let base_rates = vec![0.04, 0.045, 0.05, 0.055];

        let base = Arc::new(
            DiscreteCurve::new(
                today(),
                base_tenors,
                base_rates,
                ValueType::zero_rate(Compounding::Continuous),
                InterpolationMethod::Linear,
            )
            .unwrap(),
        );

        // Create spread curve: +100bps
        let spread_curve = CurveBuilder::rate_curve(today())
            .spread_over(base.clone(), 100.0)
            .build()
            .unwrap();

        // Check spread is applied
        let base_rate = base.value_at(5.0);
        let spread_rate = spread_curve.value_at(5.0);
        assert_relative_eq!(spread_rate - base_rate, 0.01, epsilon = 1e-10);
    }

    #[test]
    fn test_delegate_curve() {
        // Create base curve
        let base_tenors = vec![1.0, 2.0, 5.0, 10.0];
        let base_rates = vec![0.04, 0.045, 0.05, 0.055];

        let base: CurveRef = Arc::new(
            DiscreteCurve::new(
                today(),
                base_tenors,
                base_rates,
                ValueType::zero_rate(Compounding::Continuous),
                InterpolationMethod::Linear,
            )
            .unwrap(),
        );

        // Delegate to base curve
        let delegated = CurveBuilder::rate_curve(today())
            .delegate(base.clone())
            .build()
            .unwrap();

        // Should have same values
        let base_rate = base.value_at(5.0);
        let delegated_rate = delegated.value_at(5.0);
        assert_relative_eq!(delegated_rate, base_rate, epsilon = 1e-10);
    }

    #[test]
    fn test_build_rate_curve_wrapper() {
        let tenors = vec![1.0, 2.0, 5.0, 10.0];
        let rates = vec![0.04, 0.045, 0.05, 0.055];

        let rate_curve = CurveBuilder::rate_curve(today())
            .with_zeros(tenors, rates, Compounding::Continuous)
            .build_rate_curve()
            .unwrap();

        // Use rate curve methods
        let df = rate_curve.discount_factor_at_tenor(2.0).unwrap();
        let expected = (-0.045 * 2.0_f64).exp();
        assert_relative_eq!(df, expected, epsilon = 1e-6);
    }

    #[test]
    fn test_empty_builder_error() {
        let result = CurveBuilder::rate_curve(today()).build();
        assert!(result.is_err());
    }
}
