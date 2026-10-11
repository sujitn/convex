//! Simultaneous original coordinates; rebuild quotes once, then frozen overlays.
use super::*;
use convex_core::contracts::QuoteType;
use serde::{Deserialize, Serialize};

/// One finite move in the original canonical coordinate (not a Greek amount).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct QuoteGraphMove {
    /// Exact original factor, source revision, native unit and response policy.
    pub key: QuoteRiskKey,
    /// Coordinate change; native quote conversion remains in the original owner.
    pub delta: f64,
}

// Expanded slots detect overlap even when differently named factors target the
// same actual input. A parallel grid cannot coexist with one of its own nodes.
fn slots(key: &QuoteRiskKey) -> QuoteRiskResult<Vec<(String, String, Option<usize>)>> {
    let curve = |c: &convex_curves::contracts::CurveKey| format!("{c:?}");
    Ok(match &key.definition().coordinate {
        RiskCoordinate::Quote {
            curve: c,
            helper_id,
            ..
        } => vec![(curve(c), format!("quote:{helper_id}"), None)],
        RiskCoordinate::ParallelQuotes { curve: c, quotes } => quotes
            .iter()
            .map(|(id, _, _)| (curve(c), format!("quote:{id}"), None))
            .collect(),
        RiskCoordinate::Zero { curve: c, node } => vec![(curve(c), "zero".into(), *node)],
        RiskCoordinate::Basis { curve: c, node } => vec![(curve(c), "basis".into(), *node)],
        RiskCoordinate::Hazard { curve: c, node } => vec![(curve(c), "hazard".into(), *node)],
        RiskCoordinate::Recovery { curve: c } => vec![(curve(c), "recovery".into(), None)],
        RiskCoordinate::Raw { .. }
            if key.definition().factor.definition().quote_type == QuoteType::JointZeroRate =>
        {
            let (d, p) = joint_curve_keys(key)?;
            vec![
                (curve(&d), "zero".into(), None),
                (curve(&p), "zero".into(), None),
            ]
        }
        RiskCoordinate::Raw { input_id, .. } => vec![(
            format!(
                "{input_id:?}:{:?}",
                key.definition().factor.definition().target
            ),
            format!("raw:{:?}", key.definition().factor.definition().quote_type),
            None,
        )],
    })
}

/// Bind an optional parallel scenario to the actual full graph inputs. Indirect
/// rebuilt changes cannot silently reuse the original cached model curves.
pub fn admit_optional_scenario_curves(
    prepared: &crate::optional_bond::PreparedOptionalBond,
    baseline: &convex_curves::contracts::CurveSet,
    scenario: &convex_curves::contracts::CurveSet,
    moves: &[QuoteGraphMove],
) -> QuoteRiskResult<()> {
    validate_quote_graph_moves(moves)?;
    admit_optional_curve_set(prepared, baseline)?;
    prepared.admit_scenario_baseline(baseline)?;
    let (discount, projection) = prepared.curve_recipes();
    for recipe in std::iter::once(discount).chain(projection) {
        let mut expected = baseline.get(&recipe.key)?.as_ref().clone();
        for m in moves {
            let applies = match &m.key.definition().coordinate {
                RiskCoordinate::Zero { curve, node: None } => *curve == recipe.key,
                RiskCoordinate::Raw { .. }
                    if m.key.definition().factor.definition().quote_type
                        == QuoteType::JointZeroRate =>
                {
                    let (d, p) = joint_curve_keys(&m.key)?;
                    d == recipe.key || p == recipe.key
                }
                _ => false,
            };
            if applies {
                expected = graph::shifted(&expected, &m.key, m.delta)?;
            }
        }
        let actual = scenario.get(&recipe.key)?;
        if actual.recipe() != expected.recipe()
            || actual.interpolation_grid()? != expected.interpolation_grid()?
        {
            return Err(crate::optional_bond::OptionalBondError::Unsupported {
                capability: "optional graph scenario",
                reason: "original full shaped/rebuilt curve integration required".into(),
            }
            .into());
        }
    }
    Ok(())
}

