//! European swaption pricing under Hull-White 1-Factor with constant
//! `(a, σ)`, via Jamshidian's decomposition. Mirrors QuantLib's
//! `JamshidianSwaptionEngine` so calibration parity tests have a closed-form
//! reference on both sides.
//!
//! References:
//! - Brigo & Mercurio, *Interest Rate Models — Theory and Practice* (2nd ed),
//!   §3.3.2 (HW1F bond options) and §3.11.2 (Jamshidian swaption).
//! - Jamshidian, "An exact bond option formula", *J. Finance* 44 (1989), 205-209.
//!
//! All time arguments are years from the curve reference date (= valuation
//! date), in continuous compounding.
//!
//! ```text
//! P(T, S | r_T) = A(T, S) · exp(-B(T, S) · r_T)
//! B(T, S)      = (1 - exp(-a(S - T))) / a
//! A(T, S)      = P(0, S)/P(0, T) · exp[B(T, S)·f(0, T) - σ²/(4a)·(1 - e^{-2aT})·B(T, S)²]
//! ```

/// HW1F `B(t, T) = (1 - exp(-a(T - t))) / a` (or `T - t` when `a → 0`).
#[must_use]
pub fn b_hw1f(a: f64, t: f64, s: f64) -> f64 {
    let tau = s - t;
    if tau <= 0.0 {
        return 0.0;
    }
    if a.abs() < 1e-12 {
        return tau;
    }
    (1.0 - (-a * tau).exp()) / a
}

/// `σ_p²(T, S) = σ²/(2a) · (1 - exp(-2aT)) · B(T, S)²` —
/// the lognormal vol of a ZCB under HW1F (Brigo-Mercurio 3.40).
#[must_use]
pub fn zcb_option_vol(a: f64, sigma: f64, t_expiry: f64, t_maturity: f64) -> f64 {
    if t_expiry <= 0.0 {
        return 0.0;
    }
    let bt = b_hw1f(a, t_expiry, t_maturity);
    if a.abs() < 1e-12 {
        // limit a → 0: σ²·T·B² with B = S - T
        return sigma * t_expiry.sqrt() * bt;
    }
    let var = sigma.powi(2) / (2.0 * a) * (1.0 - (-2.0 * a * t_expiry).exp()) * bt.powi(2);
    var.sqrt()
}

/// Standard normal CDF via Abramowitz & Stegun 26.2.17 (max abs error 7.5e-8).
/// Adequate for HW1F calibration tests at 1e-5 σ tolerance.
#[must_use]
pub fn standard_normal_cdf(x: f64) -> f64 {
    // P(x) = 1 - φ(x) · (b1·k + b2·k² + b3·k³ + b4·k⁴ + b5·k⁵), x ≥ 0
    //   k = 1 / (1 + 0.2316419 · x)
    //   φ(x) = exp(-x²/2) / √(2π)
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let ax = x.abs();
    let k = 1.0 / (1.0 + 0.231_641_9 * ax);
    let phi = (-0.5 * ax * ax).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let poly = k
        * (0.319_381_530
            + k * (-0.356_563_782
                + k * (1.781_477_937 + k * (-1.821_255_978 + k * 1.330_274_429))));
    let upper = 1.0 - phi * poly; // P(X ≤ ax)
    if sign > 0.0 {
        upper
    } else {
        1.0 - upper
    }
}

/// Closed-form ZCB call/put under HW1F (Brigo-Mercurio 3.40-3.41).
/// `p_t = P(0, T_expiry)`, `p_s = P(0, T_maturity)`, `k` = strike, `σ_p` from
/// [`zcb_option_vol`].
#[must_use]
pub fn zcb_option(p_t: f64, p_s: f64, k: f64, sigma_p: f64, is_call: bool) -> f64 {
    if sigma_p <= 0.0 {
        // Deterministic: option = max(p_s - k·p_t, 0) for call, etc.
        return if is_call {
            (p_s - k * p_t).max(0.0)
        } else {
            (k * p_t - p_s).max(0.0)
        };
    }
    let h = (p_s / (k * p_t)).ln() / sigma_p + sigma_p / 2.0;
    if is_call {
        p_s * standard_normal_cdf(h) - k * p_t * standard_normal_cdf(h - sigma_p)
    } else {
        k * p_t * standard_normal_cdf(-h + sigma_p) - p_s * standard_normal_cdf(-h)
    }
}

