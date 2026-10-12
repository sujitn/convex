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
    /// Separately identified legal residual, excluding exact prior paid cash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub residual: Option<CohortResidualClaim>,
}
/// Explicit legal acquisition cutoff; identities resolve same-date receipt order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CohortCashCutoff {
    /// Actual acquisition economic date.
    pub date: Date,
    /// Actual immutable acquisition information cutoff.
    pub known_at: UtcTimestamp,
    /// Complete original actual receipt prefix retained by the prior cash owner.
    pub receipt_ids: Vec<l::EventId>,
}
/// Legal owned fraction of the original remaining claim, not a new cash model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CohortResidualClaim {
    /// Unique original legal residual identity across this complete book.
    pub id: l::EventId,
    /// Owned residual fraction in (0,1]; the complement is explicitly excluded.
    pub fraction: ExactDecimal,
    /// Exact prior cash ownership facts, under the entitlement's legal source.
    pub cutoff: CohortCashCutoff,
}
/// Separately acquired original earned bond cash, independent of security custody.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CohortDatedEntitlement {
    /// Complete original native cash cohort and obligation.
    pub origin: TradeId,
    /// Exact original earned coupon or principal key.
    pub key: l::ObligationKey,
    /// Exact original native proposal event, available at the cash cutoff.
    pub proposal_id: l::EventId,
    /// Original entitlement revision; its legacy residual must be absent.
    pub entitlement_source: ReferencePin,
    /// Exact legal residual fraction and complete prior paid receipt prefix.
    pub residual: CohortResidualClaim,
    /// Actual immutable legal cash ownership document.
    pub source: ReferencePin,
    /// Position information sets where this independently acquired cash applies.
    pub applied_points: Vec<usize>,
}
/// Explicit legal transfer of a share of one original earned cash claim.
/// Fractions refer to its original entitled residual, never security face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CohortClaimTransfer {
    /// Unique immutable legal transfer identity, in supplied causal order.
    pub id: l::EventId,
    /// Original native cohort whose checked cash and receipts remain the source.
    pub origin: TradeId,
    /// Complete original obligation key scoped to that origin.
    pub key: l::ObligationKey,
    /// Exact original earned/recovery proposal event identity.
    pub proposal_id: l::EventId,
    /// Exact original legal entitlement source revision.
    pub entitlement_source: ReferencePin,
    /// Original residual identity, if the source owns a residual fraction.
    pub residual_id: Option<l::EventId>,
    /// Current legal holder relinquishing only this outstanding cash share.
    pub from: TradeId,
    /// Complete legal recipient, independently of held security exposure.
    pub to: TradeId,
    /// Positive share of original entitled cash; current ownership cannot exceed1.
    pub fraction: ExactDecimal,
    /// Exact available economic/knowledge cutoff and ordered prior receipt IDs.
    pub cutoff: CohortCashCutoff,
    /// Actual immutable legal transfer document.
    pub source: ReferencePin,
    /// Ordered/independent point indices where the supplied legal fact applies.
    pub applied_points: Vec<usize>,
}
/// Actual original receipt and the share owned by this recipient at that time.
#[derive(Debug, Clone, Serialize)]
pub struct CohortClaimReceiptAllocation {
    /// Complete original checked event, retained at its original native source.
    pub event: l::LifecycleEvent,
    /// Exact share then owned, before any subsequent legal cash transfer.
    pub fraction: ExactDecimal,
    /// Exact signed owned receipt, after the original residual fraction.
    pub amount: Money,
}
/// Derived conserved ownership over the original native cash/proposal owner.
#[derive(Debug, Clone, Serialize)]
pub struct CohortClaimOwnershipValue {
    /// Original native cash cohort; this is not the receiving security lot.
    pub origin: TradeId,
    /// Original complete obligation, including total recorded original receipts.
    pub obligation: l::CashObligation,
    /// Original legal entitlement/source and optional residual/cutoff.
    pub entitlement: CohortClaimEntitlement,
    /// Complete original checked earned/recovery proposal.
    pub proposal: l::LifecycleEvent,
    /// Original complete residual diagnostics, including excluded prior receipts.
    pub residual_ownership: Option<ResidualClaimValue>,
    /// Full active legal transfer prefix for this original claim.
    pub transfers: Vec<CohortClaimTransfer>,
    /// Exact current share of original entitled outstanding cash.
    pub fraction: ExactDecimal,
    /// Actual received cash retains its then-current original recipient.
    pub receipts: Vec<CohortClaimReceiptAllocation>,
    /// Exact allocated received plus outstanding cash.
    pub owned_proposed: Money,
    /// Exact cumulative actual cash retained by this recipient.
    pub owned_recorded: Money,
    /// Exact current signed receivable/payable, independent of security face.
    pub owned_outstanding: Money,
}
/// Derived exact owned cash, with all original checked excluded receipt facts.
#[derive(Debug, Clone, Serialize)]
pub struct ResidualClaimValue {
    /// Complete independently dated cash acquisition, when explicitly supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dated_entitlement: Option<CohortDatedEntitlement>,
    /// Complete legal identity, source and original obligation key.
    pub entitlement: CohortClaimEntitlement,
    /// Complete original checked earned/recovery proposal, known at acquisition.
    pub proposal: l::LifecycleEvent,
    /// Full original checked receipts retained by the prior cash owner.
    pub prior_receipts: Vec<l::LifecycleEvent>,
    /// Exact original cash already received before legal acquisition.
    pub prior_recorded: Money,
    /// Owned part of the original residual amount at acquisition.
    pub owned_proposed: Money,
    /// Owned actual subsequent receipts, independent of held security exposure.
    pub owned_recorded: Money,
    /// Owned exact remaining receivable/payable, priced by the original owner.
    pub owned_outstanding: Money,
}
/// An original signed lot is acquired once and can be disposed once, in full.
/// Partial lots require separate original cohorts; no average cost is inferred.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CohortAcquisition {
    /// Exact named successor of a conserved original security lot split.
    LotSplit {
        /// Actual legal split identity, separate from execution fills.
        split_id: l::EventId,
    },
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
/// Complete supplied legal partition of one full original signed bond lot.
/// Earned original cash stays with its explicit original owner; this is not a fill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CohortLotSplit {
    /// Locally unique immutable legal split identity.
    pub id: l::EventId,
    /// Exact full original lot whose held exposure ends.
    pub parent: TradeId,
    /// Two or more distinct full native successor cohorts, in supplied order.
    pub children: Vec<TradeId>,
    /// Actual economic split date, at an original processed accrual boundary.
    pub effective_date: Date,
    /// Actual information availability; never inferred from the economic date.
    pub known_at: UtcTimestamp,
    /// Actual immutable supplied legal custody record.
    pub source: ReferencePin,
    /// Exact full information sets where this legal fact has been applied.
    /// Includes ordered endpoints and independently available counterfactuals.
    pub applied_points: Vec<usize>,
}
pub(super) fn split_active(split: &CohortLotSplit, point: usize) -> bool {
    split.applied_points.contains(&point)
}
fn split_delta(
    splits: &[CohortLotSplit],
    owners: &[CohortOwnership],
    point: usize,
    index: usize,
) -> ReconciliationResult<Decimal> {
    splits
        .iter()
        .filter(|s| split_active(s, point))
        .try_fold(Decimal::ZERO, |sum, s| {
            let q = if s.parent == owners[index].trade_id {
                -owners[index].quantity.decimal()
            } else if s.children.contains(&owners[index].trade_id) {
                owners[index].quantity.decimal()
            } else {
                Decimal::ZERO
            };
            dec(sum.checked_add(q))
        })
}
fn split_out(path: &PreparedCausalPnl, point: usize, index: usize) -> bool {
    path.cohorts.as_ref().is_some_and(|owners| {
        path.lot_splits
            .iter()
            .any(|s| s.parent == owners[index].trade_id && split_active(s, point))
            || path
                .physical_partitions
                .iter()
                .any(|p| p.split.parent == owners[index].trade_id && split_active(&p.split, point))
    })
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
/// Complete legal custody partition of one already checked original delivery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PhysicalDeliveryPartition {
    /// Conserved original asset parent, full children and actual legal cutoffs.
    pub split: CohortLotSplit,
    /// Original linked future holding every actual delivery/receipt event.
    pub future: TradeId,
    /// Exact original checked delivery, including notice and selected security.
    pub delivery_id: l::EventId,
    /// Complete ordered original security receipt prefix at custody acquisition.
    pub receipt_ids: Vec<l::EventId>,
}
/// Actual supplied disposal from an original checked security receipt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PhysicalDisposedReceipt {
    /// Original actual security receipt identity, not a replacement event.
    pub receipt_id: l::EventId,
    /// Exact signed received face removed from this legal custody owner.
    pub face: ExactDecimal,
}
/// Complete actual received-stock disposal; outstanding delivery and cash stay separate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PhysicalDeliveryDisposal {
    /// Immutable unique supplied legal fact, distinct from the actual fill.
    pub id: l::EventId,
    /// Exact acquired original custody cohort executing the actual disposal.
    pub asset: TradeId,
    /// Original linked future retaining full delivery and native receipt history.
    pub future: TradeId,
    /// Exact complete original checked delivery event.
    pub delivery_id: l::EventId,
    /// Existing actual interval fill, including consideration and fees.
    pub fill_id: l::EventId,
    /// Actual disposal economic date, equal to the actual fill date.
    pub effective_date: Date,
    /// Actual disposal knowledge cutoff, equal to the actual fill cutoff.
    pub known_at: UtcTimestamp,
    /// Complete immutable supplied legal security-disposal record.
    pub source: ReferencePin,
    /// Complete original ordered available security receipt prefix at disposal.
    pub receipt_ids: Vec<l::EventId>,
    /// Exact signed disposal from named already received original security.
    pub receipts: Vec<PhysicalDisposedReceipt>,
    /// Complete ordered and independently available Position information sets.
    pub applied_points: Vec<usize>,
}
/// Full actual disposal and original receipt provenance, valued by the original fill owner.
#[derive(Debug, Clone, Serialize)]
pub struct PhysicalDisposalValue {
    /// Complete actual immutable legal fact.
    pub facts: PhysicalDeliveryDisposal,
    /// Full original actual fill; no consideration or fees are inferred.
    pub fill: PnlFill,
    /// Exact signed received face removed, opposite to the actual fill quantity.
    pub face: ExactDecimal,
    /// Complete original checked receipt events and exactly disposed face.
    pub receipts: Vec<PhysicalReceiptAllocation>,
}
/// Original actual security receipt with this legal cohort's exact custody face.
#[derive(Debug, Clone, Serialize)]
pub struct PhysicalReceiptAllocation {
    /// Complete original checked event; no synthetic receipt is created.
    pub event: l::LifecycleEvent,
    /// Exact signed allocated original face.
    pub face: ExactDecimal,
}
/// Complete original provenance and conserved custody, independent of invoice cash.
#[derive(Debug, Clone, Serialize)]
pub struct PhysicalPartitionValue {
    /// Full immutable legal partition facts.
    pub facts: PhysicalDeliveryPartition,
    /// Exact original full proposed security face.
    pub original_proposed_face: ExactDecimal,
    /// Exact original externally received security face.
    pub original_received_face: ExactDecimal,
    /// Exact original remaining security receivable/payable face.
    pub original_outstanding_face: ExactDecimal,
    /// This cohort's current complete legal custody share.
    pub fraction: ExactDecimal,
    /// Every actual native receipt and exact owned custody, in original order.
    pub receipts: Vec<PhysicalReceiptAllocation>,
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
    /// Every full original actual partial/full security receipt in replay order.
    pub security_receipts: Vec<l::LifecycleEvent>,
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
    /// Current custody partition, retaining every original full delivery fact.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partition: Option<PhysicalPartitionValue>,
    /// Invoice is retained as provenance; its cash is valued by another owner.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub invoice_excluded: bool,
    /// Complete successive custody ancestry, omitted for a single original partition.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lineage: Vec<PhysicalDeliveryPartition>,
    /// Actual disposals by this legal cohort; paid cash never follows custody implicitly.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disposals: Vec<PhysicalDisposalValue>,
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
        CohortAcquisition::LotSplit { split_id } => {
            path.lot_splits
                .iter()
                .any(|s| &s.id == split_id && split_active(s, point))
                || path
                    .physical_partitions
                    .iter()
                    .any(|p| &p.split.id == split_id && split_active(&p.split, point))
        }
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
            && dated_entitlement(path, index, key).is_none_or(|d| d.applied_points.contains(&point))
    })
}
fn dated_entitlement<'a>(
    path: &'a PreparedCausalPnl,
    index: usize,
    key: &l::ObligationKey,
) -> Option<&'a CohortDatedEntitlement> {
    path.dated_entitlements
        .iter()
        .find(|d| path.input.points[0].positions[index].identity.id == d.origin && d.key == *key)
}
fn claim_residual<'a>(
    path: &'a PreparedCausalPnl,
    index: usize,
    legal: &'a CohortClaimEntitlement,
) -> Option<&'a CohortResidualClaim> {
    dated_entitlement(path, index, &legal.key)
        .map(|d| &d.residual)
        .or(legal.residual.as_ref())
}
pub(super) fn state_pin(state: &l::LifecycleState) -> ReconciliationResult<String> {
    let view = state.view();
    let instrument = match state.instrument() {
        l::LifecycleInstrument::Bond { contract, credit } => {
            serde_json::json!({"bond":contract.terms(),"credit":credit,
                "periods":contract.periods().collect::<Vec<_>>(),
                "principal":contract.principal_cashflows()})
        }
        l::LifecycleInstrument::ZeroCoupon { bond, credit } => {
            serde_json::json!({"zero":bond,"credit":credit})
        }
        l::LifecycleInstrument::Swap(contract) => serde_json::json!({"swap":contract.terms()}),
        l::LifecycleInstrument::Cds(contract) => serde_json::json!({"cds":contract.terms()}),
        l::LifecycleInstrument::Futures {
            contract,
            opening_mark,
        } => serde_json::json!({"future":contract,"opening_mark":opening_mark}),
    };
    let proposals = view
        .obligations
        .iter()
        .map(|o| state.cash_proposal_event(&o.key))
        .collect::<Vec<_>>();
    serde_json::to_string(&(
        instrument,
        view,
        proposals,
        state.payment_receipt_events(),
        state.security_receipt_events(),
        state.delivery_event(),
        state.delivered_bond_terms(),
    ))
    .map_err(|_| {
        bad(
            "cohort state",
            "complete original state serialization failed",
        )
    })
}
fn payment(event: &l::LifecycleEvent) -> ReconciliationResult<&l::PaymentReceipt> {
    match &event.action {
        l::LifecycleAction::Payment(receipt) => Ok(receipt),
        _ => Err(bad(
            "cash receipt",
            "original checked payment event required",
        )),
    }
}
pub(super) fn residual_value(
    path: &PreparedCausalPnl,
    point: usize,
    index: usize,
    state: &l::LifecycleState,
    claim: &l::CashObligation,
) -> ReconciliationResult<Option<ResidualClaimValue>> {
    let Some(entitlement) = path.cohorts.as_ref().and_then(|owners| {
        owners[index]
            .entitlements
            .iter()
            .find(|e| e.key == claim.key)
    }) else {
        return Ok(None);
    };
    let dated = dated_entitlement(path, index, &claim.key);
    let Some(residual) = claim_residual(path, index, entitlement) else {
        return Ok(None);
    };
    if !acquired(path, point, index) || dated.is_some_and(|d| !d.applied_points.contains(&point)) {
        return Ok(None);
    }
    let mut prior = Decimal::ZERO;
    let mut receipts = Vec::new();
    let excluded = residual
        .cutoff
        .receipt_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    for event in state.payment_receipt_events() {
        let receipt = payment(event)?;
        if excluded.contains(&receipt.receipt_id) {
            prior = dec(prior.checked_add(receipt.amount.value().decimal()))?;
            receipts.push(event.clone());
        }
    }
    if receipts.len() != residual.cutoff.receipt_ids.len() {
        return Err(bad(
            "residual receipts",
            "original excluded receipt prefix absent at marking",
        ));
    }
    let fraction = residual.fraction.decimal();
    let original = claim.amount.value().decimal();
    let recorded = claim.recorded.value().decimal();
    let value = |a: Decimal, b: Decimal| {
        Ok::<_, ReconciliationError>(Money::new(
            ExactDecimal::new(dec(a.checked_sub(b).and_then(|x| x.checked_mul(fraction)))?),
            claim.amount.currency(),
        ))
    };
    Ok(Some(ResidualClaimValue {
        dated_entitlement: dated.cloned(),
        entitlement: entitlement.clone(),
        proposal: state
            .cash_proposal_event(&claim.key)
            .ok_or_else(|| bad("residual proposal", "original earned source absent"))?
            .clone(),
        prior_receipts: receipts,
        prior_recorded: Money::new(ExactDecimal::new(prior), claim.amount.currency()),
        owned_proposed: value(original, prior)?,
        owned_recorded: value(recorded, prior)?,
        owned_outstanding: value(original, recorded)?,
    }))
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
    prepare_cohort_pnl_with_splits(input, cohorts, links, &[], states)
}
/// Admit explicitly sourced original security partitions before financial marks.
/// Complete native historical cash is retained, never converted into a fill.
pub fn prepare_cohort_pnl_with_splits(
    input: &CausalPnlInput,
    cohorts: &[CohortOwnership],
    links: &[PhysicalDeliveryLink],
    splits: &[CohortLotSplit],
    states: &[Vec<l::LifecycleState>],
) -> ReconciliationResult<PreparedCausalPnl> {
    prepare_cohort_pnl_with_transfers(input, cohorts, links, splits, &[], states)
}
/// Admit sourced original earned cash transfers without changing original lots.
/// Previously received cash remains with the legal recipient at its receipt.
pub fn prepare_cohort_pnl_with_transfers(
    input: &CausalPnlInput,
    cohorts: &[CohortOwnership],
    links: &[PhysicalDeliveryLink],
    splits: &[CohortLotSplit],
    transfers: &[CohortClaimTransfer],
    states: &[Vec<l::LifecycleState>],
) -> ReconciliationResult<PreparedCausalPnl> {
    prepare_cohort_pnl_with_delivery_partitions(
        input,
        cohorts,
        links,
        splits,
        transfers,
        &[],
        states,
    )
}
/// Admit complete delivered security custody and independently owned invoice cash.
/// Existing preparation APIs delegate with no physical partitions.
pub fn prepare_cohort_pnl_with_delivery_partitions(
    input: &CausalPnlInput,
    cohorts: &[CohortOwnership],
    links: &[PhysicalDeliveryLink],
    splits: &[CohortLotSplit],
    transfers: &[CohortClaimTransfer],
    partitions: &[PhysicalDeliveryPartition],
    states: &[Vec<l::LifecycleState>],
) -> ReconciliationResult<PreparedCausalPnl> {
    prepare_cohort_pnl_with_disposals(
        input,
        cohorts,
        links,
        splits,
        transfers,
        partitions,
        &[],
        states,
    )
}
/// Reconcile successive delivered custody and complete actual received-stock disposals.
/// All earlier preparation signatures retain their original empty-disposal behavior.
#[allow(clippy::too_many_arguments)]
pub fn prepare_cohort_pnl_with_disposals(
    input: &CausalPnlInput,
    cohorts: &[CohortOwnership],
    links: &[PhysicalDeliveryLink],
    splits: &[CohortLotSplit],
    transfers: &[CohortClaimTransfer],
    partitions: &[PhysicalDeliveryPartition],
    disposals: &[PhysicalDeliveryDisposal],
    states: &[Vec<l::LifecycleState>],
) -> ReconciliationResult<PreparedCausalPnl> {
    prepare_cohort_pnl_with_dated_entitlements(
        input,
        cohorts,
        links,
        splits,
        transfers,
        partitions,
        disposals,
        &[],
        states,
    )
}
/// Admit independently dated earned bond cash using the original residual owner.
/// Existing preparation APIs retain empty dated-entitlement behavior.
#[allow(clippy::too_many_arguments)]
pub fn prepare_cohort_pnl_with_dated_entitlements(
    input: &CausalPnlInput,
    cohorts: &[CohortOwnership],
    links: &[PhysicalDeliveryLink],
    splits: &[CohortLotSplit],
    transfers: &[CohortClaimTransfer],
    partitions: &[PhysicalDeliveryPartition],
    disposals: &[PhysicalDeliveryDisposal],
    dated: &[CohortDatedEntitlement],
    states: &[Vec<l::LifecycleState>],
) -> ReconciliationResult<PreparedCausalPnl> {
    let n = input.points.first().map_or(0, |p| p.positions.len());
    if !(1..=128).contains(&n)
        || cohorts.len() != n
        || links.len() > n
        || splits.len() > n
        || transfers.len() > 128
        || partitions.len() > n
        || disposals.len() > 128
        || dated.len() > 128
        || dated
            .iter()
            .any(|d| d.applied_points.len() > 33 || d.residual.cutoff.receipt_ids.len() > 4096)
        || disposals.iter().any(|d| {
            d.receipt_ids.len() > 4096
                || d.receipts.is_empty()
                || d.receipts.len() > 4096
                || d.applied_points.len() > 33
        })
        || partitions.iter().any(|p| {
            p.receipt_ids.len() > 4096
                || p.split.applied_points.len() > 33
                || p.split.children.len() < 2
                || p.split.children.len() > n
        })
        || transfers
            .iter()
            .any(|t| t.applied_points.len() > 33 || t.cutoff.receipt_ids.len() > 4096)
        || splits
            .iter()
            .any(|s| s.children.len() < 2 || s.children.len() > n || s.applied_points.len() > 33)
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
        || states
            .iter()
            .flatten()
            .any(|s| s.applied_event_count() > 4096)
        || cohorts.iter().any(|c| {
            c.entitlements.len() > 4096
                || c.quantity.decimal().is_zero()
                || c.entitlements.iter().any(|e| {
                    e.residual
                        .as_ref()
                        .is_some_and(|r| r.cutoff.receipt_ids.len() > 4096)
                })
        })
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
    if dated
        .iter()
        .any(|d| transfers.iter().any(|t| t.id == d.residual.id))
    {
        return Err(bad(
            "dated cash identity",
            "cash acquisition and cash transfer require distinct original legal identities",
        ));
    }
    if partitions.iter().any(|p| {
        splits.iter().any(|s| s.id == p.split.id) || transfers.iter().any(|t| t.id == p.split.id)
    }) || disposals.iter().any(|d| {
        splits.iter().any(|s| s.id == d.id)
            || transfers.iter().any(|t| t.id == d.id)
            || partitions.iter().any(|p| p.split.id == d.id)
    }) {
        return Err(bad(
            "physical legal identity",
            "custody identity must be distinct from security splits and cash transfers",
        ));
    }
    let mut normalized = input.clone();
    admit_splits(input, cohorts, links, splits, partitions, states)?;
    for (i, point) in normalized.points.iter_mut().enumerate() {
        for (j, pos) in point.positions.iter_mut().enumerate() {
            pos.current.quantity = ExactDecimal::new(dec(pos
                .current
                .quantity
                .decimal()
                .checked_sub(split_delta(splits, cohorts, i, j)?))?);
        }
    }
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
                let received = future.security_receipt_events().iter().try_fold(
                    Decimal::ZERO,
                    |total, event| match &event.action {
                        l::LifecycleAction::SecurityReceipt { face, .. } => {
                            dec(total.checked_add(face.decimal()))
                        }
                        _ => Err(bad(
                            "security receipts",
                            "original checked security event required",
                        )),
                    },
                )?;
                let outstanding = dec(invoice
                    .amount
                    .value()
                    .decimal()
                    .checked_sub(invoice.recorded.value().decimal()))?;
                handoffs[i][a] = Some(PhysicalDeliveryValue {
                    future: link.future.clone(),
                    delivery: event.clone(),
                    security_receipt: receipt,
                    security_receipts: future.security_receipt_events().to_vec(),
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
                    partition: None,
                    invoice_excluded: false,
                    lineage: Vec::new(),
                    disposals: Vec::new(),
                });
            } else if future.view().status != l::EconomicStatus::Active {
                return Err(ReconciliationError::Incomplete(
                    "actual checked physical delivery event absent",
                ));
            }
        }
    }
    admit_physical_partitions(
        input,
        cohorts,
        links,
        partitions,
        disposals,
        dated,
        states,
        &mut handoffs,
    )?;
    for (i, point) in normalized.points.iter_mut().enumerate() {
        for (j, pos) in point.positions.iter_mut().enumerate() {
            if let Some(h) = &handoffs[i][j] {
                let baseline = handoffs[0][j]
                    .as_ref()
                    .map_or(Decimal::ZERO, |h| h.received_face.decimal());
                pos.current.quantity = ExactDecimal::new(dec(pos
                    .current
                    .quantity
                    .decimal()
                    .checked_sub(h.received_face.decimal())
                    .and_then(|q| q.checked_add(baseline)))?);
                for disposal in &h.disposals {
                    pos.current.quantity = ExactDecimal::new(dec(pos
                        .current
                        .quantity
                        .decimal()
                        .checked_add(disposal.fill.quantity.decimal()))?);
                }
            }
        }
    }
    // Run every original path/fill/time/FX/bound check after removing only exact
    // native checked interval security receipts from position conservation.
    let mut path = prepare_causal_pnl(&normalized)?;
    path.input = input.clone();
    path.cohorts = Some(cohorts.to_vec());
    path.handoffs = handoffs;
    path.cohort_state_pins = Some(
        states
            .iter()
            .map(|row| row.iter().map(state_pin).collect())
            .collect::<ReconciliationResult<_>>()?,
    );
    path.lot_splits = splits.to_vec();
    path.physical_partitions = partitions.to_vec();
    path.physical_disposals = disposals.to_vec();
    path.dated_entitlements = dated.to_vec();
    let mut residual_ids = std::collections::BTreeSet::new();
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
                CohortAcquisition::LotSplit { .. } => {
                    if is_acquired {
                        path.handoffs[i][j]
                            .as_ref()
                            .map_or(owner.quantity.decimal(), |h| h.received_face.decimal())
                    } else {
                        Decimal::ZERO
                    }
                }
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
            let expected = if split_out(&path, i, j) {
                if disposed.is_some() || !pos.pending_quantity.decimal().is_zero() {
                    return Err(bad(
                        "retired split lot",
                        "split parent cannot also be disposed or retain pending exposure",
                    ));
                }
                Decimal::ZERO
            } else if let Some(f) = disposed {
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
                !matches!(&owner.acquisition,CohortAcquisition::Fill{fill_id} if fill_id==&f.id) && owner.disposal_fill.as_ref()!=Some(&f.id) && !path.physical_disposals.iter().any(|d| d.asset == owner.trade_id && d.fill_id == f.id && d.applied_points.contains(&i))
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
                if owned_keys.contains(&claim.key)
                    && prior_received.contains(&claim.key)
                    && !owner
                        .entitlements
                        .iter()
                        .any(|e| e.key == claim.key && e.residual.is_some())
                {
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
        for entitlement in &owner.entitlements {
            if let Some(residual) = &entitlement.residual {
                if !residual_ids.insert(residual.id.clone()) {
                    return Err(bad(
                        "residual identity",
                        "unique original residual identity required",
                    ));
                }
                admit_residual(&path, j, entitlement, residual, states)?;
            }
        }
    }
    admit_dated_entitlements(&path, states, &mut residual_ids)?;
    path.claim_ownership = admit_claim_transfers(&path, transfers, states)?;
    Ok(path)
}

