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
        risk::with_product(product, &original, |product| {
            value(convex_analytics::workflow::curve_hedge_risk(
                product, graph, input,
            )?)
        })
    })
}
