//! Observed curve-move decomposition: least-squares projection (SVD) of the
//! pillar-wise change `Δr(τ) = r_t1(τ) − r_t0(τ)` onto a {level, slope,
//! curvature} basis. The unexplained part per tenor is the reported fit
//! residual. Component curves for repricing are rebuilt from the loadings via
//! the same basis ([`CurveDecomposition::component_shift_decimal`]) so the fit
//! and the reconstruction cannot drift.
//!
//! Basis (pivot `p`, span `[τ_min, τ_max]`): `b_L = 1`;
//! `b_S = (τ − p)/(τ_max − τ_min)`; `b_C = 1 − 2|τ − p|/max(|τ_min−p|,|τ_max−p|)`
//! (a tent: +1 at the pivot, −1 at the far wing).
//!
//! This is a deliberate *reporting* parameterization, not a canonical model.
//! Bucketed KRD attribution (reusing `key_rate_profile`) is the more standard
//! desk approach and is the documented v2 alternative; the parametric view is
//! used here because the brief asked for parallel/slope/curvature.

use convex_core::types::Compounding;
use convex_curves::{DiscreteCurve, RateCurve};
use nalgebra::{DMatrix, DVector};

use crate::error::{AnalyticsError, AnalyticsResult};

/// One curve factor (the residual is not a component — it is what these three
/// fail to explain).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveComponent {
    /// Level: uniform shift across all tenors.
    Parallel,
    /// Slope: linear tilt about the pivot.
    Slope,
    /// Curvature: belly-vs-wing hump about the pivot.
    Curvature,
}

/// Loadings in bp, the analysis span, and the per-tenor unexplained move.
///
/// Identity: at every analysis tenor, the three component shifts plus the
/// residual sum **exactly** to the observed `Δr` (decomposition is exact —
/// least-squares fit + residual).
#[derive(Debug, Clone, PartialEq)]
pub struct CurveDecomposition {
    /// Level loading (bp): the uniform shift component.
    pub parallel_bps: f64,
    /// Slope loading (bp): steepening over the full analysis span.
    pub slope_bps: f64,
    /// Curvature loading (bp): belly-vs-wing hump amplitude.
    pub curvature_bps: f64,
    /// Pivot tenor (years) the slope/curvature basis is centered on.
    pub pivot_tenor_years: f64,
    tenor_min: f64,
    tenor_max: f64,
    // Per-tenor unexplained Δr (bp). Internal: the surfaced diagnostic is
    // the L1 norm via `fit_residual_l1_bps()`; this feeds it and the
    // exactness check.
    residual_by_tenor: Vec<(f64, f64)>,
}

impl CurveDecomposition {
    /// Basis row `(b_L, b_S, b_C)` at `tenor_years`. Single source of truth
    /// for both the fit and component-curve reconstruction.
    fn basis_row(tenor_years: f64, pivot: f64, tmin: f64, tmax: f64) -> (f64, f64, f64) {
        let span = tmax - tmin;
        let half = (tmin - pivot).abs().max((tmax - pivot).abs());
        let b_l = 1.0;
        let b_s = if span > 0.0 {
            (tenor_years - pivot) / span
        } else {
            0.0
        };
        let b_c = if half > 0.0 {
            1.0 - 2.0 * (tenor_years - pivot).abs() / half
        } else {
            0.0
        };
        (b_l, b_s, b_c)
    }

    /// Decimal zero-rate shift contributed by `component` at `tenor_years`.
    /// `Σ over components + residual/1e4 == Δr` at every analysis tenor.
    #[must_use]
    pub fn component_shift_decimal(&self, component: CurveComponent, tenor_years: f64) -> f64 {
        let (b_l, b_s, b_c) = Self::basis_row(
            tenor_years,
            self.pivot_tenor_years,
            self.tenor_min,
            self.tenor_max,
        );
        let bps = match component {
            CurveComponent::Parallel => self.parallel_bps * b_l,
            CurveComponent::Slope => self.slope_bps * b_s,
            CurveComponent::Curvature => self.curvature_bps * b_c,
        };
        bps * 1.0e-4
    }

