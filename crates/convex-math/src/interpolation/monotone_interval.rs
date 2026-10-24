//! Local forward polynomials and their common analytic primitive.

use crate::{bounds::finite_slice, MathError, MathResult};

#[derive(Debug, Clone)]
struct Piece {
    start: f64,
    width: f64,
    coefficients: [f64; 3],
    area_before: f64,
}

impl Piece {
    fn new(start: f64, end: f64, coefficients: [f64; 3], area_before: f64) -> Self {
        Self {
            start,
            width: end - start,
            coefficients,
            area_before,
        }
    }
    fn evaluate(&self, x: f64) -> (f64, f64) {
        let s = (x - self.start) / self.width;
        let [c, b, a] = self.coefficients;
        let forward = c + s * (b + s * a);
        let area = self.area_before + self.width * s * (c + s * (b / 2.0 + s * a / 3.0));
        (forward, area)
    }
    fn check(&self) -> MathResult<()> {
        finite_slice(&self.coefficients, "monotone local polynomial")?;
        finite_slice(
            &[self.start, self.width, self.area_before],
            "monotone local domain",
        )?;
        if self.width <= 0.0 {
            return Err(MathError::invalid_input(
                "monotone regime has an unrepresentable local interval",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(super) struct Interval {
    first: Piece,
    second: Option<Piece>,
}

impl Interval {
    pub(super) fn new(mean: f64, left: f64, right: f64) -> MathResult<Self> {
        let (g0, g1) = (left - mean, right - mean);
        let opposite = (g0 < 0.0 && g1 > 0.0) || (g0 > 0.0 && g1 < 0.0);
        let result = if g0 == 0.0
            || g1 == 0.0
            || (opposite && g1.abs() >= 0.5 * g0.abs() && g1.abs() <= 2.0 * g0.abs())
        {
            // Exact axes use the original zero-area quadratic. This avoids
            // singular rational regimes without clipping their integral.
            Self {
                first: Piece::new(0.0, 1.0, [left, -4.0 * g0 - 2.0 * g1, 3.0 * (g0 + g1)], 0.0),
                second: None,
            }
        } else if opposite && g1.abs() > 2.0 * g0.abs() {
            let eta = (g1 + 2.0 * g0) / (g1 - g0);
            let first = Piece::new(0.0, eta, [left, 0.0, 0.0], 0.0);
            let second = Piece::new(eta, 1.0, [left, 0.0, g1 - g0], first.evaluate(eta).1);
            Self {
                first,
                second: Some(second),
            }
        } else if opposite {
            let eta = 3.0 * g1 / (g1 - g0);
            let delta = g0 - g1;
            let first = Piece::new(0.0, eta, [left, -2.0 * delta, delta], 0.0);
            let second = Piece::new(eta, 1.0, [right, 0.0, 0.0], first.evaluate(eta).1);
            Self {
                first,
                second: Some(second),
            }
        } else {
            let eta = g1 / (g0 + g1);
            let shift = -g0 * eta;
            let delta = g0 - shift;
            let first = Piece::new(0.0, eta, [left, -2.0 * delta, delta], 0.0);
            let second = Piece::new(
                eta,
                1.0,
                [mean + shift, 0.0, g1 - shift],
                first.evaluate(eta).1,
            );
            Self {
                first,
                second: Some(second),
            }
        };
        result.first.check()?;
        if let Some(piece) = &result.second {
            piece.check()?;
        }
        Ok(result)
    }

    fn piece(&self, x: f64) -> &Piece {
        match &self.second {
            Some(second) if x > second.start => second,
            _ => &self.first,
        }
    }

    pub(super) fn evaluate(&self, x: f64) -> (f64, f64) {
        self.piece(x).evaluate(x)
    }

    pub(super) fn origin_average(&self, x: f64) -> Option<(f64, f64)> {
        let piece = self.piece(x);
        if piece.start != 0.0 {
            return None;
        }
        let s = x / piece.width;
        let [c, b, a] = piece.coefficients;
        Some((
            c + s * (b / 2.0 + s * a / 3.0),
            (b / 2.0 + 2.0 * a * s / 3.0) / piece.width,
        ))
    }
}
