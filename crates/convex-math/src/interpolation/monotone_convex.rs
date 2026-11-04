//! Nonnegative Hagan–West continuous-zero interpolation.
//!
//! Uses the four original piecewise regimes and one common analytic primitive.
//! Endpoint estimates precede simultaneous positivity bounds. Exactly one zero
//! endpoint deviation uses the original zero-area quadratic, an explicit axis
//! convention that avoids singular rational pieces. No amelioration is applied.
//! Forward rates are continuous; their first derivative need not be continuous
//! at pillars. Zero rates are C1 for positive time. This is an interpolation
//! model, not a production or market-convention certificate.
//!
//! Reference: Hagan & West (2006), equations (30)–(62), (81).

use crate::bounds::{finite, finite_slice};
use crate::error::{MathError, MathResult};
use crate::interpolation::Interpolator;
#[path = "monotone_interval.rs"]
mod interval;
use interval::Interval;

/// Complete nonnegative Hagan–West interpolation of continuous zero rates.
///
/// Inputs must have strictly positive, increasing times and nonnegative interval
/// forwards, including the first interval from zero. Zero intervals are valid.
/// Extrapolation, when enabled, integrates the terminal instantaneous forward.
#[derive(Debug, Clone)]
pub struct MonotoneConvex {
    times: Vec<f64>,
    zero_rates: Vec<f64>,
    discrete_forwards: Vec<f64>,
    // Includes the analytic origin, followed by every original input pillar.
    f_inst: Vec<f64>,
    intervals: Vec<Interval>,
    allow_extrapolation: bool,
}

impl MonotoneConvex {
    /// Construct the complete nonnegative model; no rate is clipped at query.
    pub fn new(times: Vec<f64>, zero_rates: Vec<f64>) -> MathResult<Self> {
        super::validate_points(&times, &zero_rates, 2)?;
        if times[0] <= 0.0 {
            return Err(MathError::invalid_input("first time must be positive"));
        }
        let mut discrete_forwards = Vec::with_capacity(times.len());
        discrete_forwards.push(zero_rates[0]);
        for i in 1..times.len() {
            discrete_forwards.push(
                (zero_rates[i] * times[i] - zero_rates[i - 1] * times[i - 1])
                    / (times[i] - times[i - 1]),
            );
        }
        finite_slice(&discrete_forwards, "discrete forwards")?;
        if discrete_forwards.iter().any(|&f| f < 0.0) {
            return Err(MathError::invalid_input(
                "negative interval forward, including origin interval",
            ));
        }
        let f_inst = Self::node_forwards(&times, &discrete_forwards)?;
        let intervals = discrete_forwards
            .iter()
            .enumerate()
            .map(|(i, &mean)| Interval::new(mean, f_inst[i], f_inst[i + 1]))
            .collect::<MathResult<Vec<_>>>()?;
        Ok(Self {
            times,
            zero_rates,
            discrete_forwards,
            f_inst,
            intervals,
            allow_extrapolation: false,
        })
    }

    fn node_forwards(times: &[f64], means: &[f64]) -> MathResult<Vec<f64>> {
        let n = times.len();
        let mut nodes = vec![0.0; n + 1];
        for i in 1..n {
            let left = times[i - 1] - if i == 1 { 0.0 } else { times[i - 2] };
            let right = times[i] - times[i - 1];
            nodes[i] = right / (left + right) * means[i - 1] + left / (left + right) * means[i];
        }
        nodes[0] = 1.5 * means[0] - 0.5 * nodes[1];
        nodes[n] = 1.5 * means[n - 1] - 0.5 * nodes[n - 1];
        finite_slice(&nodes, "instantaneous forward estimates")?;
        for i in 0..=n {
            let cap = 2.0
                * if i == 0 {
                    means[0]
                } else if i == n {
                    means[n - 1]
                } else {
                    means[i - 1].min(means[i])
                };
            finite(cap, "instantaneous forward bound")?;
            nodes[i] = nodes[i].clamp(0.0, cap);
        }
        Ok(nodes)
    }

