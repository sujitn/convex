//! Explicit dated single-curve European swaption sigma calibration.

use super::*;
use convex_bonds::options::hw1f::{swaption, SwaptionPayment};
use convex_core::{
    calendars::Calendar,
    cashflows::{CouponSchedule, DateRules, ScheduleSpec},
    daycounts::DayCountConvention,
    Currency,
};
use convex_math::{
    optimization::{
        bounded_least_squares, FitDiagnostics, FitParameter, FitResidual, LeastSquaresSettings,
    },
    stats::standard_normal_cdf,
};
use serde::{Deserialize, Serialize};

/// Quotation is an annualized decimal, never guessed from percentage or bp values.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "convention", rename_all = "snake_case", deny_unknown_fields)]
pub enum SwaptionVolatility {
    /// Absolute rate diffusion, e.g. 0.007 = 70 normal-vol bp.
    Normal {
        /// Annualized decimal rate volatility.
        annualized_decimal: f64,
    },
    /// Relative diffusion with explicit additive decimal-rate displacement.
    ShiftedLognormal {
        /// Annualized relative volatility (0.20 = 20%).
        annualized_decimal: f64,
        /// Displacement added to both forward and strike.
        shift: f64,
    },
}
impl SwaptionVolatility {
    /// Native decimal-vol quote; normal and lognormal values have different units.
    pub fn value(self) -> f64 {
        match self {
            Self::Normal { annualized_decimal }
            | Self::ShiftedLognormal {
                annualized_decimal, ..
            } => annualized_decimal,
        }
    }
    fn validate(self) -> Result<(), OptionalBondError> {
        let upper = match self {
            Self::Normal { .. } => 1.0,
            Self::ShiftedLognormal { shift, .. } => {
                if !shift.is_finite() || shift.abs() > 10.0 {
                    return Err(invalid(
                        "volatility.shift",
                        "requires finite decimal displacement in [-10,10]",
                    ));
                }
                10.0
            }
        };
        if !self.value().is_finite() || self.value() <= 0.0 || self.value() > upper {
            return Err(invalid(
                "volatility",
                "positive finite native decimal volatility required",
            ));
        }
        Ok(())
    }
}

/// Full supplied fixed-leg schedule, not a rounded expiry/tail proxy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SwaptionCalibrationQuote {
    /// Unique supplied instrument/source identity.
    pub id: String,
    /// Exact exercise and underlying single-curve swap start.
    pub exercise_date: Date,
    /// Independent fixed-leg schedule, including stubs/EOM.
    pub fixed_schedule: ScheduleSpec,
    /// Actual accrual/payment rules; delayed-payment calibration is unsupported.
    pub dates: DateRules,
    /// Full day-count convention and retained irregular references.
    pub fixed_day_count: DayCountConvention,
    /// Nonnegative fixed rate for monotone Jamshidian decomposition.
    pub strike_rate: f64,
    /// Put = payer, Call = receiver.
    pub side: OptionSide,
    /// Exact quotation convention and displacement.
    pub volatility: SwaptionVolatility,
    /// Maximum repricing residual in this quote's annualized decimal-vol unit.
    pub native_tolerance: f64,
    /// Positive native-vol normalization scale for the fitting objective.
    pub scale: f64,
    /// Nonnegative objective weight; zero never waives native repricing.
    pub weight: f64,
}

/// Numerical sigma bounds/start/identification, with exogenous mean reversion.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SigmaCalibrationSettings {
    /// Initial short-rate diffusion.
    pub initial_sigma: f64,
    /// Lower closed bound; must be strictly positive for identification.
    pub lower_sigma: f64,
    /// Upper closed bound, at most 1.
    pub upper_sigma: f64,
    /// Maximum LM attempts (1..10000).
    pub max_iterations: usize,
    /// Weighted scaled native-vol RMS postcondition.
    pub objective_tolerance: f64,
    /// Absolute short-rate sigma difference for the undamped Jacobian.
    pub jacobian_step: f64,
    /// Minimum undamped singular ratio.
    pub minimum_singular_ratio: f64,
    /// Separate bounded SVD iteration count.
    pub svd_iterations: usize,
}

/// One complete pure calibration request; all market resources are actual values.
pub struct SigmaCalibrationInputs<'a> {
    /// Caller-assigned immutable output identity.
    pub model_key: ModelKey,
    /// Model root and information date, without a global evaluation date.
    pub valuation_date: Date,
    /// Explicit contract currency.
    pub currency: Currency,
    /// Exact currency/collateral discount curve.
    pub discount: &'a BuiltCurve,
    /// Actual fixed-leg calendar for all supplied quotes.
    pub calendar: &'a dyn Calendar,
    /// Fixed exogenous mean reversion; it is not silently fitted or defaulted.
    pub mean_reversion: f64,
    /// Complete quotes in their declared native volatility convention.
    pub quotes: &'a [SwaptionCalibrationQuote],
    /// Explicit finite bounds, starts and numerical postconditions.
    pub settings: SigmaCalibrationSettings,
}

