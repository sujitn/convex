//! Forward coupon option expectations under an explicitly supplied payment measure.
//! No market data, discount/forward substitution, assumed volatility or clipping
//! of a future forecast. Full bond term/fixing integration is a separate owner.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::hw1f::OptionSide;
use convex_math::portable;
use serde::{Deserialize, Serialize};

/// Annual index-forward volatility; the caller supplies actual expiry time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ForwardCouponModel {
    /// Bachelier volatility in annual decimal rate per square-root year.
    Normal { volatility: f64 },
    /// Black volatility per square-root year, with nonnegative decimal shift.
    ShiftedLognormal { volatility: f64, displacement: f64 },
}

/// Model/input/numerical failure carries no successful option value.
#[derive(Debug, Clone, thiserror::Error)]
pub enum CouponOptionError {
    /// Invalid explicitly supplied model, payoff or time.
    #[error("floating coupon option {field}: {reason}")]
    Invalid {
        field: &'static str,
        reason: &'static str,
    },
    /// Nonfinite/negative model result is not clipped to a successful zero.
    #[error("floating coupon option numerical failure: {0}")]
    Numerical(&'static str),
}
type Result<T> = std::result::Result<T, CouponOptionError>;
fn invalid(field: &'static str, reason: &'static str) -> CouponOptionError {
    CouponOptionError::Invalid { field, reason }
}
fn finite(x: f64) -> Result<f64> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(CouponOptionError::Numerical("nonfinite result"))
    }
}
fn cdf(x: f64) -> f64 {
    0.5 * portable::erfc(-x / std::f64::consts::SQRT_2)
}
fn density(x: f64) -> f64 {
    portable::exp(-0.5 * x * x) / (2.0 * std::f64::consts::PI).sqrt()
}

impl ForwardCouponModel {
    /// Validate declared parameters without changing their sign or value.
    pub fn validate(self) -> Result<()> {
        match self {
            Self::Normal { volatility }
                if volatility.is_finite() && (0.0..=1.0).contains(&volatility) =>
            {
                Ok(())
            }
            Self::ShiftedLognormal {
                volatility,
                displacement,
            } if volatility.is_finite()
                && (0.0..=10.0).contains(&volatility)
                && displacement.is_finite()
                && (0.0..=10.0).contains(&displacement) =>
            {
                Ok(())
            }
            _ => Err(invalid(
                "model",
                "finite nonnegative bounded volatility/displacement required",
            )),
        }
    }
    fn volatility(self) -> f64 {
        match self {
            Self::Normal { volatility } | Self::ShiftedLognormal { volatility, .. } => volatility,
        }
    }
}

/// Undiscounted option rate expectation and derivatives in original index units.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RateOptionValue {
    /// Annual decimal rate expectation. Currency/accrual/DF scale elsewhere.
    pub expected_payoff: f64,
    /// Derivative per annual decimal forward; absent at an intrinsic kink.
    pub forward_delta: Option<f64>,
    /// Second derivative per annual decimal forward squared; absent at a kink.
    pub forward_gamma: Option<f64>,
    /// Derivative per declared annual volatility. Absent at a degenerate kink.
    pub annual_vol_vega: Option<f64>,
}

/// True normal/shifted-lognormal call or put on one supplied index forward.
///
/// The supplied forward must be the mean under the actual coupon payment measure.
/// No convexity/payment-lag adjustment or expiry clock is inferred. Zero expiry
/// or explicitly zero volatility gives the deterministic intrinsic payoff;
/// the derivative discontinuity at strike is reported as unavailable.
pub fn forward_rate_option(
    model: ForwardCouponModel,
    expiry_time: f64,
    forward: f64,
    strike: f64,
    side: OptionSide,
) -> Result<RateOptionValue> {
    model.validate()?;
    if !expiry_time.is_finite()
        || !(0.0..=150.0).contains(&expiry_time)
        || !forward.is_finite()
        || !strike.is_finite()
        || forward.abs() > 10.0
        || strike.abs() > 10.0
    {
        return Err(invalid(
            "rate/time",
            "finite rates within10 and expiry in [0,150] required",
        ));
    }
    let sign = match side {
        OptionSide::Call => 1.0,
        OptionSide::Put => -1.0,
    };
    let shifted = match model {
        ForwardCouponModel::Normal { .. } => None,
        ForwardCouponModel::ShiftedLognormal { displacement, .. } => {
            let f = finite(forward + displacement)?;
            let k = finite(strike + displacement)?;
            if f <= 0.0 {
                return Err(invalid(
                    "shifted forward",
                    "positive shifted forward required",
                ));
            }
            Some((f, k))
        }
    };
    let deviation = finite(model.volatility() * expiry_time.sqrt())?;
    if deviation == 0.0 && model.volatility() > 0.0 && expiry_time > 0.0 {
        return Err(CouponOptionError::Numerical("option deviation underflow"));
    }
    if deviation == 0.0 {
        let difference = sign * (forward - strike);
        let kink = difference == 0.0;
        return Ok(RateOptionValue {
            expected_payoff: difference.max(0.0),
            forward_delta: if kink {
                None
            } else {
                Some(if difference > 0.0 { sign } else { 0.0 })
            },
            forward_gamma: if kink { None } else { Some(0.0) },
            annual_vol_vega: if kink { None } else { Some(0.0) },
        });
    }
    let (payoff, delta, gamma, vega) = if let Some((f, k)) = shifted {
        if k <= 0.0 {
            // A shifted-lognormal rate cannot fall below minus its displacement.
            if side == OptionSide::Call {
                (forward - strike, 1.0, 0.0, 0.0)
            } else {
                (0.0, 0.0, 0.0, 0.0)
            }
        } else {
            let d1 = finite((portable::log(f) - portable::log(k)) / deviation + deviation / 2.0)?;
            let d2 = d1 - deviation;
            (
                sign * (f * cdf(sign * d1) - k * cdf(sign * d2)),
                sign * cdf(sign * d1),
                density(d1) / (f * deviation),
                f * expiry_time.sqrt() * density(d1),
            )
        }
    } else {
        let d = finite((forward - strike) / deviation)?;
        (
            sign * (forward - strike) * cdf(sign * d) + deviation * density(d),
            sign * cdf(sign * d),
            density(d) / deviation,
            expiry_time.sqrt() * density(d),
        )
    };
    finite(payoff)?;
    if payoff < 0.0 {
        return Err(CouponOptionError::Numerical("negative option expectation"));
    }
    Ok(RateOptionValue {
        expected_payoff: payoff,
        forward_delta: Some(finite(delta)?),
        forward_gamma: Some(finite(gamma)?),
        annual_vol_vega: Some(finite(vega)?),
    })
}