    fn check_time(&self, t: f64) -> MathResult<()> {
        finite(t, "query coordinate")?;
        if t < 0.0 {
            return Err(MathError::invalid_input("negative model time"));
        }
        if !self.allow_extrapolation && t > self.max_x() {
            return Err(MathError::ExtrapolationNotAllowed {
                x: t,
                min: 0.0,
                max: self.max_x(),
            });
        }
        Ok(())
    }

    fn interval_index(&self, t: f64) -> usize {
        match self.times.binary_search_by(|p| p.total_cmp(&t)) {
            Ok(i) | Err(i) => i,
        }
    }

    fn coordinate(&self, i: usize, t: f64) -> (f64, f64, f64) {
        let start = if i == 0 { 0.0 } else { self.times[i - 1] };
        let width = self.times[i] - start;
        (start, width, (t - start) / width)
    }

    /// Original analytic forward, continuous at every input pillar.
    pub fn forward_rate(&self, t: f64) -> MathResult<f64> {
        self.check_time(t)?;
        let f = if t == 0.0 {
            self.f_inst[0]
        } else if t >= self.max_x() {
            self.f_inst[self.times.len()]
        } else if let Ok(i) = self.times.binary_search_by(|p| p.total_cmp(&t)) {
            self.f_inst[i + 1]
        } else {
            let i = self.interval_index(t);
            self.intervals[i].evaluate(self.coordinate(i, t).2).0
        };
        finite(f, "monotone forward")?;
        if f < 0.0 {
            return Err(MathError::invalid_input(
                "negative arithmetic result in nonnegative model",
            ));
        }
        Ok(f)
    }

    /// Exact analytic integral of the same forward used by forward_rate.
    pub fn integrated_forward(&self, t: f64) -> MathResult<f64> {
        self.check_time(t)?;
        if t == 0.0 {
            return Ok(0.0);
        }
        if t > self.max_x() {
            let last = self.times.len() - 1;
            return finite(
                self.times[last] * self.zero_rates[last]
                    + (t - self.times[last]) * self.f_inst[last + 1],
                "monotone tail integral",
            );
        }
        if let Ok(i) = self.times.binary_search_by(|p| p.total_cmp(&t)) {
            return finite(t * self.zero_rates[i], "monotone node integral");
        }
        let i = self.interval_index(t);
        let (start, width, x) = self.coordinate(i, t);
        let previous = if i == 0 {
            0.0
        } else {
            start * self.zero_rates[i - 1]
        };
        finite(
            previous + width * self.intervals[i].evaluate(x).1,
            "monotone integral",
        )
    }

    /// Enable a terminal flat-forward tail with the same integrated economics.
    #[must_use]
    pub fn with_extrapolation(mut self) -> Self {
        self.allow_extrapolation = true;
        self
    }
    /// Original interval averages, including the origin interval.
    #[must_use]
    pub fn discrete_forwards(&self) -> &[f64] {
        &self.discrete_forwards
    }
    /// Analytic origin followed by every supplied node forward.
    #[must_use]
    pub fn node_forward_rates(&self) -> &[f64] {
        &self.f_inst
    }
    /// Original positive input times; no origin pillar is fabricated.
    #[must_use]
    pub fn times(&self) -> &[f64] {
        &self.times
    }
    /// Original continuous zero quotes.
    #[must_use]
    pub fn zero_rates(&self) -> &[f64] {
        &self.zero_rates
    }
}