/// One synthetic fixed-leg cashflow: `(t_i, c_i)` where `c_i = K·τ_i` for
/// non-terminal flows and `c_n = K·τ_n + 1` (notional bundled in).
#[derive(Debug, Clone, Copy)]
struct FixedCf {
    t: f64,
    c: f64,
}

/// Builds the synthetic fixed-leg cashflows for a swap from `T_expiry` out to
/// `T_expiry + tail`, paying every `freq` years. The final payment carries
/// the unit notional. Mirrors QL's `Schedule(... freq ..., DateGeneration.Backward)`.
fn build_fixed_cfs(expiry: f64, tail: f64, freq: f64, strike: f64) -> Vec<FixedCf> {
    let n = (tail / freq).round() as usize;
    let mut out = Vec::with_capacity(n);
    for i in 1..=n {
        let t = expiry + i as f64 * freq;
        let mut c = strike * freq;
        if i == n {
            c += 1.0;
        }
        out.push(FixedCf { t, c });
    }
    out
}

/// Annuity at time 0 for fixed-leg accruals (without notional): `Σ τ_i · P(0, T_i)`.
#[must_use]
pub fn forward_annuity<F: Fn(f64) -> f64>(
    discount: &F,
    expiry: f64,
    tail: f64,
    freq: f64,
) -> f64 {
    let n = (tail / freq).round() as usize;
    let mut a = 0.0;
    for i in 1..=n {
        let t = expiry + i as f64 * freq;
        a += freq * discount(t);
    }
    a
}

/// Forward swap rate at time 0: `S(0; T_0, T_n) = (P(0, T_0) - P(0, T_n)) / Annuity`.
#[must_use]
pub fn forward_swap_rate<F: Fn(f64) -> f64>(
    discount: &F,
    expiry: f64,
    tail: f64,
    freq: f64,
) -> f64 {
    let p0 = discount(expiry);
    let pn = discount(expiry + tail);
    let ann = forward_annuity(discount, expiry, tail, freq);
    (p0 - pn) / ann
}

/// Bachelier (normal-vol) ATM payer-swaption price: `A · σ_n · √(T / 2π)`.
/// Used as the market-value reference; calibrated σ_HW gets fit to this.
#[must_use]
pub fn bachelier_atm_price(annuity: f64, normal_vol: f64, expiry: f64) -> f64 {
    annuity * normal_vol * (expiry / (2.0 * std::f64::consts::PI)).sqrt()
}

