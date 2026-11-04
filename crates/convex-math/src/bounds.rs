//! Resource ceilings and finite arithmetic checks for numerical kernels.

use crate::{MathError, MathResult};

/// Hard per-phase ceiling, independent of the unchanged default of 100 iterations.
pub const MAX_SOLVER_ITERATIONS: u32 = 10_000;
/// Maximum points in one interpolation or tridiagonal operation.
pub const MAX_DATA_POINTS: usize = 65_536;
/// Maximum dense matrix dimension; bounds cubic elimination work.
pub const MAX_MATRIX_DIMENSION: usize = 256;

pub(crate) fn finite(value: f64, name: &str) -> MathResult<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(MathError::NonFinite {
            operation: name.to_owned(),
        })
    }
}

pub(crate) fn finite_slice(values: &[f64], name: &str) -> MathResult<()> {
    if values.len() > MAX_DATA_POINTS || values.iter().any(|v| !v.is_finite()) {
        return Err(MathError::invalid_input(format!(
            "{name} exceeds the point bound or contains nonfinite values"
        )));
    }
    Ok(())
}

pub(crate) fn interval(a: f64, b: f64) -> MathResult<()> {
    finite(a, "interval endpoint")?;
    finite(b, "interval endpoint")?;
    finite(b - a, "interval width")?;
    if a == b {
        return Err(MathError::invalid_input("interval endpoints must differ"));
    }
    Ok(())
}

pub(crate) fn opposite_signs(a: f64, b: f64) -> bool {
    a.is_sign_negative() != b.is_sign_negative()
}
