//! Explicit domains, day counts, compounding and supported query capabilities.

use super::{
    build::{point_tenor, positive},
    inputs::{validate_date, validate_tenor},
    *,
};
use crate::{ExtrapolationMethod, InterpolationMethod};
use convex_core::{
    daycounts::DayCountConvention,
    types::{Compounding, Date},
};
use serde::{Deserialize, Serialize};

/// Side of a survival pillar for instantaneous hazard. Derivatives can jump;
/// a caller must choose rather than receive an undocumented averaged intensity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum HazardSide {
    /// Interval ending at the queried pillar.
    Left,
    /// Interval starting at the queried pillar.
    Right,
}

/// Pure query inputs. Rates are decimal annualized; basis and compounding are
/// required. No zero rate is fabricated at the zero-length origin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum CurveQuery {
    /// Analytic continuous MonotoneConvex forward, with an explicit side for
    /// the origin and any declared terminal tail. Other source families fail.
    MonotoneConvexForward {
        /// Original actual date or model time.
        point: CurvePoint,
        /// Original interval or the actual declared tail at a boundary.
        side: ForwardSide,
    },
    /// Original analytic UFR tail forward at/after its explicit LLP, including
    /// the right join limit. Other families and pre-LLP queries fail.
    UfrForward {
        /// Actual date/model time of the declared tail.
        point: CurvePoint,
    },
    /// Exact original segment forward with caller-selected knot side. Available
    /// only on the explicit native FlatForward grid and its declared extension.
    FlatForwardRate {
        /// Actual date or model time.
        point: CurvePoint,
        /// Actual interval at an exact supplied knot.
        side: ForwardSide,
    },
    /// Original parametric instantaneous forward per curve-time year, including
    /// its finite origin limit. Only inside the exact model horizon; no proxy.
    InstantaneousForward {
        /// Actual date or model time of the supplied parametric model.
        point: CurvePoint,
    },
    /// Actual interpolated representation value.
    Raw {
        /// Date or model time.
        point: CurvePoint,
    },
    /// Discount factor from reference date.
    DiscountFactor {
        /// Date or model time.
        point: CurvePoint,
    },
    /// P(end)/P(start), including an explicitly allowed forward-start origin.
    DiscountRatio {
        /// Period start.
        start: CurvePoint,
        /// Period end.
        end: CurvePoint,
    },
    /// Equivalent rate from P(date), using the requested year fraction.
    ZeroRate {
        /// Date or model time.
        point: CurvePoint,
        /// Output rate compounding.
        compounding: Compounding,
        /// Output rate basis.
        day_count: DayCountConvention,
    },
    /// Equivalent rate from P(end)/P(start); date periods use the actual
    /// start/end year fraction, not a difference of rounded nominal tenors.
    ForwardRate {
        /// Period start.
        start: CurvePoint,
        /// Period end.
        end: CurvePoint,
        /// Output compounding over that period.
        compounding: Compounding,
        /// Output accrual basis.
        day_count: DayCountConvention,
    },
    /// Credit survival probability.
    SurvivalProbability {
        /// Date or model time.
        point: CurvePoint,
    },
    /// Probability of default between boundaries, unconditional Q(start)-Q(end)
    /// or conditional on survival to start: 1-Q(end)/Q(start).
    DefaultProbability {
        /// Start boundary.
        start: CurvePoint,
        /// End boundary.
        end: CurvePoint,
        /// Whether to condition on survival to start.
        conditional: bool,
    },
    /// Instantaneous hazard per year of curve time, with explicit knot side.
    HazardRate {
        /// Date or model time.
        point: CurvePoint,
        /// Derivative interval at a pillar.
        side: HazardSide,
    },
}

/// Numeric result units; hazard is per year of the curve's declared axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CurveQueryUnit {
    /// Annualized decimal rate.
    DecimalRate,
    /// Positive discount factor or ratio.
    DiscountFactor,
    /// Dimensionless probability in `[0,1]`.
    Probability,
    /// Intensity per curve-time year.
    HazardPerYear,
}

