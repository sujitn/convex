//! Fictional callable, floating rate note, sinking-fund and perpetual terms built on a
//! supplied fixture bond, with the resources, published fixings and projection recipe a
//! floating note needs.
#![allow(dead_code)] // Suites use different subsets of these constructors.
use convex_core::{
    calendars::{Calendar, SIFMACalendar},
    contracts::*,
    ids::FloatingRateIndex,
    Date,
};
use convex_ports::{
    fixing::{FixingKey, PublishedFixing},
    quote_event::{FieldUpdate, QuoteEvent, QuoteEventKind, QuoteField},
    reference_data::{BondReferenceData, BondType, CallScheduleEntry, FloatingRateTerms},
    reference_master::{
        CallStyle, CallTerms, ConventionDefinition, FloatingConventions, FloatingMechanics,
        MasterChange, MasterDefinition, PerpetualTerms, RateIndexDefinition, RateIndexKind,
        ReferencePin,
    },
    snapshot::QuoteRequirement,
};
use rust_decimal::Decimal;
use std::str::FromStr;

fn version(value: u64) -> Version {
    Version::new(value).unwrap()
}

/// A pricing rule quoting `instrument_id` by `method`, in force from 2025.
pub fn pricing_rule(instrument_id: &str, method: serde_json::Value) -> MasterChange {
    serde_json::from_value(serde_json::json!({
        "expected_version": null,
        "definition": {"kind": "pricing_rule", "definition": {
            "instrument_id": instrument_id,
            "method": method,
            "effective": {"from": "2025-01-01T00:00:00Z", "until": null},
            "provenance": {"source": "desk", "evidence_reference": "product-test"},
        }},
    }))
    .unwrap()
}

/// A quote `requirement` reading `field` only, and its `seed` image holding `mark` in it.
pub fn quote_in(
    requirement: &mut QuoteRequirement,
    seed: &mut QuoteEvent,
    field: QuoteField,
    mark: Decimal,
) {
    requirement.fields = vec![field];
    let QuoteEventKind::Image { fields } = &mut seed.event else {
        panic!("an image seed")
    };
    fields[0].field = field;
    let FieldUpdate::Set(value) = &mut fields[0].update else {
        panic!("a set field")
    };
    value.value = mark;
    value.unit = field.unit();
}

