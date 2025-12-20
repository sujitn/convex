//! Segmented curve implementation.
//!
//! A `SegmentedCurve` combines multiple curve segments, each with its own
//! source and interpolation method. This enables different interpolation
//! for short, medium, and long tenors.

use std::sync::Arc;

use convex_core::types::Date;

use crate::error::{CurveError, CurveResult};
use crate::term_structure::{CurveRef, TermStructure};
use crate::value_type::ValueType;
use crate::curves::{CurveTransform, DelegationFallback};

/// Source of data for a curve segment.
#[derive(Clone)]
pub enum SegmentSource {
    /// Direct point data.
    Discrete {
        /// Tenors in years.
        tenors: Vec<f64>,
        /// Values at each tenor.
        values: Vec<f64>,
    },

    /// Delegate to another curve.
    Delegated {
        /// The curve to delegate to.
        curve: CurveRef,
        /// Fallback behavior.
        fallback: DelegationFallback,
    },

    /// Derived from a base curve with transformation.
    Derived {
        /// Base curve.
        base: CurveRef,
        /// Transformation to apply.
        transform: CurveTransform,
    },
}

impl std::fmt::Debug for SegmentSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SegmentSource::Discrete { tenors, values } => {
                f.debug_struct("Discrete")
                    .field("n_points", &tenors.len())
                    .field("tenor_range", &(tenors.first(), tenors.last()))
                    .finish()
            }
            SegmentSource::Delegated { fallback, .. } => {
                f.debug_struct("Delegated")
                    .field("fallback", fallback)
                    .finish()
            }
            SegmentSource::Derived { transform, .. } => {
                f.debug_struct("Derived")
                    .field("transform", transform)
                    .finish()
            }
        }
    }
}

/// A segment of a curve with its own source and tenor range.
#[derive(Clone)]
pub struct CurveSegment {
    /// Start of the segment (inclusive).
    pub start: f64,
    /// End of the segment (exclusive, or None for infinity).
    pub end: Option<f64>,
    /// Source of data for this segment.
    source: SegmentSource,
    /// Pre-computed curve for this segment.
    curve: CurveRef,
}

impl std::fmt::Debug for CurveSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CurveSegment")
            .field("start", &self.start)
            .field("end", &self.end)
            .field("source", &self.source)
            .finish()
    }
}

impl CurveSegment {
    /// Creates a new curve segment.
    pub fn new(
        start: f64,
        end: Option<f64>,
        source: SegmentSource,
        curve: CurveRef,
    ) -> Self {
        Self {
            start,
            end,
            source,
            curve,
        }
    }

    /// Returns true if this segment covers the given tenor.
    #[must_use]
    pub fn covers(&self, t: f64) -> bool {
        if t < self.start {
            return false;
        }
        match self.end {
            Some(end) => t < end,
            None => true, // Unbounded
        }
    }

    /// Returns the value at the given tenor.
    #[must_use]
    pub fn value_at(&self, t: f64) -> f64 {
        self.curve.value_at(t)
    }

    /// Returns the derivative at the given tenor.
    pub fn derivative_at(&self, t: f64) -> Option<f64> {
        self.curve.derivative_at(t)
    }
}

/// A curve composed of multiple segments with different sources/interpolation.
///
/// This is the most flexible curve type, allowing:
/// - Different interpolation for short, medium, long tenors
/// - Mixing discrete data with delegated curves
/// - Layering transformations at specific tenor ranges
///
/// # Example
///
/// ```rust,ignore
/// let curve = CurveBuilder::rate_curve(today)
///     .segment(0.0..2.0)
///         .discrete_zeros(short_tenors, short_rates, Compounding::Continuous)
///         .interpolate(InterpolationMethod::Linear)
///     .segment(2.0..10.0)
///         .delegate(swap_curve)
///         .interpolate(InterpolationMethod::MonotoneConvex)
///     .segment(10.0..)
///         .delegate(long_curve)
///         .extrapolate(ExtrapolationMethod::FlatForward)
///     .build()?;
/// ```
#[derive(Clone)]
pub struct SegmentedCurve {
    /// Reference date.
    reference_date: Date,
    /// Segments in order by tenor.
    segments: Vec<CurveSegment>,
    /// Overall value type.
    value_type: ValueType,
    /// Minimum tenor across all segments.
    min_tenor: f64,
    /// Maximum tenor across all segments.
    max_tenor: f64,
}