/// Result with actual conventions and extrapolation disclosure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CurveQueryValue {
    /// Finite value in unit.
    pub value: f64,
    /// Actual numeric unit.
    pub unit: CurveQueryUnit,
    /// Exact immutable curve/version.
    pub curve: CurveKey,
    /// Date-to-model-time convention.
    pub time_basis: DayCountConvention,
    /// Output rate basis, where applicable.
    pub day_count: Option<DayCountConvention>,
    /// Output rate compounding, where applicable.
    pub compounding: Option<Compounding>,
    /// Whether any positive-time query point used extension beyond the node grid.
    pub extrapolated: bool,
}

/// Raw grid domain and explicit extension policy. Positive time outside the grid
/// fails with None; the discount origin is always P(0)=1 by definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CurveDomain {
    /// First raw pillar time.
    pub minimum_tenor: f64,
    /// Last raw pillar time.
    pub maximum_tenor: f64,
    /// Absolute query ceiling even when extension is enabled.
    pub query_ceiling: f64,
    /// Actual time basis.
    pub time_basis: DayCountConvention,
    /// Extension applied to the interpolated representation value.
    pub extrapolation: ExtrapolationMethod,
}

impl BuiltCurve {
    /// Explicit supported query domain/policy, not a certification of market fit.
    pub fn domain(&self) -> CurveDomain {
        let (minimum_tenor, maximum_tenor) = self.bounds();
        CurveDomain {
            minimum_tenor,
            maximum_tenor,
            query_ceiling: MAX_CURVE_TENOR,
            time_basis: self.recipe().time_basis,
            extrapolation: self.recipe().extrapolation,
        }
    }
    /// Convert an actual date using the recipe's day count, including leap years.
    pub fn date_to_tenor(&self, date: Date) -> CurveContractResult<f64> {
        point_tenor(
            self.recipe().reference_date,
            self.recipe().time_basis,
            CurvePoint::Date(date),
        )
    }
    /// Convert model time to a canonical date using an explicit rounding rule.
    /// This is a bounded inverse search (at most 16 bisections over 54,900 days),
    /// including ACT/ACT ISDA; no nominal 365-day approximation is used.
    pub fn tenor_to_date(&self, tenor: f64, rounding: TenorRounding) -> CurveContractResult<Date> {
        validate_tenor(tenor)?;
        let reference = self.recipe().reference_date;
        let final_date = Date::from_ymd(9999, 12, 31)
            .map_err(|e| CurveContractError::Numerical(e.to_string()))?;
        let mut low = 0_i64;
        let mut high = reference
            .days_between(&final_date)
            .min(convex_core::cashflows::MAX_CONTRACT_DAYS);
        if high == 0 {
            if tenor == 0.0 {
                return Ok(reference);
            }
            return Err(CurveContractError::invalid(
                "tenor",
                "no later canonical date",
            ));
        }
        let time_at = |days: i64| -> CurveContractResult<f64> {
            if days == 0 {
                return Ok(0.0);
            }
            let date = reference
                .checked_add_days(days)
                .map_err(|e| CurveContractError::Numerical(e.to_string()))?;
            year_fraction(reference, date, self.recipe().time_basis)
        };
        if tenor > time_at(high)? {
            return Err(CurveContractError::invalid(
                "tenor",
                "outside canonical date span",
            ));
        }
        for _ in 0..16 {
            if high - low <= 1 {
                break;
            }
            let mid = low + (high - low) / 2;
            if time_at(mid)? <= tenor {
                low = mid;
            } else {
                high = mid;
            }
        }
        if high - low > 1 {
            return Err(CurveContractError::BoundExceeded("inverse date search"));
        }
        let before = time_at(low)?;
        let after = time_at(high)?;
        let days = if tenor == before {
            low
        } else if tenor == after {
            high
        } else {
            match rounding {
                TenorRounding::AtOrBefore => low,
                TenorRounding::AtOrAfter => high,
                TenorRounding::Nearest => {
                    if tenor - before <= after - tenor {
                        low
                    } else {
                        high
                    }
                }
            }
        };
        let date = reference
            .checked_add_days(days)
            .map_err(|e| CurveContractError::Numerical(e.to_string()))?;
        validate_date(date)?;
        Ok(date)
    }
    /// Execute a fallible query with finite/domain postconditions. Unsupported
    /// curve roles, convention-dependent queries and extrapolation are explicit.
    pub fn query(&self, query: &CurveQuery) -> CurveContractResult<CurveQueryValue> {
        let recipe = self.recipe();
        let mut day_count = None;
        let mut compounding = None;
        let mut times = Vec::with_capacity(2);
        let mut time = |point: CurvePoint| -> CurveContractResult<f64> {
            let t = point_tenor(recipe.reference_date, recipe.time_basis, point)?;
            times.push(t);
            Ok(t)
        };
        let (value, unit) = match *query {
            CurveQuery::MonotoneConvexForward { point, side } => {
                return self.query_monotone_forward(point, side);
            }
            CurveQuery::UfrForward { point } => {
                return self.query_ufr_forward(point);
            }
            CurveQuery::FlatForwardRate { point, side } => {
                return self.query_flat_forward(point, side);
            }
            CurveQuery::InstantaneousForward { point } => {
                let t = time(point)?;
                day_count = Some(recipe.time_basis);
                compounding = Some(Compounding::Continuous);
                (self.instantaneous_forward(t)?, CurveQueryUnit::DecimalRate)
            }
            CurveQuery::Raw { point } => {
                let t = time(point)?;
                (self.raw(t)?, self.raw_unit())
            }
            CurveQuery::DiscountFactor { point } => {
                (self.discount(time(point)?)?, CurveQueryUnit::DiscountFactor)
            }
            CurveQuery::DiscountRatio { start, end } => {
                let (t1, t2) = (time(start)?, time(end)?);
                ordered(t1, t2)?;
                let ratio = self.discount(t2)? / self.discount(t1)?;
                positive(ratio, "discount ratio")?;
                (ratio, CurveQueryUnit::DiscountFactor)
            }
            CurveQuery::ZeroRate {
                point,
                compounding: c,
                day_count: dc,
            } => {
                let t = time(point)?;
                ordered(0.0, t)?;
                let tau = self.query_fraction(
                    CurvePoint::Date(recipe.reference_date),
                    point,
                    0.0,
                    t,
                    dc,
                )?;
                day_count = Some(dc);
                compounding = Some(c);
                (
                    rate_from_discount(self.discount(t)?, tau, c)?,
                    CurveQueryUnit::DecimalRate,
                )
            }
            CurveQuery::ForwardRate {
                start,
                end,
                compounding: c,
                day_count: dc,
            } => {
                let (t1, t2) = (time(start)?, time(end)?);
                ordered(t1, t2)?;
                let tau = self.query_fraction(start, end, t1, t2, dc)?;
                day_count = Some(dc);
                compounding = Some(c);
                let ratio = self.discount(t2)? / self.discount(t1)?;
                (
                    rate_from_discount(ratio, tau, c)?,
                    CurveQueryUnit::DecimalRate,
                )
            }
            CurveQuery::SurvivalProbability { point } => {
                (self.survival(time(point)?)?, CurveQueryUnit::Probability)
            }
            CurveQuery::DefaultProbability {
                start,
                end,
                conditional,
            } => {
                let (t1, t2) = (time(start)?, time(end)?);
                ordered(t1, t2)?;
                (
                    self.default_probability(t1, t2, conditional)?,
                    CurveQueryUnit::Probability,
                )
            }
            CurveQuery::HazardRate { point, side } => (
                self.hazard(time(point)?, side)?,
                CurveQueryUnit::HazardPerYear,
            ),
        };
        // Discount/survival queries use the exact origin identity without a
        // source evaluation. Raw queries do evaluate a delegated origin.
        times.retain(|&t| t > 0.0 || matches!(query, CurveQuery::Raw { .. }));
        self.query_value(value, unit, day_count, compounding, &times)
    }
    fn query_monotone_forward(
        &self,
        point: CurvePoint,
        side: ForwardSide,
    ) -> CurveContractResult<CurveQueryValue> {
        let recipe = self.recipe();
        let t = point_tenor(recipe.reference_date, recipe.time_basis, point)?;
        let (value, extended) = self.monotone_forward(t, side)?;
        let mut result = self.query_value(
            value,
            CurveQueryUnit::DecimalRate,
            Some(recipe.time_basis),
            Some(Compounding::Continuous),
            &[t],
        )?;
        result.extrapolated |= extended;
        Ok(result)
    }
    fn query_ufr_forward(&self, point: CurvePoint) -> CurveContractResult<CurveQueryValue> {
        let recipe = self.recipe();
        let t = point_tenor(recipe.reference_date, recipe.time_basis, point)?;
        let mut result = self.query_value(
            self.ufr_forward(t)?,
            CurveQueryUnit::DecimalRate,
            Some(recipe.time_basis),
            Some(Compounding::Continuous),
            &[t],
        )?;
        // This query consumes the declared tail, including its exact right
        // boundary limit; raw/discount at the join use the original base.
        result.extrapolated = true;
        Ok(result)
    }
    fn query_flat_forward(
        &self,
        point: CurvePoint,
        side: ForwardSide,
    ) -> CurveContractResult<CurveQueryValue> {
        let recipe = self.recipe();
        let t = point_tenor(recipe.reference_date, recipe.time_basis, point)?;
        let (value, extended) = self.flat_forward(t, side)?;
        let mut result = self.query_value(
            value,
            CurveQueryUnit::DecimalRate,
            Some(recipe.time_basis),
            Some(Compounding::Continuous),
            &[t],
        )?;
        result.extrapolated |= extended;
        Ok(result)
    }
    fn query_value(
        &self,
        value: f64,
        unit: CurveQueryUnit,
        day_count: Option<DayCountConvention>,
        compounding: Option<Compounding>,
        times: &[f64],
    ) -> CurveContractResult<CurveQueryValue> {
        let recipe = self.recipe();
        if !value.is_finite() {
            return Err(CurveContractError::Numerical(
                "nonfinite query result".into(),
            ));
        }
        let extrapolated = times.iter().any(|&t| self.extension_used(t));
        Ok(CurveQueryValue {
            value,
            unit,
            curve: recipe.key.clone(),
            time_basis: recipe.time_basis,
            day_count,
            compounding,
            extrapolated,
        })
    }
    fn raw_unit(&self) -> CurveQueryUnit {
        match self.recipe().representation {
            CurveRepresentation::DiscountFactors => CurveQueryUnit::DiscountFactor,
            CurveRepresentation::SurvivalProbabilities => CurveQueryUnit::Probability,
            _ => CurveQueryUnit::DecimalRate,
        }
    }
    fn default_probability(&self, t1: f64, t2: f64, conditional: bool) -> CurveContractResult<f64> {
        let (q1, q2) = (self.survival(t1)?, self.survival(t2)?);
        let value = if conditional { 1.0 - q2 / q1 } else { q1 - q2 };
        if !(0.0..=1.0).contains(&value) {
            return Err(CurveContractError::Numerical(
                "default probability outside [0,1]".into(),
            ));
        }
        Ok(value)
    }
    fn query_fraction(
        &self,
        start: CurvePoint,
        end: CurvePoint,
        t1: f64,
        t2: f64,
        dc: DayCountConvention,
    ) -> CurveContractResult<f64> {
        if let (CurvePoint::Date(d1), CurvePoint::Date(d2)) = (start, end) {
            return year_fraction(d1, d2, dc);
        }
        if dc != self.recipe().time_basis {
            return Err(CurveContractError::unsupported(
                "tenor rate basis conversion",
                "use actual Date points to request a different day-count basis",
            ));
        }
        Ok(t2 - t1)
    }
    fn hazard(&self, t: f64, side: HazardSide) -> CurveContractResult<f64> {
        let q = self.survival(t)?;
        let (tenors, values) = self.probability_grid()?;
        let first = *tenors
            .first()
            .ok_or_else(|| CurveContractError::invalid("hazard", "empty grid"))?;
        let last = *tenors
            .last()
            .ok_or_else(|| CurveContractError::invalid("hazard", "empty grid"))?;
        if t < first || t > last || (t == last && side == HazardSide::Right) {
            if self.recipe().extrapolation == ExtrapolationMethod::Flat {
                return Ok(0.0);
            }
            return Err(CurveContractError::invalid(
                "hazard.side",
                "selected interval outside the grid without Flat extension",
            ));
        }
        if t == first && side == HazardSide::Left {
            return Err(CurveContractError::invalid(
                "hazard.side",
                "no interval before the origin",
            ));
        }
        let upper = match side {
            HazardSide::Left => tenors.partition_point(|v| *v < t),
            HazardSide::Right => tenors.partition_point(|v| *v <= t),
        };
        if upper == 0 || upper >= tenors.len() {
            return Err(CurveContractError::invalid(
                "hazard",
                "no selected interval",
            ));
        }
        let dt = tenors[upper] - tenors[upper - 1];
        let result = match self.recipe().interpolation {
            InterpolationMethod::LogLinear => -(values[upper] / values[upper - 1]).ln() / dt,
            InterpolationMethod::Linear => -(values[upper] - values[upper - 1]) / dt / q,
            _ => {
                return Err(CurveContractError::unsupported(
                    "hazard",
                    "requires monotone Linear or LogLinear survival interpolation",
                ))
            }
        };
        if !result.is_finite() || result < 0.0 {
            return Err(CurveContractError::Numerical(
                "negative/nonfinite hazard".into(),
            ));
        }
        Ok(result)
    }
}

