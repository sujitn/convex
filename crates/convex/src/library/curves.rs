//! Full original calibration resources and native graph dispatch.
use super::inputs::ResolvedInputs;
use super::*;
use convex_analytics::{
    cash_calibration as c, credit_calibration as d, quote_risk as q, rate_calibration as r,
};
use convex_core::contracts::*;
use serde::{Deserialize, Serialize};

/// Original rate recipe plus actual immutable source quote revisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RateCalibrationInput {
    /// Full dated helpers, mappings, numerical method and native tolerances.
    pub recipe: r::RateCurveRecipe,
    /// One actual immutable revision per helper in original source order.
    pub quote_versions: Vec<Version>,
    /// Actual original calendar stamp for each futures helper; None otherwise.
    pub futures_calendar_stamps: Vec<Option<convex_core::futures::ReferenceStamp>>,
}
/// Actual resources corresponding to one original cash helper.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CashHelperCalendars {
    /// Original independent bond calendar revisions.
    Bond {
        /// Four actual pins, no supplied terms are reduced.
        pins: BondCalendarPins,
    },
    /// Actual original loan calendar.
    Repo {
        /// Native identity/version checked against the loan.
        pin: convex_rates::ReferencePin,
    },
}
/// Original cash recipe with its actual matched helper calendars.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CashCalibrationInput {
    /// Full original securities/loans, role/model/quote settings and dependencies.
    pub recipe: c::CashCurveRecipe,
    /// One actual resource shape per helper, preserving original order.
    pub calendars: Vec<CashHelperCalendars>,
}
/// Original calibration economic/history instant, independent of system time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CalibrationContext {
    /// Full original valuation/payment/fixing eligibility policy.
    pub cashflows: convex_rates::CashflowContext,
    /// Actual supplied economic instant.
    pub as_of: UtcTimestamp,
}
/// Complete rate/CDS/government/issuer-basis/repo graph over supplied dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CalibrationInput {
    /// Full original rate curve recipes and quote revisions.
    pub rates: Vec<RateCalibrationInput>,
    /// Full original single-name CDS recipes, calendars inside terms.
    pub credit: Vec<d::CdsCurveRecipe>,
    /// Full original securities/loans and actual calendar revisions.
    pub cash: Vec<CashCalibrationInput>,
    /// Actual required time policy for rate recipes.
    pub rate_context: Option<CalibrationContext>,
    /// Original identification/Jacobian/SVD controls.
    pub controls: FitNumericalControls,
}
/// Full native graph build, scenario or product risk; quote rebuild is never a
/// calibrated-node shortcut and all native helper residual guards still apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalibrationOperation {
    /// Full actual build plus original native repricing/work/publication evidence.
    Build {},
    /// Complete native valuation on the original actually rebuilt quote graph.
    Price {
        /// Full original product and quotation state.
        product: Box<super::risk::RiskProductInput>,
    },
    /// Original exact quote/node/recovery scenario and dependency propagation.
    Scenario {
        /// Full native factor/unit/model/response key.
        key: q::QuoteRiskKey,
        /// Actual move in the key's original native unit.
        delta: f64,
    },
    /// Actual product signed graph risk with original five-PV/half-step guards.
    ProductRisk {
        /// Full original product and quotation state.
        product: Box<super::risk::RiskProductInput>,
        /// Full actual curve/quote/convention/model/unit key.
        key: q::QuoteRiskKey,
        /// Signed native quantity/reference units, scaled once.
        position: q::RiskPosition,
        /// Actual caller snapshot identity.
        snapshot_id: SnapshotId,
        /// Original bump/half-step stability/work admission.
        settings: q::QuoteRiskSettings,
    },
    /// Full native product repricing on one complete graph scenario.
    ProductScenario {
        /// Original full product and quotation.
        product: Box<super::risk::RiskProductInput>,
        /// Full explicit scenario key.
        key: q::QuoteRiskKey,
        /// Original native-unit move.
        delta: f64,
        /// Exact signed native quantity.
        position: q::RiskPosition,
        /// Actual immutable snapshot identity.
        snapshot_id: SnapshotId,
    },
}