impl std::fmt::Debug for SegmentedCurve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentedCurve")
            .field("reference_date", &self.reference_date)
            .field("n_segments", &self.segments.len())
            .field("min_tenor", &self.min_tenor)
            .field("max_tenor", &self.max_tenor)
            .field("value_type", &self.value_type)
            .finish()
    }
}

impl SegmentedCurve {
    /// Creates a new segmented curve.
    ///
    /// # Arguments
    ///
    /// * `reference_date` - Valuation date
    /// * `segments` - Ordered list of segments (must not overlap)
    /// * `value_type` - What the values represent
    ///
    /// # Errors
    ///
    /// Returns an error if segments overlap or have gaps.
    pub fn new(
        reference_date: Date,
        segments: Vec<CurveSegment>,
        value_type: ValueType,
    ) -> CurveResult<Self> {
        if segments.is_empty() {
            return Err(CurveError::builder_error("At least one segment required"));
        }

        // Validate segments don't overlap and are ordered
        for i in 1..segments.len() {
            let prev_end = segments[i - 1].end;
            let curr_start = segments[i].start;

            match prev_end {
                Some(end) if end > curr_start => {
                    return Err(CurveError::SegmentOverlap { tenor: curr_start });
                }
                Some(end) if end < curr_start => {
                    return Err(CurveError::SegmentGap {
                        from: end,
                        to: curr_start,
                    });
                }
                _ => {}
            }
        }

        let min_tenor = segments[0].start;
        let max_tenor = segments
            .last()
            .and_then(|s| s.end)
            .unwrap_or(100.0); // Default max if unbounded

        Ok(Self {
            reference_date,
            segments,
            value_type,
            min_tenor,
            max_tenor,
        })
    }

    /// Finds the segment covering the given tenor.
    fn find_segment(&self, t: f64) -> Option<&CurveSegment> {
        self.segments.iter().find(|seg| seg.covers(t))
    }

    /// Returns the number of segments.
    #[must_use]
    pub fn num_segments(&self) -> usize {
        self.segments.len()
    }

    /// Returns the segments.
    #[must_use]
    pub fn segments(&self) -> &[CurveSegment] {
        &self.segments
    }
}

impl TermStructure for SegmentedCurve {
    fn reference_date(&self) -> Date {
        self.reference_date
    }

    fn value_at(&self, t: f64) -> f64 {
        match self.find_segment(t) {
            Some(segment) => segment.value_at(t),
            None => {
                // Out of range - use nearest segment
                if t < self.min_tenor {
                    self.segments[0].value_at(self.min_tenor)
                } else {
                    self.segments
                        .last()
                        .map(|s| s.value_at(self.max_tenor))
                        .unwrap_or(f64::NAN)
                }
            }
        }
    }

    fn tenor_bounds(&self) -> (f64, f64) {
        (self.min_tenor, self.max_tenor)
    }

    fn value_type(&self) -> ValueType {
        self.value_type.clone()
    }

    fn derivative_at(&self, t: f64) -> Option<f64> {
        self.find_segment(t)?.derivative_at(t)
    }

    fn max_date(&self) -> Date {
        self.tenor_to_date(self.max_tenor)
    }
}

// Thread safety
unsafe impl Send for SegmentedCurve {}
unsafe impl Sync for SegmentedCurve {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscreteCurve;
    use crate::InterpolationMethod;
    use approx::assert_relative_eq;
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::Compounding;

    fn short_segment_curve() -> CurveRef {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        Arc::new(
            DiscreteCurve::new(
                today,
                vec![0.0, 0.5, 1.0, 2.0],
                vec![0.03, 0.035, 0.04, 0.045],
                ValueType::DiscountFactor,
                InterpolationMethod::Linear,
            )
            .unwrap(),
        )
    }