/// Reject duplicate, aliased and overlapping coordinates before any rebuild.
/// Recovery plus quote rebuilding is permitted; direct grid plus quotes on the
/// same output is ambiguous and requires a separately defined model.
pub fn validate_quote_graph_moves(moves: &[QuoteGraphMove]) -> QuoteRiskResult<()> {
    if moves.len() > 16 {
        return Err(invalid("scenario moves", "at most16 original coordinates"));
    }
    let mut expanded = Vec::new();
    for m in moves {
        finite(m.delta, "scenario move")?;
        for s in slots(&m.key)? {
            if expanded.iter().any(|p: &(String, String, Option<usize>)| {
                p.0 == s.0
                    && ((p.1 == s.1 && (p.2.is_none() || s.2.is_none() || p.2 == s.2))
                        || (p.1.starts_with("quote:")
                            && matches!(s.1.as_str(), "zero" | "basis" | "hazard"))
                        || (s.1.starts_with("quote:")
                            && matches!(p.1.as_str(), "zero" | "basis" | "hazard")))
            }) {
                return Err(invalid(
                    "scenario moves",
                    "overlapping original coordinates",
                ));
            }
            expanded.push(s);
        }
    }
    Ok(())
}

/// Full nonlinear graph for simultaneous coordinates. All quote/helper moves
/// and supplied rebuilding nodes are applied BEFORE one original dependency
/// build. Frozen coordinates overlay that result, never changing helper quotes.
/// Raw product coordinates are retained by the calling complete product owner.
pub fn evaluate_quote_graph_moves(
    prepared: &PreparedQuoteRiskGraph<'_>,
    moves: &[QuoteGraphMove],
) -> QuoteRiskResult<QuoteGraphScenario> {
    validate_quote_graph_moves(moves)?;
    if moves.is_empty() {
        return Ok(prepared.baseline().clone());
    }
    let mut supplied = prepared.input.supplied.clone();
    let mut rebuilding = Vec::new();
    let mut overlays = Vec::new();
    let mut rebuild = false;
    for m in moves {
        let d = m.key.definition();
        match &d.coordinate {
            RiskCoordinate::Raw { .. }
                if d.factor.definition().quote_type != QuoteType::JointZeroRate => {}
            RiskCoordinate::Zero { curve, .. } | RiskCoordinate::Basis { curve, .. }
                if d.response == RiskResponse::RebuildDependents =>
            {
                let base = prepared.input.supplied.get(curve)?;
                if base.recipe().currency != d.factor.definition().currency {
                    return Err(invalid("factor currency", "actual curve currency differs"));
                }
                let actual = supplied.get(curve)?;
                supplied = graph::replace(&supplied, graph::shifted(actual, &m.key, m.delta)?)?;
                rebuild = true;
            }
            _ if d.response == RiskResponse::RebuildDependents => {
                let curve = match &d.coordinate {
                    RiskCoordinate::Quote { curve, .. }
                    | RiskCoordinate::ParallelQuotes { curve, .. }
                    | RiskCoordinate::Recovery { curve } => curve,
                    _ => return Err(invalid("response", "unsupported rebuilding coordinate")),
                };
                if prepared.baseline().curves.get(curve)?.recipe().currency
                    != d.factor.definition().currency
                {
                    return Err(invalid("factor currency", "actual curve currency differs"));
                }
                rebuilding.push((&m.key, m.delta));
                rebuild = true;
            }
            _ => overlays.push(m),
        }
    }
    let mut out = if rebuild {
        let input = QuoteRiskGraphInput {
            rates: prepared.input.rates,
            credit: prepared.input.credit,
            cash: prepared.input.cash,
            supplied: &supplied,
            history: prepared.input.history,
            rate_context: prepared.input.rate_context,
            controls: prepared.input.controls,
        };
        graph::build_many(&input, &prepared.order, &rebuilding, |id, date| {
            prepared
                .publications
                .get(&(id.clone(), date))
                .copied()
                .ok_or_else(|| convex_core::cashflows::CashflowError::InvalidTerms {
                    field: "scenario publication",
                    reason: "outside original frozen requirements".into(),
                })
        })?
    } else {
        QuoteGraphScenario {
            curves: prepared.baseline().curves.clone(),
            build_order: Vec::new(),
            repricing: Vec::new(),
            price_evaluations: 0,
            publications: prepared.baseline().publications.clone(),
            calibrations: Vec::new(),
        }
    };
    for m in overlays {
        // Only frozen overlays reach this original evaluator, hence no extra fit
        // or mutable publication/history path can be introduced here.
        let view = PreparedQuoteRiskGraph {
            input: QuoteRiskGraphInput {
                rates: prepared.input.rates,
                credit: prepared.input.credit,
                cash: prepared.input.cash,
                supplied: prepared.input.supplied,
                history: prepared.input.history,
                rate_context: prepared.input.rate_context,
                controls: prepared.input.controls,
            },
            order: prepared.order.clone(),
            baseline: out.clone(),
            publications: prepared.publications.clone(),
        };
        out.curves = evaluate_quote_graph(&view, &m.key, m.delta)?.curves;
    }
    Ok(out)
}
