//! Explicit hypothetical credit inputs. Scenario effects are not additive risk
//! buckets or realized ledger events; native quote CS01 is separate from upfront.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::credit_calibration::*;
use convex_core::{
    contracts::{
        EntityId, FactorFamily, FactorTarget, FiniteF64, InstrumentId, ModelId, QuoteType,
        RiskBasis, RiskFactorKey, Version,
    },
    daycounts::DayCountConvention,
    Currency, Date,
};
use convex_credit::{
    price_cds, CdsCalendars, CdsContract, CdsPricingContext, CdsSide, CdsValuation,
    CreditReference, InitialPaymentState, ReferencePin, Seniority,
};
use convex_curves::{
    contracts::{
        build_curve_set, BuiltCurve, CalibrationMode, CurveKey, CurveNode, CurvePoint, CurveQuery,
        CurveRepresentation, CurveRole, CurveSet, CurveSource, FitNumericalControls, MarketQuote,
        QuoteUnit,
    },
    ExtrapolationMethod, InterpolationMethod,
};
use convex_math::portable;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Economic response held fixed or rebuilt under a parameter shock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CreditResponse {
    /// Hold the actual calibrated survival probabilities/intensities fixed.
    HoldSurvival,
    /// Recalibrate CDS inputs at unchanged ORIGINAL quotes and tolerances.
    RecalibrateQuotes,
}
/// One hypothetical quote change; the retained version is its original source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CdsQuoteMove {
    /// Exact original helper identity.
    pub helper_id: String,
    /// Exact source observation being shocked; never a latest lookup.
    pub expected_quote_version: Version,
    /// Signed change in that helper's original unit.
    pub delta: MarketQuote,
}
/// Disjoint shock axes. A CDS quote move cannot also carry an interest-rate move.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum CreditShock {
    /// Parallel continuous-zero shift of one actual collateral DF curve.
    DiscountZero {
        /// Caller-assigned hypothetical version of the same discount identity.
        output_discount: CurveKey,
        /// Signed decimal annualized continuous-zero move; this is node risk.
        shift_decimal: f64,
        /// Whether survival is held or dependent credit is recalibrated.
        credit_response: CreditResponse,
    },
    /// CDS running or clean/dirty upfront input moves, followed by full rebuilding.
    Quotes {
        /// Unique explicit helper moves; missing IDs and unit mismatches fail.
        moves: Vec<CdsQuoteMove>,
    },
    /// New fractional recovery assumption, independent of rates and quotes.
    Recovery {
        /// Fraction in `[0,1]`; full recovery requires holding survival fixed.
        fraction: FiniteF64,
        /// Fixed intensities and quote-recalibrated recovery have different effects.
        credit_response: CreditResponse,
    },
}
/// Immutable hypothetical identity and exactly one shock axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CreditScenarioRequest {
    /// Caller-assigned scenario identity; no published quote version is invented.
    pub id: String,
    /// New caller-assigned hypothetical version of the same credit identity.
    pub output_credit: CurveKey,
    /// Complete shock semantics.
    pub shock: CreditShock,
}
/// Actual immutable original inputs, used to prepare a checked risk workflow.
pub struct CreditRiskInput<'a> {
    /// Full original native-quote calibration recipe.
    pub recipe: &'a CdsCurveRecipe,
    /// Actual calendars in original helper order.
    pub helper_calendars: &'a [CdsCalendars<'a>],
    /// Actual supplied discount dependencies, excluding the credit output.
    pub supplied: &'a CurveSet,
    /// Full checked trade being repriced; its key/reference must match the fit.
    pub trade: &'a CdsContract,
    /// Actual trade calendar pins, also used to validate hypothetical terms.
    pub trade_calendars: CdsCalendars<'a>,
    /// Current surviving valuation and cash state.
    pub context: &'a CdsPricingContext,
    /// Explicit numerical identification controls.
    pub controls: FitNumericalControls,
    /// Exact caller-selected risk semantics revision, checked against factor keys.
    pub risk_model: ModelId,
}
/// Private checked baseline; callers cannot fabricate a fitted risk baseline.
pub struct PreparedCreditRisk<'a> {
    input: CreditRiskInput<'a>,
    baseline: CalibratedCdsCurve,
    base_value: CdsValuation,
}
impl PreparedCreditRisk<'_> {
    /// Full original actually calibrated inputs and measured diagnostics.
    pub fn baseline(&self) -> &CalibratedCdsCurve {
        &self.baseline
    }
    /// Actual original signed trade PV.
    pub fn base_value(&self) -> &CdsValuation {
        &self.base_value
    }
}
/// Prepare one actual full calibration and baseline pricing without side effects.
pub fn prepare_credit_risk(
    input: CreditRiskInput<'_>,
) -> Result<PreparedCreditRisk<'_>, CreditCalibrationError> {
    if input.trade.terms().survival_curve != input.recipe.curve.key
        || input.context.valuation_date != input.recipe.curve.reference_date
    {
        return Err(invalid(
            "risk.baseline",
            "trade credit key and valuation must match the exact fit",
        ));
    }
    let output = build_credit_curve_set(
        &[CdsCurveBuildInput {
            recipe: input.recipe,
            calendars: input.helper_calendars,
        }],
        input.supplied,
        input.controls,
    )?;
    let value = price_cds(input.trade, input.context, &output.curves)?;
    let baseline = output
        .calibrations
        .into_iter()
        .next()
        .ok_or_else(|| invalid("risk.baseline", "missing actual calibration"))?;
    Ok(PreparedCreditRisk {
        input,
        baseline,
        base_value: value,
    })
}
/// Complete hypothetical effect. Held survival has no fabricated repricing
/// certificate; an actual rebuild includes its full financial diagnostics.
#[derive(Debug, Clone)]
pub struct CreditScenarioResult {
    /// Exact request, including the original quote revisions and shock units.
    pub request: CreditScenarioRequest,
    /// Actual original trade valuation.
    pub base: CdsValuation,
    /// Actual trade valuation under exactly the specified shock.
    pub shocked: CdsValuation,
    /// Signed shocked minus original monetary PV, not an additive attribution.
    pub pv_change_currency: f64,
    /// Actual hypothetical credit input; role/recovery and supplied keys retained.
    pub credit_curve: BuiltCurve,
    /// Present only when all original native quotes were actually rebuilt.
    pub calibration: Option<CalibratedCdsCurve>,
    /// Full changed economic/quote input recipe; quote versions remain original
    /// source observations and are qualified by the hypothetical request.
    pub scenario_recipe: CdsCurveRecipe,
    /// Complete actual scenario inputs, reusable for other supplied trades.
    pub curves: CurveSet,
}
fn revision(base: &CurveKey, next: &CurveKey) -> Result<(), CreditCalibrationError> {
    next.validate()?;
    if base.id != next.id || base.version == next.version {
        return Err(invalid(
            "scenario.version",
            "same identity and distinct explicit hypothetical version required",
        ));
    }
    Ok(())
}
fn copies_without(supplied: &CurveSet, excluded: &str) -> Result<CurveSet, CreditCalibrationError> {
    let mut next = CurveSet::new();
    for key in supplied.keys().filter(|key| key.id != excluded) {
        next = next.with_curve(supplied.get(key)?.as_ref().clone())?;
    }
    Ok(next)
}
fn shifted_discount(
    base: &BuiltCurve,
    key: CurveKey,
    shift: f64,
) -> Result<BuiltCurve, CreditCalibrationError> {
    if !shift.is_finite()
        || shift.abs() > 1.0
        || base.recipe().representation != CurveRepresentation::DiscountFactors
        || base.recipe().time_basis != DayCountConvention::Act365Fixed
        || base.recipe().interpolation != InterpolationMethod::LogLinear
        || base.recipe().extrapolation != ExtrapolationMethod::None
    {
        return Err(invalid("discount.shock","finite continuous-zero move within [-1,1] on an actual ACT/365F log-linear DF grid required"));
    }
    let (times, values) = base.interpolation_grid()?;
    let nodes = times
        .iter()
        .zip(values)
        .map(|(&t, &p)| CurveNode {
            pillar: CurvePoint::Tenor(t),
            quote: MarketQuote {
                value: p * portable::exp(-shift * t),
                unit: QuoteUnit::DiscountFactor,
            },
        })
        .collect();
    let mut recipe = base.recipe().clone();
    recipe.key = key;
    recipe.source = CurveSource::Nodes(nodes);
    recipe.dependencies.clear();
    recipe.calibration = CalibrationMode::Direct;
    let curves = build_curve_set(&[recipe.clone()], &CurveSet::new())?.curves;
    Ok(curves.get(&recipe.key)?.as_ref().clone())
}
/// Apply one disjoint shock and reprice the SAME trade economics. Neither rates
/// nor recovery silently mutate CDS market quotes. Nonlinear effects are not
/// asserted to sum across different risk-response definitions.
pub fn evaluate_credit_scenario(
    prepared: &PreparedCreditRisk<'_>,
    request: &CreditScenarioRequest,
) -> Result<CreditScenarioResult, CreditCalibrationError> {
    let input = &prepared.input;
    let original = input.recipe;
    revision(&original.curve.key, &request.output_credit)?;
    if !label(&request.id) {
        return Err(invalid("scenario.id", "bounded explicit identity required"));
    }
    let mut recipe = original.clone();
    recipe.curve.key = request.output_credit.clone();
    for h in &mut recipe.helpers {
        h.terms.survival_curve = request.output_credit.clone();
    }
    let mut trade = input.trade.terms().clone();
    trade.survival_curve = request.output_credit.clone();
    let mut supplied = input.supplied.clone();
    let response = match &request.shock {
        CreditShock::DiscountZero {
            output_discount,
            shift_decimal,
            credit_response,
        } => {
            let base_key = &input.trade.terms().discount_curve;
            revision(base_key, output_discount)?;
            let curve = shifted_discount(
                supplied.get(base_key)?.as_ref(),
                output_discount.clone(),
                *shift_decimal,
            )?;
            supplied = copies_without(&supplied, &base_key.id)?.with_curve(curve)?;
            for h in &mut recipe.helpers {
                if h.terms.discount_curve == *base_key {
                    h.terms.discount_curve = output_discount.clone();
                }
            }
            for key in &mut recipe.curve.dependencies {
                if *key == *base_key {
                    *key = output_discount.clone();
                }
            }
            trade.discount_curve = output_discount.clone();
            *credit_response
        }
        CreditShock::Quotes { moves } => {
            if moves.is_empty() || moves.len() > recipe.helpers.len() {
                return Err(invalid(
                    "quote.moves",
                    "bounded nonempty explicit quote changes required",
                ));
            }
            let mut seen = BTreeSet::new();
            for m in moves {
                if !seen.insert(&m.helper_id)
                    || !m.delta.value.is_finite()
                    || m.delta.value.abs() > 1e12
                {
                    return Err(invalid(
                        "quote.moves",
                        "unique finite source-unit changes required",
                    ));
                }
                let h = recipe
                    .helpers
                    .iter_mut()
                    .find(|h| h.id == m.helper_id)
                    .ok_or_else(|| invalid("quote.helper", "missing original helper"))?;
                if h.quote_version != m.expected_quote_version || h.quote.unit != m.delta.unit {
                    return Err(invalid(
                        "quote.move",
                        "exact original quote version and native unit required",
                    ));
                }
                h.quote.value += m.delta.value;
                h.quote.validate()?;
            }
            CreditResponse::RecalibrateQuotes
        }
        CreditShock::Recovery {
            fraction,
            credit_response,
        } => {
            if !(0.0..=1.0).contains(&fraction.get())
                || (*credit_response == CreditResponse::RecalibrateQuotes && fraction.get() == 1.0)
            {
                return Err(invalid(
                    "recovery",
                    "recovery must be in [0,1]; full recovery cannot identify a quote-recalibrated curve",
                ));
            }
            let CurveRole::Credit { recovery, .. } = &mut recipe.curve.role else {
                return Err(invalid("role", "missing credit role"));
            };
            *recovery = fraction.get();
            for h in &mut recipe.helpers {
                h.terms.reference.recovery_fraction = *fraction;
            }
            trade.reference.recovery_fraction = *fraction;
            *credit_response
        }
    };
    let (curve, calibration, curves) = if response == CreditResponse::RecalibrateQuotes {
        let output = build_credit_curve_set(
            &[CdsCurveBuildInput {
                recipe: &recipe,
                calendars: input.helper_calendars,
            }],
            &supplied,
            input.controls,
        )?;
        let c = output
            .calibrations
            .into_iter()
            .next()
            .ok_or_else(|| invalid("scenario.calibration", "missing actual fit"))?;
        (c.curve.clone(), Some(c), output.curves)
    } else {
        let mut frozen = recipe.curve.clone();
        let (times, values) = prepared.baseline.curve.interpolation_grid()?;
        frozen.source = CurveSource::Nodes(
            times
                .iter()
                .zip(values)
                .map(|(&t, &q)| CurveNode {
                    pillar: CurvePoint::Tenor(t),
                    quote: MarketQuote {
                        value: q,
                        unit: QuoteUnit::SurvivalProbability,
                    },
                })
                .collect(),
        );
        frozen.calibration = CalibrationMode::Direct;
        frozen.dependencies.clear();
        let output = build_curve_set(&[frozen], &supplied)?;
        let curve = output.curves.get(&request.output_credit)?.as_ref().clone();
        (curve, None, output.curves)
    };
    let contract = trade.build(&input.trade_calendars)?;
    let shocked = price_cds(&contract, input.context, &curves)?;
    let delta = shocked.net_pv - prepared.base_value.net_pv;
    if !delta.is_finite() {
        return Err(invalid("scenario.pv", "nonfinite monetary change"));
    }
    Ok(CreditScenarioResult {
        request: request.clone(),
        base: prepared.base_value.clone(),
        shocked,
        pv_change_currency: delta,
        credit_curve: curve,
        calibration,
        scenario_recipe: recipe,
        curves,
    })
}