pub(super) fn describe_curves(curves: &CurveSet) -> LibraryResult<Value> {
    let mut result = Vec::with_capacity(curves.len());
    for key in curves.keys() {
        let curve = curves.get(key)?;
        result.push(describe_curve(curve));
    }
    Ok(serde_json::json!({"curves":result}))
}
pub(super) fn describe_curve(curve: &BuiltCurve) -> Value {
    let grid = match curve.interpolation_grid() {
        Ok((tenors, values)) => serde_json::json!({"tenors":tenors,"values":values}),
        Err(e) => serde_json::json!({"status":"unsupported","reason":e.to_string()}),
    };
    serde_json::json!({"recipe":curve.recipe(),"diagnostics":curve.diagnostics(),"domain":curve.domain(),"interpolation_grid":grid})
}
pub(super) fn fit_curve(
    recipe: &CurveRecipe,
    pillars: &[Date],
    controls: FitNumericalControls,
) -> LibraryResult<Value> {
    let out = convex_curves::contracts::fit_money_market_curve(recipe, pillars, controls).map_err(
        |e| match e {
            CurveFitError::Contract(e) => LibraryError::from(e),
            CurveFitError::Solver(e) => super::errors::fit_error(*e),
        },
    )?;
    let d = out.solver;
    let diagnostics = serde_json::json!({"iterations":d.iterations,"evaluations":d.evaluations,
        "scaled_rms":d.scaled_rms,"native_residuals":d.native_residuals,"rank":d.rank,
        "condition_estimate":d.condition_estimate,"active_bounds":d.active_bounds});
    let curves = CurveSet::new().with_curve(out.curve)?;
    Ok(serde_json::json!({"result":describe_curves(&curves)?,"solver":diagnostics}))
}
fn graph_value(graph: &q::QuoteGraphScenario) -> LibraryResult<Value> {
    Ok(
        serde_json::json!({"result":describe_curves(&graph.curves)?,"build_order":graph.build_order,
        "repricing":graph.repricing,"price_evaluations":graph.price_evaluations,"publications":graph.publications}),
    )
}
fn with_graph<T>(
    input: &CalibrationInput,
    resources: &ResolvedInputs<'_>,
    consume: impl FnOnce(&q::PreparedQuoteRiskGraph<'_>) -> LibraryResult<T>,
) -> LibraryResult<T> {
    if input.rates.len() + input.credit.len() + input.cash.len() > 256 {
        return Err(LibraryError::limit("at most256 curve recipes"));
    }
    // Each existing native builder is borrowed intact into the original graph.
    let mut future_histories = Vec::new();
    for rate in &input.rates {
        if rate.futures_calendar_stamps.len() != rate.recipe.helpers.len() {
            return Err(LibraryError::invalid(
                "futures_calendar_stamps",
                "one entry per original rate helper required",
            ));
        }
        let mut histories = Vec::new();
        for helper in &rate.recipe.helpers {
            histories.push(match &helper.terms {
                r::RateCalibrationTerms::RateFuture(f) => {
                    Some(convex_analytics::futures::FuturesFixings::new(
                        resources.fixing_observations().to_vec(),
                        f.clearing.fixing_selection.ok_or_else(|| {
                            LibraryError::invalid("clearing", "missing original revision selection")
                        })?,
                    )?)
                }
                _ => None,
            });
        }
        future_histories.push(histories);
    }
    let mut rate_resources = Vec::new();
    for (ri, rate) in input.rates.iter().enumerate() {
        let mut bindings = Vec::new();
        for (hi, helper) in rate.recipe.helpers.iter().enumerate() {
            let stamp = &rate.futures_calendar_stamps[hi];
            if !matches!(&helper.terms, r::RateCalibrationTerms::RateFuture(_)) && stamp.is_some() {
                return Err(LibraryError::invalid(
                    "futures_calendar_stamps",
                    "non-future helper cannot consume future calendar stamp",
                ));
            }
            bindings.push(match &helper.terms {
                r::RateCalibrationTerms::MoneyMarket(m) => {
                    r::RateHelperResources::MoneyMarket(resources.rate_binding(&m.calendar)?)
                }
                r::RateCalibrationTerms::Swap(t) => {
                    r::RateHelperResources::Swap(resources.swap_calendars(t)?)
                }
                r::RateCalibrationTerms::BasisSwap { terms, .. } => {
                    r::RateHelperResources::BasisSwap(resources.basis_calendars(terms)?)
                }
                r::RateCalibrationTerms::RateFuture(_) => r::RateHelperResources::RateFuture {
                    calendar: super::products::future_calendar(
                        stamp.as_ref().ok_or_else(|| {
                            LibraryError::missing("future calendar stamp", Value::Null)
                        })?,
                        resources,
                    )?,
                    fixings: future_histories[ri][hi].as_ref().ok_or_else(|| {
                        LibraryError::missing("original clearing history", Value::Null)
                    })?,
                },
            });
        }
        rate_resources.push(bindings);
    }
    let rates = input
        .rates
        .iter()
        .zip(&rate_resources)
        .map(|(i, resources)| q::RateRiskCurveInput {
            build: r::RateCurveBuildInput {
                recipe: &i.recipe,
                resources,
            },
            quote_versions: &i.quote_versions,
        })
        .collect::<Vec<_>>();
    let credit_calendars = input
        .credit
        .iter()
        .map(|i| {
            i.helpers
                .iter()
                .map(|h| resources.credit_calendars(&h.terms))
                .collect::<LibraryResult<Vec<_>>>()
        })
        .collect::<LibraryResult<Vec<_>>>()?;
    let credit = input
        .credit
        .iter()
        .zip(&credit_calendars)
        .map(|(recipe, calendars)| d::CdsCurveBuildInput { recipe, calendars })
        .collect::<Vec<_>>();
    let mut cash_resources = Vec::new();
    // Calendar version pins supplement the original full helper's names, without
    // mutating any helper economics or replacing the original native builder.
    let mut bond_inputs = Vec::new();
    for i in &input.cash {
        if i.calendars.len() != i.recipe.helpers.len() {
            return Err(LibraryError::invalid(
                "cash calendars",
                "one exact resource shape per original helper required",
            ));
        }
        let inputs = i
            .recipe
            .helpers
            .iter()
            .zip(&i.calendars)
            .map(|(h, c)| match (h, c) {
                (
                    c::CashCalibrationHelper::Bond { input: h },
                    CashHelperCalendars::Bond { pins },
                ) => Ok(Some(BondInput {
                    terms: h.terms.clone(),
                    calendars: pins.clone(),
                })),
                (c::CashCalibrationHelper::Repo { .. }, CashHelperCalendars::Repo { .. }) => {
                    Ok(None)
                }
                _ => Err(LibraryError::invalid(
                    "cash calendars",
                    "full original helper/resource family mismatch",
                )),
            })
            .collect::<LibraryResult<Vec<_>>>()?;
        bond_inputs.push(inputs);
    }
    for (ci, i) in input.cash.iter().enumerate() {
        let mut bindings = Vec::new();
        for (hi, (h, c)) in i.recipe.helpers.iter().zip(&i.calendars).enumerate() {
            bindings.push(match (h, c) {
                (c::CashCalibrationHelper::Bond { input: h }, CashHelperCalendars::Bond { .. }) => {
                    c::CashHelperResources::Bond {
                        ids: &h.terms.calendars,
                        calendars: resources.bond_calendars(
                            bond_inputs[ci][hi].as_ref().ok_or_else(|| {
                                LibraryError::missing("bond calendars", Value::Null)
                            })?,
                        )?,
                    }
                }
                (c::CashCalibrationHelper::Repo { .. }, CashHelperCalendars::Repo { pin }) => {
                    c::CashHelperResources::Repo(resources.rate_binding(pin)?)
                }
                _ => {
                    return Err(LibraryError::invalid(
                        "cash resources",
                        "native helper family mismatch",
                    ))
                }
            });
        }
        cash_resources.push(bindings);
    }
    let cash = input
        .cash
        .iter()
        .zip(&cash_resources)
        .map(|(i, resources)| c::CashCurveBuildInput {
            recipe: &i.recipe,
            resources,
        })
        .collect::<Vec<_>>();
    let rate_context = input
        .rate_context
        .as_ref()
        .map(|c| r::RateCalibrationContext {
            cashflows: c.cashflows,
            as_of: c.as_of,
        });
    let graph = q::prepare_quote_risk_graph(
        q::QuoteRiskGraphInput {
            rates: &rates,
            credit: &credit,
            cash: &cash,
            supplied: &resources.curves,
            history: &resources.history,
            rate_context,
            controls: input.controls,
        },
        resources.publications.callback(),
    )?;
    consume(&graph)
}
pub(super) fn calibration_market(
    input: &CalibrationInput,
    resources: &ResolvedInputs<'_>,
) -> LibraryResult<(CurveSet, Value)> {
    with_graph(input, resources, |g| {
        Ok((g.baseline().curves.clone(), graph_value(g.baseline())?))
    })
}
pub(super) fn calibration(
    input: &CalibrationInput,
    action: &CalibrationOperation,
    resources: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    with_graph(input, resources, |graph| match action {
        CalibrationOperation::Build {} => graph_value(graph.baseline()),
        CalibrationOperation::Price { product } => {
            let valuation = super::risk::graph_price(product, graph.baseline(), resources)?;
            Ok(serde_json::json!({"valuation":valuation,"graph":graph_value(graph.baseline())?}))
        }
        CalibrationOperation::Scenario { key, delta } => {
            graph_value(&q::evaluate_quote_graph(graph, key, *delta)?)
        }
        CalibrationOperation::ProductRisk {
            product,
            key,
            position,
            snapshot_id,
            settings,
        } => super::risk::graph_risk(
            product,
            graph,
            key.clone(),
            position.clone(),
            snapshot_id.clone(),
            settings.clone(),
            resources,
        ),
        CalibrationOperation::ProductScenario {
            product,
            key,
            delta,
            position,
            snapshot_id,
        } => {
            let shocked = q::evaluate_quote_graph(graph, key, *delta)?;
            let result = super::risk::graph_scenario(
                product,
                graph.baseline(),
                &shocked,
                position.clone(),
                snapshot_id.clone(),
                resources,
            )?;
            Ok(serde_json::json!({"valuation":result,"graph":graph_value(&shocked)?}))
        }
    })
}
