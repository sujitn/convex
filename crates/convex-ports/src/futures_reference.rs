//! Futures/cash master resources over the existing atomic reference repository.

#![forbid(unsafe_code)]

use convex_core::futures::{CashHedgeUniverse, DeliveryBasket, FuturesContract};
use serde::{Deserialize, Serialize};

use crate::reference_master::{EffectiveInterval, ReferenceProvenance};
use crate::reference_master::{MasterDefinition, MasterKey, MasterRecord};
use crate::reference_repository::{ReferenceCommit, ReferenceError, ReferenceRecord};
use convex_core::contracts::{BusinessDate, Version};
use convex_core::futures::{FuturesSettlement, InstrumentPin, RateAccrual, ResourcePin};

/// Complete supplied futures master definition; storage assigns no economic defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FuturesDefinition {
    /// Actual disjoint exchange contract facts.
    pub contract: FuturesContract,
    /// Economic applicability of this definition revision.
    pub effective: EffectiveInterval,
    /// Permitted source evidence, retained through import/replay.
    pub provenance: ReferenceProvenance,
}

/// Versioned eligible delivery securities and published conversion factors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BasketDefinition {
    /// Exact basket and reciprocal contract pins.
    pub basket: DeliveryBasket,
    /// Economic applicability, independently of recording time.
    pub effective: EffectiveInterval,
    /// Original exchange or provider evidence.
    pub provenance: ReferenceProvenance,
}

/// Versioned actual cash hedge candidate inputs, including rejected/indicative candidates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CashUniverseDefinition {
    /// Exact security/quote/lot/funding/borrow/cost inputs.
    pub universe: CashHedgeUniverse,
    /// Economic applicability, independently of recording time.
    pub effective: EffectiveInterval,
    /// Permitted original source evidence.
    pub provenance: ReferenceProvenance,
}