fn ordered(t1: f64, t2: f64) -> CurveContractResult<()> {
    if t2 <= t1 {
        return Err(CurveContractError::invalid(
            "query.period",
            "end must strictly follow start; a zero-length rate is undefined",
        ));
    }
    Ok(())
}
/// Explicit date rounding; nearest ties choose the earlier date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum TenorRounding {
    /// Greatest representable date no later than tenor.
    AtOrBefore,
    /// Smallest representable date no earlier than tenor.
    AtOrAfter,
    /// Closest model time, with earlier-date ties.
    Nearest,
}

/// A capability statement is implementation discovery, not a model certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CurveCapabilityStatus {
    /// Callable checked route for its stated inputs.
    Supported,
    /// Terms are representable; algorithm/model is deliberately pending.
    DescribedUnsupported,
}

/// Pure static discovery for the checked contract route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CurveCapability {
    /// Named construction/query family.
    pub name: &'static str,
    /// Actual dispatch status.
    pub status: CurveCapabilityStatus,
    /// Required combination and unsupported boundary.
    pub conditions: &'static str,
}

/// Discover supported combinations without building objects or starting a service.
pub fn curve_capabilities() -> Vec<CurveCapability> {
    use CurveCapabilityStatus::{DescribedUnsupported as Pending, Supported};
    vec![
        CurveCapability { name: "direct_rate_nodes", status: Supported, conditions: "DiscountFactors or ZeroRates; Linear, LogLinear on DF, CubicSpline; rate roles; validated compounding" },
        CurveCapability { name: "dated_parametric_curves", status: Supported, conditions: "Complete original NelsonSiegel/Svensson continuous-zero parameters, actual time basis, explicit finite horizon, None/raw Flat tails and every supplied dated zero quote revision/native residual; exact model/DF/finite-period and in-horizon instantaneous forward queries; no sampled grid" },
        CurveCapability { name: "native_parametric_zero_fit", status: Supported, conditions: "Explicit fit_parametric_curve; original four/six closed parameter bounds, 1..128 actual dated native-unit zeros/scales/weights, original bounded least squares; full undamped weighted identification and every native residual mandatory, including zero-weight quotes; measured solver failures return no curve" },
        CurveCapability { name: "dated_segmented_delegated_curves", status: Supported, conditions: "Same-role/currency/reference/time basis/representation rate DF/zero sources at exact versions; complete native fallback/ranges; 1..128 contiguous segments from zero, explicit raw C0 join bounds; source tails disclosed, <=32 derivations; no grid/integration alias" },
        CurveCapability { name: "explicit_step_zero_grid", status: Supported, conditions: "Complete PiecewiseConstant source/method; continuous-zero rate roles, right-owned steps/final supplied endpoint, 2..1024 native-unit nodes; no hazard/DF/basis reinterpretation or derivative at a jump" },
        CurveCapability { name: "explicit_flat_forward_grid", status: Supported, conditions: "Complete FlatForward source/method; original continuous-zero forward integrals, original finite negative-rate domain, explicit origin consistency; None/raw Flat/Linear or exact original forward extension" },
        CurveCapability { name: "one_sided_flat_forward_queries", status: Supported, conditions: "Explicit FlatForwardRate Left/Right on original grid/extension, time basis and continuous units; absent sides fail, actual side/source extension disclosed including zero; no proxy on step/derived/parametric sources" },
        CurveCapability { name: "explicit_ufr_convergence", status: Supported, conditions: "Exact original FlatForward or NelsonSiegel/Svensson continuous-zero source and role/currency/reference/time basis/method/version; supplied LLP inside native domain, analytic left boundary, positive alpha and decimal UFR; integrated exponential tail, no short-end or derivative proxy; original finite query/depth bounds; heuristic, not Smith-Wilson" },
        CurveCapability { name: "analytic_ufr_forward", status: Supported, conditions: "Explicit UfrForward at/after declared LLP, original continuous rate per curve-time year; actual tail consumption disclosed even at right join; no pre-LLP or other-family derivative substitution" },
        CurveCapability { name: "complete_monotone_convex", status: Supported, conditions: "Complete supplied continuous-zero rate source, original weighted/endpoint estimates, simultaneous nonnegative bounds and all four Hagan-West regimes; exact one-zero-deviation axes use the original zero-area quadratic, no amelioration; original positive nodes, nonnegative interval means including origin, finite work and native units/time basis; no DF/hazard/signed-forward reinterpretation" },
        CurveCapability { name: "analytic_monotone_forward", status: Supported, conditions: "Explicit MonotoneConvexForward Left/Right over matching analytic forward/integral; origin Left and undeclared tail fail; None/raw Flat-zero or integrated FlatForward tail, actual terminal side consumption disclosed; no sampled derivative or family proxy" },
        CurveCapability { name: "direct_credit_survival", status: Supported, conditions: "Credit role; positive nonincreasing Q with explicit origin; Linear/LogLinear; None/Flat extrapolation" },
        CurveCapability { name: "direct_issuer_basis", status: Supported, conditions: "IssuerSpread/IssuerSpreads; signed annualized raw Linear grid; explicit None/Flat/Linear extension; no discount/survival query; analytics owns the declared cash/recovery model" },
        CurveCapability { name: "dated_deposit_fra_piecewise", status: Supported, conditions: "Unfixed simple rates; exact solved start pillars; DF Linear/LogLinear; native quote repricing; <=1023 helpers" },
        CurveCapability { name: "continuous_zero_spread", status: Supported, conditions: "Exact base version; same currency/reference/time basis and inherited methods; annualized rate spread" },
        CurveCapability { name: "discount_zero_forward_queries", status: Supported, conditions: "Explicit period/compounding/basis; different output basis requires dates; origin rates undefined" },
        CurveCapability { name: "survival_default_hazard_queries", status: Supported, conditions: "Credit survival only; conditional flag and hazard knot side explicit; intensity per curve-time year" },
        CurveCapability { name: "date_tenor_conversion", status: Supported, conditions: "ACT/360, ACT/365F, ACT/ACT ISDA; canonical years 1..9999; explicit inverse rounding" },
        CurveCapability { name: "discount_equivalent_views_and_rolling", status: Supported, conditions: "Immutable exact base; inherited role/currency/time basis/methods; explicit sticky-forward/sticky-zero; no resampling; <=32 derived views" },
        CurveCapability { name: "dated_money_market_global_fit", status: Supported, conditions: "Explicit fit_money_market_curve API; bounded scaled native objective and all-quote postconditions; identified continuous-zero parameters; original recipe dispatch remains v1" },
        CurveCapability { name: "extrapolation", status: Supported, conditions: "None, raw-value Flat, raw-value Linear for rate curves; valid output postconditions; ceiling 150 years" },
        CurveCapability { name: "product_helper_global_calibration", status: Pending, conditions: "Generic description-only recipe dispatch remains unsupported; explicit analytics credit/rate/cash_calibration APIs compose full supplied product helpers and bounded fits" },
        CurveCapability { name: "other_interpolation_extrapolation", status: Pending, conditions: "Signed-forward or ameliorated MonotoneConvex variants, UFR joins over other native families, regulatory Smith-Wilson, product-price method calibration, quote-risk rebuilding and credit/basis segmented hazard/integration contracts retain separate constraints; no method substitution" },
    ]
}
