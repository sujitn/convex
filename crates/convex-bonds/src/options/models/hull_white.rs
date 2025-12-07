//! Hull-White one-factor short rate model.
//!
//! The Hull-White model is defined by:
//!
//! ```text
//! dr = (θ(t) - a*r)dt + σ*dW
//! ```
//!
//! Where:
//! - `a` = mean reversion speed
//! - `σ` = volatility
//! - `θ(t)` = time-dependent drift calibrated to fit the yield curve
//!
//! # Properties
//!
//! - Analytically tractable
//! - Fits the initial yield curve exactly
//! - Can produce negative rates (use Black-Karasinski if this is a concern)
//! - Industry standard for callable bond pricing

use super::{BinomialTree, ShortRateModel};

/// Hull-White one-factor short rate model.
///
/// This is the industry-standard model for pricing callable bonds and
/// calculating OAS. It provides exact fit to the initial yield curve
/// while modeling mean-reverting rate dynamics.
///
/// # Example
///
/// ```rust
/// use convex_bonds::options::HullWhite;
///
/// // Create model with typical parameters
/// let model = HullWhite::new(0.03, 0.01);  // 3% mean reversion, 1% vol
///
/// // From swaption ATM volatility
/// let model = HullWhite::from_swaption_vol(0.70, 0.03);  // 70% normal vol
/// ```
///
/// # Parameters
///
/// - **Mean Reversion (a)**: Speed at which rates revert to long-term level.
///   Typical values: 0.01 - 0.10 (1% to 10% per year).
///   Higher values mean faster reversion and less rate volatility at long maturities.
///
/// - **Volatility (σ)**: Instantaneous volatility of the short rate.
///   Typical values: 0.005 - 0.02 (50 to 200 bps annualized).
///   Can be calibrated from swaption volatilities.
#[derive(Debug, Clone)]
pub struct HullWhite {
    /// Mean reversion speed (a).
    mean_reversion: f64,

    /// Short rate volatility (σ).
    volatility: f64,
}

impl HullWhite {
    /// Creates a new Hull-White model with the given parameters.
    ///
    /// # Arguments
    ///
    /// * `mean_reversion` - Mean reversion speed (typically 0.01 - 0.10)
    /// * `volatility` - Short rate volatility (typically 0.005 - 0.02)
    ///
    /// # Example
    ///
    /// ```rust
    /// use convex_bonds::options::HullWhite;
    ///
    /// let model = HullWhite::new(0.03, 0.01);
    /// ```
    #[must_use]
    pub fn new(mean_reversion: f64, volatility: f64) -> Self {
        Self {
            mean_reversion: mean_reversion.max(0.001), // Prevent div by zero
            volatility: volatility.abs(),
        }
    }

    /// Creates a Hull-White model from swaption ATM volatility.
    ///
    /// This uses a simplified calibration assuming constant volatility.
    /// For production use, a full swaption calibration is recommended.
    ///
    /// # Arguments
    ///
    /// * `atm_vol` - At-the-money swaption normal volatility (e.g., 0.70 for 70 bps)
    /// * `mean_reversion` - Mean reversion speed
    ///
    /// # Example
    ///
    /// ```rust
    /// use convex_bonds::options::HullWhite;
    ///
    /// // 70 bps normal vol, 3% mean reversion
    /// let model = HullWhite::from_swaption_vol(0.0070, 0.03);
    /// ```
    #[must_use]
    pub fn from_swaption_vol(atm_vol: f64, mean_reversion: f64) -> Self {
        // Approximate conversion from swaption normal vol to short rate vol
        // This is a simplification; full calibration would use swaption pricer
        let short_rate_vol = atm_vol;
        Self::new(mean_reversion, short_rate_vol)
    }

    /// Creates a Hull-White model with default parameters.
    ///
    /// Uses mean reversion = 3%, volatility = 1%.
    #[must_use]
    pub fn default_params() -> Self {
        Self::new(0.03, 0.01)
    }