    /// Σ|residual| across the analysis tenors, in bp.
    #[must_use]
    pub fn fit_residual_l1_bps(&self) -> f64 {
        self.residual_by_tenor.iter().map(|(_, r)| r.abs()).sum()
    }
}

/// Decompose `curve_t1 − curve_t0` over `analysis_tenors` into level / slope /
/// curvature loadings (bp) plus the per-tenor residual.
///
/// Both curves are sampled at the **continuously compounded** zero rate (the
/// stored convention) so the difference is convention-consistent. Requires at
/// least 3 distinct analysis tenors (3 free parameters).
pub fn decompose_curve_move(
    curve_t0: &RateCurve<DiscreteCurve>,
    curve_t1: &RateCurve<DiscreteCurve>,
    analysis_tenors: &[f64],
    pivot_tenor_years: f64,
) -> AnalyticsResult<CurveDecomposition> {
    // A 3-factor fit needs ≥3 *distinct* tenors; fewer makes the design
    // matrix rank-deficient and the SVD returns a non-unique minimum-norm
    // solution (meaningless factor loadings). Fail fast instead.
    let n = analysis_tenors.len();
    let mut sorted: Vec<f64> = analysis_tenors.to_vec();
    sorted.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let distinct = if sorted.is_empty() {
        0
    } else {
        1 + sorted
            .windows(2)
            .filter(|w| (w[1] - w[0]).abs() > 1e-9)
            .count()
    };
    if distinct < 3 {
        return Err(AnalyticsError::InvalidInput(format!(
            "decompose_curve_move: need ≥3 distinct analysis tenors for a \
             3-factor fit (got {distinct} distinct of {n})"
        )));
    }
    let tmin = sorted[0];
    let tmax = sorted[sorted.len() - 1];
    if tmax <= tmin {
        return Err(AnalyticsError::InvalidInput(
            "decompose_curve_move: analysis tenors must span a positive range".into(),
        ));
    }
    // The curvature basis is `1 − 2|τ−p|/half`. If the pivot is on or outside
    // the span, |τ−p| is monotone across the grid and b_C collapses to an
    // affine function — collinear with level/slope, so the loadings are no
    // longer separable. Require a strictly interior pivot.
    if pivot_tenor_years <= tmin || pivot_tenor_years >= tmax {
        return Err(AnalyticsError::InvalidInput(format!(
            "decompose_curve_move: pivot {pivot_tenor_years} must lie strictly \
             inside the tenor span ({tmin}, {tmax}); a pivot on/outside the \
             bounds makes the curvature basis affine and the fit non-unique"
        )));
    }

    // y_i = Δr(τ_i) in bp; B_i = [b_L, b_S, b_C].
    let mut y = DVector::<f64>::zeros(n);
    let mut b = DMatrix::<f64>::zeros(n, 3);
    for (i, &t) in analysis_tenors.iter().enumerate() {
        let r0 = curve_t0
            .zero_rate_at_tenor(t, Compounding::Continuous)
            .map_err(|e| AnalyticsError::CurveError(format!("curve_t0 @ {t}y: {e}")))?;
        let r1 = curve_t1
            .zero_rate_at_tenor(t, Compounding::Continuous)
            .map_err(|e| AnalyticsError::CurveError(format!("curve_t1 @ {t}y: {e}")))?;
        y[i] = (r1 - r0) * 1.0e4;
        let (b_l, b_s, b_c) = CurveDecomposition::basis_row(t, pivot_tenor_years, tmin, tmax);
        b[(i, 0)] = b_l;
        b[(i, 1)] = b_s;
        b[(i, 2)] = b_c;
    }

    // Least squares via SVD (not normal equations — BᵀB squares the
    // condition number). Over-determined: n pillars, 3 basis columns.
    let a = b.clone().svd(true, true).solve(&y, 1e-12).map_err(|e| {
        AnalyticsError::CalculationFailed(format!(
            "decompose_curve_move: SVD least-squares failed ({n} tenors): {e}"
        ))
    })?;
    let (parallel_bps, slope_bps, curvature_bps) = (a[0], a[1], a[2]);

    // Residual = observed − fitted, per tenor (bp).
    let fitted = &b * &a;
    let residual_by_tenor = analysis_tenors
        .iter()
        .enumerate()
        .map(|(i, &t)| (t, y[i] - fitted[i]))
        .collect();

    Ok(CurveDecomposition {
        parallel_bps,
        slope_bps,
        curvature_bps,
        pivot_tenor_years,
        tenor_min: tmin,
        tenor_max: tmax,
        residual_by_tenor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::Date;
    use convex_curves::{InterpolationMethod, ValueType};

    const PILLARS: &[f64] = &[0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 20.0, 30.0];
    const PIVOT: f64 = 2.0;

    fn d() -> Date {
        Date::from_ymd(2026, 5, 7).unwrap()
    }

    fn curve(rates: &[f64]) -> RateCurve<DiscreteCurve> {
        let dc = DiscreteCurve::new(
            d(),
            PILLARS.to_vec(),
            rates.to_vec(),
            ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            },
            InterpolationMethod::Linear,
        )
        .unwrap();
        RateCurve::new(dc)
    }

    fn base_rates() -> Vec<f64> {
        // A gently upward-sloping EUR-ish curve.
        PILLARS.iter().map(|t| 0.020 + 0.0008 * t.sqrt()).collect()
    }

    fn b_row(t: f64) -> (f64, f64, f64) {
        let tmin = PILLARS[0];
        let tmax = PILLARS[PILLARS.len() - 1];
        CurveDecomposition::basis_row(t, PIVOT, tmin, tmax)
    }

    #[test]
    fn pure_parallel_move_recovers_level_only() {
        let r0 = base_rates();
        let r1: Vec<f64> = r0.iter().map(|r| r + 0.0010).collect(); // +10bp
        let dec = decompose_curve_move(&curve(&r0), &curve(&r1), PILLARS, PIVOT).unwrap();
        assert!(
            (dec.parallel_bps - 10.0).abs() < 1e-6,
            "parallel {}",
            dec.parallel_bps
        );
        assert!(dec.slope_bps.abs() < 1e-6, "slope {}", dec.slope_bps);
        assert!(dec.curvature_bps.abs() < 1e-6, "curv {}", dec.curvature_bps);
        assert!(dec.fit_residual_l1_bps() < 1e-6);
    }

    #[test]
    fn pure_slope_move_recovers_slope_only() {
        let r0 = base_rates();
        let k = 5.0; // 5bp steepening over the span
        let r1: Vec<f64> = PILLARS
            .iter()
            .zip(&r0)
            .map(|(&t, r)| r + k * 1e-4 * b_row(t).1)
            .collect();
        let dec = decompose_curve_move(&curve(&r0), &curve(&r1), PILLARS, PIVOT).unwrap();
        assert!(
            dec.parallel_bps.abs() < 1e-6,
            "parallel {}",
            dec.parallel_bps
        );
        assert!((dec.slope_bps - k).abs() < 1e-6, "slope {}", dec.slope_bps);
        assert!(dec.curvature_bps.abs() < 1e-6, "curv {}", dec.curvature_bps);
        assert!(dec.fit_residual_l1_bps() < 1e-6);
    }

    #[test]
    fn pure_curvature_move_recovers_curvature_only() {
        let r0 = base_rates();
        let k = 4.0;
        let r1: Vec<f64> = PILLARS
            .iter()
            .zip(&r0)
            .map(|(&t, r)| r + k * 1e-4 * b_row(t).2)
            .collect();
        let dec = decompose_curve_move(&curve(&r0), &curve(&r1), PILLARS, PIVOT).unwrap();
        assert!(dec.parallel_bps.abs() < 1e-6);
        assert!(dec.slope_bps.abs() < 1e-6);
        assert!(
            (dec.curvature_bps - k).abs() < 1e-6,
            "curv {}",
            dec.curvature_bps
        );
        assert!(dec.fit_residual_l1_bps() < 1e-6);
    }

    #[test]
    fn identical_curves_zero_everything() {
        let r0 = base_rates();
        let dec = decompose_curve_move(&curve(&r0), &curve(&r0), PILLARS, PIVOT).unwrap();
        assert!(dec.parallel_bps.abs() < 1e-9);
        assert!(dec.slope_bps.abs() < 1e-9);
        assert!(dec.curvature_bps.abs() < 1e-9);
        assert!(dec.fit_residual_l1_bps() < 1e-9);
    }

    #[test]
    fn kinked_move_shows_nonzero_residual() {
        // A spike at the 5Y pillar is not in span{1, slope, curvature}.
        let r0 = base_rates();
        let mut r1 = r0.clone();
        let i5 = PILLARS.iter().position(|&t| t == 5.0).unwrap();
        r1[i5] += 0.0020; // +20bp localized kink
        let dec = decompose_curve_move(&curve(&r0), &curve(&r1), PILLARS, PIVOT).unwrap();
        assert!(
            dec.fit_residual_l1_bps() > 1.0,
            "a localized kink must leave a real residual; got {}",
            dec.fit_residual_l1_bps()
        );
    }

    #[test]
    fn decomposition_is_exact_components_plus_residual_equals_move() {
        // Arbitrary irregular move: fitted + residual must reproduce Δr exactly.
        let r0 = base_rates();
        let r1: Vec<f64> = PILLARS
            .iter()
            .zip(&r0)
            .map(|(&t, r)| r + 1e-4 * (3.0 + 0.5 * t - 0.01 * t * t + (t).sin()))
            .collect();
        let dec = decompose_curve_move(&curve(&r0), &curve(&r1), PILLARS, PIVOT).unwrap();
        for (i, &t) in PILLARS.iter().enumerate() {
            let rebuilt = dec.component_shift_decimal(CurveComponent::Parallel, t)
                + dec.component_shift_decimal(CurveComponent::Slope, t)
                + dec.component_shift_decimal(CurveComponent::Curvature, t)
                + dec.residual_by_tenor[i].1 * 1e-4;
            let observed = r1[i] - r0[i];
            assert!(
                (rebuilt - observed).abs() < 1e-12,
                "tenor {t}: rebuilt {rebuilt} vs observed {observed}"
            );
        }
    }

    #[test]
    fn too_few_tenors_errors() {
        let r0 = vec![0.02, 0.03];
        let c = {
            let dc = DiscreteCurve::new(
                d(),
                vec![1.0, 2.0],
                r0.clone(),
                ValueType::ZeroRate {
                    compounding: Compounding::Continuous,
                    day_count: DayCountConvention::Act365Fixed,
                },
                InterpolationMethod::Linear,
            )
            .unwrap();
            RateCurve::new(dc)
        };
        let err = decompose_curve_move(&c, &c, &[1.0, 2.0], PIVOT);
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
        // Non-distinct tenors (len ≥ 3 but rank-deficient) also rejected.
        let dup = decompose_curve_move(
            &curve(&base_rates()),
            &curve(&base_rates()),
            &[2.0, 2.0, 2.0],
            PIVOT,
        );
        assert!(matches!(dup, Err(AnalyticsError::InvalidInput(_))));
    }

    #[test]
    fn pivot_outside_span_errors() {
        let r0 = base_rates();
        // PILLARS span [0.25, 30]; a pivot on/outside the bounds makes the
        // curvature basis affine → non-unique loadings.
        for bad_pivot in [0.25, 30.0, 0.0, 50.0] {
            let err = decompose_curve_move(&curve(&r0), &curve(&r0), PILLARS, bad_pivot);
            assert!(
                matches!(err, Err(AnalyticsError::InvalidInput(_))),
                "pivot {bad_pivot} should be rejected"
            );
        }
    }
}