impl Interpolator for MonotoneConvex {
    fn interpolate(&self, t: f64) -> MathResult<f64> {
        self.check_time(t)?;
        if t == 0.0 {
            return Ok(self.f_inst[0]);
        }
        if let Ok(i) = self.times.binary_search_by(|p| p.total_cmp(&t)) {
            return Ok(self.zero_rates[i]);
        }
        if t < self.times[0] {
            if let Some((average, _)) = self.intervals[0].origin_average(t / self.times[0]) {
                return finite(average, "monotone origin zero");
            }
        }
        finite(self.integrated_forward(t)? / t, "monotone zero")
    }
    fn derivative(&self, t: f64) -> MathResult<f64> {
        self.check_time(t)?;
        if t < self.times[0] {
            if let Some((_, slope)) = self.intervals[0].origin_average(t / self.times[0]) {
                return finite(slope / self.times[0], "monotone origin derivative");
            }
        }
        finite(
            (self.forward_rate(t)? - self.interpolate(t)?) / t,
            "monotone zero derivative",
        )
    }
    fn allows_extrapolation(&self) -> bool {
        self.allow_extrapolation
    }
    fn min_x(&self) -> f64 {
        0.0
    }
    fn max_x(&self) -> f64 {
        self.times[self.times.len() - 1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_monotone_convex_through_points() {
        let times = vec![0.25, 0.5, 1.0, 2.0, 3.0];
        let zero_rates = vec![0.02, 0.025, 0.03, 0.035, 0.04];

        let interp = MonotoneConvex::new(times.clone(), zero_rates.clone()).unwrap();

        // Should pass through (or be very close to) all data points
        for (t, z) in times.iter().zip(zero_rates.iter()) {
            let interpolated = interp.interpolate(*t).unwrap();
            assert_relative_eq!(interpolated, *z, epsilon = 0.001);
        }
    }

    #[test]
    fn test_monotone_convex_positive_forwards() {
        let times = vec![0.5, 1.0, 2.0, 3.0, 5.0, 10.0];
        let zero_rates = vec![0.02, 0.025, 0.03, 0.032, 0.035, 0.04];

        let interp = MonotoneConvex::new(times, zero_rates).unwrap();

        // All forward rates should be positive
        for t in [0.1, 0.5, 1.0, 1.5, 2.5, 4.0, 7.5] {
            let fwd = interp.forward_rate(t).unwrap();
            assert!(
                fwd >= 0.0,
                "Forward rate at t={} is {}, should be >= 0",
                t,
                fwd
            );
        }
    }

    #[test]
    fn test_monotone_convex_no_oscillation() {
        // Test with data that could cause oscillation with cubic splines
        let times = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let zero_rates = vec![0.03, 0.035, 0.035, 0.035, 0.04];

        let interp = MonotoneConvex::new(times, zero_rates).unwrap();

        // Check that interpolated values don't oscillate wildly
        let mut prev_z = interp.interpolate(1.0).unwrap();
        for t in [1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0] {
            let z = interp.interpolate(t).unwrap();
            // Rate should not jump by more than 1% between adjacent points
            let change = (z - prev_z).abs();
            assert!(
                change < 0.01,
                "Large jump at t={}: {} to {} (change={})",
                t,
                prev_z,
                z,
                change
            );
            prev_z = z;
        }
    }

    #[test]
    fn test_monotone_convex_derivative() {
        let times = vec![0.5, 1.0, 2.0, 3.0, 5.0];
        let zero_rates = vec![0.02, 0.025, 0.03, 0.035, 0.04];

        let interp = MonotoneConvex::new(times, zero_rates).unwrap();

        // Test derivative with numerical check
        let t = 1.5;
        let h = 1e-6;

        let z_plus = interp.interpolate(t + h).unwrap();
        let z_minus = interp.interpolate(t - h).unwrap();
        let numerical_deriv = (z_plus - z_minus) / (2.0 * h);

        let analytical_deriv = interp.derivative(t).unwrap();

        assert_relative_eq!(analytical_deriv, numerical_deriv, epsilon = 1e-4);
    }

    #[test]
    fn test_monotone_convex_rejects_negative_forwards() {
        // This data would produce negative forward rates
        let times = vec![1.0, 2.0, 3.0];
        let zero_rates = vec![0.05, 0.02, 0.04]; // z[1]*t[1] < z[0]*t[0]

        let result = MonotoneConvex::new(times, zero_rates);
        assert!(result.is_err());
    }

    #[test]
    fn test_monotone_convex_flat_curve() {
        // Flat zero rate curve
        let times = vec![1.0, 2.0, 3.0, 5.0, 10.0];
        let zero_rates = vec![0.03, 0.03, 0.03, 0.03, 0.03];

        let interp = MonotoneConvex::new(times, zero_rates).unwrap();

        // Interpolated rates should all be 3%
        for t in [0.5, 1.5, 2.5, 4.0, 7.0] {
            let z = interp.interpolate(t).unwrap();
            assert_relative_eq!(z, 0.03, epsilon = 0.001);
        }

        // Forward rates should also be 3%
        for t in [0.5, 1.5, 2.5, 4.0, 7.0] {
            let f = interp.forward_rate(t).unwrap();
            assert_relative_eq!(f, 0.03, epsilon = 0.001);
        }
    }

    #[test]
    fn test_monotone_convex_steep_curve() {
        // Steep upward sloping curve
        let times = vec![0.25, 0.5, 1.0, 2.0, 5.0, 10.0];
        let zero_rates = vec![0.01, 0.015, 0.025, 0.04, 0.05, 0.055];

        let interp = MonotoneConvex::new(times, zero_rates).unwrap();

        // All forwards should be positive
        for t in [0.1, 0.3, 0.75, 1.5, 3.0, 7.0] {
            let f = interp.forward_rate(t).unwrap();
            assert!(f > 0.0);
        }
    }

    #[test]
    fn test_monotone_convex_inverted_curve() {
        // Inverted curve (short rates higher than long rates)
        // Forwards will be lower but should still be positive
        let times = vec![0.5, 1.0, 2.0, 5.0, 10.0];
        let zero_rates = vec![0.05, 0.048, 0.045, 0.04, 0.038];

        let interp = MonotoneConvex::new(times, zero_rates).unwrap();

        // All forwards should still be positive
        for t in [0.25, 0.75, 1.5, 3.0, 7.0] {
            let f = interp.forward_rate(t).unwrap();
            assert!(f >= 0.0, "Forward at t={} is {} (should be >= 0)", t, f);
        }
    }

    #[test]
    fn test_monotone_convex_extrapolation() {
        let times = vec![1.0, 2.0, 3.0];
        let zero_rates = vec![0.03, 0.035, 0.04];

        let interp = MonotoneConvex::new(times, zero_rates).unwrap();

        // Without extrapolation enabled
        assert!(interp.interpolate(5.0).is_err());

        // With extrapolation enabled
        let interp_extrap = interp.with_extrapolation();
        let z = interp_extrap.interpolate(5.0).unwrap();
        assert!(z > 0.0);
    }

    #[test]
    fn test_discrete_forwards() {
        let times = vec![1.0, 2.0, 3.0];
        let zero_rates = vec![0.02, 0.03, 0.04];

        let interp = MonotoneConvex::new(times, zero_rates).unwrap();

        let fwds = interp.discrete_forwards();

        // f_1 = z_0 = 0.02 (flat from 0 to t[0])
        assert_relative_eq!(fwds[0], 0.02, epsilon = 1e-10);

        // f_2 = (z_1 * t_1 - z_0 * t_0) / (t_1 - t_0) = (0.03*2 - 0.02*1) / 1 = 0.04
        assert_relative_eq!(fwds[1], 0.04, epsilon = 1e-10);

        // f_3 = (z_2 * t_2 - z_1 * t_1) / (t_2 - t_1) = (0.04*3 - 0.03*2) / 1 = 0.06
        assert_relative_eq!(fwds[2], 0.06, epsilon = 1e-10);
    }
}