fn admit_residual(
    path: &PreparedCausalPnl,
    index: usize,
    entitlement: &CohortClaimEntitlement,
    residual: &CohortResidualClaim,
    states: &[Vec<l::LifecycleState>],
) -> ReconciliationResult<()> {
    let owner = &path
        .cohorts
        .as_ref()
        .ok_or_else(|| bad("cohort", "owner absent"))?[index];
    let fraction = residual.fraction.decimal();
    if !entitlement.entitled || fraction <= Decimal::ZERO || fraction > Decimal::ONE {
        return Err(bad(
            "residual fraction",
            "explicit positive owned fraction <=1 required",
        ));
    }
    if matches!(
        entitlement.key.kind,
        l::CashKind::DeliveryInvoice | l::CashKind::VariationMargin
    ) || matches!(
        owner.acquisition,
        CohortAcquisition::PhysicalDelivery { .. }
    ) {
        return Err(ReconciliationError::Incomplete("residual physical invoice or variation cash transfer requires its original separate owner"));
    }
    let acquisition = path
        .input
        .points
        .iter()
        .enumerate()
        .position(|(i, _)| acquired(path, i, index))
        .ok_or_else(|| bad("residual acquisition", "original actual acquisition absent"))?;
    let point = &path.input.points[acquisition];
    let cutoff = &residual.cutoff;
    match &owner.acquisition {
        CohortAcquisition::Fill { fill_id } => {
            let f = fill(point, fill_id)
                .ok_or_else(|| bad("residual fill", "original full acquisition absent"))?;
            if cutoff.date != f.date || cutoff.known_at != f.known_at {
                return Err(bad(
                    "residual cutoff",
                    "exact original acquisition fill date/known-at required",
                ));
            }
        }
        CohortAcquisition::Opening {} => {
            if cutoff.date > point.valuation_date || cutoff.known_at > point.known_at {
                return Err(bad(
                    "residual cutoff",
                    "original opening cash cutoff must be available",
                ));
            }
        }
        CohortAcquisition::PhysicalDelivery { .. } => {
            return Err(bad("residual", "unsupported acquisition"))
        }
        CohortAcquisition::LotSplit { .. } => {
            return Err(ReconciliationError::Incomplete("earned residual transfers to split children require their original separate claim owner"))
        }
    }
    if cutoff.known_at.datetime().date_naive() < cutoff.date.as_naive_date()
        || !(1..=9999).contains(&cutoff.date.year())
        || entitlement.key.effective_date > cutoff.date
    {
        return Err(bad(
            "residual cutoff",
            "available original earned claim/date required",
        ));
    }
    let excluded = cutoff
        .receipt_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    if excluded.len() != cutoff.receipt_ids.len() {
        return Err(bad(
            "residual receipt prefix",
            "unique exact original prior receipt identities required",
        ));
    }
    let state = &states[acquisition][index];
    let view = state.view();
    let claim = view
        .obligations
        .iter()
        .find(|o| o.key == entitlement.key)
        .ok_or(ReconciliationError::Incomplete(
            "original earned residual claim absent at acquisition",
        ))?;
    let proposal =
        state
            .cash_proposal_event(&entitlement.key)
            .ok_or(ReconciliationError::Incomplete(
                "original earned residual proposal source absent",
            ))?;
    if proposal.known_at > cutoff.known_at
        || proposal.effective_date > cutoff.date
        || proposal.event_id != claim.generated_by
    {
        return Err(bad(
            "residual proposal",
            "original earned/recovery terms must be known at actual acquisition",
        ));
    }
    let mut found = std::collections::BTreeSet::new();
    let mut prior = Decimal::ZERO;
    let mut subsequent_seen = false;
    for event in state.payment_receipt_events() {
        let receipt = payment(event)?;
        if excluded.contains(&receipt.receipt_id) {
            if receipt.obligation != entitlement.key
                || event.effective_date > cutoff.date
                || event.known_at > cutoff.known_at
                || receipt.value_date > cutoff.date
                || subsequent_seen
            {
                return Err(bad(
                    "residual receipt prefix",
                    "exact available original cash prefix for this claim required",
                ));
            }
            found.insert(&receipt.receipt_id);
            prior = dec(prior.checked_add(receipt.amount.value().decimal()))?;
        } else if receipt.obligation == entitlement.key {
            subsequent_seen = true;
            if event.effective_date < cutoff.date || event.known_at < cutoff.known_at {
                return Err(bad(
                    "residual receipt cutoff",
                    "cash preceding acquisition must retain its prior owner",
                ));
            }
        }
    }
    if found != excluded || claim.amount.value().decimal() == prior {
        return Err(bad(
            "residual claim",
            "complete original receipt prefix and nonempty residual required",
        ));
    }
    for row in states.iter().take(acquisition) {
        for event in row[index].payment_receipt_events() {
            let receipt = payment(event)?;
            if receipt.obligation == entitlement.key && !excluded.contains(&receipt.receipt_id) {
                return Err(bad(
                    "residual prior owner",
                    "all actual pre-acquisition cash retains its original owner",
                ));
            }
        }
    }
    for (i, row) in states.iter().enumerate() {
        if acquired(path, i, index) {
            if let Some(claim) = row[index]
                .view()
                .obligations
                .iter()
                .find(|o| o.key == entitlement.key)
            {
                residual_value(path, i, index, &row[index], claim)?;
            } else {
                return Err(ReconciliationError::Incomplete(
                    "original residual obligation absent from acquired point",
                ));
            }
        }
    }
    Ok(())
}

