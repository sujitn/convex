//! Right-owned piecewise constants; no linear proxy or derivative at a jump.

use super::Interpolator;
use crate::{bounds::finite, MathError, MathResult};

/// Hold `ys[i]` over `[xs[i], xs[i+1])`, and retain the final supplied value.
/// Setup uses the original bounded finite/strictly-increasing point validator.
#[derive(Debug, Clone)]
pub struct PiecewiseConstant {
    xs: Vec<f64>,
    ys: Vec<f64>,
}

impl PiecewiseConstant {
    /// At least two matched, finite, strictly increasing supplied points.
    pub fn new(xs: Vec<f64>, ys: Vec<f64>) -> MathResult<Self> {
        super::validate_points(&xs, &ys, 2)?;
        Ok(Self { xs, ys })
    }

    fn index(&self, x: f64) -> MathResult<usize> {
        finite(x, "query coordinate")?;
        if x < self.min_x() || x > self.max_x() {
            return Err(MathError::ExtrapolationNotAllowed {
                x,
                min: self.min_x(),
                max: self.max_x(),
            });
        }
        Ok(match self.xs.binary_search_by(|p| p.total_cmp(&x)) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        })
    }
}

impl Interpolator for PiecewiseConstant {
    fn interpolate(&self, x: f64) -> MathResult<f64> {
        Ok(self.ys[self.index(x)?])
    }

    fn derivative(&self, x: f64) -> MathResult<f64> {
        let i = self.index(x)?;
        if i > 0 && x == self.xs[i] && self.ys[i] != self.ys[i - 1] {
            return Err(MathError::invalid_input(
                "derivative is undefined at a step jump",
            ));
        }
        Ok(0.0)
    }

    fn min_x(&self) -> f64 {
        self.xs[0]
    }

    fn max_x(&self) -> f64 {
        self.xs[self.xs.len() - 1]
    }
}