    fn long_segment_curve() -> CurveRef {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        Arc::new(
            DiscreteCurve::new(
                today,
                vec![2.0, 5.0, 10.0, 30.0],
                vec![0.045, 0.05, 0.055, 0.06],
                ValueType::DiscountFactor,
                InterpolationMethod::Linear,
            )
            .unwrap(),
        )
    }

    #[test]
    fn test_single_segment() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let curve = short_segment_curve();

        let segment = CurveSegment::new(
            0.0,
            Some(2.0),
            SegmentSource::Discrete {
                tenors: vec![0.0, 2.0],
                values: vec![0.03, 0.045],
            },
            curve,
        );

        let segmented = SegmentedCurve::new(
            today,
            vec![segment],
            ValueType::DiscountFactor,
        )
        .unwrap();

        assert_eq!(segmented.num_segments(), 1);
        assert!(segmented.in_range(1.0));
    }

    #[test]
    fn test_multi_segment() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();

        let short = CurveSegment::new(
            0.0,
            Some(2.0),
            SegmentSource::Discrete {
                tenors: vec![0.0, 2.0],
                values: vec![0.03, 0.045],
            },
            short_segment_curve(),
        );

        let long = CurveSegment::new(
            2.0,
            None,
            SegmentSource::Discrete {
                tenors: vec![2.0, 30.0],
                values: vec![0.045, 0.06],
            },
            long_segment_curve(),
        );

        let segmented = SegmentedCurve::new(
            today,
            vec![short, long],
            ValueType::DiscountFactor,
        )
        .unwrap();

        assert_eq!(segmented.num_segments(), 2);

        // Test value from short segment
        let val_1y = segmented.value_at(1.0);
        assert!(val_1y > 0.0);

        // Test value from long segment
        let val_10y = segmented.value_at(10.0);
        assert_relative_eq!(val_10y, 0.055, epsilon = 1e-10);
    }

    #[test]
    fn test_segment_coverage() {
        let segment = CurveSegment::new(
            2.0,
            Some(10.0),
            SegmentSource::Discrete {
                tenors: vec![],
                values: vec![],
            },
            short_segment_curve(),
        );

        assert!(!segment.covers(1.0));
        assert!(segment.covers(2.0));
        assert!(segment.covers(5.0));
        assert!(!segment.covers(10.0)); // End is exclusive
        assert!(!segment.covers(15.0));
    }

    #[test]
    fn test_unbounded_segment() {
        let segment = CurveSegment::new(
            5.0,
            None, // Unbounded
            SegmentSource::Discrete {
                tenors: vec![],
                values: vec![],
            },
            short_segment_curve(),
        );

        assert!(!segment.covers(4.0));
        assert!(segment.covers(5.0));
        assert!(segment.covers(100.0));
        assert!(segment.covers(1000.0));
    }

    #[test]
    fn test_segment_overlap_error() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();

        let seg1 = CurveSegment::new(
            0.0,
            Some(5.0), // Ends at 5
            SegmentSource::Discrete { tenors: vec![], values: vec![] },
            short_segment_curve(),
        );

        let seg2 = CurveSegment::new(
            3.0, // Starts at 3 - overlaps!
            Some(10.0),
            SegmentSource::Discrete { tenors: vec![], values: vec![] },
            long_segment_curve(),
        );

        let result = SegmentedCurve::new(
            today,
            vec![seg1, seg2],
            ValueType::DiscountFactor,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_segment_gap_error() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();

        let seg1 = CurveSegment::new(
            0.0,
            Some(2.0), // Ends at 2
            SegmentSource::Discrete { tenors: vec![], values: vec![] },
            short_segment_curve(),
        );

        let seg2 = CurveSegment::new(
            5.0, // Starts at 5 - gap from 2 to 5!
            None,
            SegmentSource::Discrete { tenors: vec![], values: vec![] },
            long_segment_curve(),
        );

        let result = SegmentedCurve::new(
            today,
            vec![seg1, seg2],
            ValueType::DiscountFactor,
        );

        assert!(result.is_err());
    }
}
