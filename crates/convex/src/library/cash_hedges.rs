//! Thin original full security, graph and checked legal mapping resolution.
use super::*;
use convex_analytics::{cash_calibration as c, credit_risk as credit, workflow};

/// Complete original issuer model and independently supplied cross-credit scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CashBasisHedgeInput {
    /// Exact residual-basis curve in the original graph.
    pub basis: CurveKey,
    /// Original full reference-only or survival/par-recovery model.
    pub model: c::BondBasisModel,
    /// Complete original bond/CDS/basis/liquidity mapping for the credit model.
    pub credit: Option<BondCdsHedgeInput>,
    /// Explicit baseline mapped basis comparison in decimal annual rates.
    pub basis_tolerance: f64,
    /// Original positive face, identity, common factors, snapshot and work bound.
    pub hedge: workflow::GraphHedgeRiskInput,
}

pub(super) fn cash_basis_hedge_risk(
    graph: &CalibrationInput,
    security: &CashSecurityInput,
    input: &CashBasisHedgeInput,
    resources: &inputs::ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    curves::with_graph(graph, resources, |graph| {
        let original = resources.with_curves(graph.baseline().curves.clone());
        let bound = cash::helper_resources(&security.helper, &security.calendars, &original)?;
        let checked = input
            .credit
            .as_ref()
            .map(|input| {
                let contract = input
                    .cds
                    .clone()
                    .build(&original.credit_calendars(&input.cds)?)?;
                Ok::<_, LibraryError>(credit::admit_bond_cds_hedge(
                    input.mapping.as_ref(),
                    &input.bond,
                    &contract,
                    original.curves.get(&input.credit)?.as_ref(),
                    &input.basis,
                )?)
            })
            .transpose()?;
        value(c::cash_basis_hedge_risk(
            &security.helper,
            &bound,
            &input.basis,
            &input.model,
            checked.as_ref(),
            input.basis_tolerance,
            graph,
            &input.hedge,
        )?)
    })
}