/// Complete source of model parameters; assumptions are never called calibration.
#[derive(Debug, Clone, serde::Serialize)]
pub enum ModelOrigin {
    /// Caller chose parameters and supplied the reason/version.
    Assumption {
        /// Nonempty supplied rationale.
        reason: String,
    },
    /// Successful fit to declared quotes on an exact supplied curve.
    SwaptionCalibration {
        /// Original discount curve/version.
        discount_curve: CurveKey,
        /// ACT/365F model root and quote information date.
        valuation_date: Date,
        /// Exogenous a was held fixed.
        fixed_mean_reversion: f64,
        /// Original full quote inputs.
        quotes: Vec<SwaptionCalibrationQuote>,
        /// Actual bounded identification/convergence.
        #[serde(serialize_with = "serialize_fit_diagnostics")]
        solver: FitDiagnostics,
        /// All native quote and premium results.
        residuals: Vec<SwaptionCalibrationResidual>,
    },
}
/// Immutable checked model; production pricing cannot construct default parameters.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OptionalBondModel {
    key: ModelKey,
    parameters: Hw1fParameters,
    origin: ModelOrigin,
}
impl OptionalBondModel {
    /// An explicit parameter assumption, including sigma=0 if deliberately supplied.
    pub fn assumed(
        key: ModelKey,
        parameters: Hw1fParameters,
        reason: String,
    ) -> Result<Self, OptionalBondError> {
        key.validate()?;
        parameters.validate()?;
        if reason.trim().is_empty() || reason.len() > 8192 {
            return Err(invalid(
                "model assumption",
                "bounded nonempty rationale required",
            ));
        }
        Ok(Self {
            key,
            parameters,
            origin: ModelOrigin::Assumption { reason },
        })
    }
    /// Exact caller identity.
    pub fn key(&self) -> &ModelKey {
        &self.key
    }
    /// Checked constant parameters; risk holds these fixed.
    pub fn parameters(&self) -> Hw1fParameters {
        self.parameters
    }
    /// Assumption versus actual calibrated quote/curve provenance.
    pub fn origin(&self) -> &ModelOrigin {
        &self.origin
    }
}

