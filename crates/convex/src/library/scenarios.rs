//! Thin full native mixed-book composition; all financial owners remain pure.
use super::*;
use convex_analytics::workflow as w;

/// Complete supplied product; issuer cash preserves contextual quote resources.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BookScenarioProductInput {
    /// Original complete bond/CDS/IRS/OIS/rate/delivery/optional input.
    Product {
        /// Existing complete product specification, all source/numerical policies.
        product: Box<RiskProductInput>,
    },
    /// Original full observed/model cash, including contextual ICMA entitlement.
    CashBasis {
        /// Original security/observation/calendar/context resources.
        security: Box<CashSecurityInput>,
        /// Exact original graph residual-basis curve.
        basis: CurveKey,
        /// Complete original promised/default-recovery valuation model.
        model: convex_analytics::cash_calibration::BondBasisModel,
    },
}
/// Exact one-to-one expected position and its full original native economics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BookScenarioHoldingInput {
    /// Exact expected position identity; all quantities/pins live in book.positions.
    pub id: convex_core::contracts::TradeId,
    /// Full original product, including eligible fixing/clearing/cash state.
    pub product: BookScenarioProductInput,
}
/// Same-information-set hypothetical nonlinear model marking and attribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MixedBookScenarioInput {
    /// Full native declaration, source pins, ordered moves, FX and work policy.
    pub book: w::BookScenarioInput,
    /// Complete exact original product inputs, in expected position order.
    pub holdings: Vec<BookScenarioHoldingInput>,
}

pub(super) fn book_scenario(
    graph: &convex_analytics::quote_risk::PreparedQuoteRiskGraph<'_>,
    input: &MixedBookScenarioInput,
    resources: &inputs::ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    if input.holdings.len() != input.book.positions.len()
        || input
            .holdings
            .iter()
            .zip(&input.book.positions)
            .any(|(h, p)| h.id != p.identity.id)
    {
        return Err(LibraryError::invalid(
            "book holdings",
            "exact complete original expected product identities in declared order required",
        ));
    }
    for h in &input.holdings {
        if let BookScenarioProductInput::Product { product } = &h.product {
            if let RiskProductInput::BondFuture {
                input: future,
                policy,
                ..
            } = product.as_ref()
            {
                policy.reserve(future.members.len(), input.book.moves.len())?;
            }
        }
    }
    let path = w::prepare_book_scenarios(graph, &input.book)?;
    let original = resources.with_curves(graph.baseline().curves.clone());
    let mut trades = Vec::with_capacity(input.holdings.len());
    for (i, holding) in input.holdings.iter().enumerate() {
        let trade = match &holding.product {
            BookScenarioProductInput::CashBasis {
                security,
                basis,
                model,
            } => {
                let bound =
                    cash::helper_resources(&security.helper, &security.calendars, &original)?;
                w::revalue_book_position(
                    &path,
                    i,
                    &w::BookScenarioProduct::CashBasis {
                        helper: &security.helper,
                        resources: &bound,
                        basis,
                        model,
                    },
                )?
            }
            BookScenarioProductInput::Product { product } => match product.as_ref() {
                RiskProductInput::BondFuture {
                    input: future,
                    discount,
                    policy,
                } => products::with_bond_future(future, &original, |inputs| {
                    Ok(w::revalue_book_position(
                        &path,
                        i,
                        &w::BookScenarioProduct::Delivery {
                            inputs,
                            discount: discount.native(&original)?,
                            policy: *policy,
                        },
                    )?)
                })?,
                RiskProductInput::OptionalBond {
                    input: optional,
                    instrument,
                    oas_decimal,
                    ..
                } => {
                    let prepared = products::prepare_optional(optional, &original)?;
                    w::revalue_book_position(
                        &path,
                        i,
                        &w::BookScenarioProduct::Optional {
                            prepared: &prepared,
                            instrument,
                            oas_decimal: *oas_decimal,
                        },
                    )?
                }
                _ => risk::with_product(product, &original, |product| {
                    Ok(w::revalue_book_position(
                        &path,
                        i,
                        &w::BookScenarioProduct::Standard(product),
                    )?)
                })?,
            },
        };
        trades.push(trade);
    }
    let report = w::aggregate_book_scenarios(&path, trades)?;
    let graphs = path
        .points()
        .iter()
        .map(|p| {
            Ok(serde_json::json!({"moves":p.moves,
        "graph":curves::graph_value(&p.graph)?,
        "calibrations":p.graph.calibrations.iter().map(replay::fit_value).collect::<Vec<_>>() }))
        })
        .collect::<LibraryResult<Vec<_>>>()?;
    Ok(
        serde_json::json!({"request":input,"report":report,"graphs":graphs,
        "meaning":"same-information-set hypothetical model marking; no realized/lifecycle/posted cash or trading authority"}),
    )
}
