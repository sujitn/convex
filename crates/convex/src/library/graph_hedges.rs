//! Thin full-product/graph resolution; native owners calculate every factor.
use super::*;

pub(super) fn graph_hedge_risk(
    graph: &curves::CalibrationInput,
    product: &risk::RiskProductInput,
    input: &convex_analytics::workflow::GraphHedgeRiskInput,
    resources: &inputs::ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    curves::with_graph(graph, resources, |graph| {
        let original = resources.with_curves(graph.baseline().curves.clone());
        if let risk::RiskProductInput::OptionalBond {
            input: optional,
            instrument,
            oas_decimal,
            policies,
        } = product
        {
            if *instrument != input.identity.instrument {
                return Err(LibraryError::invalid(
                    "optional instrument",
                    "original product and hedge security revisions differ",
                ));
            }
            let prepared = products::prepare_optional(optional, &original)?;
            return value(convex_analytics::workflow::optional_hedge_risk(
                &prepared,
                *oas_decimal,
                graph,
                input,
                *policies,
            )?);
        }
        risk::with_product(product, &original, |product| {
            value(convex_analytics::workflow::curve_hedge_risk(
                product, graph, input,
            )?)
        })
    })
}
