//! Explicit one-sided full-contract derivatives in the increasing parameter units.
use super::*;

/// Caller-selected side; model domains never select or change this implicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CouponParameterSide {
    /// Evaluate original parameter plus positive native moves.
    Forward,
    /// Evaluate original parameter minus positive native moves.
    Backward,
}
impl CouponParameterSide {
    fn sign(self) -> f64 {
        match self {
            Self::Forward => 1.0,
            Self::Backward => -1.0,
        }
    }
}

/// Explicit side and the original native first/second h/h2 admission policy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CouponDirectionalDerivativeSettings {
    /// Original selected side; it is retained even for an interior point.
    pub side: CouponParameterSide,
    /// Positive native bump and separate nonvacuous first/second tolerances.
    pub derivative: CouponParameterDerivativeSettings,
}

/// One actual unrounded complete valuation, without a fabricated opposite side.
#[derive(Debug, Serialize)]
pub struct CouponDirectionalPrice {
    /// Absolute multiplier0,0.5,1,1.5,2 or3 of the positive native bump.
    pub multiplier: f64,
    /// Actual absolute selected parameter after applying the original side.
    pub parameter: f64,
    /// Full original contract/model/history/curve proof at that parameter.
    pub valuation: TermCouponOptionPrice,
}

/// Dedicated one-sided evidence; this is not a Central canonical hedge record.
#[derive(Debug, Serialize)]
pub struct CouponParameterDirectionalDerivatives {
    /// Exact original source/revision/index/observation/payment/parameter units.
    pub coordinate: CouponParameterCoordinate,
    /// Original absolute native parameter before every hypothetical move.
    pub original_parameter: f64,
    /// Complete caller-selected side and original native precision controls.
    pub settings: CouponDirectionalDerivativeSettings,
    /// Derivative with respect to increasing native parameter, on the chosen side.
    pub first: f64,
    /// Same derivative from the independently evaluated half-size stencil.
    pub half_first: f64,
    /// Chosen-side currency curvature per squared native parameter unit.
    pub second: f64,
    /// Same curvature from independently evaluated half-size moves.
    pub half_second: f64,
    /// Binary64 cancellation guard; not total pricer/model error.
    pub first_resolution: f64,
    /// Same arithmetic guard for the second derivative's native units.
    pub second_resolution: f64,
    /// Actual completed full price count; exactly six on every success.
    pub price_evaluations: usize,
    /// Every full actual native valuation in absolute-multiplier order.
    pub prices: Vec<CouponDirectionalPrice>,
}

fn stencil(base: f64, one: f64, two: f64, three: f64, step: f64) -> Result<(f64, f64)> {
    // Differences retain the algebraic coefficients without repeatedly summing
    // the much larger baseline currency PV. The arithmetic guard still applies.
    let one = finite(one - base)?;
    let two = finite(two - base)?;
    let three = finite(three - base)?;
    Ok((
        finite((4.0 * one - two) / (2.0 * step))?,
        finite((-5.0 * one + 4.0 * two - three) / (step * step))?,
    ))
}

impl PreparedTermCouponModel<'_> {
    /// Revalue the complete original native product six times on one explicit
    /// side. Preserve every other parameter, history, publication and curve.
    /// Model domains, both refinements and both resolution guards must pass.
    pub fn parameter_directional_derivatives(
        &self,
        curves: &CurveSet,
        coordinate: &CouponParameterCoordinate,
        settings: CouponDirectionalDerivativeSettings,
    ) -> Result<CouponParameterDirectionalDerivatives> {
        settings.derivative.validate()?;
        self.admit_baseline(curves)?;
        let (_, original_parameter, _) =
            super::super::scenario::changed_options(self.input.options, coordinate, 0.0)?;
        let step = settings.side.sign() * settings.derivative.bump;
        let mut points = Vec::with_capacity(6);
        for multiplier in [0.0, 0.5, 1.0, 1.5, 2.0, 3.0] {
            let parameter = if multiplier == 0.0 {
                original_parameter
            } else {
                finite(original_parameter + step * multiplier)?
            };
            if points.iter().any(|(_, p)| *p == parameter) {
                return Err(invalid(
                    "directional parameter resolution",
                    "distinct representable original native parameters required",
                ));
            }
            points.push((multiplier, parameter));
        }
        let mut prices = Vec::with_capacity(points.len());
        for (multiplier, parameter) in points {
            let (options, _, parameter) = super::super::scenario::parameter_options(
                self.input.options,
                coordinate,
                parameter,
            )?;
            let valuation = price_input(
                &TermCouponModelInputs {
                    bond: self.input.bond,
                    context: self.input.context,
                    history: self.input.history,
                    options: &options,
                    projection: self.input.projection.clone(),
                    discount: self.input.discount.clone(),
                },
                &self.publications,
                curves,
            )?;
            prices.push(CouponDirectionalPrice {
                multiplier,
                parameter,
                valuation,
            });
        }
        let p = |i: usize| prices[i].valuation.present_value;
        if p(0) != self.baseline().present_value {
            return Err(invalid(
                "directional coupon baseline",
                "original full native PV differs",
            ));
        }
        let (first, second) = stencil(p(0), p(2), p(4), p(5), step)?;
        let (half_first, half_second) = stencil(p(0), p(1), p(2), p(3), step / 2.0)?;
        let scale = prices
            .iter()
            .map(|p| p.valuation.present_value.abs())
            .fold(0.0, f64::max);
        let rounding = finite(64.0 * f64::EPSILON * scale)?;
        if rounding == 0.0 {
            return Err(invalid(
                "directional derivative resolution",
                "positive representable native PV arithmetic scale required",
            ));
        }
        let half = settings.derivative.bump / 2.0;
        let first_resolution = finite(rounding / half)?;
        let second_resolution = finite(rounding / (half * half))?;
        settings
            .derivative
            .admit(1, first, half_first, first_resolution)?;
        settings
            .derivative
            .admit(2, second, half_second, second_resolution)?;
        Ok(CouponParameterDirectionalDerivatives {
            coordinate: coordinate.clone(),
            original_parameter,
            settings,
            first,
            half_first,
            second,
            half_second,
            first_resolution,
            second_resolution,
            price_evaluations: prices.len(),
            prices,
        })
    }
}
