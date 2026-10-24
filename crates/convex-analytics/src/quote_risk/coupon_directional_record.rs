//! Supplied directional record conformance; native product proof stays with its owner.
use super::*;
use crate::floating_coupon::{
    directional_parameter_stencil, CouponModelParameter, CouponParameterSide,
};

/// Explicit curvature normalization, separate from the first-order key unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CouponParameterCurvatureUnit {
    /// Currency per squared original native model parameter.
    CurrencyPerModelParameterSquared,
}

/// Complete supplied six-price numerical evidence. Validation checks arithmetic,
/// identity and policies; actual product repricing remains the native owner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CouponDirectionalSensitivity {
    /// Complete original source/point/unit key, without shortened identifiers.
    pub key: QuoteRiskKey,
    /// Actual signed quantity, original reference face and currency.
    pub position: RiskPosition,
    /// Actual supplied information set.
    pub snapshot_id: SnapshotId,
    /// Native and signed first/second precision, selected side and policy version.
    pub settings: CouponDirectionalRiskSettings,
    /// Original native parameter before all hypothetical moves.
    pub original_parameter: f64,
    /// Actual original-side sample parameters in multiplier0,.5,1,1.5,2,3 order.
    pub native_parameters: [f64; 6],
    /// Actual unrounded reference currency PVs in the same order.
    pub native_prices: [f64; 6],
    /// Signed currency per increasing native parameter unit.
    pub first: f64,
    /// Independently evaluated half-step signed derivative.
    pub half_first: f64,
    /// Signed currency per squared native parameter unit.
    pub second: f64,
    /// Independently evaluated half-step signed curvature.
    pub half_second: f64,
    /// Original admitted signed first-order arithmetic guard.
    pub first_resolution: f64,
    /// Original admitted signed second-order arithmetic guard.
    pub second_resolution: f64,
    /// Explicit curvature unit; the original key carries first-order units.
    pub second_unit: CouponParameterCurvatureUnit,
}
impl CouponDirectionalSensitivity {
    /// Recheck every supplied sample, native/signed policy and normalization.
    /// This is not a stochastic model, source approval or order authorization.
    pub fn validate(&self) -> QuoteRiskResult<()> {
        let point = coupon_parameter_coordinate(&self.key)?;
        if self.position.quantity_unit != RiskQuantityUnit::Notional
            || self.key.definition().factor.definition().currency != self.position.currency
            || self.key.definition().native_unit != coupon_parameter_units(point.parameter).1
            || self.key.definition().unit != QuoteRiskUnit::CurrencyPerModelParameter
            || self.key.definition().response != RiskResponse::FrozenCurves
        {
            return Err(invalid(
                "directional record",
                "original notional/currency/native parameter units and frozen response required",
            ));
        }
        self.settings.native.derivative.validate()?;
        self.settings.first.validate()?;
        self.settings.second.validate()?;
        finite(self.original_parameter, "original parameter")?;
        let h = self.settings.native.derivative.bump;
        let step = match self.settings.native.side {
            CouponParameterSide::Forward => h,
            CouponParameterSide::Backward => -h,
        };
        for (i, multiplier) in [0.0, 0.5, 1.0, 1.5, 2.0, 3.0].into_iter().enumerate() {
            let actual = finite(self.native_parameters[i], "native parameter")?;
            let expected = if multiplier == 0.0 {
                self.original_parameter
            } else {
                finite(self.original_parameter + step * multiplier, "native move")?
            };
            if actual != expected || self.native_parameters[..i].contains(&actual) {
                return Err(invalid(
                    "directional parameters",
                    "all exact distinct original-side sample parameters required",
                ));
            }
            let domain = match point.parameter {
                CouponModelParameter::NormalVolatility => (0.0..=1.0).contains(&actual),
                CouponModelParameter::LognormalVolatility
                | CouponModelParameter::LognormalDisplacement => (0.0..=10.0).contains(&actual),
                CouponModelParameter::NormalFactorLoading { .. } => true,
            };
            if !domain {
                return Err(invalid(
                    "directional domain",
                    "original supplied parameter family bounds required",
                ));
            }
            finite(self.native_prices[i], "native PV")?;
        }
        let p = &self.native_prices;
        let (first, second) = directional_parameter_stencil(p[0], p[2], p[4], p[5], step)?;
        let (half_first, half_second) =
            directional_parameter_stencil(p[0], p[1], p[2], p[3], step / 2.0)?;
        let rounding = finite(
            64.0 * f64::EPSILON * p.iter().map(|v| v.abs()).fold(0.0, f64::max),
            "native resolution",
        )?;
        if rounding == 0.0 {
            return Err(invalid(
                "native resolution",
                "positive representable reference PV arithmetic scale required",
            ));
        }
        let native_first_resolution = finite(rounding / (h / 2.0), "native first resolution")?;
        let native_second_resolution = finite(
            rounding / ((h / 2.0) * (h / 2.0)),
            "native second resolution",
        )?;
        self.settings
            .native
            .derivative
            .admit(1, first, half_first, native_first_resolution)?;
        self.settings
            .native
            .derivative
            .admit(2, second, half_second, native_second_resolution)?;
        let scale = self.position.scale()?;
        let (first, half_first, second, half_second) = (
            scaled(first, scale)?,
            scaled(half_first, scale)?,
            scaled(second, scale)?,
            scaled(half_second, scale)?,
        );
        let first_resolution = admit(
            1,
            first,
            half_first,
            native_first_resolution,
            scale,
            self.settings.first,
        )?;
        let second_resolution = admit(
            2,
            second,
            half_second,
            native_second_resolution,
            scale,
            self.settings.second,
        )?;
        if self.first != first
            || self.half_first != half_first
            || self.second != second
            || self.half_second != half_second
            || self.first_resolution != first_resolution
            || self.second_resolution != second_resolution
        {
            return Err(invalid(
                "directional record",
                "reported signed derivatives/resolution disagree with all retained native PVs",
            ));
        }
        for value in self.native_prices {
            scaled(value, scale)?;
        }
        Ok(())
    }
    /// Exact first-order factor policy used by the existing constrained solver.
    pub fn risk_settings(&self) -> QuoteRiskSettings {
        QuoteRiskSettings {
            bump: self.settings.native.derivative.bump,
            stability: self.settings.first,
            policy_version: self.settings.policy_version,
        }
    }
    /// Original signed monetary baseline after validated single position scaling.
    pub fn base_pv(&self) -> QuoteRiskResult<f64> {
        self.validate()?;
        scaled(self.native_prices[0], self.position.scale()?)
    }
}

