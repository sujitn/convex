//! Closed-form European swaption pricing under HW1F with constant `(a, σ)`,
//! via Jamshidian's decomposition. See Brigo & Mercurio §3.3.2 / §3.11.2.

use convex_math::solvers::{brent, SolverConfig};
use convex_math::stats::standard_normal_cdf;

/// HW1F `B(t, T) = (1 - exp(-a(T - t))) / a`, with the `a → 0` limit.
pub(crate) fn b_hw1f(a: f64, t: f64, s: f64) -> f64 {
    let tau = s - t;
    if a.abs() < 1e-12 {
        return tau;
    }
    (1.0 - (-a * tau).exp()) / a
}

/// Lognormal vol of `P(T, S)` under HW1F (Brigo-Mercurio 3.40).
pub(crate) fn zcb_option_vol(a: f64, sigma: f64, t_expiry: f64, t_maturity: f64) -> f64 {
    let bt = b_hw1f(a, t_expiry, t_maturity);
    if a.abs() < 1e-12 {
        return sigma * t_expiry.sqrt() * bt;
    }
    let var = sigma.powi(2) / (2.0 * a) * (1.0 - (-2.0 * a * t_expiry).exp()) * bt.powi(2);
    var.sqrt()
}

/// Closed-form ZCB call/put under HW1F (Brigo-Mercurio 3.40-3.41).
pub(crate) fn zcb_option(p_t: f64, p_s: f64, k: f64, sigma_p: f64, is_call: bool) -> f64 {
    let h = (p_s / (k * p_t)).ln() / sigma_p + sigma_p / 2.0;
    if is_call {
        p_s * standard_normal_cdf(h) - k * p_t * standard_normal_cdf(h - sigma_p)
    } else {
        k * p_t * standard_normal_cdf(-h + sigma_p) - p_s * standard_normal_cdf(-h)
    }
}

/// Annuity at time 0 for a fixed leg of `n = round(tail / freq)` accruals
/// from `expiry` to `expiry + tail`: `Σ τ_i · P(0, T_i)`.
pub fn forward_annuity<F: Fn(f64) -> f64>(
    discount: &F,
    expiry: f64,
    tail: f64,
    freq: f64,
) -> f64 {
    let n = (tail / freq).round() as usize;
    (1..=n).map(|i| freq * discount(expiry + i as f64 * freq)).sum()
}

/// Forward swap rate at time 0: `(P(0, T_0) - P(0, T_n)) / Annuity`.
pub fn forward_swap_rate<F: Fn(f64) -> f64>(
    discount: &F,
    expiry: f64,
    tail: f64,
    freq: f64,
) -> f64 {
    (discount(expiry) - discount(expiry + tail)) / forward_annuity(discount, expiry, tail, freq)
}

/// Bachelier ATM payer-swaption price: `A · σ_n · √(T / 2π)`. Used as the
/// market-value reference when calibrating against ATM normal-vol quotes.
pub fn bachelier_atm_price(annuity: f64, normal_vol: f64, expiry: f64) -> f64 {
    annuity * normal_vol * (expiry / (2.0 * std::f64::consts::PI)).sqrt()
}

/// HW1F payer-swaption price via Jamshidian's decomposition with annual
/// fixed-leg accruals (matching the QL helpers we calibrate against).
///
/// Caller passes the time-0 instantaneous forward rate `f_0t` at the swaption
/// expiry — typically `curve.instantaneous_forward(expiry)` — so the pricer
/// stays free of curve-interpolation arithmetic.
pub fn payer_swaption_hw1f<F: Fn(f64) -> f64>(
    discount: &F,
    a: f64,
    sigma: f64,
    f_0t: f64,
    expiry: f64,
    tail: f64,
    strike: f64,
) -> f64 {
    let n = tail.round() as usize;
    let p_t = discount(expiry);

    let theta_factor = if a.abs() < 1e-12 {
        sigma.powi(2) * expiry / 2.0
    } else {
        sigma.powi(2) / (4.0 * a) * (1.0 - (-2.0 * a * expiry).exp())
    };

    let big_b = |s: f64| b_hw1f(a, expiry, s);
    let big_a = |s: f64| {
        let bts = big_b(s);
        discount(s) / p_t * (bts * f_0t - theta_factor * bts.powi(2)).exp()
    };

    // Synthetic fixed-leg cashflows: τ_i = 1, c_i = K (final flow adds the
    // unit notional). Bond_value(r) = Σ c_i · A(T, T_i) · exp(-B(T, T_i) · r),
    // monotone decreasing → a single root r* with bond_value(r*) = 1.
    let cs: Vec<(f64, f64)> = (1..=n)
        .map(|i| {
            let ti = expiry + i as f64;
            let ci = if i == n { strike + 1.0 } else { strike };
            (ti, ci)
        })
        .collect();

    let bond_value = |r: f64| -> f64 {
        cs.iter().map(|(ti, ci)| ci * big_a(*ti) * (-big_b(*ti) * r).exp()).sum()
    };

    let cfg = SolverConfig::new(1e-12, 100);
    let r_star = brent(|r| bond_value(r) - 1.0, -1.0, 1.0, &cfg)
        .map_or(0.0, |res| res.root);

    cs.iter()
        .map(|(ti, ci)| {
            let xi = big_a(*ti) * (-big_b(*ti) * r_star).exp();
            ci * zcb_option(p_t, discount(*ti), xi, zcb_option_vol(a, sigma, expiry, *ti), false)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_curve(rate: f64) -> impl Fn(f64) -> f64 {
        move |t: f64| (-rate * t).exp()
    }

    #[test]
    fn b_hw1f_limits() {
        assert!((b_hw1f(1e-15, 0.0, 5.0) - 5.0).abs() < 1e-9);
        let bt = b_hw1f(0.03, 0.0, 1.0);
        assert!((bt - (1.0 - (-0.03f64).exp()) / 0.03).abs() < 1e-12);
    }

    #[test]
    fn forward_swap_rate_close_to_curve_rate() {
        let curve = flat_curve(0.04);
        let s = forward_swap_rate(&curve, 2.0, 5.0, 1.0);
        assert!((0.03..=0.05).contains(&s));
    }

    #[test]
    fn payer_swaption_monotone_in_sigma() {
        let curve = flat_curve(0.04);
        let strike = forward_swap_rate(&curve, 2.0, 5.0, 1.0);
        let p_low = payer_swaption_hw1f(&curve, 0.03, 0.005, 0.04, 2.0, 5.0, strike);
        let p_high = payer_swaption_hw1f(&curve, 0.03, 0.020, 0.04, 2.0, 5.0, strike);
        assert!(p_low > 0.0 && p_high > p_low);
        assert!(p_high < forward_annuity(&curve, 2.0, 5.0, 1.0));
    }
}