    /// Returns the B(t,T) function used in Hull-White pricing.
    ///
    /// B(t,T) = (1 - exp(-a*(T-t))) / a
    #[allow(dead_code)]
    fn b_factor(&self, t: f64, big_t: f64) -> f64 {
        let a = self.mean_reversion;
        let tau = big_t - t;
        if tau <= 0.0 {
            return 0.0;
        }
        (1.0 - (-a * tau).exp()) / a
    }

    /// Calculates θ(t) to fit the initial zero curve.
    ///
    /// θ(t) = ∂f(0,t)/∂t + a*f(0,t) + σ²*(1-exp(-2at))/(2a)
    ///
    /// where f(0,t) is the instantaneous forward rate.
    #[allow(clippy::similar_names)]
    fn theta_at(&self, t: f64, zero_rates: &dyn Fn(f64) -> f64) -> f64 {
        let a = self.mean_reversion;
        let sigma = self.volatility;
        let epsilon = 0.0001;

        // For a flat curve, theta simplifies considerably
        // For general curve, use numerical approximation

        // Calculate instantaneous forward rate f(0,t)
        // f(0,t) = r(t) + t * dr/dt (for continuously compounded zero rates)
        let t_safe = t.max(epsilon);
        let r_t = zero_rates(t_safe);
        let r_t_eps = zero_rates(t_safe + epsilon);

        // Approximate forward rate as r(t) + t * dr/dt
        let dr_dt = (r_t_eps - r_t) / epsilon;
        let forward = r_t + t_safe * dr_dt;

        // For df/dt, use central difference where possible
        let forward_plus = {
            let r_plus = zero_rates(t_safe + epsilon);
            let r_plus_eps = zero_rates(t_safe + 2.0 * epsilon);
            let dr_plus = (r_plus_eps - r_plus) / epsilon;
            r_plus + (t_safe + epsilon) * dr_plus
        };

        let forward_minus = if t_safe > epsilon {
            let r_minus = zero_rates(t_safe - epsilon);
            let r_minus_eps = zero_rates(t_safe);
            let dr_minus = (r_minus_eps - r_minus) / epsilon;
            r_minus + (t_safe - epsilon) * dr_minus
        } else {
            forward // Use same value at boundary
        };

        let df_dt = (forward_plus - forward_minus) / (2.0 * epsilon);

        // θ(t) = df/dt + a*f + σ²*(1-exp(-2at))/(2a)
        df_dt + a * forward + sigma * sigma * (1.0 - (-2.0 * a * t_safe).exp()) / (2.0 * a)
    }
}

impl ShortRateModel for HullWhite {
    fn build_tree(
        &self,
        zero_rates: &dyn Fn(f64) -> f64,
        maturity: f64,
        steps: usize,
    ) -> BinomialTree {
        let dt = maturity / steps as f64;
        let sqrt_dt = dt.sqrt();
        let a = self.mean_reversion;
        let sigma = self.volatility;

        let mut tree = BinomialTree::new(steps, dt);

        // Initial rate from the curve
        let initial_rate = zero_rates(dt);
        tree.set_rate(0, 0, initial_rate);

        // Rate step size
        let dr = sigma * sqrt_dt;

        // Build tree forward
        for i in 0..steps {
            let t = i as f64 * dt;

            // Calculate drift adjustment to fit the curve
            let theta = self.theta_at(t, zero_rates);

            for j in 0..=i {
                let r = tree.rate_at(i, j);

                // Drift: θ(t) - a*r
                let drift = theta - a * r;

                // Up and down rates
                let r_up = r + drift * dt + dr;
                let r_down = r + drift * dt - dr;

                // Set rates at next step
                if j < i + 1 {
                    tree.set_rate(i + 1, j + 1, r_up);
                }
                tree.set_rate(i + 1, j, r_down);

                // Risk-neutral probabilities (approximately 0.5 for symmetric tree)
                let p_up = 0.5;
                let p_down = 0.5;
                tree.set_probabilities(i, j, p_up, p_down);
            }
        }

        tree
    }

    fn volatility(&self, _t: f64) -> f64 {
        // Constant volatility in standard Hull-White
        self.volatility
    }

    fn mean_reversion(&self) -> f64 {
        self.mean_reversion
    }