impl TermCouponDirectionalParameterSensitivity {
    /// Retain all actual native samples in a separately checkable solver record.
    /// The full original product proof remains in this calculation's native field.
    pub fn hedge_record(&self) -> QuoteRiskResult<CouponDirectionalSensitivity> {
        let native_parameters: [f64; 6] = self
            .native
            .prices
            .iter()
            .map(|p| p.parameter)
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| invalid("directional samples", "six original parameters required"))?;
        let native_prices: [f64; 6] = self
            .native
            .prices
            .iter()
            .map(|p| p.valuation.present_value)
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| invalid("directional samples", "six original full PVs required"))?;
        let record = CouponDirectionalSensitivity {
            key: self.key.clone(),
            position: self.position.clone(),
            snapshot_id: self.snapshot_id.clone(),
            settings: self.settings,
            original_parameter: self.native.original_parameter,
            native_parameters,
            native_prices,
            first: self.first,
            half_first: self.half_first,
            second: self.second,
            half_second: self.half_second,
            first_resolution: self.first_resolution,
            second_resolution: self.second_resolution,
            second_unit: CouponParameterCurvatureUnit::CurrencyPerModelParameterSquared,
        };
        record.validate()?;
        Ok(record)
    }
}

/// Explicit complete numerical evidence; old central wire records stay unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(
    tag = "kind",
    content = "result",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HedgeSensitivityEvidence {
    /// Original five-price central record and its complete original policy.
    Central(QuoteSensitivity),
    /// Actual selected-side six-price record with both precision orders.
    Directional(CouponDirectionalSensitivity),
}
