//! Thin complete information-set reconstruction over original pure owners.
use super::*;
use convex_analytics::{optional_bond as o, quote_risk as q, workflow as w};
mod admission;

/// Full original product and replay input bound to one expected holding.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CausalPnlHoldingInput {
    /// Exact expected original holding/cohort identity.
    pub id: convex_core::contracts::TradeId,
    /// Complete original product, models, native numerical policies and contexts.
    pub product: BookScenarioProductInput,
    /// Original complete opening state and all actual supplied replay events.
    pub lifecycle: LifecycleInput,
}
/// Complete values; caller snapshot and manifest identify precisely this bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CausalPnlInformationSet {
    /// Exact time/FX/quantities/fills/identity declaration.
    pub point: w::PnlPointInput,
    /// Whole actual calendars/curves/calibration/publication/history data.
    pub inputs: LibraryInputs,
    /// Complete expected native products and replay states, in original order.
    pub holdings: Vec<CausalPnlHoldingInput>,
}
/// Complete original curve/calibration data, never a price result or remote ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CausalPnlMarketInput {
    /// Whole original native source recipes and actual values.
    pub curves: Vec<CurveRecipe>,
    /// Whole original native helper graph where actually fitted.
    pub calibration: Option<Box<CalibrationInput>>,
}
/// Lossless local binding to full supplied values. This is wire deduplication,
/// not a repository, runtime cache, provider or external snapshot lookup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CausalPnlPointBinding {
    /// Full exact point/quantity/FX/fill/manifest declaration.
    pub point: w::PnlPointInput,
    /// Exact local index of a whole supplied market bundle.
    pub market: usize,
    /// Exact local indices of complete original holdings, in expected order.
    pub holdings: Vec<usize>,
    /// Actual interval received revisions appended to common original history.
    pub additional_fixings: Vec<convex_core::fixings::FixingObservation>,
    /// Actual administrator decisions appended to common original schedule.
    pub additional_publications: Vec<convex_core::fixings::FixingRequirement>,
}
fn materialize(
    input: &MixedBookPnlInput,
    b: &CausalPnlPointBinding,
) -> LibraryResult<CausalPnlInformationSet> {
    let market = input.markets.get(b.market).ok_or_else(|| {
        LibraryError::invalid(
            "market binding",
            "exact supplied local market index missing",
        )
    })?;
    let mut resources = input.resources.clone();
    resources.curves = market.curves.clone();
    resources.calibration = market.calibration.clone();
    resources.fixings.extend(b.additional_fixings.clone());
    resources
        .publications
        .extend(b.additional_publications.clone());
    let holdings = b
        .holdings
        .iter()
        .map(|i| {
            input.holdings.get(*i).cloned().ok_or_else(|| {
                LibraryError::invalid(
                    "holding binding",
                    "exact supplied local full holding index missing",
                )
            })
        })
        .collect::<LibraryResult<Vec<_>>>()?;
    Ok(CausalPnlInformationSet {
        point: b.point.clone(),
        inputs: resources,
        holdings,
    })
}
/// Full after-information set for one independently classified causal step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CausalPnlStep {
    /// Declared causal category; unrelated economic changes fail admission.
    pub cause: w::PnlCause,
    /// Full native input data after this one admitted phase.
    pub after: CausalPnlPointBinding,
}
/// Full standalone counterfactual, or explicit economic dependence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CausalPnlCounterfactual {
    /// Reconstructed/priced full original information set changing only this cause.
    Available {
        /// Whole actual input values and replay state.
        information: Box<CausalPnlPointBinding>,
    },
    /// No full economically admissible independent experiment exists.
    Unavailable {
        /// Actual missing prior fixing/event/amendment prerequisite.
        reason: String,
    },
}
/// Complete explicit before/after causal reconciliation. Outer LibraryInputs must
/// be empty: no second hidden resource set or snapshot lookup enters the answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MixedBookPnlInput {
    /// Complete common calendars and original history/publication inputs; curves/calibration must be empty.
    pub resources: LibraryInputs,
    /// Complete actual immutable market bundles, shared through local indices only.
    pub markets: Vec<CausalPnlMarketInput>,
    /// Complete actual immutable products/opening states/replay prefixes, shared through local indices only.
    pub holdings: Vec<CausalPnlHoldingInput>,
    /// Full exact opening values/states/history and pending positions.
    pub before: CausalPnlPointBinding,
    /// Ordered causes with complete after values; final endpoint is closing set.
    pub steps: Vec<CausalPnlStep>,
    /// One explicit full standalone or dependent declaration for each phase.
    pub standalone: Vec<CausalPnlCounterfactual>,
    /// Explicit common reporting currency.
    pub reporting_currency: Currency,
    /// Explicit complete input/native-mark bound,1..33.
    pub max_points: usize,
    /// Explicit complete original product-call bound,1..4224.
    pub max_product_evaluations: usize,
}
fn native_error(e: w::ReconciliationError) -> LibraryError {
    match e {
        w::ReconciliationError::Workflow(e) => e.into(),
        w::ReconciliationError::Lifecycle(e) => events::native_error(e),
        w::ReconciliationError::Cash(e) => e.into(),
        w::ReconciliationError::Incomplete(reason) => {
            LibraryError::missing("lifecycle economics", serde_json::json!({"reason":reason}))
        }
    }
}
/// Explicit original cohorts and complete actual physical asset/invoice links.
/// This opt-in contract preserves the existing single-holding book_pnl route.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CohortBookPnlInput {
    /// Complete original before/path/standalone values, terms and replay inputs.
    pub book: MixedBookPnlInput,
    /// One fixed signed original lifecycle lot and complete cash ownership per holding.
    pub cohorts: Vec<w::CohortOwnership>,
    /// One actual complete receiving/delivering asset owner per linked future.
    pub physical_deliveries: Vec<w::PhysicalDeliveryLink>,
    /// Explicit conserved full original security partitions; no execution fills.
    #[serde(default)]
    pub lot_splits: Vec<w::CohortLotSplit>,
    /// Dated legal transfers of original earned cash, separate from security lots.
    #[serde(default)]
    pub claim_transfers: Vec<w::CohortClaimTransfer>,
}
pub(super) fn reconcile_cohorts(
    input: &CohortBookPnlInput,
    outer: &LibraryInputs,
) -> LibraryResult<Value> {
    if input.cohorts.len() != input.book.before.point.positions.len()
        || input.physical_deliveries.len() > 128
        || input.claim_transfers.len() > 128
        || input
            .claim_transfers
            .iter()
            .any(|t| t.applied_points.len() > 33 || t.cutoff.receipt_ids.len() > 4096)
        || input.cohorts.iter().any(|c| c.entitlements.len() > 4096)
    {
        return Err(LibraryError::invalid(
            "cohort coverage",
            "bounded complete original cohorts and entitlement coverage required",
        ));
    }
    if input.cohorts.iter().any(|c| {
        matches!(&c.acquisition, w::CohortAcquisition::PhysicalDelivery { future }
            if !input.physical_deliveries.iter().any(|l| &l.future == future && l.asset == c.trade_id))
    }) {
        return Err(LibraryError::missing(
            "physical delivery",
            serde_json::json!({"reason":"complete original physical asset owner link absent"}),
        ));
    }
    reconcile_inner(&input.book, outer, Some(input))
}
/// Reconstruct each complete actual information set once; all native numerical
/// owners keep original prices, histories, solver domains and work/convergence.
pub(super) fn reconcile(input: &MixedBookPnlInput, outer: &LibraryInputs) -> LibraryResult<Value> {
    reconcile_inner(input, outer, None)
}
fn reconcile_inner(
    input: &MixedBookPnlInput,
    outer: &LibraryInputs,
    cohorts: Option<&CohortBookPnlInput>,
) -> LibraryResult<Value> {
    let count = 1
        + input.steps.len()
        + input
            .standalone
            .iter()
            .filter(|c| matches!(c, CausalPnlCounterfactual::Available { .. }))
            .count();
    let n = input.before.point.positions.len();
    if !input.resources.curves.is_empty()
        || input.resources.calibration.is_some()
        || input.markets.len() > 33
        || input.markets.is_empty()
        || input.holdings.len() > 4224
        || input.holdings.is_empty()
        || !(1..=16).contains(&input.steps.len())
        || input.standalone.len() != input.steps.len()
        || !(1..=33).contains(&input.max_points)
        || count > input.max_points
        || !(1..=128).contains(&n)
        || !(1..=4224).contains(&input.max_product_evaluations)
        || count * n > input.max_product_evaluations
    {
        return Err(LibraryError::invalid("resources","bounded complete local bundles, path, coverage and empty common curve/calibration fields required"));
    }
    // Bound every binding before cloning its full native values. Admission and
    // preparation subsequently verify identities, exact units and uniqueness.
    let bindings = std::iter::once(&input.before)
        .chain(input.steps.iter().map(|s| &s.after))
        .chain(input.standalone.iter().filter_map(|s| match s {
            CausalPnlCounterfactual::Available { information } => Some(information.as_ref()),
            CausalPnlCounterfactual::Unavailable { .. } => None,
        }));
    if bindings.into_iter().any(|b| {
        b.holdings.len() != n
            || b.point.positions.len() != n
            || b.point.fills.len() > 4096
            || b.point.fx.len() > 128
    }) {
        return Err(LibraryError::invalid(
            "coverage",
            "bounded complete coverage, fills and FX required before materialization",
        ));
    }
    let mut sets = vec![materialize(input, &input.before)?];
    for s in &input.steps {
        sets.push(materialize(input, &s.after)?);
    }
    let mut standalone = Vec::new();
    for c in &input.standalone {
        match c {
            CausalPnlCounterfactual::Available { information } => {
                standalone.push(w::PnlStandalone::Available { point: sets.len() });
                sets.push(materialize(input, information)?);
            }
            CausalPnlCounterfactual::Unavailable { reason } => {
                standalone.push(w::PnlStandalone::Unavailable {
                    reason: reason.clone(),
                })
            }
        }
    }
    admission::validate(input, outer, &sets, cohorts)?;
    let declaration = w::CausalPnlInput {
        causes: input.steps.iter().map(|s| s.cause).collect(),
        points: sets.iter().map(|s| s.point.clone()).collect(),
        standalone,
        reporting_currency: input.reporting_currency,
        max_points: input.max_points,
        max_product_evaluations: input.max_product_evaluations,
    };
    let cohort_replays = if let Some(c) = cohorts {
        // Original replay/coverage admission precedes model valuation. Every
        // complete state is rebuilt once from the actual point's resources.
        let replay = sets
            .iter()
            .map(|set| {
                let r = inputs::ResolvedInputs::new(&set.inputs)?;
                set.holdings
                    .iter()
                    .map(|h| events::replay_native(&h.lifecycle, &r))
                    .collect::<LibraryResult<Vec<_>>>()
            })
            .collect::<LibraryResult<Vec<_>>>()?;
        let states = replay
            .iter()
            .map(|rows| rows.iter().map(|r| r.state.clone()).collect())
            .collect::<Vec<_>>();
        Some((
            w::prepare_cohort_pnl_with_transfers(
                &declaration,
                &c.cohorts,
                &c.physical_deliveries,
                &c.lot_splits,
                &c.claim_transfers,
                &states,
            )
            .map_err(native_error)?,
            replay,
        ))
    } else {
        None
    };
    let plain_path;
    let path = if let Some((path, _)) = &cohort_replays {
        path
    } else {
        plain_path = w::prepare_causal_pnl(&declaration).map_err(native_error)?;
        &plain_path
    };
    let mut rows = (0..input.before.point.positions.len())
        .map(|_| Vec::new())
        .collect::<Vec<_>>();
    let mut information = Vec::new();
    for (i, set) in sets.iter().enumerate() {
        let resolved = inputs::ResolvedInputs::new(&set.inputs)?;
        let mut states = Vec::new();
        let graph = q::QuoteGraphScenario {
            curves: resolved.curves.clone(),
            build_order: Vec::new(),
            repricing: Vec::new(),
            price_evaluations: 0,
            publications: set.inputs.publications.clone(),
            calibrations: Vec::new(),
        };
        for (j, h) in set.holdings.iter().enumerate() {
            let plain_replay;
            let replay = if let Some((_, rows)) = &cohort_replays {
                &rows[i][j]
            } else {
                plain_replay = events::replay_native(&h.lifecycle, &resolved)?;
                &plain_replay
            };
            let discount = resolved.curves.get(&set.point.positions[j].cash_discount)?;
            let mark = |p: &w::CausalPnlProduct<'_>| {
                w::mark_causal_pnl_position(path, i, j, &replay.state, p, discount.as_ref())
                    .map_err(native_error)
            };
            let value = match &h.product {
                BookScenarioProductInput::CashBasis {
                    security,
                    basis,
                    model,
                } => {
                    let resources =
                        cash::helper_resources(&security.helper, &security.calendars, &resolved)?;
                    mark(&w::CausalPnlProduct::CashBasis {
                        helper: &security.helper,
                        resources: &resources,
                        model,
                        basis,
                        curves: &resolved.curves,
                    })?
                }
                BookScenarioProductInput::Product { product } => match product.as_ref() {
                    RiskProductInput::BondFuture {
                        input,
                        discount,
                        policy,
                    } => products::with_bond_future(input, &resolved, |inputs| {
                        mark(&w::CausalPnlProduct::Delivery {
                            inputs,
                            discount: discount.native(&resolved)?,
                            graph: &graph,
                            policy: *policy,
                        })
                    })?,
                    RiskProductInput::OptionalBond {
                        input, oas_decimal, ..
                    } => {
                        let contract = resolved.bond(&input.bond)?;
                        let model = products::model(&input.model, &resolved)?;
                        let projection = input
                            .projection
                            .as_ref()
                            .map(|k| resolved.curves.get(k))
                            .transpose()?;
                        let native = o::OptionalBondInputs {
                            discount: resolved.curves.get(&input.discount)?.as_ref(),
                            projection: projection.map(|p| p.as_ref()),
                            model: &model,
                            exercise_calendar: resolved.calendar(&input.bond.calendars.exercise)?,
                        };
                        mark(&w::CausalPnlProduct::Optional {
                            contract: &contract,
                            context: input.context,
                            projection_context: &input.projection_context,
                            market: resolved.market(),
                            inputs: native,
                            settings: input.settings,
                            oas_decimal: *oas_decimal,
                        })?
                    }
                    _ => risk::with_product(product, &resolved, |product| {
                        mark(&w::CausalPnlProduct::Standard(product))
                    })?,
                },
            };
            rows[j].push(value);
            states.push(serde_json::json!({"id":h.id,"transitions":replay.transitions.iter().map(|(event,effects)|serde_json::json!({"event":event,"effects":effects})).collect::<Vec<_>>()}));
        }
        information.push(serde_json::json!({"snapshot_id":set.point.snapshot_id,"manifest":set.point.manifest,"market_calibration":resolved.calibration,"lifecycle":states}));
    }
    let report = w::aggregate_causal_pnl(path, rows).map_err(native_error)?;
    if let Some(c) = cohorts {
        let mut comparisons = Vec::new();
        for link in &c.physical_deliveries {
            let future = report
                .trades
                .iter()
                .find(|t| t.id == link.future)
                .ok_or_else(|| {
                    LibraryError::invalid("delivery", "complete original future report absent")
                })?;
            let asset = report
                .trades
                .iter()
                .find(|t| t.id == link.asset)
                .ok_or_else(|| {
                    LibraryError::invalid("delivery", "complete original asset report absent")
                })?;
            for (i, mark) in asset.marks.iter().enumerate() {
                if let Some(h) = &mark.physical_delivery {
                    let ctd = future.marks[..=i].iter().rev().find_map(|m| m.ctd.as_ref());
                    comparisons.push(serde_json::json!({"point":i,"future":link.future,"asset":link.asset,"actual_deliverable":h.instrument,"last_model_ctd":ctd,"matches_selection":ctd.map(|x| x.selection_ties.contains(&h.instrument)),"matches_fair":ctd.map(|x| x.fair_ties.contains(&h.instrument))}));
                }
            }
        }
        return Ok(
            serde_json::json!({"request":c,"report":report,"information_sets":information,"delivery_comparisons":comparisons,"meaning":"causal reconciliation of explicit original cohort exposure, cash entitlements and actual supplied physical delivery/security/invoice facts; no authentication, ledger posting or execution authority"}),
        );
    }
    Ok(
        serde_json::json!({"request":input,"report":report,"information_sets":information,"meaning":"causal reconciliation of full supplied model values and actual supplied lifecycle/fill/receipt facts; no authoritative ledger, cash authentication, order or execution service"}),
    )
}
