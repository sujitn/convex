//! Optimization helpers used by the analytics calibration paths.

use crate::bounds::{finite, interval, MAX_SOLVER_ITERATIONS};
use crate::{MathError, MathResult};

/// Golden-section minimiser on `[a, b]`. Robust for unimodal smooth objectives.
/// Returns the argmin.
pub fn golden_section<F: Fn(f64) -> f64>(f: F, a: f64, b: f64, tol: f64, max_iter: usize) -> f64 {
    // Compatibility API cannot represent an error. New/external callers use the
    // checked API; an unsuccessful minimisation never returns a plausible argmin.
    golden_section_checked(f, a, b, tol, max_iter).unwrap_or(f64::NAN)
}

/// Bounded golden-section minimisation with finite input/output and explicit
/// nonconvergence. The objective must be unimodal, terminating and nonpanicking.
pub fn golden_section_checked<F: Fn(f64) -> f64>(
    f: F,
    mut a: f64,
    mut b: f64,
    tol: f64,
    max_iter: usize,
) -> MathResult<f64> {
    interval(a, b)?;
    if a > b
        || !tol.is_finite()
        || tol <= 0.0
        || max_iter == 0
        || max_iter > MAX_SOLVER_ITERATIONS as usize
    {
        return Err(MathError::invalid_input(
            "invalid minimisation interval, tolerance or budget",
        ));
    }
    let phi = (5.0_f64.sqrt() - 1.0) / 2.0;
    let mut c = b - phi * (b - a);
    let mut d = a + phi * (b - a);
    let mut fc = finite(f(c), "objective evaluation")?;
    let mut fd = finite(f(d), "objective evaluation")?;
    for _ in 0..max_iter {
        if (b - a).abs() < tol {
            return finite(a * 0.5 + b * 0.5, "argmin");
        }
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - phi * (b - a);
            fc = finite(f(c), "objective evaluation")?;
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + phi * (b - a);
            fd = finite(f(d), "objective evaluation")?;
        }
    }
    if (b - a).abs() < tol {
        return finite(a * 0.5 + b * 0.5, "argmin");
    }
    Err(MathError::convergence_failed(
        max_iter as u32,
        (b - a).abs(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_section_quadratic() {
        let opt = golden_section(|x: f64| (x - 3.0).powi(2), -10.0, 10.0, 1e-9, 200);
        assert!((opt - 3.0).abs() < 1e-6);
    }
}
