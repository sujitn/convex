//! HW1F single-σ calibration on an ATM co-terminal swaption strip, with
//! mean reversion held fixed (industry standard for callable corporate OAS).

use convex_bonds::options::swaption_hw1f::{
    bachelier_atm_price, forward_annuity, forward_swap_rate, try_payer_swaption_hw1f,
};
use convex_curves::{Compounding, RateCurveDyn};
use convex_math::optimization::golden_section_checked;
use convex_math::portable;

use crate::error::{AnalyticsError, AnalyticsResult};

/// One ATM co-terminal swaption used for HW1F σ calibration. Fixed-leg
/// accrual is annual, matching the QL helpers we reconcile against.
#[derive(Debug, Clone, Copy)]
pub struct CoterminalSwaptionHelper {
    /// Years from valuation to swaption exercise.
    pub expiry_years: f64,
    /// Tail tenor in years; co-terminal ⇒ `expiry + tail = bond residual maturity`.
    pub tail_years: f64,
    /// ATM normal vol in basis points.
    pub atm_normal_vol_bps: f64,
}

/// Result of an HW1F single-σ calibration.
#[derive(Debug, Clone, Copy)]
pub struct Hw1fCalibration {
    /// Mean reversion (echoed from the input, not calibrated).
    pub a: f64,
    /// Calibrated short-rate volatility (decimal).
    pub sigma: f64,
}

/// Calibrate HW1F `σ` against `helpers` with `a` fixed. Minimises the sum
/// of squared relative-price residuals (matching QuantLib's
/// `BlackCalibrationHelper.RelativePriceError`).
pub fn calibrate_hw1f_sigma(
    curve: &dyn RateCurveDyn,
    a: f64,
    helpers: &[CoterminalSwaptionHelper],
) -> AnalyticsResult<Hw1fCalibration> {
    if helpers.is_empty()
        || helpers.len() > 128
        || !a.is_finite()
        || !(0.0..=10.0).contains(&a)
        || helpers.iter().any(|h| {
            !h.expiry_years.is_finite()
                || !h.tail_years.is_finite()
                || !h.atm_normal_vol_bps.is_finite()
                || h.expiry_years <= 0.0
                || h.tail_years.round() < 1.0
                || h.expiry_years + h.tail_years > 150.0
                || h.atm_normal_vol_bps <= 0.0
                || h.atm_normal_vol_bps > 100_000.0
        })
    {
        return Err(AnalyticsError::InvalidInput(
            "calibrate_hw1f_sigma: invalid or oversized helper strip".into(),
        ));
    }

    let curve_error = std::cell::RefCell::new(None);
    let discount = |t: f64| match curve.zero_rate(t, Compounding::Continuous) {
        Ok(z) if z.is_finite() => {
            let df = portable::exp(-z * t);
            if df.is_finite() && df > 0.0 {
                df
            } else {
                f64::NAN
            }
        }
        Ok(_) => f64::NAN,
        Err(error) => {
            *curve_error.borrow_mut() = Some(AnalyticsError::from(error));
            f64::NAN
        }
    };

    // Pre-compute strikes, market values, and instantaneous forwards once —
    // none depend on σ, so the SSR closure stays cheap.
    let strikes: Vec<f64> = helpers
        .iter()
        .map(|h| forward_swap_rate(&discount, h.expiry_years, h.tail_years, 1.0))
        .collect();
    let market: Vec<f64> = helpers
        .iter()
        .map(|h| {
            let ann = forward_annuity(&discount, h.expiry_years, h.tail_years, 1.0);
            bachelier_atm_price(ann, h.atm_normal_vol_bps / 1e4, h.expiry_years)
        })
        .collect();
    let f0t: Vec<f64> = helpers
        .iter()
        .map(|h| curve.instantaneous_forward(h.expiry_years))
        .collect::<Result<_, _>>()?;
    if let Some(error) = curve_error.borrow_mut().take() {
        return Err(error);
    }
    if strikes
        .iter()
        .chain(&market)
        .chain(&f0t)
        .any(|v| !v.is_finite())
        || market.iter().any(|v| *v <= 0.0)
    {
        return Err(AnalyticsError::InvalidInput(
            "HW1F strip has nonfinite values or zero market price".into(),
        ));
    }

    let ssr = |sigma: f64| -> f64 {
        helpers
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let model = try_payer_swaption_hw1f(
                    &discount,
                    a,
                    sigma,
                    f0t[i],
                    h.expiry_years,
                    h.tail_years,
                    strikes[i],
                )
                .unwrap_or(f64::NAN);
                let res = (model - market[i]) / market[i];
                res * res
            })
            .sum()
    };

    let result = golden_section_checked(ssr, 1e-5, 0.10, 1e-9, 200);
    if let Some(error) = curve_error.into_inner() {
        return Err(error);
    }
    let sigma = result?;
    Ok(Hw1fCalibration { a, sigma })
}

#[cfg(test)]
use convex_bonds::options::swaption_hw1f::payer_swaption_hw1f;

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::types::Date;
    use convex_curves::curves::DiscountCurveBuilder;

    fn flat_curve(rate: f64) -> impl RateCurveDyn {
        DiscountCurveBuilder::new(Date::from_ymd(2025, 12, 31).unwrap())
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
    fn recovers_self_consistent_sigma() {
        let curve = flat_curve(0.04);
        let a_true = 0.03;
        let sigma_true = 0.01;

        let discount = |t: f64| {
            let z = curve.zero_rate(t, Compounding::Continuous).unwrap_or(0.0);
            (-z * t).exp()
        };

        let helpers: Vec<_> = [(1.0_f64, 4.0_f64), (2.0, 3.0), (3.0, 2.0)]
            .into_iter()
            .map(|(e, tail)| {
                let strike = forward_swap_rate(&discount, e, tail, 1.0);
                let ann = forward_annuity(&discount, e, tail, 1.0);
                let f_0t = curve.instantaneous_forward(e).unwrap();
                let hw_price =
                    payer_swaption_hw1f(&discount, a_true, sigma_true, f_0t, e, tail, strike);
                let bach_vol = hw_price / (ann * (e / (2.0 * std::f64::consts::PI)).sqrt());
                CoterminalSwaptionHelper {
                    expiry_years: e,
                    tail_years: tail,
                    atm_normal_vol_bps: bach_vol * 1e4,
                }
            })
            .collect();

        let cal = calibrate_hw1f_sigma(&curve, a_true, &helpers).unwrap();
        assert!((cal.sigma - sigma_true).abs() < 1e-5);
    }

    #[test]
    fn rejects_empty_strip() {
        let curve = flat_curve(0.04);
        assert!(calibrate_hw1f_sigma(&curve, 0.03, &[]).is_err());
    }
}