/// Every result retains its native quotation and premium per unit notional.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SwaptionCalibrationResidual {
    /// Original instrument ID.
    pub id: String,
    /// Original convention, units and displacement.
    pub input_quote: SwaptionVolatility,
    /// Model-implied annualized decimal volatility in the same convention.
    pub model_quote: f64,
    /// Model minus input, in this quote's native decimal-vol unit.
    pub native_residual: f64,
    /// Declared native-unit bound.
    pub tolerance: f64,
    /// Market quote converted to price per unit notional.
    pub market_premium: f64,
    /// Analytic model price per unit notional.
    pub model_premium: f64,
    /// Full schedule forward and fixed annuity used for quotation.
    pub forward_rate: f64,
    /// Currency annuity per unit notional.
    pub annuity: f64,
}
struct PreparedSwaption {
    expiry: f64,
    discount: f64,
    payments: Vec<SwaptionPayment>,
    forward: f64,
    annuity: f64,
}
fn prepare(
    quote: &SwaptionCalibrationQuote,
    root: Date,
    discount: &BuiltCurve,
    calendar: &dyn Calendar,
) -> Result<PreparedSwaption, OptionalBondError> {
    if quote.id.trim().is_empty()
        || quote.id.len() > 256
        || quote.exercise_date <= root
        || quote.fixed_schedule.start_date != quote.exercise_date
        || !quote.native_tolerance.is_finite()
        || quote.native_tolerance <= 0.0
        || !quote.scale.is_finite()
        || quote.scale <= 0.0
        || !quote.weight.is_finite()
        || quote.weight < 0.0
        || !quote.strike_rate.is_finite()
        || !(0.0..=10.0).contains(&quote.strike_rate)
    {
        return Err(invalid(
            "swaption quote",
            "invalid identity, dated start, strike or residual controls",
        ));
    }
    quote.volatility.validate()?;
    let schedule = CouponSchedule::generate(&quote.fixed_schedule, &quote.dates, calendar)?;
    if schedule.periods().is_empty()
        || schedule.periods().len() > 512
        || schedule.periods()[0].accrual_dates().0 != quote.exercise_date
        || schedule
            .periods()
            .iter()
            .any(|p| p.payment_date() != p.accrual_dates().1 || p.ex_coupon().is_some())
    {
        return Err(unsupported("swaption calibration terms", "requires physical expiry-start single-curve swap with payments at actual accrual ends and no ex-coupon"));
    }
    let root_df = df(discount, root)?;
    let expiry_df = df(discount, quote.exercise_date)? / root_df;
    let payments: Vec<_> = schedule
        .periods()
        .iter()
        .map(|p| {
            Ok(SwaptionPayment {
                time: time(root, p.payment_date())?,
                accrual: number(p.year_fraction(
                    p.accrual_dates().0,
                    p.accrual_dates().1,
                    quote.fixed_day_count,
                )?)?,
                discount: df(discount, p.payment_date())? / root_df,
            })
        })
        .collect::<Result<_, OptionalBondError>>()?;
    let annuity = finite(
        payments.iter().map(|p| p.accrual * p.discount).sum(),
        "swaption annuity",
    )?;
    if annuity <= 0.0 {
        return Err(invalid("swaption annuity", "must be positive"));
    }
    let last = payments
        .last()
        .ok_or_else(|| invalid("fixed leg", "empty"))?;
    let forward = finite((expiry_df - last.discount) / annuity, "swaption forward")?;
    let result = PreparedSwaption {
        expiry: time(root, quote.exercise_date)?,
        discount: expiry_df,
        payments,
        forward,
        annuity,
    };
    quotation_price(&result, quote, quote.volatility.value())?;
    Ok(result)
}
fn quotation_price(
    p: &PreparedSwaption,
    q: &SwaptionCalibrationQuote,
    volatility: f64,
) -> Result<f64, OptionalBondError> {
    let sign = if q.side == OptionSide::Put { 1.0 } else { -1.0 };
    let difference = sign * (p.forward - q.strike_rate);
    if let SwaptionVolatility::ShiftedLognormal { shift, .. } = q.volatility {
        if p.forward + shift <= 0.0 || q.strike_rate + shift <= 0.0 {
            return Err(invalid(
                "volatility.shift",
                "shifted forward and strike must be strictly positive",
            ));
        }
    }
    if volatility == 0.0 {
        return Ok(p.annuity * difference.max(0.0));
    }
    let sd = volatility * p.expiry.sqrt();
    let value = match q.volatility {
        SwaptionVolatility::Normal { .. } => {
            let z = difference / sd;
            p.annuity
                * (difference * standard_normal_cdf(z)
                    + sd * (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt())
        }
        SwaptionVolatility::ShiftedLognormal { shift, .. } => {
            let forward = p.forward + shift;
            let strike = q.strike_rate + shift;
            let d1 = (forward / strike).ln() / sd + sd / 2.0;
            let d2 = d1 - sd;
            p.annuity
                * sign
                * (forward * standard_normal_cdf(sign * d1)
                    - strike * standard_normal_cdf(sign * d2))
        }
    };
    finite(value, "volatility quotation premium")?;
    if value < 0.0 {
        return Err(invalid(
            "volatility quotation premium",
            "negative mathematical option price",
        ));
    }
    Ok(value)
}
fn model_premium(
    p: &PreparedSwaption,
    q: &SwaptionCalibrationQuote,
    parameters: Hw1fParameters,
) -> Result<f64, OptionalBondError> {
    Ok(swaption(
        parameters,
        p.expiry,
        p.discount,
        &p.payments,
        q.strike_rate,
        q.side,
    )?)
}
fn implied_quote(
    p: &PreparedSwaption,
    q: &SwaptionCalibrationQuote,
    premium: f64,
) -> Result<f64, OptionalBondError> {
    let mut low = 0.0;
    let mut high = match q.volatility {
        SwaptionVolatility::Normal { .. } => 1.0,
        SwaptionVolatility::ShiftedLognormal { .. } => 10.0,
    };
    let lower_price = quotation_price(p, q, low)?;
    let upper_price = quotation_price(p, q, high)?;
    if premium < lower_price || premium > upper_price {
        return Err(invalid(
            "implied volatility",
            "model premium outside declared convention's inversion domain",
        ));
    }
    for _ in 0..64 {
        let mid = f64::midpoint(low, high);
        let value = quotation_price(p, q, mid)?;
        if value < premium {
            low = mid;
        } else {
            high = mid;
        }
    }
    let value = f64::midpoint(low, high);
    if (quotation_price(p, q, value)? - premium).abs() > 1e-12 + 1e-12 * premium.abs() {
        return Err(invalid(
            "implied volatility",
            "premium residual postcondition failed",
        ));
    }
    Ok(value)
}

/// Inspect a full dated quote on explicit parameters without fitting it.
pub fn reprice_swaption(
    quote: &SwaptionCalibrationQuote,
    root: Date,
    discount: &BuiltCurve,
    calendar: &dyn Calendar,
    parameters: Hw1fParameters,
) -> Result<SwaptionCalibrationResidual, OptionalBondError> {
    discount_role(discount, discount.recipe().currency)?;
    let p = prepare(quote, root, discount, calendar)?;
    residual(quote, &p, parameters)
}
fn residual(
    q: &SwaptionCalibrationQuote,
    p: &PreparedSwaption,
    parameters: Hw1fParameters,
) -> Result<SwaptionCalibrationResidual, OptionalBondError> {
    let model_premium = model_premium(p, q, parameters)?;
    let model_quote = implied_quote(p, q, model_premium)?;
    Ok(SwaptionCalibrationResidual {
        id: q.id.clone(),
        input_quote: q.volatility,
        model_quote,
        native_residual: model_quote - q.volatility.value(),
        tolerance: q.native_tolerance,
        market_premium: quotation_price(p, q, q.volatility.value())?,
        model_premium,
        forward_rate: p.forward,
        annuity: p.annuity,
    })
}

/// Fit only sigma; fixed mean reversion is explicit because a single strip does
/// not identify rate correlations. All native vol residuals and undamped rank
/// must pass, including zero-weight quotes. A failed fit returns no model.
pub fn calibrate_sigma(
    inputs: SigmaCalibrationInputs<'_>,
) -> Result<OptionalBondModel, OptionalBondError> {
    let SigmaCalibrationInputs {
        model_key: key,
        valuation_date: root,
        currency,
        discount,
        calendar,
        mean_reversion,
        quotes,
        settings,
    } = inputs;
    key.validate()?;
    discount_role(discount, currency)?;
    Hw1fParameters {
        mean_reversion,
        volatility: settings.initial_sigma,
    }
    .validate()?;
    if quotes.is_empty()
        || quotes.len() > 128
        || settings.lower_sigma <= 0.0
        || settings.upper_sigma > 1.0
    {
        return Err(invalid(
            "calibration inputs",
            "requires 1..128 quotes and positive sigma bounds within [0,1]",
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut ordered: Vec<_> = quotes.iter().collect();
    ordered.sort_by(|a, b| a.id.cmp(&b.id));
    for q in &ordered {
        if !ids.insert(&q.id) {
            return Err(invalid("calibration quotes", "duplicate instrument ID"));
        }
    }
    let prepared: Vec<_> = ordered
        .iter()
        .map(|q| prepare(q, root, discount, calendar))
        .collect::<Result<_, _>>()?;
    let residuals: Vec<_> = ordered
        .iter()
        .map(|q| FitResidual {
            scale: q.scale,
            weight: q.weight,
            tolerance: q.native_tolerance,
        })
        .collect();
    let fit = bounded_least_squares(
        &[FitParameter {
            initial: settings.initial_sigma,
            lower: settings.lower_sigma,
            upper: settings.upper_sigma,
        }],
        &residuals,
        LeastSquaresSettings {
            max_iterations: settings.max_iterations,
            objective_tolerance: settings.objective_tolerance,
            jacobian_step: settings.jacobian_step,
            minimum_singular_ratio: settings.minimum_singular_ratio,
            svd_iterations: settings.svd_iterations,
        },
        |values| {
            ordered
                .iter()
                .zip(&prepared)
                .map(|(q, p)| {
                    let price = model_premium(
                        p,
                        q,
                        Hw1fParameters {
                            mean_reversion,
                            volatility: values[0],
                        },
                    )?;
                    Ok(implied_quote(p, q, price)? - q.volatility.value())
                })
                .collect::<Result<Vec<_>, OptionalBondError>>()
        },
    )
    .map_err(|e| OptionalBondError::Calibration(Box::new(e)))?;
    let parameters = Hw1fParameters {
        mean_reversion,
        volatility: fit.parameters[0],
    };
    let results = ordered
        .iter()
        .zip(&prepared)
        .map(|(q, p)| residual(q, p, parameters))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(OptionalBondModel {
        key,
        parameters,
        origin: ModelOrigin::SwaptionCalibration {
            discount_curve: discount.recipe().key.clone(),
            valuation_date: root,
            fixed_mean_reversion: mean_reversion,
            quotes: ordered.into_iter().cloned().collect(),
            solver: fit.diagnostics,
            residuals: results,
        },
    })
}

// Retain every measured solver field without changing the numerical kernel.
fn serialize_fit_diagnostics<S: serde::Serializer>(
    d: &FitDiagnostics,
    s: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeStruct;
    let mut output = s.serialize_struct("FitDiagnostics", 7)?;
    output.serialize_field("iterations", &d.iterations)?;
    output.serialize_field("evaluations", &d.evaluations)?;
    output.serialize_field("scaled_rms", &d.scaled_rms)?;
    output.serialize_field("native_residuals", &d.native_residuals)?;
    output.serialize_field("rank", &d.rank)?;
    output.serialize_field("condition_estimate", &d.condition_estimate)?;
    output.serialize_field("active_bounds", &d.active_bounds)?;
    output.end()
}
