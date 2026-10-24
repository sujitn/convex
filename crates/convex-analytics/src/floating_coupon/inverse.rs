//! One selected closed-bracket root of the complete original native coupon PV.
use super::*;
use convex_core::contracts::ModelMoney;

/// Explicit native parameter/currency convergence and actual work controls.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CouponParameterInverseSettings {
    /// Actual lower parameter in its original native units; never expanded.
    pub lower: f64,
    /// Actual upper parameter in its original native units.
    pub upper: f64,
    /// Absolute full currency-PV target allowance, in (0,1e6].
    pub price_tolerance: f64,
    /// Maximum surviving interior bracket width, in (0,1e-6].
    pub parameter_tolerance: f64,
    /// Bisection iteration ceiling,1..256.
    pub max_iterations: u32,
    /// Full native price ceiling,4..512, including baseline/endpoints/refresh.
    pub max_price_evaluations: usize,
}
impl CouponParameterInverseSettings {
    /// No default price, implied domain expansion or automatic relaxed retry.
    pub fn validate(self) -> Result<()> {
        if !self.lower.is_finite()
            || !self.upper.is_finite()
            || self.lower >= self.upper
            || self.lower < -10.0
            || self.upper > 10.0
            || !self.price_tolerance.is_finite()
            || self.price_tolerance <= 0.0
            || self.price_tolerance > 1e6
            || !self.parameter_tolerance.is_finite()
            || self.parameter_tolerance <= 0.0
            || self.parameter_tolerance > 1e-6
            || !(1..=256).contains(&self.max_iterations)
            || !(4..=512).contains(&self.max_price_evaluations)
        {
            return Err(invalid("parameter inverse settings", "finite closed native domain within10, positive price/parameter precision and bounded work required"));
        }
        Ok(())
    }
}

/// Actual unrounded currency result at one full native parameter valuation.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CouponParameterPricePoint {
    /// Exact actual supplied parameter, not an increment or market publication.
    pub parameter: f64,
    /// Complete native currency PV at that parameter.
    pub present_value: f64,
    /// Actual native PV minus the original target.
    pub residual: f64,
}

/// Endpoint acceptance is distinct from interior width and actual-zero stopping.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CouponInverseTermination {
    /// One endpoint meets the declared price policy; original bracket is retained.
    EndpointPriceTolerance,
    /// An actual native valuation has exactly zero target residual.
    ExactNativeResidual,
    /// Both actual price allowance and surviving parameter width pass.
    RefinedBracket,
}

/// Full original baseline/final state and measured selected-root diagnostics.
/// The result makes no global parameter/model uniqueness claim.
#[derive(Debug, Serialize)]
pub struct CouponParameterInverse {
    /// Complete original source/index/observation/payment/parameter units.
    pub coordinate: CouponParameterCoordinate,
    /// Actual original source parameter before the hypothetical inverse.
    pub original_parameter: f64,
    /// Actual selected parameter in the caller's closed bracket.
    pub parameter: f64,
    /// Original full currency target, without per100 or notional conversion.
    pub target: ModelMoney,
    /// Complete original convergence and work controls.
    pub settings: CouponParameterInverseSettings,
    /// Actual endpoint/zero/width stopping distinction.
    pub termination: CouponInverseTermination,
    /// False: a selected bracketed root does not identify all possible roots.
    pub globally_unique: bool,
    /// Actual completed bisection iterations, including exact evaluated zeros.
    pub iterations: u32,
    /// Actual full price calls, including the final fresh native valuation.
    pub price_evaluations: usize,
    /// Conservative arithmetic guard, not total pricer/model accuracy.
    pub price_resolution: f64,
    /// Actual final currency target residual.
    pub native_residual: f64,
    /// Actual remaining lower bracket; endpoint stopping retains original bounds.
    pub lower: CouponParameterPricePoint,
    /// Actual remaining upper bracket.
    pub upper: CouponParameterPricePoint,
    /// Every actual parameter/PV/residual call in original evaluation order.
    pub evaluations: Vec<CouponParameterPricePoint>,
    /// Complete original baseline with unchanged source revisions.
    pub base: TermCouponOptionPrice,
    /// Fresh full final valuation; only selected parameter value changes.
    pub valuation: TermCouponOptionPrice,
}

struct InverseEvaluator<'a, 'b> {
    prepared: &'a PreparedTermCouponModel<'b>,
    curves: &'a CurveSet,
    coordinate: &'a CouponParameterCoordinate,
    target: f64,
    limit: usize,
    trace: Vec<CouponParameterPricePoint>,
}
impl InverseEvaluator<'_, '_> {
    fn evaluate(
        &mut self,
        parameter: f64,
    ) -> Result<(CouponParameterPricePoint, TermCouponOptionPrice)> {
        if self.trace.len() >= self.limit {
            return Err(TermCouponOptionError::InverseWork {
                price_evaluations: self.trace.len(),
                limit: self.limit,
            });
        }
        let (options, _, actual) = super::super::scenario::parameter_options(
            self.prepared.input.options,
            self.coordinate,
            parameter,
        )?;
        let value = price_input(
            &TermCouponModelInputs {
                bond: self.prepared.input.bond,
                context: self.prepared.input.context,
                history: self.prepared.input.history,
                options: &options,
                projection: self.prepared.input.projection.clone(),
                discount: self.prepared.input.discount.clone(),
            },
            &self.prepared.publications,
            self.curves,
        )?;
        let point = CouponParameterPricePoint {
            parameter: actual,
            present_value: value.present_value,
            residual: finite(value.present_value - self.target)?,
        };
        self.trace.push(point);
        Ok((point, value))
    }
}