    fn name(&self) -> &'static str {
        "Hull-White"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hull_white_creation() {
        let model = HullWhite::new(0.03, 0.01);
        assert!((model.mean_reversion - 0.03).abs() < 1e-10);
        assert!((model.volatility - 0.01).abs() < 1e-10);
    }

    #[test]
    fn test_from_swaption_vol() {
        let model = HullWhite::from_swaption_vol(0.0070, 0.03);
        assert!((model.mean_reversion - 0.03).abs() < 1e-10);
        assert!((model.volatility - 0.0070).abs() < 1e-10);
    }

    #[test]
    fn test_default_params() {
        let model = HullWhite::default_params();
        assert!((model.mean_reversion - 0.03).abs() < 1e-10);
        assert!((model.volatility - 0.01).abs() < 1e-10);
    }

    #[test]
    fn test_b_factor() {
        let model = HullWhite::new(0.05, 0.01);

        // B(0, 1) = (1 - exp(-0.05)) / 0.05 ≈ 0.975
        let b = model.b_factor(0.0, 1.0);
        assert!((b - 0.975).abs() < 0.01);

        // B(t, t) = 0
        assert!(model.b_factor(1.0, 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_build_tree() {
        let model = HullWhite::new(0.03, 0.01);

        // Flat 5% curve
        let flat_curve = |_t: f64| 0.05;

        let tree = model.build_tree(&flat_curve, 1.0, 4);

        assert_eq!(tree.steps, 4);
        assert!((tree.dt - 0.25).abs() < 1e-10);

        // Initial rate should be close to 5%
        let r0 = tree.rate_at(0, 0);
        assert!((r0 - 0.05).abs() < 0.01);

        // Check that tree spans reasonable rates
        let r_final_up = tree.rate_at(4, 4);
        let r_final_down = tree.rate_at(4, 0);
        assert!(r_final_up > r_final_down);
    }

    #[test]
    fn test_build_tree_upward_sloping() {
        let model = HullWhite::new(0.03, 0.01);

        // Upward sloping curve
        let upward_curve = |t: f64| 0.03 + 0.01 * t;

        let tree = model.build_tree(&upward_curve, 2.0, 8);

        // With upward sloping curve, rates should generally increase
        let initial = tree.rate_at(0, 0);
        let mid_avg = (tree.rate_at(4, 0) + tree.rate_at(4, 4)) / 2.0;

        // Average rate should be higher later in upward sloping curve
        assert!(mid_avg > initial - 0.02);
    }

    #[test]
    fn test_volatility_method() {
        let model = HullWhite::new(0.03, 0.015);

        assert!((model.volatility(0.0) - 0.015).abs() < 1e-10);
        assert!((model.volatility(5.0) - 0.015).abs() < 1e-10);
    }

    #[test]
    fn test_mean_reversion_method() {
        let model = HullWhite::new(0.05, 0.01);
        assert!((model.mean_reversion() - 0.05).abs() < 1e-10);
    }

    #[test]
    fn test_name() {
        let model = HullWhite::new(0.03, 0.01);
        assert_eq!(model.name(), "Hull-White");
    }

    #[test]
    fn test_tree_probabilities() {
        let model = HullWhite::new(0.03, 0.01);
        let flat_curve = |_t: f64| 0.05;
        let tree = model.build_tree(&flat_curve, 1.0, 4);

        // Check probabilities sum to 1
        let p_up = tree.prob_up(0, 0);
        let p_down = tree.prob_down(0, 0);
        assert!((p_up + p_down - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_tree_pricing_zero_coupon() {
        let model = HullWhite::new(0.03, 0.01);
        let flat_5pct = |_t: f64| 0.05;

        let tree = model.build_tree(&flat_5pct, 1.0, 10);

        // Price a zero-coupon bond with face value 100
        let pv = tree.backward_induction_simple(100.0, 0.0);

        // Should be approximately 100 * exp(-0.05) ≈ 95.12
        // Allow some tolerance for tree discretization
        assert!((pv - 95.12).abs() < 1.0);
    }
}
