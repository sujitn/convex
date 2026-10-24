//! Floating rate notes through the runtime: stored terms pinning a rate-index resource, its
//! published fixings in the durable store, and stored discount and projection recipes,
//! valued by `convex::library` at the observed clean mid.
#[path = "support/configuration_setup.rs"]
mod configuration;
#[path = "support/curve_replay.rs"]
mod recipes;
use configuration::runtime::*;
use convex_core::{
    calendars::{Calendar, SIFMACalendar},
    contracts::*,
    ids::FloatingRateIndex,
    Date,
};
use convex_engine::{floating_valuation::*, snapshot::RUNTIME_MEASURES, RuntimeInput};
use convex_ports::{
    fixing::{FixingKey, FixingRepository, PublishedFixing},
    reference_data::{BondReferenceData, BondType, FloatingRateTerms},
    reference_master::{
        ConventionDefinition, FloatingConventions, FloatingMechanics, MasterChange,
        MasterDefinition, RateIndexDefinition, RateIndexKind, ReferencePin,
    },
    reference_repository::{ReferenceChange, ReferenceCommand},
    snapshot::SnapshotRepository,
};
use rust_decimal::Decimal;
use std::str::FromStr;

/// Fictional quarterly note on the fixture bond: 45 bp over SOFR, paying two days late.
fn frn() -> BondReferenceData {
    let mut bond = fixture::bond();
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
            version: fixture::version(1),
        },
        overrides: FloatingConventions::default(),
    });
    bond
}
/// The note's ACT/360 convention pack; overnight compounding accrues on ACT/360 or ACT/365F.
fn convention(bond: &BondReferenceData) -> MasterChange {
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
fn index(bond: &BondReferenceData) -> MasterChange {
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
fn projection() -> serde_json::Value {
    let mut recipe = recipes::recipe();
    recipe["key"]["id"] = "USD-SOFR-PROJECTION".into();
    recipe["role"] = serde_json::json!({"kind":"Projection","index_id":"USD-SOFR"});
    recipe
}
/// Every SIFMA business day from 1 July to 1 October at 4.30%, on the rule's schedule.
fn published() -> Vec<PublishedFixing> {
    let calendar = SIFMACalendar::new();
    let mut fixings = Vec::new();
    let mut day = Date::parse("2026-07-01").unwrap();
    while day <= Date::parse("2026-10-01").unwrap() {
        if calendar.is_business_day(day) {
            let at = fixture::time(&format!("{}T13:00:00Z", calendar.add_business_days(day, 1)));
            fixings.push(PublishedFixing {
                schema_version: 1,
                key: FixingKey {
                    index_id: IndexId::new("USD-SOFR").unwrap(),
                    observation_date: BusinessDate::try_from(day).unwrap(),
                    source: SourceId::new("fixture-administrator").unwrap(),
                },
                version: fixture::version(1),
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

/// Version 2 of BOND-001 is the note; configuration version 2 names the projection curve.
/// The plan requests every router measure and the discount margin, over stored recipes.
async fn frn_setup(fixings: Vec<PublishedFixing>, projection_role: bool) -> Setup {
    let mut setup = setup().await;
    let bond = frn();
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("frn-terms").unwrap(),
                changes: vec![ReferenceChange::Update {
                    bond: Box::new(bond.clone()),
                    expected_version: fixture::version(1),
                }],
                master_changes: vec![convention(&bond), index(&bond)],
            },
            fixture::time("2025-06-01T00:00:00Z"),
        )
        .await
        .unwrap();
    if !fixings.is_empty() {
        setup.fixings.publish(&fixings).await.unwrap();
    }
    let mut config = fixture::config();
    config.version = 2;
    config.analytics_curves.frn_reference_curve =
        projection_role.then(|| convex_core::ids::CurveId::new("USD-SOFR-PROJECTION"));
    setup.storage.configs.save(&config).await.unwrap();
    let plan = &mut setup.plan;
    plan.assembly.instruments[0].version = fixture::version(2);
    plan.assembly.configuration.version = fixture::version(2);
    plan.assembly.curves.clear();
    plan.assembly.curve_recipes = vec![
        recipes::record(recipes::recipe()),
        recipes::record(projection()),
    ];
    plan.assembly
        .required_curves
        .push(CurveId::new("USD-SOFR-PROJECTION").unwrap());
    plan.measures = RUNTIME_MEASURES
        .iter()
        .chain(["discount_margin_mid"].iter())
        .map(|m| MeasureId::new(*m).unwrap())
        .collect();
    setup
}
async fn run(setup: &Setup) -> Vec<convex_ports::runtime::PublishedValuation> {
    let engine = make_engine(
        setup,
        archive(setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    let published = engine.runtime_results();
    engine.shutdown().await.unwrap();
    published
}

#[tokio::test]
async fn a_stored_sofr_note_publishes_its_discount_margin_from_derived_fixings() {
    let setup = frn_setup(published(), true).await;
    let outcome = run(&setup).await;
    assert_eq!(outcome.len(), 1);
    let result = &outcome[0].result;
    assert_eq!(
        result.status(),
        ResultStatus::Complete,
        "{:?}",
        result.errors()
    );
    assert_eq!(
        result.provenance().model_version.as_str(),
        FLOATING_SNAPSHOT_MODEL
    );
    // D34: the note answers only the plan measures its model publishes.
    let names: Vec<_> = result.results().keys().map(|m| m.as_str()).collect();
    let mut expected = FLOATING_MEASURES.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
    let value = |name: &str| {
        result.results()[&MeasureId::new(name).unwrap()]
            .value
            .clone()
    };
    let MeasureValue::Price(clean) = value("clean_price_mid") else {
        panic!("observed clean mid")
    };
    assert_eq!(clean.value().decimal(), Decimal::from_str("99.25").unwrap());
    let model = |name: &str| match value(name) {
        MeasureValue::ModelPrice(v) => v.value().get(),
        MeasureValue::ModelBps(v) => v.value().get(),
        other => panic!("{name}: {other:?}"),
    };
    assert!(matches!(
        value("discount_margin_mid"),
        MeasureValue::ModelBps(_)
    ));
    assert!(model("discount_margin_mid").is_finite());
    assert!(model("accrued_interest") > 0.0);
    // The solve reprices the observed clean mid to its 1e-9 price tolerance.
    assert!((model("dirty_price_mid") - 99.25 - model("accrued_interest")).abs() < 1e-8);
    // D33: the snapshot holds exactly the due observations, from the current period's
    // lookback to Thursday, each found in the store; Friday's own fixing is forecast.
    let snapshot = setup
        .snapshots
        .get(&result.provenance().snapshot_id)
        .await
        .unwrap()
        .unwrap();
    let fixings = &snapshot.content().fixings;
    assert!(fixings.iter().all(|f| f.fixing.is_some()));
    assert_eq!(
        fixings.first().unwrap().key.observation_date.date(),
        Date::parse("2026-07-13").unwrap()
    );
    assert_eq!(
        fixings.last().unwrap().key.observation_date.date(),
        Date::parse("2026-10-01").unwrap()
    );
    assert!(result
        .provenance()
        .input_versions
        .iter()
        .any(|v| v.kind.as_str() == "rate_index" && v.id.as_str() == "usd-sofr"));
}

#[tokio::test]
async fn a_missing_due_fixing_or_projection_curve_publishes_the_reason() {
    // A due fixing absent from the store stays explicit, so the snapshot is incomplete.
    let mut fixings = published();
    fixings.retain(|f| f.key.observation_date.date() != Date::parse("2026-09-15").unwrap());
    let setup = frn_setup(fixings, true).await;
    let outcome = run(&setup).await;
    let result = &outcome[0].result;
    assert_eq!(result.status(), ResultStatus::Failed);
    let snapshot = setup
        .snapshots
        .get(&result.provenance().snapshot_id)
        .await
        .unwrap()
        .unwrap();
    assert!(snapshot.content().fixings.iter().any(|f| f.fixing.is_none()
        && f.key.observation_date.date() == Date::parse("2026-09-15").unwrap()));
    // Without the configuration's projection curve, every measure says so.
    let setup = frn_setup(published(), false).await;
    let outcome = run(&setup).await;
    let result = &outcome[0].result;
    assert_eq!(result.status(), ResultStatus::Failed);
    assert_eq!(result.errors().len(), FLOATING_MEASURES.len());
    for error in result.errors() {
        assert_eq!(error.code(), ErrorCode::UnsupportedConvention);
        assert!(
            error.message().contains("frn_reference_curve"),
            "{}",
            error.message()
        );
    }
}
#[tokio::test]
async fn plans_accept_the_discount_margin_and_still_refuse_measures_no_model_publishes() {
    let setup = frn_setup(published(), true).await;
    setup.plan.validate().unwrap();
    let mut unknown = setup.plan.clone();
    unknown
        .measures
        .push(MeasureId::new("g_spread_mid").unwrap());
    assert!(unknown.validate().is_err());
}