struct InverseRoot {
    point: CouponParameterPricePoint,
    lower: CouponParameterPricePoint,
    upper: CouponParameterPricePoint,
    iterations: u32,
    termination: CouponInverseTermination,
}
fn convergence(root: &InverseRoot, evaluations: usize) -> TermCouponOptionError {
    TermCouponOptionError::InverseConvergence {
        iterations: root.iterations,
        price_evaluations: evaluations,
        residual: root.point.residual,
        parameter_width: root.upper.parameter - root.lower.parameter,
    }
}
fn solve_bracket(
    evaluator: &mut InverseEvaluator<'_, '_>,
    settings: CouponParameterInverseSettings,
    mut lower: CouponParameterPricePoint,
    mut upper: CouponParameterPricePoint,
) -> Result<InverseRoot> {
    if lower.residual.abs() <= settings.price_tolerance
        && upper.residual.abs() <= settings.price_tolerance
    {
        return Err(TermCouponOptionError::InverseUnidentified {
            lower: lower.parameter,
            upper: upper.parameter,
            low_residual: lower.residual,
            high_residual: upper.residual,
            price_tolerance: settings.price_tolerance,
        });
    }
    for point in [lower, upper] {
        if point.residual.abs() <= settings.price_tolerance {
            return Ok(InverseRoot {
                point,
                lower,
                upper,
                iterations: 0,
                termination: CouponInverseTermination::EndpointPriceTolerance,
            });
        }
    }
    if lower.residual.is_sign_positive() == upper.residual.is_sign_positive() {
        return Err(TermCouponOptionError::InverseNotBracketed {
            lower: lower.parameter,
            upper: upper.parameter,
            low_residual: lower.residual,
            high_residual: upper.residual,
        });
    }
    let mut root = InverseRoot {
        point: lower,
        lower,
        upper,
        iterations: 0,
        termination: CouponInverseTermination::RefinedBracket,
    };
    for iteration in 1..=settings.max_iterations {
        let mid = 0.5 * lower.parameter + 0.5 * upper.parameter;
        if mid == lower.parameter || mid == upper.parameter {
            return Err(convergence(&root, evaluator.trace.len()));
        }
        let (point, _) = evaluator.evaluate(mid)?;
        if point.residual == 0.0 {
            return Ok(InverseRoot {
                point,
                lower: point,
                upper: point,
                iterations: iteration,
                termination: CouponInverseTermination::ExactNativeResidual,
            });
        }
        if point.residual.is_sign_positive() == lower.residual.is_sign_positive() {
            lower = point;
        } else {
            upper = point;
        }
        root = InverseRoot {
            point: if lower.residual.abs() < upper.residual.abs() {
                lower
            } else {
                upper
            },
            lower,
            upper,
            iterations: iteration,
            termination: CouponInverseTermination::RefinedBracket,
        };
        if upper.parameter - lower.parameter <= settings.parameter_tolerance
            && root.point.residual.abs() <= settings.price_tolerance
        {
            return Ok(root);
        }
    }
    Err(convergence(&root, evaluator.trace.len()))
}

impl PreparedTermCouponModel<'_> {
    /// Select one full native price root in the exact supplied parameter bracket.
    /// Original domains/history/publications/curves stay; no global uniqueness claim.
    pub fn parameter_price_inverse(
        &self,
        curves: &CurveSet,
        coordinate: &CouponParameterCoordinate,
        target: ModelMoney,
        settings: CouponParameterInverseSettings,
    ) -> Result<CouponParameterInverse> {
        settings.validate()?;
        self.admit_baseline(curves)?;
        if target.currency() != self.currency() {
            return Err(invalid(
                "parameter inverse target",
                "original native currency required",
            ));
        }
        let (_, original_parameter, _) =
            super::super::scenario::changed_options(self.input.options, coordinate, 0.0)?;
        let mut evaluator = InverseEvaluator {
            prepared: self,
            curves,
            coordinate,
            target: target.value().get(),
            limit: settings.max_price_evaluations,
            trace: Vec::new(),
        };
        let (base_point, base) = evaluator.evaluate(original_parameter)?;
        if base.present_value != self.baseline().present_value {
            return Err(invalid(
                "parameter inverse baseline",
                "original complete native PV differs",
            ));
        }
        let (lower, _) = evaluator.evaluate(settings.lower)?;
        let (upper, _) = evaluator.evaluate(settings.upper)?;
        let scale = [
            base_point.present_value,
            lower.present_value,
            upper.present_value,
            target.value().get(),
        ]
        .into_iter()
        .map(f64::abs)
        .fold(0.0, f64::max);
        let price_resolution = finite(32.0 * f64::EPSILON * scale)?;
        if scale > 0.0 && price_resolution == 0.0 {
            return Err(invalid(
                "parameter inverse precision",
                "positive native arithmetic scale underflow",
            ));
        }
        if price_resolution > settings.price_tolerance {
            return Err(TermCouponOptionError::InverseResolution {
                resolution: price_resolution,
                price_tolerance: settings.price_tolerance,
            });
        }
        let root = solve_bracket(&mut evaluator, settings, lower, upper)?;
        let (point, valuation) = evaluator.evaluate(root.point.parameter)?;
        if point.residual.abs() > settings.price_tolerance
            || point.present_value != root.point.present_value
        {
            return Err(convergence(&root, evaluator.trace.len()));
        }
        Ok(CouponParameterInverse {
            coordinate: coordinate.clone(),
            original_parameter,
            parameter: point.parameter,
            target,
            settings,
            termination: root.termination,
            globally_unique: false,
            iterations: root.iterations,
            price_evaluations: evaluator.trace.len(),
            price_resolution,
            native_residual: point.residual,
            lower: root.lower,
            upper: root.upper,
            evaluations: evaluator.trace,
            base,
            valuation,
        })
    }
}
