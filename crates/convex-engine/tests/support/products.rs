//! Fictional callable and floating rate note terms built on a supplied fixture bond, with
//! the resources, published fixings and projection recipe a floating note needs.
#![allow(dead_code)] // Suites use different subsets of these constructors.
use convex_core::{
    calendars::{Calendar, SIFMACalendar},
    contracts::*,
    ids::FloatingRateIndex,
    Date,
};
use convex_ports::{
    fixing::{FixingKey, PublishedFixing},
    reference_data::{BondReferenceData, BondType, CallScheduleEntry, FloatingRateTerms},
    reference_master::{
        CallStyle, CallTerms, ConventionDefinition, FloatingConventions, FloatingMechanics,
        MasterChange, MasterDefinition, RateIndexDefinition, RateIndexKind, ReferencePin,
    },
};
use rust_decimal::Decimal;
use std::str::FromStr;

fn version(value: u64) -> Version {
    Version::new(value).unwrap()
}

/// `bond` with explicit annual calls from 2027, under call `style`.
pub fn callable(mut bond: BondReferenceData, style: CallStyle) -> BondReferenceData {
    bond.bond_type = BondType::FixedCallable;
    bond.is_callable = true;
    bond.call_schedule = [
        ("2027-01-15", "101"),
        ("2028-01-15", "100.5"),
        ("2029-01-15", "100"),
    ]
    .iter()
    .map(|(date, price)| CallScheduleEntry {
        call_date: Date::parse(date).unwrap(),
        call_price: Decimal::from_str(price).unwrap(),
        is_make_whole: false,
    })
    .collect();
    bond.contractual_terms.as_mut().unwrap().call_terms = Some(CallTerms {
        style,
        notice_days: 30,
    });
    bond
}

/// `bond` as a quarterly note: 45 bp over SOFR, paying two days late.
pub fn frn(mut bond: BondReferenceData) -> BondReferenceData {
    bond.bond_type = BondType::FloatingRate;
    bond.coupon_rate = None;
    bond.frequency = 4;
    bond.floating_terms = Some(FloatingRateTerms {
        index: FloatingRateIndex::Sofr,
        spread: Decimal::from(45),
        reset_frequency: 4,
        cap: None,
        floor: None,
    });
    bond.day_count = "ACT/360".into();
    let terms = bond.contractual_terms.as_mut().unwrap();
    terms.settings.day_count = convex_core::daycounts::DayCountConvention::Act360;
    terms.convention.id = ReferenceDataId::new("fixture-convention-act360").unwrap();
    terms.payment_lag = 2;
    terms.floating_mechanics = Some(FloatingMechanics {
        index: ReferencePin {
            id: ReferenceDataId::new("usd-sofr").unwrap(),
            version: version(1),
        },
        overrides: FloatingConventions::default(),
    });
    bond
}

/// The note's ACT/360 convention pack; overnight compounding accrues on ACT/360 or ACT/365F.
pub fn convention(bond: &BondReferenceData) -> MasterChange {
    let terms = bond.contractual_terms.as_ref().unwrap();
    MasterChange {
        expected_version: None,
        definition: MasterDefinition::Convention(Box::new(ConventionDefinition {
            convention_id: terms.convention.id.clone(),
            settings: terms.settings.clone(),
            effective: terms.effective,
            provenance: terms.provenance.clone(),
        })),
    }
}

/// Fictional rule on the bond's own calendar: published the next business day at 13:00 UTC.
pub fn index(bond: &BondReferenceData) -> MasterChange {
    let terms = bond.contractual_terms.as_ref().unwrap();
    MasterChange {
        expected_version: None,
        definition: MasterDefinition::RateIndex(Box::new(RateIndexDefinition {
            rate_index_id: ReferenceDataId::new("usd-sofr").unwrap(),
            index_id: IndexId::new("USD-SOFR").unwrap(),
            currency: bond.currency,
            kind: RateIndexKind::Overnight,
            fixing_source: SourceId::new("fixture-administrator").unwrap(),
            calendar: terms.calendar.clone(),
            calendar_code: terms.calendar_code,
            publication_lag_business_days: 1,
            publication_utc_seconds: 13 * 3600,
            conventions: FloatingConventions {
                lookback_business_days: Some(2),
                observation_shift: Some(false),
                lockout_business_days: Some(0),
                ..FloatingConventions::default()
            },
            effective: terms.effective,
            provenance: terms.provenance.clone(),
        })),
    }
}

/// `recipe` re-keyed as the SOFR projection curve.
pub fn projection(mut recipe: serde_json::Value) -> serde_json::Value {
    recipe["key"]["id"] = "USD-SOFR-PROJECTION".into();
    recipe["role"] = serde_json::json!({"kind":"Projection","index_id":"USD-SOFR"});
    recipe
}

/// Every SIFMA business day from 1 July to 1 October at 4.30%, on the rule's schedule.
pub fn published() -> Vec<PublishedFixing> {
    let calendar = SIFMACalendar::new();
    let mut fixings = Vec::new();
    let mut day = Date::parse("2026-07-01").unwrap();
    while day <= Date::parse("2026-10-01").unwrap() {
        if calendar.is_business_day(day) {
            let at =
                UtcTimestamp::parse(&format!("{}T13:00:00Z", calendar.add_business_days(day, 1)))
                    .unwrap();
            fixings.push(PublishedFixing {
                schema_version: 1,
                key: FixingKey {
                    index_id: IndexId::new("USD-SOFR").unwrap(),
                    observation_date: BusinessDate::try_from(day).unwrap(),
                    source: SourceId::new("fixture-administrator").unwrap(),
                },
                version: version(1),
                rate: DecimalRate::new(ExactDecimal::new(Decimal::from_str("0.043").unwrap())),
                published_at: at,
                recorded_at: at,
                is_preliminary: false,
                entitled: true,
                evidence_reference: "fictional".into(),
            });
        }
        day = day.add_days(1);
    }
    fixings
}
