//! HW1F single-σ calibration against an ATM co-terminal swaption strip.
//!
//! Mean reversion `a` is fixed exogenously (industry standard for callable
//! corporate OAS — Bloomberg OAS1, Strata, QL `Examples/CallableBonds`); this
//! solver fits the single free parameter `σ` against `N` ATM payer-swaption
//! helpers using the QuantLib-equivalent `RelativePriceError` metric:
//!
//! ```text
//! σ̂ = argmin_σ Σ_i [(model_i(σ) − market_i) / market_i]²
//! ```
//!
//! Helper market value: Bachelier ATM price under the supplied normal vol.
//! Helper model value:  closed-form HW1F payer swaption via Jamshidian's
//! decomposition (see [`convex_bonds::options::swaption_hw1f`]).
//!
//! 1D objective ⇒ golden-section search on σ ∈ [σ_min, σ_max] is robust and
//! deterministic; a Brent minimizer would converge in fewer iterations but
//! the dependency is not justified for a unimodal, monotone-derivative
//! function evaluated <100× per calibration.

use convex_bonds::options::swaption_hw1f::{
    bachelier_atm_price, forward_annuity, forward_swap_rate, payer_swaption_hw1f,
};
use convex_curves::{Compounding, RateCurveDyn};

use crate::error::{AnalyticsError, AnalyticsResult};

/// Single ATM co-terminal swaption helper used for calibration.
#[derive(Debug, Clone, Copy)]
pub struct CoterminalSwaptionHelper {
    /// Years from valuation to swaption exercise (= bond call date).
    pub expiry_years: f64,
    /// Tail tenor in years; co-terminal ⇒ `expiry + tail = bond residual maturity`.
    pub tail_years: f64,
    /// Fixed-leg accrual basis in years (1.0 = annual, 0.5 = semi-annual).
    pub fixed_freq_years: f64,
    /// ATM normal vol in basis points (e.g. 95.0 ⇒ 95 bp).
    pub atm_normal_vol_bps: f64,
}

/// Result of a HW1F single-σ calibration: the calibrated `(a, σ)`, plus the
/// price-space RMSE of the residual at the optimum (small ⇒ good fit).
#[derive(Debug, Clone, Copy)]
pub struct Hw1fCalibration {
    /// Mean reversion (held fixed at the input value; echoed for traceability).
    pub a: f64,
    /// Calibrated short-rate volatility (decimal — e.g. 0.01 = 1%).
    pub sigma: f64,
    /// Price-space RMSE of the residual at the optimum (`sqrt(SSR / N)`).
    pub rmse_price: f64,
}

/// Calibrate HW1F `σ` against `helpers`, holding `a` fixed.
///
/// Discount-factor accessor `discount(t)` must give `P(0, t)` continuously
/// compounded (matches QuantLib's `YieldTermStructure::discount`). The
/// SOFR OIS curve in `reconciliation/curves.json` already meets this.
///
/// Returns `Hw1fCalibration { a, sigma, rmse_price }` where `sigma` is the
/// optimum and `rmse_price` is `sqrt(SSR / N)` in price space.
///
/// # Errors
///
/// - `helpers` is empty.
/// - All helper market values are zero (degenerate strip).
pub fn calibrate_hw1f_sigma(
    curve: &dyn RateCurveDyn,
    a: f64,
    helpers: &[CoterminalSwaptionHelper],
) -> AnalyticsResult<Hw1fCalibration> {
    if helpers.is_empty() {
        return Err(AnalyticsError::InvalidInput(
            "calibrate_hw1f_sigma: empty helper strip".into(),
        ));
    }

    // Reusable closure: P(0, t) under the supplied curve. We use the curve's
    // continuous-compounded zero rate and exponentiate ourselves so the
    // discount call signature matches what swaption_hw1f expects.
    let discount = |t: f64| -> f64 {
        if t <= 0.0 {
            return 1.0;
        }
        let z = curve
            .zero_rate(t, Compounding::Continuous)
            .unwrap_or(0.05);
        (-z * t).exp()
    };

    // Pre-compute market values + ATM strikes once. These don't depend on σ.
    let mut market_values = Vec::with_capacity(helpers.len());
    let mut strikes = Vec::with_capacity(helpers.len());
    for h in helpers {
        let strike =
            forward_swap_rate(&discount, h.expiry_years, h.tail_years, h.fixed_freq_years);
        let ann = forward_annuity(&discount, h.expiry_years, h.tail_years, h.fixed_freq_years);
        let market = bachelier_atm_price(ann, h.atm_normal_vol_bps / 1e4, h.expiry_years);
        if market <= 0.0 {
            return Err(AnalyticsError::InvalidInput(format!(
                "calibrate_hw1f_sigma: helper expiry={}y tail={}y has non-positive market value {}",
                h.expiry_years, h.tail_years, market
            )));
        }
        market_values.push(market);
        strikes.push(strike);
    }

    // Sum-of-squared-relative-residuals as a function of σ.
    let ssr = |sigma: f64| -> f64 {
        let mut s = 0.0;
        for (i, h) in helpers.iter().enumerate() {
            let model = payer_swaption_hw1f(
                &discount,
                a,
                sigma,
                h.expiry_years,
                h.tail_years,
                h.fixed_freq_years,
                strikes[i],
            );
            let res = (model - market_values[i]) / market_values[i];
            s += res * res;
        }
        s
    };

    // Golden-section search on [σ_lo, σ_hi]. The HW1F-payer-swaption price is
    // monotone increasing and smooth in σ, so the SSR has a unique interior
    // minimum (or sits at the boundary if the strip is unreachable). 1e-5
    // brackets gives ≥1e-5 σ precision in ~30 iterations.
    let sigma_lo = 1e-5;
    let sigma_hi = 0.10; // 10% — well above any plausible HW1F σ
    let sigma_opt = golden_section(ssr, sigma_lo, sigma_hi, 1e-9, 200);

    // Compute RMSE in price space at the optimum for diagnostics.
    let mut sse_price = 0.0;
    for (i, h) in helpers.iter().enumerate() {
        let model = payer_swaption_hw1f(
            &discount,
            a,
            sigma_opt,
            h.expiry_years,
            h.tail_years,
            h.fixed_freq_years,
            strikes[i],
        );
        let diff = model - market_values[i];
        sse_price += diff * diff;
    }
    let rmse_price = (sse_price / helpers.len() as f64).sqrt();

    Ok(Hw1fCalibration {
        a,
        sigma: sigma_opt,
        rmse_price,
    })
}