/// A quote `requirement` also reading `field`, and its `seed` image also holding `mark`
/// in it.
pub fn also_quote_in(
    requirement: &mut QuoteRequirement,
    seed: &mut QuoteEvent,
    field: QuoteField,
    mark: Decimal,
) {
    requirement.fields.push(field);
    let QuoteEventKind::Image { fields } = &mut seed.event else {
        panic!("an image seed")
    };
    let mut added = fields[0].clone();
    added.field = field;
    let FieldUpdate::Set(value) = &mut added.update else {
        panic!("a set field")
    };
    value.value = mark;
    value.unit = field.unit();
    fields.push(added);
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

/// `bond` as the sinking-fund bond of the independent reference
/// `tests/scripts/sinking_fund_quote_reference.py`: 6% semiannual 30/360 US, unadjusted,
/// from 15 March 2020 to 15 March 2032, with its five mandatory sinks, under a convention
/// pack of its own ([`convention`]).
pub fn sinker(mut bond: BondReferenceData) -> BondReferenceData {
    let pack: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/sinking-fund-quotes.json")).unwrap();
    let stated = &pack["bond"];
    let text = |field: &str| stated[field].as_str().unwrap();
    bond.bond_type = BondType::Amortizing;
    bond.is_sinkable = true;
    bond.issue_date = Date::parse(text("issue_date")).unwrap();
    bond.maturity_date = Some(Date::parse(text("maturity_date")).unwrap());
    bond.coupon_rate = Some(text("coupon_rate").parse().unwrap());
    bond.day_count = text("day_count").into();
    bond.face_value = text("face_value").parse().unwrap();
    bond.first_coupon_date = None;
    let terms = bond.contractual_terms.as_mut().unwrap();
    terms.dated_date = BusinessDate::parse(text("dated_date")).unwrap();
    terms.stub = convex_ports::reference_master::BondStub::None;
    terms.redemption_value = ExactDecimal::parse(text("redemption_value")).unwrap();
    terms.settings.day_count = convex_core::daycounts::DayCountConvention::Thirty360US;
    terms.settings.business_day_convention =
        convex_core::calendars::BusinessDayConvention::Unadjusted;
    terms.settings.end_of_month = false;
    terms.convention.id = ReferenceDataId::new("fixture-convention-sinker").unwrap();
    terms.sinking_schedule = serde_json::from_value(stated["sinking_schedule"].clone()).unwrap();
    bond
}

/// `bond` as the fixed perpetual of the independent reference
/// `tests/scripts/perpetual_quote_reference.py`: 6.5% semiannual 30/360 US from 15 January
/// 2025, payments rolled to the following business day on a weekends-only calendar,
/// callable at par every coupon date from 15 January 2030 on 30 days' notice, under a
/// convention pack and calendar of its own ([`convention`]).
pub fn perpetual(mut bond: BondReferenceData) -> BondReferenceData {
    let pack: serde_json::Value = serde_json::from_str(include_str!(
        "../../../convex-analytics/tests/fixtures/perpetual-quotes.json"
    ))
    .unwrap();
    let stated = &pack["bonds"][0]["terms"];
    let date = |value: &serde_json::Value| Date::parse(value.as_str().unwrap()).unwrap();
    let decimal = |value: &serde_json::Value| Decimal::from_str(value.as_str().unwrap()).unwrap();
    let call = &stated["redemption"]["call"];
    bond.bond_type = BondType::Perpetual;
    bond.is_callable = true;
    bond.issue_date = date(&stated["issue_date"]);
    bond.maturity_date = None;
    bond.coupon_rate = Some(decimal(&stated["profiles"]["coupon"][0]["value"]));
    bond.day_count = "30/360 US".into();
    bond.first_coupon_date = None;
    let terms = bond.contractual_terms.as_mut().unwrap();
    terms.dated_date = BusinessDate::try_from(date(&stated["schedule"]["start_date"])).unwrap();
    terms.stub = convex_ports::reference_master::BondStub::None;
    terms.settings.day_count = convex_core::daycounts::DayCountConvention::Thirty360US;
    terms.settings.business_day_convention =
        convex_core::calendars::BusinessDayConvention::Following;
    terms.settings.end_of_month = false;
    terms.calendar.id = ReferenceDataId::new("fixture-calendar-weekends").unwrap();
    terms.calendar_code = convex_ports::reference_master::BondCalendar::WeekendsOnly;
    terms.convention.id = ReferenceDataId::new("fixture-convention-perpetual").unwrap();
    terms.perpetual = Some(PerpetualTerms {
        first_call_date: BusinessDate::try_from(date(&stated["schedule"]["end_date"])).unwrap(),
        call_interval_months: call["interval_months"].as_u64().unwrap() as u32,
        call_price: ExactDecimal::new(decimal(&call["price_per_100"])),
        notice_days: 30,
    });
    bond
}

/// [`perpetual`] rolled on month end from 31 August 2025, first callable on 31 August
/// 2030. Its semiannual 30/360 US grid reaches February, whose last day counts short, so
/// the stored-terms checks admit it and the library does not.
pub fn month_end_perpetual(bond: BondReferenceData) -> BondReferenceData {
    let mut bond = perpetual(bond);
    bond.issue_date = Date::parse("2025-08-31").unwrap();
    let terms = bond.contractual_terms.as_mut().unwrap();
    terms.dated_date = BusinessDate::parse("2025-08-31").unwrap();
    terms.settings.end_of_month = true;
    terms.perpetual.as_mut().unwrap().first_call_date = BusinessDate::parse("2030-08-31").unwrap();
    bond
}

/// The bond's own convention pack: the note's ACT/360, or the sinker's 30/360 US
/// unadjusted. Overnight compounding accrues on ACT/360 or ACT/365F.
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
