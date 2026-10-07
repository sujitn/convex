//! Complete original calibrated credit risk and explicit legal hedge admission.
use super::inputs::ResolvedInputs;
use super::*;
use convex_analytics::{credit_calibration as c, credit_risk as r};
use convex_core::contracts::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Full original credit recipe and surviving trade; no supplied fitted cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CreditRiskInput {
    /// Exact source quote revisions, full helpers and original numerical policy.
    pub recipe: c::CdsCurveRecipe,
    /// Complete original trade.
    pub terms: convex_credit::CdsTerms,
    /// Explicit actual surviving state and initial cash state.
    pub context: convex_credit::CdsPricingContext,
    /// Unchanged original identification limits.
    pub controls: FitNumericalControls,
    /// Actual selected native risk semantics revision.
    pub risk_model: ModelId,
}
/// Disjoint native credit responses; parameter risk cannot become spread CS01.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CreditRiskOperation {
    /// Actual calibrated baseline and original helper valuations/diagnostics.
    Baseline {},
    /// Full original shock/rebuild or explicitly held survival.
    Scenario {
        /// Exact source-unit shock and hypothetical revisions.
        request: r::CreditScenarioRequest,
    },
    /// Signed native rate/running quote/upfront/recovery derivative.
    Sensitivity {
        /// Original canonical factor identity.
        factor: RiskFactorKey,
        /// Exact original response/bump.
        bump: r::CreditRiskBump,
        /// Distinct supplied hypothetical revisions.
        revisions: Box<r::CreditRiskRevisions>,
    },
    /// Actual pending or supplied immediate auction recovery; no zero default.
    JumpToDefault {
        /// Full original default/accrual/cash/recovery assumptions.
        scenario: r::ImmediateDefaultScenario,
    },
}
fn calibration_value(c: &c::CalibratedCdsCurve) -> LibraryResult<Value> {
    let solver = match &c.solver {
        c::CreditSolverDiagnostics::Piecewise { helper_ids, roots } => {
            json!({"kind":"piecewise","helper_ids":helper_ids,
            "roots":roots.iter().map(|r|json!({"root":r.root,"residual":r.residual,"iterations":r.iterations})).collect::<Vec<_>>()})
        }
        c::CreditSolverDiagnostics::Global(d) => {
            json!({"kind":"global","diagnostics":super::errors::fit_diagnostics(d)})
        }
    };
    Ok(
        json!({"recipe":c.recipe,"curve":super::curves::describe_curve(&c.curve),
        "hazards_per_act365f_year":c.hazards_per_act365f_year,"repriced_helpers":c.repriced_helpers,
        "solver":solver,"price_evaluations":c.price_evaluations}),
    )
}
fn scenario_value(s: &r::CreditScenarioResult) -> LibraryResult<Value> {
    Ok(
        json!({"request":s.request,"base":s.base,"shocked":s.shocked,"pv_change_currency":s.pv_change_currency,
        "credit_curve":super::curves::describe_curve(&s.credit_curve),
        "calibration":s.calibration.as_ref().map(calibration_value).transpose()?,"scenario_recipe":s.scenario_recipe,
        "curves":super::curves::describe_curves(&s.curves)?}),
    )
}
pub(super) fn credit_risk(
    input: &CreditRiskInput,
    action: &CreditRiskOperation,
    resources: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let calendars = input
        .recipe
        .helpers
        .iter()
        .map(|h| resources.credit_calendars(&h.terms))
        .collect::<LibraryResult<Vec<_>>>()?;
    let trade_calendars = resources.credit_calendars(&input.terms)?;
    let trade = input.terms.clone().build(&trade_calendars)?;
    let prepared = r::prepare_credit_risk(r::CreditRiskInput {
        recipe: &input.recipe,
        helper_calendars: &calendars,
        supplied: &resources.curves,
        trade: &trade,
        trade_calendars,
        context: &input.context,
        controls: input.controls,
        risk_model: input.risk_model.clone(),
    })?;
    match action {
        CreditRiskOperation::Baseline {} => Ok(
            json!({"baseline":calibration_value(prepared.baseline())?,"valuation":prepared.base_value()}),
        ),
        CreditRiskOperation::Scenario { request } => {
            scenario_value(&r::evaluate_credit_scenario(&prepared, request)?)
        }
        CreditRiskOperation::JumpToDefault { scenario } => {
            value(r::immediate_jump_to_default(&prepared, scenario)?)
        }
        CreditRiskOperation::Sensitivity {
            factor,
            bump,
            revisions,
        } => {
            let risk = r::credit_sensitivity(&prepared, factor, bump.clone(), revisions)?;
            Ok(
                json!({"factor":risk.factor,"measure":risk.measure,"bump":risk.bump,"value":risk.value,
                "plus":scenario_value(&risk.plus)?,"minus":scenario_value(&risk.minus)?}),
            )
        }
    }
}
/// Complete legal/recovery/liquidity/basis identity, separate from optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondCdsHedgeInput {
    /// Actual original legal mapping; absent mapping remains missing.
    pub mapping: Option<r::BondCdsMapping>,
    /// Actual separately supplied bond identity.
    pub bond: r::BondCreditIdentity,
    /// Full original hedge CDS terms.
    pub cds: convex_credit::CdsTerms,
    /// Actual supplied calibrated credit curve.
    pub credit: CurveKey,
    /// Independent original residual basis and liquidity assumption.
    pub basis: r::BondBasisInput,
}
pub(super) fn bond_cds_hedge(
    input: &BondCdsHedgeInput,
    resources: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let contract = input
        .cds
        .clone()
        .build(&resources.credit_calendars(&input.cds)?)?;
    value(r::admit_bond_cds_hedge(
        input.mapping.as_ref(),
        &input.bond,
        &contract,
        resources.curves.get(&input.credit)?.as_ref(),
        &input.basis,
    )?)
}
