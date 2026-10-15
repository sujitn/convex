//! Full original parameter derivatives with explicit native currency h/h2 policy.
use super::*;

/// Caller-supplied native parameter and currency-derivative refinement policy.
/// No market-volatility or interest-rate normalization is inferred.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CouponParameterDerivativeSettings {
    /// Positive move in the selected parameter's own units, in [1e-8,0.1].
    pub bump: f64,
    /// Absolute first derivative h/h2 difference, currency per parameter unit.
    pub absolute_first_tolerance: f64,
    /// Relative first derivative bound applied to the larger absolute derivative.
    pub relative_first_tolerance: f64,
    /// Absolute second derivative h/h2 difference, currency per squared unit.
    pub absolute_second_tolerance: f64,
    /// Relative second derivative bound applied to the larger absolute derivative.
    pub relative_second_tolerance: f64,
}
impl CouponParameterDerivativeSettings {
    /// Validate the explicit positive move and both nonvacuous finite policies.
    pub fn validate(self) -> Result<()> {
        if !self.bump.is_finite() || !(1e-8..=0.1).contains(&self.bump) {
            return Err(invalid(
                "parameter bump",
                "finite native-unit move in [1e-8,0.1] required",
            ));
        }
        for (absolute, relative) in [
            (self.absolute_first_tolerance, self.relative_first_tolerance),
            (
                self.absolute_second_tolerance,
                self.relative_second_tolerance,
            ),
        ] {
            if !absolute.is_finite()
                || !relative.is_finite()
                || absolute < 0.0
                || !(0.0..=1.0).contains(&relative)
                || (absolute == 0.0 && relative == 0.0)
            {
                return Err(invalid(
                    "parameter derivative policy",
                    "finite nonnegative, nonvacuous absolute/relative bounds required",
                ));
            }
        }
        Ok(())
    }
    pub(super) fn admit(self, order: u8, full: f64, half: f64, resolution: f64) -> Result<()> {
        let (absolute, relative) = if order == 1 {
            (self.absolute_first_tolerance, self.relative_first_tolerance)
        } else {
            (
                self.absolute_second_tolerance,
                self.relative_second_tolerance,
            )
        };
        let bound = finite(absolute + relative * full.abs().max(half.abs()))?;
        if finite(full - half)?.abs() > bound {
            return Err(TermCouponOptionError::UnstableDerivative {
                order,
                full,
                half,
                bound,
            });
        }
        if resolution > bound {
            return Err(TermCouponOptionError::DerivativeResolution {
                order,
                resolution,
                bound,
            });
        }
        Ok(())
    }
}

/// Five actual native valuations and independently refined first/second derivatives.
#[derive(Debug, Serialize)]
pub struct CouponParameterDerivatives {
    /// Exact original consumed model point, source revision, parameter and units.
    pub coordinate: CouponParameterCoordinate,
    /// Actual original parameter, without clipping or a hypothetical publication.
    pub original_parameter: f64,
    /// Complete explicit native derivative policy.
    pub settings: CouponParameterDerivativeSettings,
    /// Central currency-PV first derivative per one native parameter unit.
    pub first: f64,
    /// Same first derivative from independently evaluated half-size moves.
    pub half_first: f64,
    /// Central currency-PV second derivative per squared native parameter unit.
    pub second: f64,
    /// Same second derivative from independently evaluated half-size moves.
    pub half_second: f64,
    /// Conservative binary64 cancellation floor for the half-step first derivative.
    /// This is an arithmetic admission guard, not a bound on model/pricer error.
    pub first_resolution: f64,
    /// Same arithmetic floor for the half-step second derivative.
    pub second_resolution: f64,
    /// Full unchanged native baseline.
    pub base: TermCouponOptionPrice,
    /// Full original native valuation at +h.
    pub plus: TermCouponOptionPrice,
    /// Full original native valuation at -h.
    pub minus: TermCouponOptionPrice,
    /// Full original native valuation at +h/2.
    pub half_plus: TermCouponOptionPrice,
    /// Full original native valuation at -h/2.
    pub half_minus: TermCouponOptionPrice,
}

impl PreparedTermCouponModel<'_> {
    /// Hold original complete curves/economics/history/publications and all other
    /// model fields fixed. Every original parameter domain is checked by native
    /// pricing at both move sizes. Failed domains or either refinement abort.
    pub fn parameter_derivatives(
        &self,
        curves: &CurveSet,
        coordinate: &CouponParameterCoordinate,
        settings: CouponParameterDerivativeSettings,
    ) -> Result<CouponParameterDerivatives> {
        settings.validate()?;
        self.admit_baseline(curves)?;
        let (_, original_parameter, _) =
            super::super::scenario::changed_options(self.input.options, coordinate, 0.0)?;
        let price = |delta| {
            let (options, _, _) =
                super::super::scenario::changed_options(self.input.options, coordinate, delta)?;
            price_input(
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
            )
        };
        let base = self.price(curves)?;
        let plus = price(settings.bump)?;
        let minus = price(-settings.bump)?;
        let half_plus = price(settings.bump / 2.0)?;
        let half_minus = price(-settings.bump / 2.0)?;
        let h = settings.bump;
        let first = finite((plus.present_value - minus.present_value) / (2.0 * h))?;
        let half_first = finite((half_plus.present_value - half_minus.present_value) / h)?;
        let second = finite(
            (plus.present_value - 2.0 * base.present_value + minus.present_value) / (h * h),
        )?;
        let half_second = finite(
            4.0 * (half_plus.present_value - 2.0 * base.present_value + half_minus.present_value)
                / (h * h),
        )?;
        let scale = [
            base.present_value,
            plus.present_value,
            minus.present_value,
            half_plus.present_value,
            half_minus.present_value,
        ]
        .into_iter()
        .map(f64::abs)
        .fold(0.0, f64::max);
        let rounding = finite(32.0 * f64::EPSILON * scale)?;
        if rounding == 0.0 {
            return Err(invalid(
                "parameter derivative resolution",
                "positive representable PV arithmetic scale required",
            ));
        }
        let first_resolution = finite(rounding / (h / 2.0))?;
        let second_resolution = finite(rounding / ((h / 2.0) * (h / 2.0)))?;
        settings.admit(1, first, half_first, first_resolution)?;
        settings.admit(2, second, half_second, second_resolution)?;
        Ok(CouponParameterDerivatives {
            coordinate: coordinate.clone(),
            original_parameter,
            settings,
            first,
            half_first,
            second,
            half_second,
            first_resolution,
            second_resolution,
            base,
            plus,
            minus,
            half_plus,
            half_minus,
        })
    }
}