/// Complete coupon expectation after supplied final coupon-rate cap/floor.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct BoundedCouponValue {
    /// Annual decimal total coupon mean, including optional time value.
    pub expected_rate: f64,
    /// Expected coupon-rate deduction from cap; zero when no cap exists.
    pub cap_adjustment: f64,
    /// Expected coupon-rate addition from floor; zero when no floor exists.
    pub floor_adjustment: f64,
    /// Original-index forward derivative; no inferred coupon-rate unit conversion.
    pub forward_delta: Option<f64>,
    /// Original-index second derivative.
    pub forward_gamma: Option<f64>,
    /// Declared index-volatility derivative.
    pub annual_vol_vega: Option<f64>,
}

/// Coupon payoff is min(cap,max(floor,gearing*index+additive)); absent bounds
/// retain the corresponding plain branch. Negative gearing reverses option side.
/// This is one explicitly modeled terminal coupon rate, not an overnight/path
/// aggregation, generic FRN price, historical fixing resolver or cash ledger.
#[allow(clippy::too_many_arguments)]
pub fn bounded_floating_coupon(
    model: ForwardCouponModel,
    expiry_time: f64,
    forward: f64,
    gearing: f64,
    additive: f64,
    cap: Option<f64>,
    floor: Option<f64>,
) -> Result<BoundedCouponValue> {
    model.validate()?;
    if !expiry_time.is_finite()
        || !(0.0..=150.0).contains(&expiry_time)
        || !forward.is_finite()
        || forward.abs() > 10.0
        || !gearing.is_finite()
        || gearing.abs() > 10_000.0
        || !additive.is_finite()
        || additive.abs() > 10.0
        || cap
            .into_iter()
            .chain(floor)
            .any(|k| !k.is_finite() || k.abs() > 10.0)
        || matches!((cap,floor),(Some(c),Some(f)) if c<f)
    {
        return Err(invalid(
            "coupon",
            "finite bounded coupon coefficients, ordered cap/floor and expiry required",
        ));
    }
    let mean = finite(gearing * forward + additive)?;
    if gearing == 0.0 {
        let cap_adjustment = cap.map_or(0.0, |k| (mean - k).max(0.0));
        let floor_adjustment = floor.map_or(0.0, |k| (k - mean).max(0.0));
        return Ok(BoundedCouponValue {
            expected_rate: finite(mean - cap_adjustment + floor_adjustment)?,
            cap_adjustment,
            floor_adjustment,
            forward_delta: Some(0.0),
            forward_gamma: Some(0.0),
            annual_vol_vega: Some(0.0),
        });
    }
    let leg = |bound, cap_leg| {
        let strike = finite((bound - additive) / gearing)?;
        let side = if (gearing > 0.0) == cap_leg {
            OptionSide::Call
        } else {
            OptionSide::Put
        };
        forward_rate_option(model, expiry_time, forward, strike, side)
    };
    let cap_value = cap.map(|k| leg(k, true)).transpose()?;
    let floor_value = floor.map(|k| leg(k, false)).transpose()?;
    let size = gearing.abs();
    let cap_adjustment = cap_value.map_or(0.0, |v| size * v.expected_payoff);
    let floor_adjustment = floor_value.map_or(0.0, |v| size * v.expected_payoff);
    let derivative =
        |base, field: fn(RateOptionValue) -> Option<f64>, scale| -> Result<Option<f64>> {
            let cap = match cap_value {
                Some(v) => field(v),
                None => Some(0.0),
            };
            let floor = match floor_value {
                Some(v) => field(v),
                None => Some(0.0),
            };
            match (cap, floor) {
                (Some(c), Some(f)) => Ok(Some(finite(base + scale * (f - c))?)),
                _ => Ok(None),
            }
        };
    let constant = matches!((cap,floor),(Some(c),Some(f)) if c==f);
    let expected_rate = if constant {
        cap.ok_or_else(|| invalid("collar", "missing constant bound"))?
    } else {
        finite(mean - cap_adjustment + floor_adjustment)?
    };
    Ok(BoundedCouponValue {
        expected_rate,
        cap_adjustment: finite(cap_adjustment)?,
        floor_adjustment: finite(floor_adjustment)?,
        forward_delta: if constant {
            Some(0.0)
        } else {
            derivative(gearing, |v| v.forward_delta, size)?
        },
        forward_gamma: if constant {
            Some(0.0)
        } else {
            derivative(0.0, |v| v.forward_gamma, size)?
        },
        annual_vol_vega: if constant {
            Some(0.0)
        } else {
            derivative(0.0, |v| v.annual_vol_vega, size)?
        },
    })
}