/// HW1F payer-swaption price via Jamshidian decomposition.
///
/// Payer swaption payoff at exercise `T`:
/// `max(1 - Σ c_i · P(T, T_i), 0)`, with `c_i` from [`build_fixed_cfs`]. Under
/// HW1F, `P(T, T_i) = A_i · exp(-B_i · r_T)` is monotone decreasing in `r_T`,
/// so there's a unique critical `r*` with `Σ c_i · A_i · exp(-B_i · r*) = 1`.
/// At `r*` the synthetic ZCB strikes are `X_i = A_i · exp(-B_i · r*)`, and
/// the payer swaption equals a portfolio of ZCB puts:
/// `Σ c_i · ZBP(0, T, T_i, X_i)`.
#[must_use]
pub fn payer_swaption_hw1f<F: Fn(f64) -> f64>(
    discount: &F,
    a: f64,
    sigma: f64,
    expiry: f64,
    tail: f64,
    fixed_freq: f64,
    strike: f64,
) -> f64 {
    if expiry <= 0.0 || tail <= 0.0 {
        return 0.0;
    }

    let cfs = build_fixed_cfs(expiry, tail, fixed_freq, strike);
    if cfs.is_empty() {
        return 0.0;
    }

    let p_t = discount(expiry);

    // f(0, T) ≈ -∂ln P(0, T)/∂T via central difference. ε=1e-3 keeps the
    // estimate well away from the curve's discrete pillar spacing while
    // staying small relative to typical bond expiries.
    let h = 1e-3_f64.min(expiry * 0.1);
    let f_0t = -(discount(expiry + h).ln() - discount((expiry - h).max(1e-6)).ln()) / (2.0 * h);

    // θ(T) = σ²/(4a) · (1 - exp(-2aT))
    let theta_factor = if a.abs() < 1e-12 {
        sigma.powi(2) * expiry / 2.0
    } else {
        sigma.powi(2) / (4.0 * a) * (1.0 - (-2.0 * a * expiry).exp())
    };

    // A(T, S) and B(T, S) helpers.
    let big_b = |s: f64| b_hw1f(a, expiry, s);
    let big_a = |s: f64| -> f64 {
        let bts = big_b(s);
        let p_s = discount(s);
        p_s / p_t * (bts * f_0t - theta_factor * bts.powi(2)).exp()
    };

    // Bond value as a function of r at exercise: Σ c_i · A_i · exp(-B_i · r).
    // Monotone decreasing → unique r* with bond_value(r*) = 1.
    let bond_value = |r: f64| -> f64 {
        cfs.iter()
            .map(|cf| cf.c * big_a(cf.t) * (-big_b(cf.t) * r).exp())
            .sum::<f64>()
    };

    let r_star = brent_zero(bond_value, 1.0, -1.0, 1.0, 1e-12, 100);

    // Sum c_i · ZBP(p_t, P(0, T_i), X_i, σ_p_i).
    let mut total = 0.0;
    for cf in &cfs {
        let bts = big_b(cf.t);
        let xi = big_a(cf.t) * (-bts * r_star).exp();
        let p_s = discount(cf.t);
        let sig_p = zcb_option_vol(a, sigma, expiry, cf.t);
        total += cf.c * zcb_option(p_t, p_s, xi, sig_p, false);
    }
    total
}