/// Central finite-difference basis; upfront/recovery derivatives are not CS01.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum CreditRiskBump {
    /// Continuous-zero node risk, normalized to a +1 bp rate move.
    DiscountZero {
        /// Positive actual central move in decimal continuous-zero units.
        step_decimal: f64,
        /// Actual survival response held fixed for both sides of the bump.
        credit_response: CreditResponse,
    },
    /// One actual running CDS input, normalized to a +1 bp SPREAD quote move.
    RunningQuote {
        /// Exact original running quote helper.
        helper_id: String,
        /// Positive actual central move in running spread basis points.
        step_bps: f64,
    },
    /// One actual clean/dirty upfront input, derivative per unit upfront fraction.
    UpfrontQuote {
        /// Exact original clean/dirty upfront helper.
        helper_id: String,
        /// Positive actual central move in fractions of notional.
        step_fraction: f64,
    },
    /// Recovery derivative per unit fraction, with explicit survival response.
    Recovery {
        /// Positive actual central move in recovery-fraction units.
        step_fraction: f64,
        /// Actual survival response held fixed for both sides of the bump.
        credit_response: CreditResponse,
    },
}
/// Caller-supplied immutable scenario revisions for both sides of a central bump.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CreditRiskRevisions {
    /// Positive scenario identity.
    pub plus_id: String,
    /// Negative scenario identity.
    pub minus_id: String,
    /// Positive hypothetical credit version.
    pub plus_credit: CurveKey,
    /// Negative hypothetical credit version.
    pub minus_credit: CurveKey,
    /// Required only for discount-zero bumps.
    pub plus_discount: Option<CurveKey>,
    /// Required only for discount-zero bumps.
    pub minus_discount: Option<CurveKey>,
}
/// Explicit returned measure, including non-bp parameter/price risk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CreditRiskMeasure {
    /// Signed money for a +1 bp RATE move.
    Pv01Signed,
    /// Signed money for a +1 bp RUNNING CDS SPREAD move.
    Cs01Signed,
    /// Monetary derivative per unit upfront fraction.
    UpfrontDerivative,
    /// Monetary derivative per unit recovery fraction.
    RecoveryDerivative,
}
/// Signed central sensitivity and full actually executed scenario evidence.
#[derive(Debug, Clone)]
pub struct CreditSensitivity {
    /// Complete canonical factor identity, checked against the actual input.
    pub factor: RiskFactorKey,
    /// Unit/sign of the value; upfront/recovery cannot enter a bp aggregation.
    pub measure: CreditRiskMeasure,
    /// Actual supplied bump and response semantics, for aggregation compatibility.
    pub bump: CreditRiskBump,
    /// Signed normalized monetary value, not an absolute loss scalar.
    pub value: f64,
    /// Positive actually rebuilt/frozen scenario.
    pub plus: CreditScenarioResult,
    /// Negative actually rebuilt/frozen scenario.
    pub minus: CreditScenarioResult,
}

