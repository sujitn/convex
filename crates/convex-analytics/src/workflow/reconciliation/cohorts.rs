//! Explicit original cohorts and checked physical asset/cash ownership.
use super::*;
use convex_core::futures::{FuturesSettlement, InstrumentPin};

/// Exact legal cash entitlement. Both owned and excluded claims are declared;
/// every original replayed obligation must occur exactly once, never be dropped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CohortClaimEntitlement {
    /// Exact original native obligation key, scoped to this cohort.
    pub key: l::ObligationKey,
    /// Supplied legal ownership, not inferred from an execution timestamp.
    pub entitled: bool,
    /// Actual supplied immutable entitlement/transfer document.
    pub source: ReferencePin,
}
/// An original signed lot is acquired once and can be disposed once, in full.
/// Partial lots require separate original cohorts; no average cost is inferred.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CohortAcquisition {
    /// Already owned at the before point.
    Opening {},
    /// Actual full supplied interval execution.
    Fill {
        /// Exact original actual fill identity.
        fill_id: l::EventId,
    },
    /// Actual checked original delivery, followed by a separate security receipt.
    PhysicalDelivery {
        /// Exact original future cohort; its model CTD is not a receipt.
        future: TradeId,
    },
}
/// Complete fixed original cohort identity, native lifecycle quantity and cash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CohortOwnership {
    /// Exact expected cohort identity in the original holding order.
    pub trade_id: TradeId,
    /// Exact signed original quantity in the PnlPositionInput unit.
    pub quantity: ExactDecimal,
    /// Explicit original acquisition mechanism.
    pub acquisition: CohortAcquisition,
    /// Optional actual full disposal fill. Earned entitlements survive disposal.
    pub disposal_fill: Option<l::EventId>,
    /// Complete ownership coverage across all actual original replayed claims.
    pub entitlements: Vec<CohortClaimEntitlement>,
}
/// Actual original delivery is transferred to this complete cash-bond owner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PhysicalDeliveryLink {
    /// Original future, including full basket/model before actual delivery.
    pub future: TradeId,
    /// Original complete receiving/delivering fixed-bond cohort.
    pub asset: TradeId,
    /// Actual exact security revision, independent of CTD selection.
    pub instrument: InstrumentPin,
}
/// Checked original facts and separately valued security/invoice components.
#[derive(Debug, Clone, Serialize)]
pub struct PhysicalDeliveryValue {
    /// Original futures cohort owning the actual checked delivery event.
    pub future: TradeId,
    /// Complete original delivery/notice input, never a model exercise.
    pub delivery: l::LifecycleEvent,
    /// Complete checked actual security receipt, or still outstanding asset.
    pub security_receipt: Option<l::LifecycleEvent>,
    /// Actual original selected security revision.
    pub instrument: InstrumentPin,
    /// Exact signed proposed original security face.
    pub proposed_face: ExactDecimal,
    /// Exact signed externally recorded face, independent of invoice cash.
    pub received_face: ExactDecimal,
    /// Exact signed remaining security receivable/delivery obligation.
    pub outstanding_face: ExactDecimal,
    /// Full original native invoice with actual partial/full cash receipts.
    pub invoice: l::CashObligation,
    /// Exact remaining signed invoice receivable/payable.
    pub outstanding_invoice: Money,
    /// Explicit same-currency invoice claim carrying convention.
    pub invoice_discount_ratio: f64,
    /// Full remaining invoice value; never folded into a futures quote level.
    pub invoice_present_value: ModelMoney,
    /// Original complete reference bond price scaled to outstanding asset face.
    pub outstanding_asset_present_value: Option<ModelMoney>,
}
pub(super) fn native_quantity(state: &l::LifecycleState) -> Decimal {
    match state.view().position {
        l::LifecyclePosition::BondFace(q)
        | l::LifecyclePosition::TradeCopies(q)
        | l::LifecyclePosition::CdsNotional(q) => q.decimal(),
        l::LifecyclePosition::FuturesContracts(q) => Decimal::from(q),
    }
}
fn find(input: &CausalPnlInput, id: &TradeId) -> ReconciliationResult<usize> {
    input.points[0]
        .positions
        .iter()
        .position(|p| &p.identity.id == id)
        .ok_or_else(|| bad("cohort", "foreign cohort outside exact expected coverage"))
}
fn fill<'a>(point: &'a PnlPointInput, id: &l::EventId) -> Option<&'a PnlFill> {
    point.fills.iter().find(|f| &f.id == id)
}
pub(super) fn acquired(path: &PreparedCausalPnl, point: usize, index: usize) -> bool {
    let Some(owners) = &path.cohorts else {
        return true;
    };
    match &owners[index].acquisition {
        CohortAcquisition::Opening {} => true,
        CohortAcquisition::Fill { fill_id } => fill(&path.input.points[point], fill_id).is_some(),
        CohortAcquisition::PhysicalDelivery { .. } => path.handoffs[point][index].is_some(),
    }
}
pub(super) fn entitlement(
    path: &PreparedCausalPnl,
    point: usize,
    index: usize,
    key: &l::ObligationKey,
) -> bool {
    path.cohorts.as_ref().is_none_or(|owners| {
        acquired(path, point, index)
            && owners[index]
                .entitlements
                .iter()
                .any(|e| &e.key == key && e.entitled)
    })
}
/// Admit all original checked states, exact cohort/fill/entitlement coverage and
/// actual delivery/security/invoice facts before any financial model valuation.
/// Cached state, scalar received face, invoice amount or delivered price cannot
/// be supplied in place of the original native transition owners.
pub fn prepare_cohort_pnl(
    input: &CausalPnlInput,
    cohorts: &[CohortOwnership],
    links: &[PhysicalDeliveryLink],
    states: &[Vec<l::LifecycleState>],
) -> ReconciliationResult<PreparedCausalPnl> {
    let n = input.points.first().map_or(0, |p| p.positions.len());
    if !(1..=128).contains(&n)
        || cohorts.len() != n
        || links.len() > n
        || !(1..=33).contains(&states.len())
        || states.len() != input.points.len()
        || states.len() > input.max_points
        || states.len() * n > input.max_product_evaluations
        || input.max_points > 33
        || input.max_product_evaluations > 4224
        || !(1..=16).contains(&input.causes.len())
        || input
            .points
            .iter()
            .any(|p| p.positions.len() != n || p.fills.len() > 4096 || p.fx.len() > 128)
        || states.iter().any(|s| s.len() != n)
        || cohorts
            .iter()
            .any(|c| c.entitlements.len() > 4096 || c.quantity.decimal().is_zero())
    {
        return Err(bad(
            "cohort work",
            "bounded complete original state/cohort/claim coverage required",
        ));
    }
    if states.iter().flatten().any(|state| {
        state
            .delivery_event()
            .is_some_and(|event| !links.iter().any(|link| link.future == event.trade_id))
    }) {
        return Err(ReconciliationError::Incomplete(
            "every actual physical delivery requires a separate complete asset owner",
        ));
    }
    let mut normalized = input.clone();
    let mut handoffs = vec![vec![None; n]; states.len()];
    for (k, link) in links.iter().enumerate() {
        let f = find(input, &link.future)?;
        let a = find(input, &link.asset)?;
        if f == a
            || links[..k]
                .iter()
                .any(|x| x.future == link.future || x.asset == link.asset)
            || cohorts[a].acquisition
                != (CohortAcquisition::PhysicalDelivery {
                    future: link.future.clone(),
                })
            || cohorts[f].acquisition != (CohortAcquisition::Opening {})
            || cohorts[f].disposal_fill.is_some()
            || cohorts[a].disposal_fill.is_some()
        {
            return Err(bad(
                "delivery link",
                "one distinct original opening future and separate full asset cohort required",
            ));
        }
        for (i, point) in input.points.iter().enumerate() {
            let future = &states[i][f];
            let asset = &states[i][a];
            let l::LifecycleInstrument::Futures { contract, .. } = future.instrument() else {
                return Err(bad("delivery future", "original native future required"));
            };
            let FuturesSettlement::Bond { contract_face, .. } = &contract.settlement else {
                return Err(bad(
                    "delivery future",
                    "actual physical bond settlement required",
                ));
            };
            let l::LifecycleInstrument::Bond { contract: bond, .. } = asset.instrument() else {
                return Err(bad(
                    "delivery asset",
                    "complete original native cash-bond owner required",
                ));
            };
            let pos = &point.positions[a];
            let face = dec(contract_face.decimal().checked_mul(native_quantity(future)))?;
            if pos.identity.instrument.id.as_str() != link.instrument.id.as_str()
                || pos.identity.instrument.version != link.instrument.version
                || bond.terms().currency != contract.currency
                || pos.current.currency != contract.currency
                || pos.current.quantity_unit != q::RiskQuantityUnit::Notional
                || pos.current.reference_quantity.decimal() != bond.terms().face_value
                || cohorts[a].quantity.decimal() != face
                || native_quantity(asset) != face
            {
                return Err(bad(
                    "delivery asset",
                    "exact original security revision/currency/face/native quantity required",
                ));
            }
            if let Some(event) = future.delivery_event() {
                if future.delivered_bond_terms() != Some(bond.terms()) {
                    return Err(bad(
                        "delivery economics",
                        "asset valuation must retain all original checked invoice security terms",
                    ));
                }
                if !matches!(&event.action, l::LifecycleAction::BondDelivery { deliverable, .. } if deliverable == &link.instrument)
                {
                    return Err(bad(
                        "delivery choice",
                        "actual delivered security differs from explicit asset owner",
                    ));
                }
                let key = l::ObligationKey {
                    kind: l::CashKind::DeliveryInvoice,
                    effective_date: event.effective_date,
                    ordinal: 0,
                };
                let invoice = future
                    .view()
                    .obligations
                    .into_iter()
                    .find(|o| o.key == key)
                    .ok_or(ReconciliationError::Incomplete(
                        "original checked invoice absent",
                    ))?;
                if !cohorts[f]
                    .entitlements
                    .iter()
                    .any(|e| e.key == key && e.entitled)
                {
                    return Err(bad("invoice ownership", "actual original invoice ownership must be transferred with the physical asset"));
                }
                let receipt = future.security_receipt_event().cloned();
                let received = if receipt.is_some() {
                    face
                } else {
                    Decimal::ZERO
                };
                let outstanding = dec(invoice
                    .amount
                    .value()
                    .decimal()
                    .checked_sub(invoice.recorded.value().decimal()))?;
                handoffs[i][a] = Some(PhysicalDeliveryValue {
                    future: link.future.clone(),
                    delivery: event.clone(),
                    security_receipt: receipt,
                    instrument: link.instrument.clone(),
                    proposed_face: ExactDecimal::new(face),
                    received_face: ExactDecimal::new(received),
                    outstanding_face: ExactDecimal::new(dec(face.checked_sub(received))?),
                    outstanding_invoice: Money::new(
                        ExactDecimal::new(outstanding),
                        contract.currency,
                    ),
                    invoice_discount_ratio: 1.0,
                    invoice_present_value: money(0.0, contract.currency)?,
                    invoice,
                    outstanding_asset_present_value: None,
                });
            } else if future.view().status != l::EconomicStatus::Active {
                return Err(ReconciliationError::Incomplete(
                    "actual checked physical delivery event absent",
                ));
            }
            let received = handoffs[i][a]
                .as_ref()
                .map_or(Decimal::ZERO, |h| h.received_face.decimal());
            let baseline = handoffs[0][a]
                .as_ref()
                .map_or(Decimal::ZERO, |h| h.received_face.decimal());
            normalized.points[i].positions[a].current.quantity = ExactDecimal::new(dec(pos
                .current
                .quantity
                .decimal()
                .checked_sub(received)
                .and_then(|q| q.checked_add(baseline)))?);
        }
    }
    // Run every original path/fill/time/FX/bound check after removing only exact
    // native checked interval security receipts from position conservation.
    let mut path = prepare_causal_pnl(&normalized)?;
    path.input = input.clone();
    path.cohorts = Some(cohorts.to_vec());
    path.handoffs = handoffs;
    for (j, owner) in cohorts.iter().enumerate() {
        let initial = &input.points[0].positions[j];
        let entitlement_keys = owner
            .entitlements
            .iter()
            .map(|e| &e.key)
            .collect::<std::collections::BTreeSet<_>>();
        let owned_keys = owner
            .entitlements
            .iter()
            .filter(|e| e.entitled)
            .map(|e| &e.key)
            .collect::<std::collections::BTreeSet<_>>();
        if matches!(
            owner.acquisition,
            CohortAcquisition::PhysicalDelivery { .. }
        ) && !links.iter().any(|l| l.asset == owner.trade_id)
        {
            return Err(ReconciliationError::Incomplete(
                "actual physical asset owner link absent",
            ));
        }
        if owner.trade_id != initial.identity.id
            || entitlement_keys.len() != owner.entitlements.len()
        {
            return Err(bad(
                "cohort",
                "exact ordered identity and unique complete entitlement keys required",
            ));
        }
        let prior_received = states
            .iter()
            .enumerate()
            .filter(|(i, _)| !acquired(&path, *i, j))
            .flat_map(|(_, row)| row[j].view().obligations)
            .filter(|o| !o.recorded.value().decimal().is_zero())
            .map(|o| o.key)
            .collect::<std::collections::BTreeSet<_>>();
        let mut keys = std::collections::BTreeSet::new();
        let mut acquisition_seen = matches!(owner.acquisition, CohortAcquisition::Opening {});
        let mut disposal_seen = owner.disposal_fill.is_none();
        for (i, point) in input.points.iter().enumerate() {
            let state = &states[i][j];
            let view = state.view();
            if view.status == l::EconomicStatus::DefaultPending {
                return Err(ReconciliationError::Incomplete(
                    "actual original default recovery terms absent",
                ));
            }
            let pos = &point.positions[j];
            let native = if matches!(state.instrument(), l::LifecycleInstrument::Swap(_))
                && pos.current.quantity_unit == q::RiskQuantityUnit::Notional
            {
                dec(owner
                    .quantity
                    .decimal()
                    .checked_div(pos.current.reference_quantity.decimal()))?
            } else {
                owner.quantity.decimal()
            };
            if view.trade_id != owner.trade_id || native_quantity(state) != native {
                return Err(bad(
                    "cohort quantity",
                    "same fixed signed original native lifecycle cohort required",
                ));
            }
            let is_acquired = acquired(&path, i, j);
            acquisition_seen |= is_acquired;
            let disposed = owner.disposal_fill.as_ref().and_then(|id| fill(point, id));
            disposal_seen |= disposed.is_some();
            let expected = match &owner.acquisition {
                CohortAcquisition::Opening {} => owner.quantity.decimal(),
                CohortAcquisition::Fill { fill_id } => {
                    if let Some(f) = fill(point, fill_id) {
                        if f.trade_id != owner.trade_id || f.quantity != owner.quantity {
                            return Err(bad(
                                "acquisition",
                                "exact full original signed cohort fill required",
                            ));
                        }
                        if disposed.is_some_and(|d| {
                            d.date < f.date
                                || d.known_at < f.known_at
                                || point.fills.iter().position(|x| x.id == d.id)
                                    < point.fills.iter().position(|x| x.id == f.id)
                        }) {
                            return Err(bad(
                                "disposal order",
                                "acquisition must precede actual disposal",
                            ));
                        }
                        owner.quantity.decimal()
                    } else {
                        Decimal::ZERO
                    }
                }
                CohortAcquisition::PhysicalDelivery { .. } => path.handoffs[i][j]
                    .as_ref()
                    .map_or(Decimal::ZERO, |h| h.received_face.decimal()),
            };
            let expected = if let Some(f) = disposed {
                if !is_acquired
                    || f.trade_id != owner.trade_id
                    || f.quantity.decimal() != -owner.quantity.decimal()
                {
                    return Err(bad(
                        "disposal",
                        "actual full disposal of acquired original cohort required",
                    ));
                }
                Decimal::ZERO
            } else {
                expected
            };
            if pos.current.quantity.decimal() != expected || point.fills.iter().filter(|f| f.trade_id == owner.trade_id).any(|f| {
                !matches!(&owner.acquisition,CohortAcquisition::Fill{fill_id} if fill_id==&f.id) && owner.disposal_fill.as_ref()!=Some(&f.id)
            }) || (!is_acquired && !pos.pending_quantity.decimal().is_zero()) {
                return Err(bad("cohort exposure", "only declared full acquisition/disposal/actual receipt changes exposure; dormant pending forbidden"));
            }
            for claim in view.obligations {
                keys.insert(claim.key.clone());
                if !entitlement_keys.contains(&claim.key) {
                    return Err(ReconciliationError::Incomplete(
                        "complete owned/excluded original claim coverage absent",
                    ));
                }
                if owned_keys.contains(&claim.key) && prior_received.contains(&claim.key) {
                    return Err(bad("cash transfer", "historically received cash cannot be acquired with an outstanding claim; supply a separate original residual claim owner"));
                }
                if let CohortAcquisition::Fill { fill_id } = &owner.acquisition {
                    if let Some(f) = fill(point, fill_id) {
                        // A sold earned claim is a separately supplied entitlement;
                        // original native economics never derive ownership from date.
                        if f.date < input.points[0].valuation_date {
                            return Err(bad("acquisition", "outside supplied interval"));
                        }
                    }
                }
            }
        }
        if !acquisition_seen
            || !disposal_seen
            || owner.entitlements.iter().any(|e| !keys.contains(&e.key))
        {
            return Err(bad(
                "cohort coverage",
                "unused acquisition/disposal/entitlement facts outside complete path",
            ));
        }
    }
    Ok(path)
}