/// Golden-section minimiser on `[a, b]`. Robust for unimodal, smooth
/// objectives; deterministic and dependency-free. Returns the argmin.
fn golden_section<F: Fn(f64) -> f64>(f: F, mut a: f64, mut b: f64, tol: f64, max_iter: usize) -> f64 {
    let phi = (5.0_f64.sqrt() - 1.0) / 2.0; // ≈ 0.618
    let mut c = b - phi * (b - a);
    let mut d = a + phi * (b - a);
    let mut fc = f(c);
    let mut fd = f(d);
    for _ in 0..max_iter {
        if (b - a).abs() < tol {
            break;
        }
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - phi * (b - a);
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + phi * (b - a);
            fd = f(d);
        }
    }
    0.5 * (a + b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::types::Date;
    use convex_curves::curves::DiscountCurveBuilder;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn flat_curve(rate: f64) -> impl RateCurveDyn {
        DiscountCurveBuilder::new(date(2025, 12, 31))
            .add_zero_rate(0.0001, rate)
            .add_zero_rate(0.5, rate)
            .add_zero_rate(1.0, rate)
            .add_zero_rate(2.0, rate)
            .add_zero_rate(3.0, rate)
            .add_zero_rate(5.0, rate)
            .add_zero_rate(7.0, rate)
            .add_zero_rate(10.0, rate)
            .with_extrapolation()
            .build()
            .unwrap()
    }

    #[test]
    fn calibrates_to_a_self_consistent_strip() {
        // Generate a strip with a known HW1F (a, σ), feeding the resulting
        // bachelier-implied vols back as helper inputs. A self-consistent
        // calibration should recover σ to ~1e-5.
        let curve = flat_curve(0.04);
        let a_true = 0.03;
        let sigma_true = 0.01;

        let mut helpers = Vec::new();
        for (e, tail) in [(1.0_f64, 4.0_f64), (2.0, 3.0), (3.0, 2.0)] {
            // Synthesize "market" Bachelier vol such that the Bachelier price
            // matches the HW1F price at (a_true, sigma_true). We solve for
            // σ_n that gives bachelier_atm_price == HW1F price.
            //   σ_n = HW1F_price / (annuity · √(T / 2π))
            let discount = |t: f64| {
                let z = curve.zero_rate(t, Compounding::Continuous).unwrap_or(0.0);
                (-z * t).exp()
            };
            let strike = forward_swap_rate(&discount, e, tail, 1.0);
            let ann = forward_annuity(&discount, e, tail, 1.0);
            let hw_price = payer_swaption_hw1f(&discount, a_true, sigma_true, e, tail, 1.0, strike);
            let bach_norm_vol = hw_price / (ann * (e / (2.0 * std::f64::consts::PI)).sqrt());
            helpers.push(CoterminalSwaptionHelper {
                expiry_years: e,
                tail_years: tail,
                fixed_freq_years: 1.0,
                atm_normal_vol_bps: bach_norm_vol * 1e4,
            });
        }

        let cal = calibrate_hw1f_sigma(&curve, a_true, &helpers).unwrap();
        assert!(
            (cal.sigma - sigma_true).abs() < 1e-5,
            "calibrated σ {} vs truth {}",
            cal.sigma,
            sigma_true
        );
        assert!(
            cal.rmse_price < 1e-7,
            "self-consistent strip RMSE should be ~0, got {}",
            cal.rmse_price
        );
    }

    #[test]
    fn rejects_empty_strip() {
        let curve = flat_curve(0.04);
        assert!(calibrate_hw1f_sigma(&curve, 0.03, &[]).is_err());
    }
}