/// Check a supplied factor vector before portfolio/hedge use. A canonical factor
/// may occur only once, including frozen/recalibrated variants of the same rate
/// or recovery factor. The owning risk job must choose one response definition.
pub fn validate_credit_risk_vector(
    values: &[CreditSensitivity],
) -> Result<(), CreditCalibrationError> {
    if values.is_empty() || values.len() > 64 {
        return Err(invalid(
            "risk.vector",
            "bounded nonempty risk vector required",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for value in values {
        if !value.value.is_finite() || !seen.insert(&value.factor) {
            return Err(invalid(
                "risk.vector",
                "missing/nonfinite or duplicate canonical factors cannot be aggregated",
            ));
        }
    }
    Ok(())
}
/// Execute the reference central bump. Rate, running-spread and upfront/recovery
/// results have different bases/units and cannot be substituted for one another.
pub fn credit_sensitivity(
    prepared: &PreparedCreditRisk<'_>,
    factor: &RiskFactorKey,
    bump: CreditRiskBump,
    revisions: &CreditRiskRevisions,
) -> Result<CreditSensitivity, CreditCalibrationError> {
    let input = &prepared.input;
    let f = factor.definition();
    if f.currency != input.trade.terms().currency
        || f.model_version != input.risk_model
        || f.convention_version != input.trade.terms().reference.document.version
        || revisions.plus_id == revisions.minus_id
        || revisions.plus_credit == revisions.minus_credit
    {
        return Err(invalid("risk.factor","exact currency/document/risk-model revisions and distinct central scenario identities required"));
    }
    let (plus, minus, measure, scale, target, quote_type, basis, tenor) =
        match &bump {
            CreditRiskBump::DiscountZero {
                step_decimal,
                credit_response,
            } => {
                if !step_decimal.is_finite() || *step_decimal <= 0.0 || *step_decimal > 1.0 {
                    return Err(invalid(
                        "risk.bump",
                        "positive finite rate move <=1 required",
                    ));
                }
                let p = revisions.plus_discount.clone().ok_or_else(|| {
                    invalid("risk.revisions", "positive discount revision required")
                })?;
                let m = revisions.minus_discount.clone().ok_or_else(|| {
                    invalid("risk.revisions", "negative discount revision required")
                })?;
                if p == m {
                    return Err(invalid(
                        "risk.revisions",
                        "distinct discount revisions required",
                    ));
                }
                (
                    CreditShock::DiscountZero {
                        output_discount: p,
                        shift_decimal: *step_decimal,
                        credit_response: *credit_response,
                    },
                    CreditShock::DiscountZero {
                        output_discount: m,
                        shift_decimal: -step_decimal,
                        credit_response: *credit_response,
                    },
                    CreditRiskMeasure::Pv01Signed,
                    0.0001 / (2.0 * step_decimal),
                    input.trade.terms().discount_curve.id.clone(),
                    QuoteType::ZeroRate,
                    RiskBasis::ZeroNode,
                    "parallel".into(),
                )
            }
            CreditRiskBump::RunningQuote {
                helper_id,
                step_bps,
            }
            | CreditRiskBump::UpfrontQuote {
                helper_id,
                step_fraction: step_bps,
            } => {
                let h = input
                    .recipe
                    .helpers
                    .iter()
                    .find(|h| &h.id == helper_id)
                    .ok_or_else(|| invalid("risk.helper", "missing quote helper"))?;
                if !step_bps.is_finite() || *step_bps <= 0.0 {
                    return Err(invalid("risk.bump", "positive finite native move required"));
                }
                let running = matches!(bump, CreditRiskBump::RunningQuote { .. });
                if running != (h.quote_kind == CdsQuoteKind::RunningSpread) {
                    return Err(invalid(
                        "risk.quote",
                        "running CS01 and upfront derivatives are distinct",
                    ));
                }
                let delta = if running {
                    h.quote.model_rate_quote(step_bps / 10000.0)?.value
                } else {
                    *step_bps
                };
                if delta < 32.0 * h.tolerance {
                    return Err(invalid(
                        "risk.resolution",
                        "quote bump must exceed 32 original native residual tolerances",
                    ));
                }
                let movement = |value| CreditShock::Quotes {
                    moves: vec![CdsQuoteMove {
                        helper_id: helper_id.clone(),
                        expected_quote_version: h.quote_version,
                        delta: MarketQuote {
                            value,
                            unit: h.quote.unit,
                        },
                    }],
                };
                (
                    movement(delta),
                    movement(-delta),
                    if running {
                        CreditRiskMeasure::Cs01Signed
                    } else {
                        CreditRiskMeasure::UpfrontDerivative
                    },
                    1.0 / (2.0 * step_bps),
                    input.recipe.curve.key.id.clone(),
                    if running {
                        QuoteType::CdsSpread
                    } else {
                        QuoteType::CdsUpfront
                    },
                    RiskBasis::MarketQuoteRecalibrated,
                    format!(
                        "{}D",
                        input.recipe.curve.reference_date.days_between(&h.pillar)
                    ),
                )
            }
            CreditRiskBump::Recovery {
                step_fraction,
                credit_response,
            } => {
                if !step_fraction.is_finite() || *step_fraction <= 0.0 || *step_fraction >= 1.0 {
                    return Err(invalid(
                        "risk.bump",
                        "positive recovery-fraction move below one required",
                    ));
                }
                let r = input.trade.terms().reference.recovery_fraction.get();
                (
                    CreditShock::Recovery {
                        fraction: FiniteF64::new(r + step_fraction)
                            .map_err(|_| invalid("recovery", "nonfinite bump"))?,
                        credit_response: *credit_response,
                    },
                    CreditShock::Recovery {
                        fraction: FiniteF64::new(r - step_fraction)
                            .map_err(|_| invalid("recovery", "nonfinite bump"))?,
                        credit_response: *credit_response,
                    },
                    CreditRiskMeasure::RecoveryDerivative,
                    1.0 / (2.0 * step_fraction),
                    input.trade.terms().reference.entity_id.as_str().into(),
                    QuoteType::RecoveryRate,
                    RiskBasis::ModelParameter,
                    "parallel".into(),
                )
            }
        };
    if !matches!(bump, CreditRiskBump::DiscountZero { .. })
        && (revisions.plus_discount.is_some() || revisions.minus_discount.is_some())
    {
        return Err(invalid(
            "risk.revisions",
            "non-rate shocks cannot supply discount changes",
        ));
    }
    let family = match measure {
        CreditRiskMeasure::Pv01Signed => FactorFamily::InterestRate,
        CreditRiskMeasure::RecoveryDerivative => FactorFamily::Recovery,
        _ => FactorFamily::CreditSpread,
    };
    let target_matches = match &f.target {
        FactorTarget::Curve { curve_id } => {
            family != FactorFamily::Recovery && curve_id.as_str() == target
        }
        FactorTarget::Entity { entity_id } => {
            family == FactorFamily::Recovery && entity_id.as_str() == target
        }
        _ => false,
    };
    if f.family != family
        || f.quote_type != quote_type
        || f.basis != basis
        || f.tenor.as_str() != tenor
        || !target_matches
    {
        return Err(invalid(
            "risk.factor",
            "family/target/actual dated tenor/quote/bump basis differs from requested operation",
        ));
    }
    let plus = evaluate_credit_scenario(
        prepared,
        &CreditScenarioRequest {
            id: revisions.plus_id.clone(),
            output_credit: revisions.plus_credit.clone(),
            shock: plus,
        },
    )?;
    let minus = evaluate_credit_scenario(
        prepared,
        &CreditScenarioRequest {
            id: revisions.minus_id.clone(),
            output_credit: revisions.minus_credit.clone(),
            shock: minus,
        },
    )?;
    let value = (plus.shocked.net_pv - minus.shocked.net_pv) * scale;
    if !value.is_finite() {
        return Err(invalid("risk.value", "nonfinite central sensitivity"));
    }
    Ok(CreditSensitivity {
        factor: factor.clone(),
        measure,
        bump,
        value,
        plus,
        minus,
    })
}

/// Immediate conditional default assumptions. This is a scenario, not a ledger
/// transition or an inferred auction result. Later dated paths require lifecycle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ImmediateDefaultScenario {
    /// Actual/simulated scenario event identity.
    pub event_id: String,
    /// Exact entity expected to default.
    pub entity_id: EntityId,
    /// Exact obligation priority.
    pub seniority: Seniority,
    /// Supplied default day; this route requires the current step-in day.
    pub default_date: Date,
    /// Explicit proposed auction cash date, no hidden settlement lag.
    pub cash_settlement_date: Date,
    /// Contractual elapsed fraction of that default calendar day in `[0,1]`.
    pub accrual_day_fraction: FiniteF64,
    /// Actual supplied auction/recovery result, including zero/full recovery.
    pub auction_recovery: Option<FiniteF64>,
    /// Immutable source of the supplied auction result; required with recovery.
    pub auction_reference: Option<ReferencePin>,
}
/// Unknown recovery produces a pending state with NO fabricated monetary value.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum DefaultScenarioResult {
    /// Auction/recovery information is unknown; valuation cannot be completed.
    Pending {
        /// Exact requested event.
        scenario: ImmediateDefaultScenario,
    },
    /// Conditional assumed cash; does not record that payment occurred.
    Valued {
        /// Exact assumptions and source pins.
        scenario: ImmediateDefaultScenario,
        /// Signed protection cash at auction.
        protection_cash: f64,
        /// Signed premium accrued at default.
        accrued_premium_cash: f64,
        /// Conditional cash PV at current valuation.
        post_default_pv: f64,
        /// Signed post-default minus surviving PV (JTD).
        jump_to_default_currency: f64,
        /// Actual discount dependency.
        discount_curve: CurveKey,
        /// Trade currency.
        currency: Currency,
    },
}
/// Immediate JTD using explicit auction recovery, premium cutoff and settlement.
/// Outstanding initial upfront or non-immediate default requires owning lifecycle
/// handling and fails; unknown auction data remains Pending rather than zero.
pub fn immediate_jump_to_default(
    prepared: &PreparedCreditRisk<'_>,
    scenario: &ImmediateDefaultScenario,
) -> Result<DefaultScenarioResult, CreditCalibrationError> {
    let input = &prepared.input;
    let terms = input.trade.terms();
    if !label(&scenario.event_id)
        || scenario.entity_id != terms.reference.entity_id
        || scenario.seniority != terms.reference.seniority
        || scenario.default_date != input.context.step_in_date
        || scenario.default_date < terms.protection_start_date
        || scenario.default_date > terms.premium_schedule.end_date
        || scenario.cash_settlement_date < scenario.default_date
        || scenario
            .default_date
            .days_between(&scenario.cash_settlement_date)
            > 366
        || !(0.0..=1.0).contains(&scenario.accrual_day_fraction.get())
        || input.context.initial_payment_state == InitialPaymentState::Pending
    {
        return Err(invalid("default.scenario","eligible immediate default, exact entity/seniority, explicit bounded settlement/accrual and no pending initial cash required"));
    }
    if !(1..=9999).contains(&scenario.cash_settlement_date.year()) {
        return Err(invalid("settlement", "outside canonical date domain"));
    }
    match (&scenario.auction_recovery, &scenario.auction_reference) {
        (None, None) => {
            return Ok(DefaultScenarioResult::Pending {
                scenario: scenario.clone(),
            })
        }
        (Some(r), Some(_)) if (0.0..=1.0).contains(&r.get()) => (),
        _ => {
            return Err(invalid(
                "auction",
                "recovery in [0,1] and its actual immutable source must be supplied together",
            ))
        }
    }
    let r = scenario
        .auction_recovery
        .ok_or_else(|| invalid("auction", "missing recovery"))?
        .get();
    let n = terms
        .notional
        .to_model()
        .map_err(convex_credit::CdsError::from)?
        .get();
    let coupon = terms
        .coupon_decimal
        .to_model()
        .map_err(convex_credit::CdsError::from)?
        .get();
    let period = input
        .trade
        .premium_periods()
        .iter()
        .find(|p| scenario.default_date >= p.accrual_start && scenario.default_date < p.accrual_end)
        .ok_or_else(|| invalid("default.accrual", "missing actual premium period"))?;
    let denominator = match terms.premium_day_count {
        DayCountConvention::Act360 => 360.0,
        DayCountConvention::Act365Fixed => 365.0,
        _ => {
            return Err(invalid(
                "default.day_count",
                "unsupported actual premium day count",
            ))
        }
    };
    let sign = if terms.side == CdsSide::ProtectionBuyer {
        1.0
    } else {
        -1.0
    };
    let protection_cash = sign * n * (1.0 - r);
    let accrued_premium_cash = if terms.pay_accrual_on_default {
        -sign
            * n
            * coupon
            * (period.accrual_start.days_between(&scenario.default_date) as f64
                + scenario.accrual_day_fraction.get())
            / denominator
    } else {
        0.0
    };
    let discount = input.supplied.get(&terms.discount_curve)?;
    let p = discount
        .query(&CurveQuery::DiscountFactor {
            point: CurvePoint::Date(scenario.cash_settlement_date),
        })?
        .value;
    let p0 = discount
        .query(&CurveQuery::DiscountFactor {
            point: CurvePoint::Date(input.context.valuation_date),
        })?
        .value;
    let post_default_pv = (protection_cash + accrued_premium_cash) * p / p0;
    let jump = post_default_pv - prepared.base_value.net_pv;
    if !post_default_pv.is_finite() || !jump.is_finite() {
        return Err(invalid("default.pv", "nonfinite conditional cash"));
    }
    Ok(DefaultScenarioResult::Valued {
        scenario: scenario.clone(),
        protection_cash,
        accrued_premium_cash,
        post_default_pv,
        jump_to_default_currency: jump,
        discount_curve: terms.discount_curve.clone(),
        currency: terms.currency,
    })
}

/// Resolved actual bond credit identity; BondTerms alone contain no issuer data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondCreditIdentity {
    /// Actual bond identity.
    pub instrument_id: InstrumentId,
    /// Actual instrument economic revision.
    pub instrument_version: Version,
    /// Source of the actual issuer/priority mapping.
    pub reference: ReferencePin,
    /// Legal reference entity.
    pub entity_id: EntityId,
    /// Obligation priority.
    pub seniority: Seniority,
    /// Actual bond currency.
    pub currency: Currency,
    /// Actual bond maturity, used for basis coverage admission.
    pub maturity: Date,
}
/// Actual dated residual basis, not a CDS spread or a manufactured Z-spread.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondBasisInput {
    /// Immutable version/source of the residual basis observations/assumption.
    pub pin: ReferencePin,
    /// Same-currency cash/CDS comparison.
    pub currency: Currency,
    /// Exact issuer represented by the basis.
    pub entity_id: EntityId,
    /// Exact seniority represented by the basis.
    pub seniority: Seniority,
    /// Fractional recovery assumption used to separate hazard from residual basis.
    pub recovery_fraction: FiniteF64,
    /// Explicit continuous-zero residual basis model/version.
    pub model_version: ModelId,
    /// Supplied valuation reference date.
    pub reference_date: Date,
    /// Actual increasing dated continuous-zero residual spreads in explicit rate
    /// units. Zero is allowed only as an explicitly supplied observed/assumed input.
    pub dated_spreads: Vec<(Date, MarketQuote)>,
    /// Immutable actual liquidity assumption retained as separate hedge risk.
    pub liquidity_assumption: ReferencePin,
}
/// Exact legal/economic mapping; price-risk and execution suitability remain
/// separate. This admission never collapses a bond CS01 into CDS hedge notional.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondCdsMapping {
    /// Immutable actual mapping revision.
    pub pin: ReferencePin,
    /// Full resolved actual bond expected by this mapping.
    pub bond: BondCreditIdentity,
    /// Exact CDS reference/document/restructuring/recovery semantics.
    pub cds_reference: CreditReference,
    /// Exact calibrated credit version under this mapping.
    pub credit_curve: CurveKey,
    /// Exact separately supplied basis input expected by this mapping.
    pub basis: ReferencePin,
    /// Exact separately supplied liquidity assumption.
    pub liquidity_assumption: ReferencePin,
}
/// Checked reusable inputs; private construction prevents missing mapping bypass.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BondCdsHedgeInputs {
    mapping: BondCdsMapping,
    basis: BondBasisInput,
}
impl BondCdsHedgeInputs {
    /// Actual complete mapping; residual basis/recovery/liquidity stay explicit.
    pub fn mapping(&self) -> &BondCdsMapping {
        &self.mapping
    }
    /// Supplied basis input; it is not an independently calibrated pricing curve.
    pub fn basis(&self) -> &BondBasisInput {
        &self.basis
    }
}
/// Reject absent or incompatible legal mappings BEFORE bond-CDS hedge use.
/// This validates cross-instrument inputs; hedge optimization, a bond credit
/// valuation model and executable market/short eligibility remain owning work.
pub fn admit_bond_cds_hedge(
    mapping: Option<&BondCdsMapping>,
    bond: &BondCreditIdentity,
    cds: &CdsContract,
    credit: &BuiltCurve,
    basis: &BondBasisInput,
) -> Result<BondCdsHedgeInputs, CreditCalibrationError> {
    let m = mapping.ok_or_else(|| {
        invalid(
            "bond_cds.mapping",
            "explicit resolved bond-CDS mapping required",
        )
    })?;
    let t = cds.terms();
    let role = CurveRole::Credit {
        entity_id: t.reference.entity_id.as_str().into(),
        seniority: t.reference.seniority.as_str().into(),
        recovery: t.reference.recovery_fraction.get(),
    };
    if &m.bond != bond
        || bond.entity_id != t.reference.entity_id
        || bond.seniority != t.reference.seniority
        || bond.currency != t.currency
        || m.cds_reference != t.reference
        || m.credit_curve != t.survival_curve
        || credit.recipe().key != m.credit_curve
        || credit.recipe().role != role
        || credit.recipe().currency != t.currency
        || m.basis != basis.pin
        || m.liquidity_assumption != basis.liquidity_assumption
        || basis.entity_id != bond.entity_id
        || basis.seniority != bond.seniority
        || basis.currency != bond.currency
        || basis.recovery_fraction != t.reference.recovery_fraction
        || basis.reference_date != credit.recipe().reference_date
        || basis.dated_spreads.is_empty()
        || basis.dated_spreads.len() > 1024
        || bond.maturity <= basis.reference_date
    {
        return Err(invalid("bond_cds.mapping","exact bond/reference/CDS document/entity/seniority/currency/recovery/credit/basis/liquidity revisions required"));
    }
    let mut previous = None;
    for (date, spread) in &basis.dated_spreads {
        if !(1..=9999).contains(&date.year())
            || *date < basis.reference_date
            || previous.is_some_and(|p| *date <= p)
            || basis.reference_date.days_between(date) as f64 / 365.0 > 150.0
        {
            return Err(invalid(
                "bond_basis.dates",
                "bounded increasing actual dates and explicit horizon required",
            ));
        }
        spread.decimal_rate()?;
        previous = Some(*date);
    }
    if previous.is_none_or(|date| date < bond.maturity.max(t.premium_schedule.end_date)) {
        return Err(invalid(
            "bond_basis.horizon",
            "actual bond and CDS maturities must be covered without a tail proxy",
        ));
    }
    Ok(BondCdsHedgeInputs {
        mapping: m.clone(),
        basis: basis.clone(),
    })
}