pub(crate) fn validate_futures_links(
    commit: &ReferenceCommit,
    master: &mut impl FnMut(&MasterKey, Version) -> Result<MasterRecord, ReferenceError>,
    bond_version: &mut impl FnMut(
        &convex_core::ids::InstrumentId,
        Version,
    ) -> Result<Option<ReferenceRecord>, ReferenceError>,
) -> Result<(), ReferenceError> {
    for record in &commit.master_records {
        if let Some(stamp) = record.definition.futures_stamp() {
            let provenance = match &record.definition {
                MasterDefinition::Futures(v) => &v.provenance,
                MasterDefinition::Basket(v) => &v.provenance,
                MasterDefinition::CashUniverse(v) => &v.provenance,
                _ => continue,
            };
            if provenance.source != stamp.source
                || provenance.evidence_reference != stamp.evidence
                || !record.definition.effective().contains(stamp.source_at)
            {
                return Err(ReferenceError::InvalidInput(
                    "futures source/effective provenance conflicts",
                ));
            }
        }
        match &record.definition {
            MasterDefinition::Futures(v) => validate_contract_links(&v.contract, master)?,
            MasterDefinition::Basket(v) => {
                let owner = master(
                    &MasterKey::Futures(v.basket.contract.id.clone()),
                    v.basket.contract.version,
                )?;
                let MasterDefinition::Futures(owner) = &owner.definition else {
                    return Err(ReferenceError::InvalidInput(
                        "basket owner is not a futures contract",
                    ));
                };
                v.basket
                    .validate_contract(&owner.contract)
                    .map_err(|e| ReferenceError::InvalidInput(e.reason))?;
                let FuturesSettlement::Bond { delivery, .. } = &owner.contract.settlement else {
                    return Err(ReferenceError::InvalidInput(
                        "rate futures have no CTD basket",
                    ));
                };
                for entry in &v.basket.bonds {
                    let bond = exact_bond(commit, &entry.instrument, bond_version)?;
                    if bond.currency != v.basket.currency
                        || bond.issue_date > delivery.first_delivery.date()
                        || bond.maturity_date <= delivery.last_delivery.date()
                        || bond.bond_type != crate::reference_data::BondType::FixedBullet
                        || bond.is_callable
                        || bond.is_putable
                        || bond.is_sinkable
                        || bond.floating_terms.is_some()
                        || bond.inflation_index.is_some()
                    {
                        return Err(ReferenceError::InvalidInput(
                            "ineligible pinned deliverable security currency/terms/dates",
                        ));
                    }
                }
            }
            MasterDefinition::CashUniverse(v) => {
                for candidate in &v.universe.candidates {
                    let bond = exact_bond(commit, &candidate.instrument, bond_version)?;
                    if bond.currency != candidate.currency
                        || bond.issue_date > candidate.settlement.date()
                        || bond.maturity_date <= candidate.settlement.date()
                    {
                        return Err(ReferenceError::InvalidInput(
                            "cash hedge security currency/dates conflict",
                        ));
                    }
                    let stamps = [
                        Some(&candidate.quote),
                        candidate.financing.as_ref().map(|f| &f.stamp),
                        candidate.borrow.as_ref().map(|b| &b.stamp),
                        candidate.costs.as_ref().map(|c| &c.stamp),
                    ];
                    if stamps
                        .into_iter()
                        .flatten()
                        .any(|s| s.available_at > commit.recorded_at)
                    {
                        return Err(ReferenceError::InvalidInput(
                            "cash source not yet known at recording time",
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_contract_links(
    contract: &FuturesContract,
    master: &mut impl FnMut(&MasterKey, Version) -> Result<MasterRecord, ReferenceError>,
) -> Result<(), ReferenceError> {
    validate_calendar(
        &contract.daily_settlement.calendar,
        contract.listed_from,
        contract.expiry,
        master,
    )?;
    match &contract.settlement {
        FuturesSettlement::Bond { delivery, .. } => {
            validate_calendar(
                &delivery.calendar,
                delivery.first_notice,
                delivery.last_delivery,
                master,
            )?;
            let basket = master(
                &MasterKey::Basket(delivery.basket.id.clone()),
                delivery.basket.version,
            )?;
            let MasterDefinition::Basket(basket) = &basket.definition else {
                return Err(ReferenceError::InvalidInput(
                    "wrong pinned basket resource kind",
                ));
            };
            basket
                .basket
                .validate_contract(contract)
                .map_err(|e| ReferenceError::InvalidInput(e.reason))?;
        }
        FuturesSettlement::Rate { accrual, .. } => {
            let (start, end) = accrual.window();
            let calendar = match accrual {
                RateAccrual::Overnight { calendar, .. } | RateAccrual::Term { calendar, .. } => {
                    calendar
                }
            };
            validate_calendar(calendar, start, end, master)?;
        }
    }
    Ok(())
}

fn validate_calendar(
    pin: &ResourcePin,
    start: BusinessDate,
    end: BusinessDate,
    master: &mut impl FnMut(&MasterKey, Version) -> Result<MasterRecord, ReferenceError>,
) -> Result<(), ReferenceError> {
    let record = master(&MasterKey::Calendar(pin.id.clone()), pin.version)?;
    let MasterDefinition::Calendar(calendar) = record.definition else {
        return Err(ReferenceError::InvalidInput(
            "wrong futures calendar resource kind",
        ));
    };
    if calendar.coverage_from > start || calendar.coverage_until < end {
        return Err(ReferenceError::InvalidInput(
            "futures calendar does not cover actual contract dates",
        ));
    }
    Ok(())
}

fn exact_bond(
    commit: &ReferenceCommit,
    pin: &InstrumentPin,
    lookup: &mut impl FnMut(
        &convex_core::ids::InstrumentId,
        Version,
    ) -> Result<Option<ReferenceRecord>, ReferenceError>,
) -> Result<crate::reference_data::BondReferenceData, ReferenceError> {
    let id: convex_core::ids::InstrumentId = pin.id.clone().into();
    let record = if let Some(record) = commit
        .records
        .iter()
        .find(|r| r.instrument_id == id && r.version == pin.version)
    {
        record.clone()
    } else {
        lookup(&id, pin.version)?.ok_or(ReferenceError::InvalidInput(
            "missing exact pinned bond revision",
        ))?
    };
    if record.instrument_id != id
        || record.version != pin.version
        || record.recorded_at > commit.recorded_at
    {
        return Err(ReferenceError::InvalidInput(
            "pinned bond identity/version/known-at conflict",
        ));
    }
    record.bond.ok_or(ReferenceError::InvalidInput(
        "pinned bond revision is a tombstone",
    ))
}