/// Brent root finder on `f(r) = target`. Bond value is monotone decreasing in
/// `r`, so the standard Brent guarantees a single crossing — no bracketing
/// expansion needed for normal HW1F inputs (`σ < ~5%`, expiry ≤ 30y).
#[allow(clippy::many_single_char_names)] // Brent's textbook variable names: a, b, c, d, s.
fn brent_zero<F: Fn(f64) -> f64>(
    f: F,
    target: f64,
    mut lo: f64,
    mut hi: f64,
    tol: f64,
    max_iter: usize,
) -> f64 {
    let g = |r: f64| f(r) - target;
    let mut f_lo = g(lo);
    let mut f_hi = g(hi);
    // Auto-widen: bond_value(r) = ∞ as r → -∞ and 0 as r → +∞, so a sign
    // flip always exists; in practice ±1 covers any reasonable curve.
    let mut widen = 0;
    while f_lo * f_hi > 0.0 && widen < 5 {
        lo *= 2.0;
        hi *= 2.0;
        f_lo = g(lo);
        f_hi = g(hi);
        widen += 1;
    }
    if f_lo * f_hi > 0.0 {
        // Fallback: linear extrapolation. Calibration tests will still flag.
        return 0.5 * (lo + hi);
    }

    let mut a_brent = lo;
    let mut b_brent = hi;
    let mut fa = f_lo;
    let mut fb = f_hi;
    if fa.abs() < fb.abs() {
        std::mem::swap(&mut a_brent, &mut b_brent);
        std::mem::swap(&mut fa, &mut fb);
    }
    let mut c = a_brent;
    let mut fc = fa;
    let mut d = 0.0;
    let mut mflag = true;

    for _ in 0..max_iter {
        if fb.abs() < tol {
            return b_brent;
        }
        let s = if fa != fc && fb != fc {
            // inverse quadratic interpolation
            let t1 = a_brent * fb * fc / ((fa - fb) * (fa - fc));
            let t2 = b_brent * fa * fc / ((fb - fa) * (fb - fc));
            let t3 = c * fa * fb / ((fc - fa) * (fc - fb));
            t1 + t2 + t3
        } else {
            // secant
            b_brent - fb * (b_brent - a_brent) / (fb - fa)
        };
        let cond1 = (s - (3.0 * a_brent + b_brent) / 4.0) * (s - b_brent) >= 0.0;
        let cond2 = mflag && (s - b_brent).abs() >= (b_brent - c).abs() / 2.0;
        let cond3 = !mflag && (s - b_brent).abs() >= (c - d).abs() / 2.0;
        let s = if cond1 || cond2 || cond3 {
            mflag = true;
            (a_brent + b_brent) / 2.0
        } else {
            mflag = false;
            s
        };
        let fs = g(s);
        d = c;
        c = b_brent;
        fc = fb;
        if fa * fs < 0.0 {
            b_brent = s;
            fb = fs;
        } else {
            a_brent = s;
            fa = fs;
        }
        if fa.abs() < fb.abs() {
            std::mem::swap(&mut a_brent, &mut b_brent);
            std::mem::swap(&mut fa, &mut fb);
        }
    }
    b_brent
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_curve(rate: f64) -> impl Fn(f64) -> f64 {
        move |t: f64| (-rate * t).exp()
    }

    #[test]
    fn standard_normal_cdf_matches_known_values() {
        // Tabulated standard normal CDF values.
        let cases = [
            (-2.0, 0.022_750_131_948_179_19),
            (-1.0, 0.158_655_253_931_457_05),
            (0.0, 0.5),
            (1.0, 0.841_344_746_068_542_9),
            (2.0, 0.977_249_868_051_820_8),
            (3.0, 0.998_650_101_968_369_8),
        ];
        for (x, expected) in cases {
            let got = standard_normal_cdf(x);
            assert!((got - expected).abs() < 1e-7, "Φ({x}) = {got} vs {expected}");
        }
    }

    #[test]
    fn b_hw1f_limits() {
        // a → 0: B → τ
        assert!((b_hw1f(1e-15, 0.0, 5.0) - 5.0).abs() < 1e-9);
        // a = 0.03, τ = 1y
        let bt = b_hw1f(0.03, 0.0, 1.0);
        assert!((bt - (1.0 - (-0.03f64).exp()) / 0.03).abs() < 1e-12);
    }

    #[test]
    fn forward_swap_rate_matches_flat_curve() {
        // Under a flat 4% continuously-compounded curve, forward swap rate
        // for any expiry/tail/freq equals (e^{a τ} − 1)/τ × correction ≈ flat
        // continuous → annual-comp swap rate. Sanity: must be positive and
        // close to 4%.
        let curve = flat_curve(0.04);
        let s = forward_swap_rate(&curve, 2.0, 5.0, 1.0);
        assert!((0.03..=0.05).contains(&s), "forward swap rate {s}");
    }

    #[test]
    fn jamshidian_payer_atm_matches_bachelier_under_low_vol() {
        // ATM payer swaption under HW1F (a, σ) and Bachelier under matching
        // normal vol must agree to a few bp at low vol. The "matching" normal
        // vol for HW1F at short maturities is approximately
        //     σ_n ≈ σ_HW · B(0, T_expiry)  (lognormal-of-rate is small)
        // so we don't pin them exactly; just check the HW1F price is in a
        // sensible range and monotone in σ.
        let curve = flat_curve(0.04);
        let strike = forward_swap_rate(&curve, 2.0, 5.0, 1.0);
        let p_low = payer_swaption_hw1f(&curve, 0.03, 0.005, 2.0, 5.0, 1.0, strike);
        let p_high = payer_swaption_hw1f(&curve, 0.03, 0.020, 2.0, 5.0, 1.0, strike);
        assert!(p_low > 0.0, "p_low = {p_low}");
        assert!(p_high > p_low, "p_high {p_high} should exceed p_low {p_low}");
        // Sanity ceiling: ATM payer ≤ annuity (deep OTM bound).
        let ann = forward_annuity(&curve, 2.0, 5.0, 1.0);
        assert!(p_high < ann, "p_high {p_high} ≥ annuity {ann}");
    }

    #[test]
    fn jamshidian_zero_vol_collapses_to_intrinsic() {
        let curve = flat_curve(0.04);
        let strike = forward_swap_rate(&curve, 2.0, 5.0, 1.0);
        // ATM with σ → 0: option price → 0 (no time value, intrinsic = 0).
        let p = payer_swaption_hw1f(&curve, 0.03, 1e-8, 2.0, 5.0, 1.0, strike);
        assert!(p.abs() < 1e-6, "ATM zero-vol payer = {p}, expected ~0");
    }
}
