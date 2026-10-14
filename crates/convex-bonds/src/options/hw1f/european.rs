//! Analytic European bond options and dated-payment Jamshidian decomposition.

use super::{b, finite, invalid, positive, Hw1fError, Hw1fParameters};
use convex_math::portable;
use convex_math::{
    solvers::{brent, SolverConfig},
    stats::standard_normal_cdf,
};
use serde::{Deserialize, Serialize};

/// Holder option direction; a payer swaption corresponds to coupon-bond puts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OptionSide {
    /// Buy a zero-coupon bond / receive fixed in a swaption.
    Call,
    /// Sell a zero-coupon bond / pay fixed in a swaption.
    Put,
}

/// Actual fixed payment time and full contractual accrual fraction.
#[derive(Debug, Clone, Copy)]
pub struct SwaptionPayment {
    /// Payment time after expiry, in model years.
    pub time: f64,
    /// Actual schedule/day-count year fraction; not a guessed frequency.
    pub accrual: f64,
    /// Root-normalized time-zero DF for this date.
    pub discount: f64,
}

/// European ZCB option per unit face. DFs are at expiry and underlying maturity.
pub fn bond_option(
    parameters: Hw1fParameters,
    expiry: f64,
    maturity: f64,
    expiry_discount: f64,
    maturity_discount: f64,
    strike: f64,
    side: OptionSide,
) -> Result<f64, Hw1fError> {
    parameters.validate()?;
    if !expiry.is_finite()
        || !maturity.is_finite()
        || expiry < 0.0
        || maturity < expiry
        || maturity > 150.0
        || !strike.is_finite()
        || strike <= 0.0
    {
        return Err(invalid("bond option", "invalid expiry, maturity or strike"));
    }
    positive(expiry_discount, "expiry DF")?;
    positive(maturity_discount, "maturity DF")?;
    let deviation = finite(
        parameters.volatility
            * b(parameters.mean_reversion, maturity - expiry)
            * b(2.0 * parameters.mean_reversion, expiry).sqrt(),
        "bond-option deviation",
    )?;
    let discounted_strike = positive(strike * expiry_discount, "discounted strike")?;
    if deviation == 0.0 {
        return Ok(match side {
            OptionSide::Call => (maturity_discount - discounted_strike).max(0.0),
            OptionSide::Put => (discounted_strike - maturity_discount).max(0.0),
        });
    }
    let h = finite(
        portable::log(maturity_discount / discounted_strike) / deviation + deviation / 2.0,
        "bond-option normal argument",
    )?;
    let value = match side {
        OptionSide::Call => {
            maturity_discount * standard_normal_cdf(h)
                - discounted_strike * standard_normal_cdf(h - deviation)
        }
        OptionSide::Put => {
            discounted_strike * standard_normal_cdf(-h + deviation)
                - maturity_discount * standard_normal_cdf(-h)
        }
    };
    finite(value, "bond-option price")?;
    if value < 0.0 {
        return Err(Hw1fError::Numerical("negative bond-option price"));
    }
    Ok(value)
}

/// Physical single-curve European swaption per unit notional, starting at expiry.
/// Positive fixed-leg coefficients are required for the monotone decomposition.
/// Negative-strike and multicurve calibration are explicitly outside this route.
pub fn swaption(
    parameters: Hw1fParameters,
    expiry: f64,
    expiry_discount: f64,
    payments: &[SwaptionPayment],
    strike: f64,
    side: OptionSide,
) -> Result<f64, Hw1fError> {
    parameters.validate()?;
    if !expiry.is_finite()
        || expiry <= 0.0
        || expiry >= 150.0
        || !strike.is_finite()
        || !(0.0..=10.0).contains(&strike)
        || payments.is_empty()
        || payments.len() > 512
        || payments.iter().any(|p| {
            !p.time.is_finite()
                || p.time <= expiry
                || p.time > 150.0
                || !p.accrual.is_finite()
                || p.accrual <= 0.0
                || !p.discount.is_finite()
                || p.discount <= 0.0
        })
        || payments.windows(2).any(|p| p[1].time <= p[0].time)
    {
        return Err(invalid(
            "swaption",
            "requires bounded positive dated fixed leg and nonnegative strike",
        ));
    }
    positive(expiry_discount, "swaption expiry DF")?;
    let coefficients: Vec<f64> = payments
        .iter()
        .enumerate()
        .map(|(i, p)| {
            finite(
                p.accrual * strike + f64::from(i + 1 == payments.len()),
                "fixed-leg coefficient",
            )
        })
        .collect::<Result<_, _>>()?;
    let fixed_pv = coefficients
        .iter()
        .zip(payments)
        .try_fold(0.0, |value, (c, p)| {
            finite(value + c * p.discount, "fixed-leg PV")
        })?;
    if parameters.volatility == 0.0 {
        return Ok(match side {
            OptionSide::Put => (expiry_discount - fixed_pv).max(0.0),
            OptionSide::Call => (fixed_pv - expiry_discount).max(0.0),
        });
    }
    let variance = parameters.volatility.powi(2) * b(2.0 * parameters.mean_reversion, expiry);
    let factors: Vec<(f64, f64)> = payments
        .iter()
        .map(|p| {
            let factor = b(parameters.mean_reversion, p.time - expiry);
            Ok((
                factor,
                positive(
                    p.discount / expiry_discount * portable::exp(-0.5 * factor.powi(2) * variance),
                    "forward-measure bond scale",
                )?,
            ))
        })
        .collect::<Result<_, Hw1fError>>()?;
    let objective = |state: f64| -> f64 {
        coefficients
            .iter()
            .zip(&factors)
            .map(|(c, (factor, scale))| c * scale * portable::exp(-factor * state))
            .sum::<f64>()
            - 1.0
    };
    let root = brent(objective, -2.0, 2.0, &SolverConfig::new(1e-12, 128))?.root;
    let mut value = 0.0;
    for (i, payment) in payments.iter().enumerate() {
        let (factor, scale) = factors[i];
        let bond_strike = positive(
            scale * portable::exp(-factor * root),
            "Jamshidian bond strike",
        )?;
        value += coefficients[i]
            * bond_option(
                parameters,
                expiry,
                payment.time,
                expiry_discount,
                payment.discount,
                bond_strike,
                side,
            )?;
    }
    finite(value, "swaption value")
}
