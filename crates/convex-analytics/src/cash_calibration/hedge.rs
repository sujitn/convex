//! Original issuer cash/model/graph owners into complete native hedge columns.
use super::*;
use crate::workflow::GraphHedgeRiskInput;
use crate::{credit_risk as credit, quote_risk as q, risk::hedging::constrained as h};
use convex_core::contracts::ModelMoney;
use serde::Serialize;

/// Original economic and risk failures, without an accepted partial column.
#[derive(Debug, thiserror::Error)]
pub enum CashBasisHedgeError {
    /// Original complete cash/model/contextual inverse failure.
    #[error(transparent)]
    Cash(#[from] CashCalibrationError),
    /// Original quantity or explicit mapping/manifest failure.
    #[error(transparent)]
    Workflow(#[from] crate::workflow::WorkflowError),
    /// Original five-price graph/stability failure.
    #[error(transparent)]
    Risk(#[from] q::QuoteRiskError),
    /// Exact supplied curve/role/quote failure.
    #[error(transparent)]
    Curve(#[from] CurveContractError),
}
/// All-or-error complete issuer cash hedge calculation.
pub type CashBasisHedgeResult<T> = Result<T, CashBasisHedgeError>;

/// Complete original observed and model cash, mapping and actual five-PV risk.
#[derive(Debug, Serialize)]
pub struct CashBasisHedgeRisk {
    /// Full original security/quote/settlement/credit inputs.
    pub helper: CashBondHelper,
    /// Actual residual-basis curve revision.
    pub basis: CurveKey,
    /// Original baseline model, distinct from hypothetical recovery moves.
    pub model: BondBasisModel,
    /// Original checked legal mapping and separate basis/liquidity observations.
    pub credit: Option<credit::BondCdsHedgeInputs>,
    /// Explicit decimal-rate bound for the baseline mapped basis observations.
    pub basis_tolerance: f64,
    /// Original complete application/factor/quantity/snapshot manifest.
    pub input: GraphHedgeRiskInput,
    /// Original observed price/accrued/ICMA cash conversion, never model PV.
    pub observed: CashQuoteConversion,
    /// Full original model cash with separate promised and recovery PV.
    pub valuation: BasisBondValuation,
    /// Same original helper's model-implied quote in its original native units.
    pub model_quote: MarketQuote,
    /// Model minus original observation; finite off-market residuals are diagnostic.
    pub model_residual: f64,
    /// Same checked contextual model inverse, absent for context-free helpers.
    pub model_icma: Option<Box<crate::workflow::IcmaQuotation>>,
    /// Original contextual work, including the baseline native-unit inverse.
    pub icma_work: Option<IcmaHelperWork>,
    /// Five original factor callbacks each, separate from graph/cash work bounds.
    pub reserved_factor_evaluations: usize,
    /// Every intended actually evaluated factor, with explicit absent reporting FX.
    pub vector: h::HedgeRiskVector,
}

/// Compose complete original issuer cash risk. Credit scope is supplied only by
/// the original checked bond/CDS mapping. Pricing never invents a legal scope.
/// This calculation does not qualify external manifests or authorize orders.
#[allow(clippy::too_many_arguments)]
pub fn cash_basis_hedge_risk(
    helper: &CashBondHelper,
    resources: &CashHelperResources<'_>,
    basis: &CurveKey,
    model: &BondBasisModel,
    credit: Option<&credit::BondCdsHedgeInputs>,
    basis_tolerance: f64,
    graph: &q::PreparedQuoteRiskGraph<'_>,
    input: &GraphHedgeRiskInput,
) -> CashBasisHedgeResult<CashBasisHedgeRisk> {
    use crate::workflow::WorkflowError;
    let invalid = |field, reason: &str| {
        CashBasisHedgeError::Workflow(WorkflowError::Invalid {
            field,
            reason: reason.into(),
        })
    };
    if input.factors.is_empty() || input.factors.len() > 16 {
        return Err(invalid(
            "factors",
            "1..16 complete original factors required",
        ));
    }
    let reserved = 5 * input.factors.len();
    if input.max_factor_evaluations == 0
        || input.max_factor_evaluations > 80
        || reserved > input.max_factor_evaluations
    {
        return Err(invalid(
            "factor work",
            "explicit five-price reservation exhausted",
        ));
    }
    for (i, factor) in input.factors.iter().enumerate() {
        factor.settings.validate_for(&factor.key)?;
        if input.factors[..i].iter().any(|f| f.key == factor.key) {
            return Err(invalid("factors", "duplicate original factor"));
        }
    }
    let position = &input.reference_position;
    position.scale()?;
    if position.quantity_unit != q::RiskQuantityUnit::Notional
        || position.quantity != position.reference_quantity
        || position.reference_quantity.decimal() != helper.terms.face_value
        || position.currency != helper.terms.currency
    {
        return Err(invalid(
            "reference_position",
            "exact positive original-face reference required",
        ));
    }
    if !basis_tolerance.is_finite() || basis_tolerance <= 0.0 || basis_tolerance > 1e-10 {
        return Err(invalid(
            "basis_tolerance",
            "explicit decimal-rate bound in(0,1e-10] required",
        ));
    }
    let identity = &input.identity;
    let bond = helper.credit_identity.as_ref().ok_or_else(|| {
        invalid(
            "credit identity",
            "original issuer security mapping required",
        )
    })?;
    if identity.instrument.id.as_str() != helper.id
        || identity.instrument.id.as_str() != bond.instrument_id.as_str()
        || identity.instrument.version != bond.instrument_version
    {
        return Err(invalid(
            "instrument",
            "actual security identity/revision mismatch",
        ));
    }
    if let CashQuoteKind::ContextualIcmaYield { input: icma } = &helper.quote_kind {
        if icma.sources.security.id != identity.instrument.id
            || icma.sources.security.version != identity.instrument.version
        {
            return Err(invalid(
                "ICMA security",
                "exact original contextual security revision required",
            ));
        }
    }
    let curves = &graph.baseline().curves;
    let baseline_basis = curves.get(basis)?;
    let credit_scope = match (model, credit) {
        (BondBasisModel::ReferenceZeroSpread { .. }, None) => Vec::new(),
        (_, Some(checked)) => {
            let mapping = checked.mapping();
            let actual = checked.basis();
            if mapping.bond != *bond
                || actual.reference_date != baseline_basis.recipe().reference_date
                || !matches!(&baseline_basis.recipe().role, CurveRole::IssuerSpread { model_id, .. }
                    if actual.model_version.as_str() == model_id)
            {
                return Err(invalid(
                    "credit mapping",
                    "exact original bond/credit/basis/model/recovery revisions required",
                ));
            }
            if let BondBasisModel::CreditParRecovery {
                survival, recovery, ..
            } = model
            {
                if mapping.credit_curve != *survival
                    || mapping.cds_reference.recovery_fraction.get() != *recovery
                {
                    return Err(invalid(
                        "credit mapping",
                        "exact modeled survival/recovery required",
                    ));
                }
            }
            for (date, spread) in &actual.dated_spreads {
                let value = baseline_basis.issuer_spread(CurvePoint::Date(*date))?;
                if (value - spread.decimal_rate()?).abs() > basis_tolerance {
                    return Err(invalid(
                        "basis observations",
                        "mapped baseline basis differs from actual supplied curve",
                    ));
                }
            }
            vec![mapping.cds_reference.clone()]
        }
        _ => {
            return Err(invalid(
                "credit mapping",
                "credit model requires checked original mapping",
            ))
        }
    };
    let prepared = pricing::prepare(helper, resources, baseline_basis.recipe().reference_date)?;
    if let Some(icma) = &prepared.icma {
        // Reserve each full factor cash pass before pricing, conservatively
        // retaining the original model-price-plus-inverse reservation per pass.
        for _ in 0..=reserved {
            icma.reserve_model()?;
        }
    }
    let valuation = pricing::basis_price(&prepared, helper, model, baseline_basis, curves)?;
    let (model_quote, model_icma) =
        pricing::model_quote(&prepared, helper, &valuation.price, baseline_basis)?;
    let model_residual = finite(
        model_quote.value - helper.quote.value,
        "model quote residual",
    )?;
    let mut sensitivities = Vec::with_capacity(input.factors.len());
    for factor in &input.factors {
        let result = q::curve_quote_sensitivity(
            graph,
            factor.key.clone(),
            position.clone(),
            input.snapshot_id.clone(),
            factor.settings.clone(),
            |scenario| {
                let value = (|| -> CashBasisHedgeResult<ModelMoney> {
                    // Recovery coordinates change the actual credit role. Bind the
                    // SAME original par-recovery owner to that scenario assumption.
                    let mut scenario_model = model.clone();
                    if let BondBasisModel::CreditParRecovery {
                        survival, recovery, ..
                    } = &mut scenario_model
                    {
                        let survival = scenario.curves.get(survival)?;
                        let CurveRole::Credit {
                            recovery: actual, ..
                        } = survival.recipe().role
                        else {
                            return Err(invalid(
                                "credit scenario",
                                "actual original credit role required",
                            ));
                        };
                        *recovery = actual;
                    }
                    let value = pricing::basis_price(
                        &prepared,
                        helper,
                        &scenario_model,
                        scenario.curves.get(basis)?.as_ref(),
                        &scenario.curves,
                    )?;
                    Ok(q::risk_reference_pv(
                        value.price.present_value,
                        helper.terms.currency,
                    )?)
                })();
                value.map_err(|e| q::QuoteRiskError::Invalid {
                    field: "full original issuer cash".into(),
                    reason: e.to_string(),
                })
            },
        )?;
        if result.base_pv != valuation.price.present_value {
            return Err(invalid(
                "baseline",
                "original model and full graph base PV disagree",
            ));
        }
        sensitivities.push(result);
    }
    Ok(CashBasisHedgeRisk {
        helper: helper.clone(),
        basis: basis.clone(),
        model: model.clone(),
        credit: credit.cloned(),
        basis_tolerance,
        input: input.clone(),
        observed: prepared.observed.clone(),
        valuation,
        model_quote,
        model_residual,
        model_icma,
        icma_work: prepared.icma.as_ref().map(|i| i.work()),
        reserved_factor_evaluations: reserved,
        vector: h::HedgeRiskVector {
            id: identity.id.clone(),
            product: h::HedgeProduct::CashBond,
            instrument: identity.instrument.clone(),
            economics: identity.economics.clone(),
            calculation: identity.calculation.clone(),
            entity: identity.entity.clone(),
            settlement: helper.settlement,
            credit_scope,
            reference_position: position.clone(),
            sensitivities,
            fx: None,
        },
    })
}
