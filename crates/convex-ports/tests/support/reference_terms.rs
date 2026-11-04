//! Explicit fixture settings, never used by production construction.
#![allow(dead_code)] // Each test suite uses a subset of these shared fixture constructors.
use convex_core::contracts::*;
use convex_core::daycounts::DayCountConvention;
use convex_ports::reference_data::BondReferenceData;
use convex_ports::reference_master::*;
use std::str::FromStr;

pub fn terms(bond: &BondReferenceData) -> BondContractTerms {
    let mut value: BondContractTerms =
        serde_json::from_str(include_str!("../fixtures/bond_contract_terms.json")).unwrap();
    value.dated_date = BusinessDate::try_from(bond.issue_date).unwrap();
    value.stub = if bond.first_coupon_date.is_some() {
        BondStub::ShortFirst
    } else {
        BondStub::None
    };
    value.issuer.entity_id = EntityId::new(&bond.issuer_id).unwrap();
    value.settings.day_count = DayCountConvention::from_str(&bond.day_count).unwrap();
    value.convention.id =
        ReferenceDataId::new(format!("fixture-{:?}", value.settings.day_count)).unwrap();
    value
}

pub fn complete(mut bond: BondReferenceData) -> BondReferenceData {
    // Make an existing implicit short front period explicit in fictional test inputs.
    // This keeps the maturity-anchored cash-flow dates of the original fixtures.
    if bond.first_coupon_date.is_none() && [1, 2, 4, 12].contains(&bond.frequency) {
        let months = (12 / bond.frequency) as i32;
        let mut date = bond.maturity_date;
        let mut first = date;
        for _ in 0..1200 {
            let previous = date.add_months(-months).unwrap();
            if previous <= bond.issue_date {
                if previous != bond.issue_date {
                    bond.first_coupon_date = Some(first);
                }
                break;
            }
            date = previous;
            first = date;
        }
    }
    bond.contractual_terms = Some(terms(&bond));
    bond
}

pub fn resources(bond: &BondReferenceData) -> Vec<MasterChange> {
    let terms = bond.contractual_terms.as_ref().unwrap();
    let provenance = terms.provenance.clone();
    let effective = terms.effective;
    vec![
        MasterChange {
            expected_version: None,
            definition: MasterDefinition::Entity(Box::new(EntityDefinition {
                entity_id: terms.issuer.entity_id.clone(),
                legal_name: bond.issuer_name.clone(),
                parent: None,
                country: bond.country_of_risk.clone(),
                effective,
                provenance: provenance.clone(),
            })),
        },
        MasterChange {
            expected_version: None,
            definition: MasterDefinition::Calendar(Box::new(CalendarDefinition {
                calendar_id: terms.calendar.id.clone(),
                code: terms.calendar_code,
                rule_version: ModelId::new(SUPPORTED_BOND_CALENDAR_RULES).unwrap(),
                coverage_from: BusinessDate::parse("1900-01-01").unwrap(),
                coverage_until: BusinessDate::parse("2100-12-31").unwrap(),
                effective,
                provenance: provenance.clone(),
            })),
        },
        MasterChange {
            expected_version: None,
            definition: MasterDefinition::Convention(Box::new(ConventionDefinition {
                convention_id: terms.convention.id.clone(),
                settings: terms.settings.clone(),
                effective,
                provenance,
            })),
        },
    ]
}
