//! Curve bumping for sensitivity analysis.
//!
//! This module provides zero-copy curve bumping for efficient
//! calculation of risk sensitivities:
//!
//! - [`ParallelBump`]: Uniform shift of entire curve (DV01, PV01)
//! - [`KeyRateBump`]: Localized bump at specific tenor (key-rate duration)
//! - [`Scenario`]: Multiple simultaneous bumps (stress testing)
//!
//! # Design
//!
//! Bumped curves are implemented as wrapper types that apply the
//! bump on-the-fly during value access. This avoids copying the
//! entire curve data structure for each sensitivity calculation.
//!
//! # Example: DV01 Calculation
//!
//! ```rust,ignore
//! use convex_curves::bumping::ParallelBump;
//!
//! let bump = ParallelBump::new(1.0);  // 1bp
//! let bumped = bump.apply(&curve);
//!
//! let dv01 = bond.price(&curve)? - bond.price(&bumped)?;
//! ```
//!
//! # Example: Key-Rate Duration
//!
//! ```rust,ignore
//! use convex_curves::bumping::KeyRateBump;
//!
//! // Bump at 5Y tenor
//! let kr_bump = KeyRateBump::new(5.0, 1.0);
//! let bumped = kr_bump.apply(&curve);
//!
//! let kr_dv01_5y = bond.price(&curve)? - bond.price(&bumped)?;
//! ```
//!
//! # Example: Scenario Analysis
//!
//! ```rust,ignore
//! use convex_curves::bumping::{Scenario, ScenarioBump};
//!
//! let scenario = Scenario::new("Stress Test")
//!     .with_bump(ScenarioBump::parallel(100.0))
//!     .with_bump(ScenarioBump::steepener(25.0, 25.0, 5.0));
//!
//! let stressed = scenario.apply(&curve);
//! let stress_pnl = portfolio.pv(&curve)? - portfolio.pv(&stressed)?;
//! ```

mod key_rate;
mod parallel;
mod scenario;

pub use key_rate::{
    key_rate_profile, ArcKeyRateBumpedCurve, KeyRateBump, KeyRateBumpedCurve, STANDARD_KEY_TENORS,
};
pub use parallel::{ArcBumpedCurve, BumpedCurve, ParallelBump};
pub use scenario::{presets, ArcScenarioCurve, Scenario, ScenarioBump, ScenarioCurve};

// Borrowed and Arc-owned views apply exactly the same raw transformation.
fn shift_value(kind: &crate::value_type::ValueType, value: f64, shift: f64, tenor: f64) -> f64 {
    use crate::value_type::ValueType;
    match kind {
        ValueType::DiscountFactor | ValueType::SurvivalProbability => {
            value * convex_math::portable::exp(-shift * tenor)
        }
        ValueType::ZeroRate { .. }
        | ValueType::ForwardRate { .. }
        | ValueType::InstantaneousForward
        | ValueType::HazardRate
        | ValueType::ParSwapRate { .. }
        | ValueType::CreditSpread { .. }
        | ValueType::InflationIndexRatio
        | ValueType::FxForwardPoints => value + shift,
    }
}

fn checked_shift_value(
    kind: &crate::value_type::ValueType,
    value: f64,
    shift: f64,
    tenor: f64,
) -> crate::error::CurveResult<f64> {
    if !value.is_finite() || !shift.is_finite() || !tenor.is_finite() || tenor < 0.0 {
        return Err(crate::error::CurveError::invalid_value(
            "bumped query requires finite values and a nonnegative tenor",
        ));
    }
    let multiplicative = matches!(
        kind,
        crate::value_type::ValueType::DiscountFactor
            | crate::value_type::ValueType::SurvivalProbability
    );
    let result = shift_value(kind, value, shift, tenor);
    if !result.is_finite() || (multiplicative && value != 0.0 && result == 0.0) {
        return Err(crate::error::CurveError::invalid_value(
            "bumped query overflow or nonzero underflow",
        ));
    }
    Ok(result)
}