fn admit_dated_entitlements(
    path: &PreparedCausalPnl,
    states: &[Vec<l::LifecycleState>],
    ids: &mut std::collections::BTreeSet<l::EventId>,
) -> ReconciliationResult<()> {
    let input = &path.input;
    let owners = path
        .cohorts
        .as_ref()
        .ok_or_else(|| bad("dated cash", "original cohorts absent"))?;
    let mut keys = std::collections::BTreeSet::new();
    for dated in &path.dated_entitlements {
        let j = find(input, &dated.origin)?;
        let residual = &dated.residual;
        let cutoff = &residual.cutoff;
        let points = dated
            .applied_points
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if !keys.insert((j, dated.key.clone()))
            || !ids.insert(residual.id.clone())
            || residual.id == dated.proposal_id
            || path.lot_splits.iter().any(|s| s.id == residual.id)
            || path
                .physical_partitions
                .iter()
                .any(|p| p.split.id == residual.id)
            || path.physical_disposals.iter().any(|d| d.id == residual.id)
            || residual.fraction.decimal() <= Decimal::ZERO
            || residual.fraction.decimal() > Decimal::ONE
            || points.is_empty()
            || points.len() != dated.applied_points.len()
            || points.contains(&0)
            || points.iter().any(|i| *i >= states.len())
            || cutoff.date < input.points[0].valuation_date
            || cutoff.known_at < input.points[0].known_at
            || cutoff.known_at.datetime().date_naive() < cutoff.date.as_naive_date()
            || !(1..=9999).contains(&cutoff.date.year())
            || dated.key.effective_date > cutoff.date
        {
            return Err(bad("dated cash facts", "unique available interval ownership, positive fraction <=1 and distinct exact applied points required"));
        }
        if !matches!(
            dated.key.kind,
            l::CashKind::BondCoupon | l::CashKind::Principal
        ) {
            return Err(ReconciliationError::Incomplete(
                "dated cash requires original nonconditional fixed-bond coupon or principal",
            ));
        }
        let legal = owners[j]
            .entitlements
            .iter()
            .find(|e| e.key == dated.key)
            .ok_or(ReconciliationError::Incomplete(
                "original complete dated cash entitlement absent",
            ))?;
        if !legal.entitled || legal.residual.is_some() || legal.source != dated.entitlement_source {
            return Err(bad("dated cash source", "exact owned original entitlement revision and one independent residual owner required"));
        }
        let first = (1..=input.causes.len())
            .find(|i| points.contains(i))
            .ok_or_else(|| {
                bad(
                    "dated cash path",
                    "actual ordered cash acquisition required",
                )
            })?;
        if input.causes[first - 1] != PnlCause::Position
            || (first..=input.causes.len()).any(|i| !points.contains(&i))
        {
            return Err(bad(
                "dated cash cause",
                "cash acquisition belongs to Position and persists across ordered points",
            ));
        }
        let mut original = None;
        let mut prefix: &[l::LifecycleEvent] = &[];
        for (i, row) in states.iter().enumerate() {
            let state = &row[j];
            if i <= input.causes.len() {
                if !state.payment_receipt_events().starts_with(prefix) {
                    return Err(bad(
                        "dated cash history",
                        "all actual receipt events remain an immutable ordered prefix",
                    ));
                }
                prefix = state.payment_receipt_events();
            }
            if !points.contains(&i) {
                continue;
            }
            let point = &input.points[i];
            if i > input.causes.len() {
                let cause = input
                    .standalone
                    .iter()
                    .position(|s| matches!(s, PnlStandalone::Available {point} if *point == i))
                    .ok_or_else(|| {
                        bad(
                            "dated cash independent",
                            "complete independent binding required",
                        )
                    })?;
                if input.causes[cause] != PnlCause::Position {
                    return Err(bad(
                        "dated cash independent",
                        "only an available independent Position acquires legal cash",
                    ));
                }
            }
            if !acquired(path, i, j)
                || cutoff.date > point.valuation_date
                || cutoff.known_at > point.known_at
            {
                return Err(bad("dated cash availability", "original native cash party and exact legal cutoff must be acquired and available"));
            }
            let l::LifecycleInstrument::Bond {
                contract,
                credit: None,
            } = state.instrument()
            else {
                return Err(ReconciliationError::Incomplete(
                    "dated cash requires the complete original nonconditional bond owner",
                ));
            };
            if !contract.terms().exercise.is_empty()
                || contract.terms().coupon != convex_bonds::contracts::BondCoupon::Fixed
            {
                return Err(ReconciliationError::Incomplete("conditional or floating dated cash requires its separate full model claim owner"));
            }
            let claim = state
                .view()
                .obligations
                .into_iter()
                .find(|c| c.key == dated.key)
                .ok_or(ReconciliationError::Incomplete(
                    "original earned dated cash obligation absent",
                ))?;
            let proposal =
                state
                    .cash_proposal_event(&dated.key)
                    .ok_or(ReconciliationError::Incomplete(
                        "original earned dated cash proposal absent",
                    ))?;
            if proposal.event_id != dated.proposal_id
                || claim.generated_by != dated.proposal_id
                || proposal.effective_date > cutoff.date
                || proposal.known_at > cutoff.known_at
            {
                return Err(bad(
                    "dated cash proposal",
                    "exact original earned native proposal must be available at legal acquisition",
                ));
            }
            let pin = (
                claim.amount,
                claim.due_date,
                claim.generated_by.clone(),
                proposal.clone(),
            );
            if original.as_ref().is_some_and(|p| p != &pin) {
                return Err(bad(
                    "dated cash original",
                    "native proposal amount/date/source cannot change across ownership",
                ));
            }
            original = Some(pin);
            admit_cash_cutoff(&dated.key, cutoff, state)?;
            let value = residual_value(path, i, j, state, &claim)?
                .ok_or_else(|| bad("dated cash residual", "original owned residual absent"))?;
            if value.prior_recorded == claim.amount {
                return Err(bad(
                    "dated cash exhaustion",
                    "an empty or fully paid original residual cannot be acquired",
                ));
            }
        }
        // Same-date ordering is explicit: every earlier actual receipt stays excluded.
        for row in states.iter().take(first) {
            for event in row[j].payment_receipt_events() {
                let r = payment(event)?;
                if r.obligation == dated.key && !cutoff.receipt_ids.contains(&r.receipt_id) {
                    return Err(bad(
                        "dated cash prior recipient",
                        "complete earlier actual paid prefix retains its prior owner",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn transfer_active(transfer: &CohortClaimTransfer, point: usize) -> bool {
    transfer.applied_points.contains(&point)
}
fn move_claim_share(
    input: &CausalPnlInput,
    transfer: &CohortClaimTransfer,
    shares: &mut [Decimal],
) -> ReconciliationResult<()> {
    let from = find(input, &transfer.from)?;
    let to = find(input, &transfer.to)?;
    let fraction = transfer.fraction.decimal();
    if from == to || fraction <= Decimal::ZERO || fraction > shares[from] {
        return Err(bad(
            "claim share",
            "distinct parties and positive conserved available ownership required",
        ));
    }
    shares[from] = dec(shares[from].checked_sub(fraction))?;
    shares[to] = dec(shares[to].checked_add(fraction))?;
    Ok(())
}
/// Allocate one checked Decimal cash amount exactly. The final nonzero legal
/// share retains the representational remainder of the original Decimal owner.
/// No monetary tolerance or model residual is used to accept conservation.
fn partition_cash(total: Decimal, shares: &[Decimal]) -> ReconciliationResult<Vec<Decimal>> {
    let last = shares
        .iter()
        .rposition(|q| *q > Decimal::ZERO)
        .ok_or_else(|| bad("claim conservation", "complete positive ownership absent"))?;
    if shares
        .iter()
        .try_fold(Decimal::ZERO, |sum, q| dec(sum.checked_add(*q)))?
        != Decimal::ONE
        || shares.iter().any(|q| *q < Decimal::ZERO)
    {
        return Err(bad(
            "claim conservation",
            "exact complete shares must sum to1",
        ));
    }
    let mut remaining = total;
    let mut values = vec![Decimal::ZERO; shares.len()];
    for (j, share) in shares.iter().enumerate() {
        values[j] = if j == last {
            remaining
        } else {
            dec(total.checked_mul(*share))?
        };
        remaining = dec(remaining.checked_sub(values[j]))?;
    }
    Ok(values)
}

fn admit_claim_cutoff(
    transfer: &CohortClaimTransfer,
    state: &l::LifecycleState,
) -> ReconciliationResult<()> {
    admit_cash_cutoff(&transfer.key, &transfer.cutoff, state)
}
fn admit_cash_cutoff(
    key: &l::ObligationKey,
    cutoff: &CohortCashCutoff,
    state: &l::LifecycleState,
) -> ReconciliationResult<()> {
    let receipts = state
        .payment_receipt_events()
        .iter()
        .filter(|e| matches!(&e.action, l::LifecycleAction::Payment(r) if r.obligation == *key))
        .collect::<Vec<_>>();
    let count = cutoff.receipt_ids.len();
    if count > receipts.len() {
        return Err(bad(
            "claim receipt cutoff",
            "original checked receipt prefix absent",
        ));
    }
    for (k, event) in receipts.iter().enumerate() {
        let receipt = payment(event)?;
        if k < count {
            if receipt.receipt_id != cutoff.receipt_ids[k]
                || event.effective_date > cutoff.date
                || event.known_at > cutoff.known_at
                || receipt.value_date > cutoff.date
            {
                return Err(bad(
                    "claim receipt cutoff",
                    "exact ordered available original receipt prefix required",
                ));
            }
        } else if event.effective_date < cutoff.date
            || event.known_at < cutoff.known_at
            || receipt.value_date < cutoff.date
        {
            return Err(bad(
                "claim receipt cutoff",
                "all previously paid cash must retain its recipient",
            ));
        }
    }
    Ok(())
}

fn admit_claim_transfers(
    path: &PreparedCausalPnl,
    transfers: &[CohortClaimTransfer],
    states: &[Vec<l::LifecycleState>],
) -> ReconciliationResult<Vec<Vec<Vec<CohortClaimOwnershipValue>>>> {
    if transfers.is_empty() {
        return Ok(Vec::new());
    }
    let input = &path.input;
    let owners = path
        .cohorts
        .as_ref()
        .ok_or_else(|| bad("claim owners", "complete cohorts absent"))?;
    let n = owners.len();
    let mut ids = std::collections::BTreeSet::new();
    let mut groups =
        std::collections::BTreeMap::<(usize, l::ObligationKey), Vec<&CohortClaimTransfer>>::new();
    for transfer in transfers {
        let origin = find(input, &transfer.origin)?;
        let from = find(input, &transfer.from)?;
        let to = find(input, &transfer.to)?;
        let points = transfer
            .applied_points
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let cutoff = &transfer.cutoff;
        if !ids.insert(&transfer.id)
            || from == to
            || transfer.fraction.decimal() <= Decimal::ZERO
            || transfer.fraction.decimal() > Decimal::ONE
            || points.is_empty()
            || points.len() != transfer.applied_points.len()
            || points.contains(&0)
            || points.iter().any(|i| *i >= input.points.len())
            || cutoff.date < input.points[0].valuation_date
            || cutoff.known_at < input.points[0].known_at
            || cutoff.known_at.datetime().date_naive() < cutoff.date.as_naive_date()
            || !(1..=9999).contains(&cutoff.date.year())
            || transfer.key.effective_date > cutoff.date
        {
            return Err(bad("claim transfer", "unique interval legal facts, distinct parties and available positive earned ownership required"));
        }
        let entitlement = owners[origin]
            .entitlements
            .iter()
            .find(|e| e.key == transfer.key)
            .ok_or(ReconciliationError::Incomplete(
                "original claim entitlement absent",
            ))?;
        if !entitlement.entitled
            || entitlement.source != transfer.entitlement_source
            || claim_residual(path, origin, entitlement).map(|r| &r.id)
                != transfer.residual_id.as_ref()
        {
            return Err(bad(
                "claim source",
                "exact original owned entitlement/residual/source revision required",
            ));
        }
        if transfer.key.kind == l::CashKind::VariationMargin {
            return Err(ReconciliationError::Incomplete(
                "margin transfers require their separate complete cash owners",
            ));
        }
        let first = (1..=input.causes.len())
            .find(|i| points.contains(i))
            .ok_or_else(|| {
                bad(
                    "claim path",
                    "actual transfer must occur on the ordered path",
                )
            })?;
        if input.causes[first - 1] != PnlCause::Position
            || (first..=input.causes.len()).any(|i| !points.contains(&i))
        {
            return Err(bad(
                "claim cause",
                "transfer belongs to Position and persists in subsequent ordered facts",
            ));
        }
        for (i, row) in states.iter().enumerate() {
            if !points.contains(&i) {
                continue;
            }
            let point = &input.points[i];
            let cash_owner = claim_cash_owner(path, i, origin, &transfer.key)?;
            if i > input.causes.len() {
                let cause = input
                    .standalone
                    .iter()
                    .position(|s| matches!(s, PnlStandalone::Available {point} if *point == i))
                    .ok_or_else(|| bad("claim standalone", "full independent binding absent"))?;
                if input.causes[cause] != PnlCause::Position {
                    return Err(bad(
                        "claim standalone",
                        "unrelated independent cause cannot acquire cash transfers",
                    ));
                }
            }
            if cutoff.date > point.valuation_date
                || cutoff.known_at > point.known_at
                || [origin, from, to].iter().any(|j| !acquired(path, i, *j))
            {
                return Err(bad(
                    "claim availability",
                    "original proposal and all legal parties must be acquired and available",
                ));
            }
            let a = &point.positions[cash_owner];
            if [from, to].iter().any(|j| {
                let b = &point.positions[*j];
                a.current.currency != b.current.currency
                    || a.cash_discount != b.cash_discount
                    || a.overdue != b.overdue
                    || matches!(row[*j].instrument(), l::LifecycleInstrument::Futures { .. })
            }) || transfer.key.kind != l::CashKind::DeliveryInvoice
                && matches!(
                    row[origin].instrument(),
                    l::LifecycleInstrument::Futures { .. }
                )
            {
                return Err(bad(
                    "claim cash owner",
                    "same complete original currency/discount/overdue cash owners required",
                ));
            }
            let claim = row[origin]
                .view()
                .obligations
                .into_iter()
                .find(|c| c.key == transfer.key)
                .ok_or(ReconciliationError::Incomplete(
                    "original earned cash absent at transfer",
                ))?;
            let proposal = row[origin].cash_proposal_event(&transfer.key).ok_or(
                ReconciliationError::Incomplete("original cash proposal absent at transfer"),
            )?;
            if proposal.event_id != transfer.proposal_id
                || proposal.event_id != claim.generated_by
                || proposal.effective_date > cutoff.date
                || proposal.known_at > cutoff.known_at
            {
                return Err(bad(
                    "claim proposal",
                    "exact complete earned/recovery proposal must be available at transfer cutoff",
                ));
            }
            admit_claim_cutoff(transfer, &row[origin])?;
            let paid_prefix = row[origin]
                .payment_receipt_events()
                .iter()
                .filter_map(|e| match &e.action {
                    l::LifecycleAction::Payment(r)
                        if r.obligation == transfer.key
                            && cutoff.receipt_ids.contains(&r.receipt_id) =>
                    {
                        Some(r.amount.value().decimal())
                    }
                    _ => None,
                })
                .try_fold(Decimal::ZERO, |sum, q| dec(sum.checked_add(q)))?;
            if claim.amount.value().decimal() == paid_prefix {
                return Err(bad(
                    "claim exhaustion",
                    "only a nonempty original outstanding claim can transfer",
                ));
            }
            if i == first {
                for event in states[first - 1][origin].payment_receipt_events() {
                    if let l::LifecycleAction::Payment(r) = &event.action {
                        if r.obligation == transfer.key
                            && !cutoff.receipt_ids.contains(&r.receipt_id)
                        {
                            return Err(bad(
                                "claim prior recipient",
                                "every earlier ordered receipt must retain its original recipient",
                            ));
                        }
                    }
                }
            }
        }
        let records = groups.entry((origin, transfer.key.clone())).or_default();
        if let Some(prior) = records.last() {
            if cutoff.date < prior.cutoff.date
                || cutoff.known_at < prior.cutoff.known_at
                || !cutoff.receipt_ids.starts_with(&prior.cutoff.receipt_ids)
                || transfer
                    .applied_points
                    .iter()
                    .any(|i| !transfer_active(prior, *i))
            {
                return Err(bad(
                    "claim lineage",
                    "causal transfer order and complete prior ownership/receipt prefix required",
                ));
            }
        }
        records.push(transfer);
    }
    let allocations = groups
        .iter()
        .try_fold(0usize, |sum, ((origin, _), records)| {
            let first = records[0].applied_points[0];
            let mut parties = std::collections::BTreeSet::from([claim_cash_owner(
                path,
                first,
                *origin,
                &records[0].key,
            )?]);
            for t in records {
                parties.insert(find(input, &t.from)?);
                parties.insert(find(input, &t.to)?);
            }
            sum.checked_add(parties.len() * states.len())
                .ok_or_else(|| bad("claim work", "allocation reservation overflow"))
        })?;
    if allocations > 4224 {
        return Err(bad(
            "claim work",
            "bounded complete original root/party/point allocation reservation exceeded",
        ));
    }
    let mut result = vec![vec![Vec::new(); n]; states.len()];
    for ((origin, key), records) in groups {
        let mut original: Option<(l::CashObligation, l::LifecycleEvent)> = None;
        let mut ordered_receipts: &[l::LifecycleEvent] = &[];
        for (i, row) in states.iter().enumerate() {
            let active = records
                .iter()
                .copied()
                .filter(|t| transfer_active(t, i))
                .collect::<Vec<_>>();
            let view = row[origin].view();
            let Some(claim) = view.obligations.iter().find(|c| c.key == key) else {
                if !active.is_empty() {
                    return Err(ReconciliationError::Incomplete(
                        "original source claim absent from applied point",
                    ));
                }
                continue;
            };
            if !entitlement(path, i, origin, &key) {
                if !active.is_empty() {
                    return Err(bad("claim source", "original entitled claim not acquired"));
                }
                continue;
            }
            let proposal =
                row[origin]
                    .cash_proposal_event(&key)
                    .ok_or(ReconciliationError::Incomplete(
                        "original complete earned proposal absent",
                    ))?;
            if let Some((prior, event)) = &original {
                if claim.amount != prior.amount
                    || claim.due_date != prior.due_date
                    || claim.generated_by != prior.generated_by
                    || proposal != event
                {
                    return Err(bad("claim original facts", "full original amount/date/proposal cannot change across transferred ownership"));
                }
            } else {
                original = Some((claim.clone(), proposal.clone()));
            }
            if i <= input.causes.len() {
                let receipts = row[origin].payment_receipt_events();
                if !receipts.starts_with(ordered_receipts) {
                    return Err(bad(
                        "claim original receipts",
                        "all original actual ordered receipts remain an exact immutable prefix",
                    ));
                }
                ordered_receipts = receipts;
            }
            let legal = owners[origin]
                .entitlements
                .iter()
                .find(|e| e.key == key)
                .ok_or(ReconciliationError::Incomplete(
                    "original complete entitlement absent",
                ))?;
            let residual = residual_value(path, i, origin, &row[origin], claim)?;
            let base_fraction =
                claim_residual(path, origin, legal).map_or(Decimal::ONE, |r| r.fraction.decimal());
            let excluded = claim_residual(path, origin, legal)
                .map(|r| r.cutoff.receipt_ids.as_slice())
                .unwrap_or(&[]);
            let original_outstanding = residual.as_ref().map_or(
                dec(claim
                    .amount
                    .value()
                    .decimal()
                    .checked_sub(claim.recorded.value().decimal()))?,
                |r| r.owned_outstanding.value().decimal(),
            );
            let initial_cash_owner = claim_cash_owner(path, i, origin, &key)?;
            let mut shares = vec![Decimal::ZERO; n];
            shares[initial_cash_owner] = Decimal::ONE;
            let mut parties = std::collections::BTreeSet::from([initial_cash_owner]);
            for t in &active {
                move_claim_share(input, t, &mut shares)?;
                parties.insert(find(input, &t.from)?);
                parties.insert(find(input, &t.to)?);
            }
            let outstanding = partition_cash(original_outstanding, &shares)?;
            let mut recorded = vec![Decimal::ZERO; n];
            let mut receipts = vec![Vec::new(); n];
            for event in row[origin].payment_receipt_events() {
                let receipt = payment(event)?;
                if receipt.obligation != key || excluded.contains(&receipt.receipt_id) {
                    continue;
                }
                let mut at_receipt = vec![Decimal::ZERO; n];
                at_receipt[initial_cash_owner] = Decimal::ONE;
                for t in &active {
                    if !t.cutoff.receipt_ids.contains(&receipt.receipt_id) {
                        move_claim_share(input, t, &mut at_receipt)?;
                    }
                }
                let owned = dec(receipt.amount.value().decimal().checked_mul(base_fraction))?;
                let values = partition_cash(owned, &at_receipt)?;
                for j in 0..n {
                    if at_receipt[j].is_zero() {
                        continue;
                    }
                    recorded[j] = dec(recorded[j].checked_add(values[j]))?;
                    receipts[j].push(CohortClaimReceiptAllocation {
                        event: event.clone(),
                        fraction: ExactDecimal::new(at_receipt[j]),
                        amount: Money::new(ExactDecimal::new(values[j]), claim.amount.currency()),
                    });
                }
            }
            let proposal =
                row[origin]
                    .cash_proposal_event(&key)
                    .ok_or(ReconciliationError::Incomplete(
                        "complete original earned proposal absent",
                    ))?;
            for j in parties {
                result[i][j].push(CohortClaimOwnershipValue {
                    origin: owners[origin].trade_id.clone(),
                    obligation: claim.clone(),
                    entitlement: legal.clone(),
                    proposal: proposal.clone(),
                    residual_ownership: residual.clone(),
                    transfers: active.iter().map(|t| (*t).clone()).collect(),
                    fraction: ExactDecimal::new(shares[j]),
                    receipts: std::mem::take(&mut receipts[j]),
                    owned_proposed: Money::new(
                        ExactDecimal::new(dec(recorded[j].checked_add(outstanding[j]))?),
                        claim.amount.currency(),
                    ),
                    owned_recorded: Money::new(
                        ExactDecimal::new(recorded[j]),
                        claim.amount.currency(),
                    ),
                    owned_outstanding: Money::new(
                        ExactDecimal::new(outstanding[j]),
                        claim.amount.currency(),
                    ),
                });
            }
        }
    }
    Ok(result)
}

fn admit_splits(
    input: &CausalPnlInput,
    owners: &[CohortOwnership],
    links: &[PhysicalDeliveryLink],
    splits: &[CohortLotSplit],
    partitions: &[PhysicalDeliveryPartition],
    states: &[Vec<l::LifecycleState>],
) -> ReconciliationResult<()> {
    let mut parents = std::collections::BTreeSet::new();
    let mut children = std::collections::BTreeSet::new();
    let mut ids = std::collections::BTreeSet::new();
    for split in splits {
        let p = find(input, &split.parent)?;
        let indexes = split
            .children
            .iter()
            .map(|id| find(input, id))
            .collect::<ReconciliationResult<Vec<_>>>()?;
        let points = split
            .applied_points
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if !ids.insert(&split.id)
            || !parents.insert(&split.parent)
            || owners[p].disposal_fill.is_some()
            || points.is_empty()
            || points.len() != split.applied_points.len()
            || points.contains(&0)
            || points.iter().any(|i| *i >= input.points.len())
            || split.known_at.datetime().date_naive() < split.effective_date.as_naive_date()
            || !(1..=9999).contains(&split.effective_date.year())
            || split.effective_date < input.points[0].valuation_date
            || split.known_at < input.points[0].known_at
            || links.iter().any(|l| {
                l.asset == split.parent
                    || l.future == split.parent
                    || split.children.contains(&l.asset)
                    || split.children.contains(&l.future)
            })
            || partitions.iter().any(|p| {
                p.split.children.contains(&split.parent)
                    || split.children.iter().any(|c| p.split.children.contains(c))
            })
        {
            return Err(bad("lot split", "unique full nonphysical parent, available interval facts and distinct applied points required"));
        }
        let mut total = Decimal::ZERO;
        for &j in &indexes {
            if p == j
                || !children.insert(&owners[j].trade_id)
                || owners[j].acquisition
                    != (CohortAcquisition::LotSplit {
                        split_id: split.id.clone(),
                    })
                || owners[j].quantity.decimal().is_sign_negative()
                    != owners[p].quantity.decimal().is_sign_negative()
            {
                return Err(bad("split children", "distinct same-direction original children acquired by this exact split required"));
            }
            total = dec(total.checked_add(owners[j].quantity.decimal()))?;
        }
        if total != owners[p].quantity.decimal() {
            return Err(bad(
                "split face",
                "exact original signed face must equal all successor lots",
            ));
        }
        let first = (1..=input.causes.len())
            .find(|i| points.contains(i))
            .ok_or_else(|| bad("split path", "actual split must appear on the ordered path"))?;
        if input.causes[first - 1] != PnlCause::Position
            || (first..=input.causes.len()).any(|i| !points.contains(&i))
        {
            return Err(bad(
                "split cause",
                "full split belongs to Position and cannot disappear from ordered facts",
            ));
        }
        for (i, row) in states.iter().enumerate() {
            if !split_active(split, i) {
                continue;
            }
            let point = &input.points[i];
            if split.effective_date > point.valuation_date || split.known_at > point.known_at {
                return Err(bad(
                    "split availability",
                    "actual effective and knowledge cutoffs must both be available",
                ));
            }
            if i > input.causes.len() {
                let cause = input
                    .standalone
                    .iter()
                    .position(|s| matches!(s, PnlStandalone::Available {point} if *point == i))
                    .ok_or_else(|| bad("split standalone", "actual standalone binding absent"))?;
                if input.causes[cause] != PnlCause::Position {
                    return Err(bad(
                        "split standalone",
                        "unrelated independent cause cannot acquire split facts",
                    ));
                }
            }
            if i != first && i <= input.causes.len() {
                continue;
            }
            let l::LifecycleInstrument::Bond {
                contract: original,
                credit,
            } = row[p].instrument()
            else {
                return Err(ReconciliationError::Incomplete(
                    "security splits require complete original bond owners",
                ));
            };
            if !original.terms().exercise.is_empty()
                || original.terms().coupon != convex_bonds::contracts::BondCoupon::Fixed
                || original.terms().rounding != convex_core::cashflows::CashRounding::Unrounded
                || !original
                    .periods()
                    .any(|x| x.accrual_dates().0 == split.effective_date)
                || row[p].view().status != l::EconomicStatus::Active
                || row[p].view().remaining_fraction.decimal() != Decimal::ONE
            {
                return Err(ReconciliationError::Incomplete("lot split requires an active unrounded nonoptional original bond at a processed accrual boundary"));
            }
            for &j in &indexes {
                let l::LifecycleInstrument::Bond {
                    contract: successor,
                    credit: mapped,
                } = row[j].instrument()
                else {
                    return Err(bad(
                        "split security",
                        "complete original successor bond required",
                    ));
                };
                let a = &point.positions[p];
                let b = &point.positions[j];
                if original.terms() != successor.terms()
                    || !original.periods().eq(successor.periods())
                    || original.principal_cashflows() != successor.principal_cashflows()
                    || credit != mapped
                    || a.identity.instrument != b.identity.instrument
                    || a.identity.entity != b.identity.entity
                    || a.current.currency != b.current.currency
                    || a.current.reference_quantity != b.current.reference_quantity
                    || a.current.quantity_unit != b.current.quantity_unit
                    || row[p].view().fixings != row[j].view().fixings
                    || row[j].view().remaining_fraction.decimal() != Decimal::ONE
                    || row[j].view().status != l::EconomicStatus::Active
                {
                    return Err(bad(
                        "split economics",
                        "same complete original security/economics/history/currency/units required",
                    ));
                }
            }
        }
        for &j in &indexes {
            if owners[j].entitlements.iter().any(|e| e.key.effective_date <= split.effective_date && e.entitled)
                || states.iter().any(|row| row[j].payment_receipt_events().iter().any(|e| matches!(&e.action, l::LifecycleAction::Payment(r) if r.obligation.effective_date <= split.effective_date)))
            {
                return Err(bad("split earned cash", "original earned cash and historical receipts stay with their original owner"));
            }
        }
        if owners[p]
            .entitlements
            .iter()
            .any(|e| e.key.effective_date > split.effective_date && e.entitled)
        {
            return Err(bad(
                "split future cash",
                "retired original parent cannot own successor earned cash",
            ));
        }
    }
    for owner in owners {
        if let CohortAcquisition::LotSplit { split_id } = &owner.acquisition {
            if !splits
                .iter()
                .any(|s| &s.id == split_id && s.children.contains(&owner.trade_id))
                && !partitions
                    .iter()
                    .any(|p| &p.split.id == split_id && p.split.children.contains(&owner.trade_id))
            {
                return Err(bad(
                    "split acquisition",
                    "exact complete original split link absent",
                ));
            }
        }
    }
    // Bound the graph by original holdings; a successor may be split again only
    // after its own actual acquisition. No cycle or repeated consumed lot exists.
    for split in splits {
        if let Some(prior) = splits.iter().find(|s| s.children.contains(&split.parent)) {
            let original_first =
                (1..=input.causes.len()).find(|i| prior.applied_points.contains(i));
            let successor_first =
                (1..=input.causes.len()).find(|i| split.applied_points.contains(i));
            if split.effective_date < prior.effective_date
                || split.known_at < prior.known_at
                || split
                    .applied_points
                    .iter()
                    .any(|p| !prior.applied_points.contains(p))
                || original_first >= successor_first
            {
                return Err(bad(
                    "split lineage",
                    "original acquisition must precede every later partition",
                ));
            }
        } else {
            let p = find(input, &split.parent)?;
            if owners[p].acquisition != (CohortAcquisition::Opening {}) {
                return Err(bad(
                    "split lineage",
                    "initial split parent must be an original opening lot",
                ));
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
struct CustodyBalance {
    pending: Decimal,
    receipts: Vec<Decimal>,
    lineage: Vec<PhysicalDeliveryPartition>,
    partition: Option<PhysicalDeliveryPartition>,
    acquired: bool,
    consumed: bool,
    disposals: Vec<PhysicalDisposalValue>,
}
#[derive(Clone, Copy)]
enum CustodyChange<'a> {
    Partition(&'a PhysicalDeliveryPartition),
    Disposal(&'a PhysicalDeliveryDisposal),
}
impl<'a> CustodyChange<'a> {
    fn points(self) -> &'a [usize] {
        match self {
            Self::Partition(p) => &p.split.applied_points,
            Self::Disposal(d) => &d.applied_points,
        }
    }
    fn cutoff(self) -> (Date, UtcTimestamp, &'a [l::EventId]) {
        match self {
            Self::Partition(p) => (p.split.effective_date, p.split.known_at, &p.receipt_ids),
            Self::Disposal(d) => (d.effective_date, d.known_at, &d.receipt_ids),
        }
    }
}
fn custody_first(input: &CausalPnlInput, points: &[usize]) -> ReconciliationResult<usize> {
    let unique = points
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let first = (1..=input.causes.len())
        .find(|i| unique.contains(i))
        .ok_or_else(|| {
            bad(
                "physical custody",
                "actual ordered legal acquisition required",
            )
        })?;
    if unique.len() != points.len()
        || unique.contains(&0)
        || unique.iter().any(|i| *i >= input.points.len())
        || input.causes[first - 1] != PnlCause::Position
        || (first..=input.causes.len()).any(|i| !unique.contains(&i))
    {
        return Err(bad(
            "physical custody cause",
            "unique Position facts must persist through every later ordered point",
        ));
    }
    for &i in points.iter().filter(|i| **i > input.causes.len()) {
        let cause = input
            .standalone
            .iter()
            .position(|s| matches!(s, PnlStandalone::Available {point} if *point == i))
            .ok_or_else(|| {
                bad(
                    "physical custody standalone",
                    "complete original independent binding required",
                )
            })?;
        if input.causes[cause] != PnlCause::Position {
            return Err(bad(
                "physical custody standalone",
                "only independent Position facts can change custody",
            ));
        }
    }
    Ok(first)
}
fn custody_cutoff(
    input: &CausalPnlInput,
    change: CustodyChange<'_>,
    first: usize,
    point: usize,
    original: &PhysicalDeliveryValue,
    prior_receipts: &[l::LifecycleEvent],
) -> ReconciliationResult<()> {
    let (date, known, ids) = change.cutoff();
    let at = &input.points[point];
    if date < input.points[0].valuation_date
        || known < input.points[0].known_at
        || known.datetime().date_naive() < date.as_naive_date()
        || date > at.valuation_date
        || known > at.known_at
        || original.delivery.effective_date > date
        || original.delivery.known_at > known
        || ids.len() > original.security_receipts.len()
    {
        return Err(bad(
            "physical custody availability",
            "complete original delivery and interval economic/knowledge/receipt cutoff required",
        ));
    }
    for (k, event) in original.security_receipts.iter().enumerate() {
        let l::LifecycleAction::SecurityReceipt { receipt_id, .. } = &event.action else {
            return Err(bad(
                "physical custody receipt",
                "original checked receipt required",
            ));
        };
        if k < ids.len() {
            if receipt_id != &ids[k] || event.effective_date > date || event.known_at > known {
                return Err(bad(
                    "physical custody receipt cutoff",
                    "complete ordered available original receipt prefix required",
                ));
            }
        } else if event.effective_date < date || event.known_at < known {
            return Err(bad(
                "physical custody receipt cutoff",
                "every prior economic/known receipt must be included",
            ));
        }
    }
    if point == first && prior_receipts.iter().any(|event|
        !matches!(&event.action, l::LifecycleAction::SecurityReceipt {receipt_id, ..} if ids.contains(receipt_id)))
    {
        return Err(bad("physical prior custody", "every earlier ordered actual receipt must occur in the legal cutoff"));
    }
    Ok(())
}
fn custody_economics(
    point: &PnlPointInput,
    row: &[l::LifecycleState],
    original: usize,
    successor: usize,
) -> ReconciliationResult<()> {
    let l::LifecycleInstrument::Bond {
        contract: bond,
        credit,
    } = row[original].instrument()
    else {
        return Err(bad(
            "physical security",
            "full original native bond required",
        ));
    };
    let l::LifecycleInstrument::Bond {
        contract: child,
        credit: mapped,
    } = row[successor].instrument()
    else {
        return Err(bad(
            "physical successor",
            "full original native bond required",
        ));
    };
    let p = &point.positions[original];
    let q = &point.positions[successor];
    if !bond.terms().exercise.is_empty()
        || bond.terms().coupon != convex_bonds::contracts::BondCoupon::Fixed
        || !matches!(
            row[original].view().status,
            l::EconomicStatus::Active | l::EconomicStatus::Matured | l::EconomicStatus::Closed
        )
        || (row[original].view().status == l::EconomicStatus::Active
            && row[original].view().remaining_fraction.decimal() != Decimal::ONE)
        || (row[original].view().status != l::EconomicStatus::Active
            && point.valuation_date < bond.terms().schedule.end_date)
    {
        return Err(ReconciliationError::Incomplete(
            "physical custody requires the original nonoptional fixed bond and contractual lifecycle",
        ));
    }
    if bond.terms() != child.terms()
        || !bond.periods().eq(child.periods())
        || bond.principal_cashflows() != child.principal_cashflows()
        || credit != mapped
        || p.identity.instrument != q.identity.instrument
        || p.identity.entity != q.identity.entity
        || p.current.currency != q.current.currency
        || p.current.reference_quantity != q.current.reference_quantity
        || p.current.quantity_unit != q.current.quantity_unit
        || row[original].view().fixings != row[successor].view().fixings
        || (row[successor].view().status == l::EconomicStatus::Active)
            != (row[original].view().status == l::EconomicStatus::Active)
        || !matches!(
            row[successor].view().status,
            l::EconomicStatus::Active | l::EconomicStatus::Matured | l::EconomicStatus::Closed
        )
        || row[successor].view().remaining_fraction != row[original].view().remaining_fraction
        || !q.pending_quantity.decimal().is_zero()
    {
        return Err(bad(
            "physical successor economics",
            "same complete original security/revision/economics/history/currency/units required",
        ));
    }
    Ok(())
}
fn custody_receive(
    events: &[l::LifecycleEvent],
    until: usize,
    cursor: &mut usize,
    balances: &mut [CustodyBalance],
) -> ReconciliationResult<()> {
    if until < *cursor || until > events.len() {
        return Err(bad(
            "physical custody receipt order",
            "original ingress must precede each exact legal receipt cutoff",
        ));
    }
    while *cursor < until {
        let l::LifecycleAction::SecurityReceipt { face, .. } = &events[*cursor].action else {
            return Err(bad(
                "physical custody receipt",
                "full original native security receipt required",
            ));
        };
        let remaining = balances
            .iter()
            .try_fold(Decimal::ZERO, |sum, b| dec(sum.checked_add(b.pending)))?;
        if remaining.is_zero()
            || face.decimal().is_sign_negative() != remaining.is_sign_negative()
            || face.decimal().abs() > remaining.abs()
        {
            return Err(bad(
                "physical custody ingress",
                "actual receipt cannot overdraw original outstanding custody",
            ));
        }
        let shares = balances
            .iter()
            .map(|b| dec(b.pending.checked_div(remaining)))
            .collect::<ReconciliationResult<Vec<_>>>()?;
        let amounts = partition_cash(face.decimal(), &shares)?;
        for (b, amount) in balances.iter_mut().zip(amounts) {
            b.pending = dec(b.pending.checked_sub(amount))?;
            b.receipts[*cursor] = amount;
        }
        *cursor += 1;
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn admit_physical_partitions(
    input: &CausalPnlInput,
    owners: &[CohortOwnership],
    links: &[PhysicalDeliveryLink],
    partitions: &[PhysicalDeliveryPartition],
    disposals: &[PhysicalDeliveryDisposal],
    dated: &[CohortDatedEntitlement],
    states: &[Vec<l::LifecycleState>],
    handoffs: &mut [Vec<Option<PhysicalDeliveryValue>>],
) -> ReconciliationResult<()> {
    let n = owners.len();
    let mut parents = std::collections::BTreeSet::new();
    let mut children = std::collections::BTreeSet::new();
    let mut ids = std::collections::BTreeSet::new();
    let mut fills = std::collections::BTreeSet::new();
    let mut changes = Vec::new();
    for facts in partitions {
        let split = &facts.split;
        let a = find(input, &split.parent)?;
        let root = links
            .iter()
            .find(|l| l.future == facts.future)
            .ok_or_else(|| {
                bad(
                    "physical partition",
                    "complete original future/asset link required",
                )
            })?;
        let original = find(input, &root.asset)?;
        let f = find(input, &facts.future)?;
        let first = custody_first(input, &split.applied_points)?;
        if !ids.insert(&split.id) || !parents.insert(a) || owners[a].disposal_fill.is_some() {
            return Err(bad(
                "physical partition",
                "unique legal identity and one consumed custody parent required",
            ));
        }
        if a != original {
            let prior = partitions
                .iter()
                .find(|p| p.split.children.contains(&split.parent))
                .ok_or_else(|| {
                    bad(
                        "physical lineage",
                        "actual original successor acquisition absent",
                    )
                })?;
            let earlier = custody_first(input, &prior.split.applied_points)?;
            if prior.future != facts.future
                || prior.delivery_id != facts.delivery_id
                || earlier >= first
                || prior.split.effective_date > split.effective_date
                || prior.split.known_at > split.known_at
                || split
                    .applied_points
                    .iter()
                    .any(|p| !prior.split.applied_points.contains(p))
            {
                return Err(bad("physical lineage", "same original delivery and prior available actual acquisition required; cycles forbidden"));
            }
        }
        for child in &split.children {
            let j = find(input, child)?;
            if j == a
                || j == f
                || !children.insert(j)
                || links
                    .iter()
                    .any(|l| l.asset == *child || l.future == *child)
                || owners[j].disposal_fill.is_some()
                || owners[j].acquisition
                    != (CohortAcquisition::LotSplit {
                        split_id: split.id.clone(),
                    })
                || owners[j].quantity.decimal().is_sign_negative()
                    != owners[original].quantity.decimal().is_sign_negative()
            {
                return Err(bad(
                    "physical children",
                    "distinct complete same-direction native custody successors required",
                ));
            }
            if owners[j].entitlements.iter().any(|e| {
                e.entitled
                    && e.key.effective_date <= split.effective_date
                    && !dated
                        .iter()
                        .any(|d| d.origin == owners[j].trade_id && d.key == e.key)
            }) || states
                .iter()
                .any(|row| !row[j].payment_receipt_events().is_empty())
            {
                return Err(bad(
                    "physical earned cash",
                    "original earned cash and historical receipts stay with their original owner",
                ));
            }
        }
        if owners[a].entitlements.iter().any(|e| {
            e.entitled
                && e.key.effective_date > split.effective_date
                && !dated
                    .iter()
                    .any(|d| d.origin == owners[a].trade_id && d.key == e.key)
        }) {
            return Err(bad(
                "physical future cash",
                "consumed parent cannot own future successor cash",
            ));
        }
        if states[first][original].view().status != l::EconomicStatus::Active {
            return Err(bad(
                "physical ended acquisition",
                "new custody requires active original security at its actual transition",
            ));
        }
        changes.push((first, CustodyChange::Partition(facts), f, original));
    }
    for facts in disposals {
        let a = find(input, &facts.asset)?;
        let root = links
            .iter()
            .find(|l| l.future == facts.future)
            .ok_or_else(|| {
                bad(
                    "physical disposal",
                    "complete original future/asset link required",
                )
            })?;
        let original = find(input, &root.asset)?;
        let f = find(input, &facts.future)?;
        let first = custody_first(input, &facts.applied_points)?;
        if states[first][original].view().status != l::EconomicStatus::Active {
            return Err(bad(
                "physical ended disposal",
                "new disposal requires active original security at its actual transition",
            ));
        }
        if !ids.insert(&facts.id)
            || !fills.insert(&facts.fill_id)
            || facts.id == facts.fill_id
            || owners[a].disposal_fill.is_some()
        {
            return Err(bad(
                "physical disposal",
                "unique original legal/actual fill identity and one disposal owner required",
            ));
        }
        if a != original {
            let prior = partitions
                .iter()
                .find(|p| p.split.children.contains(&facts.asset))
                .ok_or_else(|| {
                    bad(
                        "physical disposal acquisition",
                        "actual original custody acquisition absent",
                    )
                })?;
            if prior.future != facts.future
                || prior.delivery_id != facts.delivery_id
                || custody_first(input, &prior.split.applied_points)? >= first
                || prior.split.effective_date > facts.effective_date
                || prior.split.known_at > facts.known_at
                || facts
                    .applied_points
                    .iter()
                    .any(|p| !prior.split.applied_points.contains(p))
            {
                return Err(bad(
                    "physical disposal acquisition",
                    "actual complete custody acquisition must precede disposal",
                ));
            }
        }
        changes.push((first, CustodyChange::Disposal(facts), f, original));
    }
    changes.sort_by_key(|c| c.0);
    // Original delivery and receipt bytes are immutable across actual and independent sets.
    for link in links {
        let f = find(input, &link.future)?;
        let a = find(input, &link.asset)?;
        let relevant = changes
            .iter()
            .filter(|c| c.2 == f)
            .copied()
            .collect::<Vec<_>>();
        if relevant.is_empty() {
            continue;
        }
        let mut delivery = None;
        let mut receipt_pins = std::collections::BTreeMap::new();
        let mut ordered: &[l::LifecycleEvent] = &[];
        for (i, row) in states.iter().enumerate() {
            if i <= input.causes.len() {
                if !row[f].security_receipt_events().starts_with(ordered) {
                    return Err(bad(
                        "physical original receipts",
                        "every actual immutable ordered receipt prefix must survive",
                    ));
                }
                ordered = row[f].security_receipt_events();
            }
            let Some(original) = handoffs[i][a].clone() else {
                if relevant.iter().any(|c| c.1.points().contains(&i)) {
                    return Err(ReconciliationError::Incomplete(
                        "original checked delivery unavailable at custody acquisition",
                    ));
                }
                continue;
            };
            if delivery.as_ref().is_some_and(|d| d != &original.delivery) {
                return Err(bad(
                    "physical original delivery",
                    "complete original checked delivery must remain immutable",
                ));
            }
            delivery = Some(original.delivery.clone());
            for event in &original.security_receipts {
                let l::LifecycleAction::SecurityReceipt { receipt_id, .. } = &event.action else {
                    return Err(bad(
                        "physical original receipt",
                        "complete original checked security event required",
                    ));
                };
                if receipt_pins
                    .insert(receipt_id.clone(), event.clone())
                    .is_some_and(|p| p != *event)
                {
                    return Err(bad(
                        "physical original receipt",
                        "same actual receipt identity must retain every original byte",
                    ));
                }
            }
            let events = &original.security_receipts;
            let mut balances = vec![
                CustodyBalance {
                    pending: Decimal::ZERO,
                    receipts: vec![Decimal::ZERO; events.len()],
                    lineage: Vec::new(),
                    partition: None,
                    acquired: false,
                    consumed: false,
                    disposals: Vec::new()
                };
                n
            ];
            balances[a].pending = original.proposed_face.decimal();
            balances[a].acquired = true;
            let mut cursor = 0;
            for &(first, change, _, _) in &relevant {
                if !change.points().contains(&i) {
                    continue;
                }
                custody_cutoff(
                    input,
                    change,
                    first,
                    i,
                    &original,
                    states[first - 1][f].security_receipt_events(),
                )?;
                custody_receive(events, change.cutoff().2.len(), &mut cursor, &mut balances)?;
                match change {
                    CustodyChange::Partition(facts) => {
                        let p = find(input, &facts.split.parent)?;
                        if !balances[p].acquired
                            || balances[p].consumed
                            || facts.delivery_id != original.delivery.event_id
                        {
                            return Err(bad("physical lineage", "one acquired original live custody parent and checked delivery required"));
                        }
                        custody_economics(&input.points[i], row, a, p)?;
                        let face = balances[p]
                            .receipts
                            .iter()
                            .try_fold(balances[p].pending, |sum, q| dec(sum.checked_add(*q)))?;
                        if face.is_zero() {
                            return Err(bad(
                                "physical partition face",
                                "disposed or empty custody cannot be partitioned",
                            ));
                        }
                        let indexes = facts
                            .split
                            .children
                            .iter()
                            .map(|id| find(input, id))
                            .collect::<ReconciliationResult<Vec<_>>>()?;
                        let mut shares = vec![Decimal::ZERO; n];
                        let sum = indexes.iter().try_fold(Decimal::ZERO, |sum, &j| {
                            dec(sum.checked_add(owners[j].quantity.decimal()))
                        })?;
                        if sum != face {
                            return Err(bad("physical face", "successors must exactly conserve the remaining signed parent custody"));
                        }
                        for &j in &indexes {
                            custody_economics(&input.points[i], row, a, j)?;
                            if balances[j].acquired {
                                return Err(bad(
                                    "physical double custody",
                                    "a successor can be acquired exactly once",
                                ));
                            }
                            shares[j] = dec(owners[j].quantity.decimal().checked_div(face))?;
                            if dec(face.checked_mul(shares[j]))? != owners[j].quantity.decimal() {
                                return Err(ReconciliationError::Incomplete("physical fractions must be exactly representable by the original Decimal owner"));
                            }
                        }
                        let pending = partition_cash(balances[p].pending, &shares)?;
                        let receipts = balances[p]
                            .receipts
                            .iter()
                            .map(|q| partition_cash(*q, &shares))
                            .collect::<ReconciliationResult<Vec<_>>>()?;
                        let mut lineage = balances[p].lineage.clone();
                        lineage.push(facts.clone());
                        for &j in &indexes {
                            balances[j].acquired = true;
                            balances[j].pending = pending[j];
                            for (k, allocated) in receipts.iter().enumerate() {
                                balances[j].receipts[k] = allocated[j];
                            }
                            balances[j].lineage = lineage.clone();
                            balances[j].partition = Some(facts.clone());
                        }
                        balances[p].consumed = true;
                        balances[p].pending = Decimal::ZERO;
                        balances[p].receipts.fill(Decimal::ZERO);
                        balances[p].lineage = lineage;
                        balances[p].partition = Some(facts.clone());
                    }
                    CustodyChange::Disposal(facts) => {
                        let p = find(input, &facts.asset)?;
                        let actual = fill(&input.points[i], &facts.fill_id).ok_or_else(|| {
                            bad(
                                "physical disposal fill",
                                "complete original actual fill absent",
                            )
                        })?;
                        if !balances[p].acquired
                            || balances[p].consumed
                            || facts.delivery_id != original.delivery.event_id
                            || actual.trade_id != facts.asset
                            || actual.date != facts.effective_date
                            || actual.known_at != facts.known_at
                        {
                            return Err(bad("physical disposal", "same acquired live custody and exact original actual fill/cutoff required"));
                        }
                        custody_economics(&input.points[i], row, a, p)?;
                        let mut used = std::collections::BTreeSet::new();
                        let mut face = Decimal::ZERO;
                        let mut receipts = Vec::new();
                        for disposed in &facts.receipts {
                            let index = events.iter().position(|e| matches!(&e.action, l::LifecycleAction::SecurityReceipt {receipt_id, ..} if *receipt_id == disposed.receipt_id))
                                .ok_or_else(|| bad("physical disposal receipt", "original actual received security absent"))?;
                            let amount = disposed.face.decimal();
                            if !used.insert(index)
                                || index >= cursor
                                || amount.is_zero()
                                || amount.is_sign_negative()
                                    != original.proposed_face.decimal().is_sign_negative()
                                || amount.abs() > balances[p].receipts[index].abs()
                            {
                                return Err(bad("physical disposed stock", "unique same-direction already received original custody cannot be overdrawn"));
                            }
                            balances[p].receipts[index] =
                                dec(balances[p].receipts[index].checked_sub(amount))?;
                            face = dec(face.checked_add(amount))?;
                            receipts.push(PhysicalReceiptAllocation {
                                event: events[index].clone(),
                                face: disposed.face,
                            });
                        }
                        if face != -actual.quantity.decimal() {
                            return Err(bad("physical disposed face", "exact signed disposed receipts must oppose the full actual fill quantity"));
                        }
                        balances[p].disposals.push(PhysicalDisposalValue {
                            facts: facts.clone(),
                            fill: actual.clone(),
                            face: ExactDecimal::new(face),
                            receipts,
                        });
                    }
                }
            }
            custody_receive(events, events.len(), &mut cursor, &mut balances)?;
            let mut received = Decimal::ZERO;
            let mut outstanding = Decimal::ZERO;
            let mut disposed = Decimal::ZERO;
            for (j, b) in balances.into_iter().enumerate() {
                if !b.acquired {
                    continue;
                }
                let held = b
                    .receipts
                    .iter()
                    .try_fold(Decimal::ZERO, |sum, q| dec(sum.checked_add(*q)))?;
                received = dec(received.checked_add(held))?;
                outstanding = dec(outstanding.checked_add(b.pending))?;
                for d in &b.disposals {
                    disposed = dec(disposed.checked_add(d.face.decimal()))?;
                }
                let face = dec(held.checked_add(b.pending))?;
                let mut value = original.clone();
                value.proposed_face = ExactDecimal::new(face);
                value.received_face = ExactDecimal::new(held);
                value.outstanding_face = ExactDecimal::new(b.pending);
                if let Some(facts) = b.partition {
                    let fraction = dec(face.checked_div(original.proposed_face.decimal()))?;
                    if dec(original.proposed_face.decimal().checked_mul(fraction))? != face {
                        return Err(ReconciliationError::Incomplete(
                            "current physical custody fraction must be exactly representable",
                        ));
                    }
                    value.partition = Some(PhysicalPartitionValue {
                        facts,
                        original_proposed_face: original.proposed_face,
                        original_received_face: original.received_face,
                        original_outstanding_face: original.outstanding_face,
                        fraction: ExactDecimal::new(fraction),
                        receipts: if b.consumed {
                            Vec::new()
                        } else {
                            events
                                .iter()
                                .cloned()
                                .zip(b.receipts)
                                .map(|(event, face)| PhysicalReceiptAllocation {
                                    event,
                                    face: ExactDecimal::new(face),
                                })
                                .collect()
                        },
                    });
                }
                if b.lineage.len() > 1 {
                    value.lineage = b.lineage;
                }
                value.disposals = b.disposals;
                if j != a {
                    value.invoice_excluded = true;
                    value.outstanding_invoice = Money::new(
                        ExactDecimal::new(Decimal::ZERO),
                        original.invoice.amount.currency(),
                    );
                }
                handoffs[i][j] = Some(value);
            }
            if dec(received.checked_add(disposed))? != original.received_face.decimal()
                || outstanding != original.outstanding_face.decimal()
                || dec(received
                    .checked_add(outstanding)
                    .and_then(|q| q.checked_add(disposed)))?
                    != original.proposed_face.decimal()
            {
                return Err(bad("physical custody conservation", "received stock plus actual disposals and original outstanding receivable must conserve full signed delivery"));
            }
        }
    }
    Ok(())
}

fn claim_cash_owner(
    path: &PreparedCausalPnl,
    point: usize,
    origin: usize,
    key: &l::ObligationKey,
) -> ReconciliationResult<usize> {
    if key.kind != l::CashKind::DeliveryInvoice {
        return Ok(origin);
    }
    path.handoffs[point].iter().position(|h| h.as_ref().is_some_and(|h|
        !h.invoice_excluded && h.future == path.input.points[point].positions[origin].identity.id && h.invoice.key == *key))
        .ok_or(ReconciliationError::Incomplete("invoice transfer requires its complete original linked physical asset and checked invoice owner"))
}
