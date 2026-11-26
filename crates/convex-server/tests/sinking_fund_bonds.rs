//! A sinking-fund bond over the routes, against the independent reference
//! `tests/scripts/sinking_fund_quote_reference.py`. It is stored with its mandatory sinks
//! (a bullet's stored terms carry none), valued in Live and at request time from a price
//! per 100 current face as one per 100 original face at its factor, and calculated from a
//! price or a yield. Its rule reads the G-spread at its average life. A schedule that
//! disagrees with the bond's flag, or a width on its rule, is refused.
#[path = "../../convex-engine/tests/support/configuration_setup.rs"]
mod configuration;
#[path = "../../convex-engine/tests/support/products.rs"]
mod products;
#[path = "../../convex-engine/tests/support/curve_replay.rs"]
mod recipes;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use configuration::runtime::*;
use convex_core::contracts::*;
use convex_engine::snapshot::{SINKING_FUND_MEASURES, SINKING_FUND_SNAPSHOT_MODEL};
use convex_engine::{PricingEngine, RuntimeInput};
use convex_ports::{
    configuration::InstrumentPin,
    reference_data::{BondReferenceData, IssuerType},
    reference_master::{MasterChange, MasterDefinition},
    reference_repository::{ReferenceChange, ReferenceCommand},
};
use http_body_util::BodyExt;
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

const SINKER: &str = "SINKER-001";
/// The Live runtime's settlement.
const SETTLEMENT: &str = "2026-10-06";

fn reference() -> Value {
    serde_json::from_str(include_str!(
        "../../convex-engine/tests/fixtures/sinking-fund-quotes.json"
    ))
    .unwrap()
}

/// The reference's quote at `settlement`.
fn case(settlement: &str) -> Value {
    reference()["quotes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["settlement"] == settlement)
        .unwrap()
        .clone()
}

fn decimal(value: &Value) -> Decimal {
    match value {
        Value::String(text) => text.parse().unwrap(),
        other => other.to_string().parse().unwrap(),
    }
}

fn number(value: &Value) -> f64 {
    decimal(value).to_f64().unwrap()
}

/// The reference's bond under `SINKER`, with the convention pack its commit carries.
fn sinker() -> (BondReferenceData, Vec<MasterChange>) {
    let mut bond = fixture::bond();
    bond.instrument_id = convex_core::ids::InstrumentId::new(SINKER);
    bond.isin = Some("fixture-isin-sinker".into());
    bond.cusip = Some("fixture-cusip-sinker".into());
    let bond = products::sinker(bond);
    let convention = vec![products::convention(&bond)];
    (bond, convention)
}

/// A US Treasury note, as the `UST-OTR` set holds it.
fn note(id: &str, maturity: &str, coupon: &str, cusip: &str) -> BondReferenceData {
    let mut bond = fixture::bond();
    bond.instrument_id = convex_core::ids::InstrumentId::new(id);
    bond.isin = None;
    bond.cusip = Some(cusip.into());
    bond.issuer_type = IssuerType::Sovereign;
    bond.maturity_date = convex_core::Date::parse(maturity).unwrap();
    bond.coupon_rate = Some(coupon.parse().unwrap());
    bond.day_count = "ACT/ACT ICMA".into();
    bond.contractual_terms = Some(fixture::reference_terms::terms(&bond));
    bond
}

fn master(kind: &str, definition: Value) -> MasterChange {
    serde_json::from_value(json!({
        "expected_version": null,
        "definition": {"kind": kind, "definition": definition},
    }))
    .unwrap()
}

fn effective() -> Value {
    json!({"from": "2025-01-01T00:00:00Z", "until": null})
}

fn provenance() -> Value {
    json!({"source": "desk", "evidence_reference": "sinking-fund-test"})
}

/// The sinker's rule: quoted by price, its G-spread over `UST-OTR` read at its average
/// life.
fn sinker_rule(width: Option<Value>) -> MasterChange {
    let mut definition = json!({
        "instrument_id": SINKER,
        "method": {"kind": "price"},
        "benchmark_set_id": "UST-OTR",
        "tenor_basis": "average_life",
        "effective": effective(),
        "provenance": provenance(),
    });
    if let Some(width) = width {
        definition["width"] = width;
    }
    master("pricing_rule", definition)
}

/// The runtime fixture with the sinker and a 2- and 10-year Treasury, each valued in Live
/// from its price: the sinker at 99.5 per 100 current face, the notes at 99.50 and 98.25.
/// The `UST-OTR` set holds the notes, and the sinker's rule reads its G-spread over it at
/// its average life. The configuration is activated over the stored curve recipe.
async fn desk() -> (Setup, Arc<PricingEngine>, String) {
    let mut setup = setup().await;
    let (bond, sinker_convention) = sinker();
    let notes = [
        note("UST-2Y", "2028-01-15", "0.035", "91282CAA9"),
        note("UST-10Y", "2035-01-15", "0.04125", "91282CBB6"),
    ];
    let mut master_changes = sinker_convention;
    master_changes.extend(
        fixture::reference_terms::resources(&notes[0])
            .into_iter()
            .filter(|change| matches!(change.definition, MasterDefinition::Convention(_))),
    );
    master_changes.push(master(
        "benchmark_set",
        json!({
            "benchmark_set_id": "UST-OTR",
            "currency": "USD",
            "securities": [
                {"tenor": "2Y", "instrument_id": "UST-2Y", "version": 1},
                {"tenor": "10Y", "instrument_id": "UST-10Y", "version": 1},
            ],
            "effective": effective(),
            "provenance": provenance(),
        }),
    ));
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("sinking-fund-desk").unwrap(),
                changes: std::iter::once(&bond)
                    .chain(&notes)
                    .map(|bond| ReferenceChange::Create {
                        bond: Box::new(bond.clone()),
                    })
                    .collect(),
                master_changes,
            },
            fixture::time("2025-06-01T00:00:00Z"),
        )
        .await
        .unwrap();
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("sinking-fund-rule").unwrap(),
                changes: vec![],
                master_changes: vec![sinker_rule(None)],
            },
            fixture::time("2025-06-01T00:00:00Z"),
        )
        .await
        .unwrap();
    let record = recipes::record(recipes::recipe());
    setup.storage.curves.save_recipe(&record).await.unwrap();
    let plan = &mut setup.plan;
    plan.assembly.curves.clear();
    plan.assembly.curve_recipes = vec![record];
    for (id, price) in [(SINKER, "99.5"), ("UST-2Y", "99.50"), ("UST-10Y", "98.25")] {
        plan.assembly.instruments.push(InstrumentPin {
            instrument_id: InstrumentId::new(id).unwrap(),
            version: fixture::version(1),
        });
        let mut quote = plan.assembly.quotes[0].clone();
        quote.key.instrument_id = convex_core::ids::InstrumentId::new(id);
        plan.assembly.quotes.push(quote);
        let mut event = fixture::event(1, 0, 0, price);
        event.source_time_ms = plan.seed_events[0].source_time_ms;
        event.receive_time_ms = plan.seed_events[0].receive_time_ms;
        event.key.instrument_id = convex_core::ids::InstrumentId::new(id);
        plan.seed_events.push(event);
    }
    plan.measures = SINKING_FUND_MEASURES
        .iter()
        .chain(["g_spread_mid"].iter())
        .map(|m| MeasureId::new(*m).unwrap())
        .collect();
    let engine = make_engine(
        &setup,
        archive(&setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    let config = setup
        .storage
        .configs
        .get_version("CONFIG-RESTORE", 1)
        .await
        .unwrap()
        .unwrap();
    configuration::activate_pricing(&setup, &config).await;
    let known_at = engine.runtime_results()[0].result.provenance().known_at;
    let cutoff = known_at
        .datetime()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    (setup, engine, cutoff)
}

fn router(engine: Arc<PricingEngine>) -> axum::Router {
    convex_server::routes::create_router_with_access(
        engine,
        true,
        convex_server::access::AccessControl::local_development(),
    )
}

async fn call(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("idempotency-key", format!("sinking-fund-{method}-{uri}"));
    let request = match body {
        Some(body) => request.body(Body::from(serde_json::to_vec(&body).unwrap())),
        None => request.body(Body::empty()),
    }
    .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Each Live measure of `id`'s record, as JSON.
fn live(engine: &PricingEngine, id: &str) -> (String, Value) {
    let record = &engine.runtime_results_of([id])[0];
    let model = record.result.provenance().model_version.as_str().to_owned();
    let results = serde_json::to_value(record.result.results()).unwrap();
    (model, results)
}

/// A Live measure's number, whatever its unit.
fn measure(results: &Value, name: &str) -> f64 {
    number(&results[name]["value"]["value"])
}

/// The sinker is stored with its schedule as sent, and a bullet's stored terms carry
/// none. Live values it under its own model from its price per 100 current face: its
/// factor, its price per 100 original face, accrued and yield are the reference's, and its
/// G-spread is its yield less the notes' yields interpolated at its average life, in
/// whole basis points. At request time a price per current face at each later settlement
/// gives the reference's values, and so does the calculator, from the price or the yield.
#[tokio::test]
async fn a_sinker_is_stored_valued_in_live_and_quoted_per_original_face() {
    let (_setup, engine, cutoff) = desk().await;
    let app = router(engine.clone());

    let (status, stored) = call(&app, "GET", &format!("/api/v1/bonds/{SINKER}"), None).await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    let schedule = &stored["contractual_terms"]["sinking_schedule"];
    let sent = &reference()["bond"]["sinking_schedule"];
    assert_eq!(schedule.as_array().unwrap().len(), 5);
    for (stored, sent) in schedule
        .as_array()
        .unwrap()
        .iter()
        .zip(sent.as_array().unwrap())
    {
        assert_eq!(stored["date"], sent["date"]);
        assert_eq!(decimal(&stored["amount"]), decimal(&sent["amount"]));
        assert_eq!(decimal(&stored["price"]), decimal(&sent["price"]));
    }
    let (status, bullet) = call(&app, "GET", "/api/v1/bonds/BOND-001", None).await;
    assert_eq!(status, StatusCode::OK, "{bullet}");
    assert!(bullet["contractual_terms"]
        .get("sinking_schedule")
        .is_none());

    let at_live = case(SETTLEMENT);
    let (model, results) = live(&engine, SINKER);
    assert_eq!(model, SINKING_FUND_SNAPSHOT_MODEL);
    assert_eq!(
        decimal(&results["factor"]["value"]["value"]),
        decimal(&at_live["factor"])
    );
    assert_eq!(
        decimal(&results["clean_price_mid"]["value"]["value"]),
        decimal(&at_live["clean_price_mid"])
    );
    assert!(
        (measure(&results, "accrued_interest") - number(&at_live["accrued_interest"])).abs()
            < 1e-12
    );
    assert!((measure(&results, "ytm_mid") - number(&at_live["ytm_mid"])).abs() < 1e-10);
    let years = |maturity: &str| {
        convex_core::Date::parse(SETTLEMENT)
            .unwrap()
            .days_between(&convex_core::Date::parse(maturity).unwrap()) as f64
            / 365.0
    };
    let (_, two) = live(&engine, "UST-2Y");
    let (_, ten) = live(&engine, "UST-10Y");
    let (y2, y10) = (measure(&two, "ytm_mid"), measure(&ten, "ytm_mid"));
    let (t2, t10) = (years("2028-01-15"), years("2035-01-15"));
    let average_life = number(&at_live["average_life_years"]);
    let benchmark = y2 + (y10 - y2) * (average_life - t2) / (t10 - t2);
    let g_spread = ((number(&at_live["ytm_mid"]) - benchmark) * 10_000.0).round();
    assert_eq!(measure(&results, "g_spread_mid"), g_spread);
    // A bullet's Live record carries no factor.
    let (bullet_model, bullet) = live(&engine, "BOND-001");
    assert_ne!(bullet_model, SINKING_FUND_SNAPSHOT_MODEL);
    assert!(bullet.get("factor").is_none());

    // Inline terms under the configuration, and what-if with a supplied curve, convert a
    // price per current face too.
    let (bond, _) = sinker();
    let curve = json!({"currency": "USD", "curve": {
        "curve_id": "SUPPLIED-SINKER", "version": 1, "reference_date": SETTLEMENT,
        "source_time": "2026-10-06T00:00:00Z", "recorded_at": "2026-10-06T00:00:00Z",
        "source": "supplied-contract", "model_id": "linear-continuous-zero",
        "input_versions": [], "quality": "Indicative", "max_age_ms": 86400000,
        "interpolation": "linear", "extrapolation": "flat_zero",
        "points": [{"tenor_years": 0.01, "continuous_zero": 0.04},
                   {"tenor_years": 10.0, "continuous_zero": 0.04}],
    }});
    for extra in [
        json!({"known_at": cutoff}),
        json!({"discount_curve": curve}),
    ] {
        let mut body = json!({"bond": bond, "settlement_date": SETTLEMENT, "market_price": "99.5"});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let (status, quote) = call(&app, "POST", "/api/v1/quote", Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{quote}");
        assert_eq!(
            decimal(&quote["clean_price_mid"]),
            decimal(&at_live["clean_price_mid"])
        );
        assert_eq!(decimal(&quote["factor"]), decimal(&at_live["factor"]));
    }

    for settlement in [SETTLEMENT, "2027-08-20", "2027-12-01"] {
        let case = case(settlement);
        let price = case["clean_price_per_current_face"].as_str().unwrap();
        let (status, quote) = call(
            &app,
            "GET",
            &format!(
                "/api/v1/quotes/{SINKER}?market_price={price}&known_at={cutoff}&settlement_date={settlement}"
            ),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{quote}");
        assert_eq!(
            decimal(&quote["factor"]),
            decimal(&case["factor"]),
            "{quote}"
        );
        assert_eq!(
            decimal(&quote["clean_price_mid"]),
            decimal(&case["clean_price_mid"])
        );
        assert!(quote["clean_price_bid"].is_null());
        assert!(
            (number(&quote["accrued_interest"]) - number(&case["accrued_interest"])).abs() < 1e-12
        );
        assert!(
            (number(&quote["ytm_mid"]) - number(&case["ytm_mid"])).abs() < 1e-10,
            "{quote}"
        );

        let calculate = |method: Value, level: &str| {
            let body = json!({"method": method, "level": level, "settlement_date": settlement, "known_at": cutoff});
            let app = app.clone();
            async move {
                call(
                    &app,
                    "POST",
                    &format!("/api/v1/bonds/{SINKER}/calculate"),
                    Some(body),
                )
                .await
            }
        };
        let (status, by_price) = calculate(json!({"kind": "price"}), price).await;
        assert_eq!(status, StatusCode::OK, "{by_price}");
        assert_eq!(
            decimal(&by_price["clean_price_mid"]),
            decimal(&case["clean_price_mid"])
        );
        assert_eq!(decimal(&by_price["factor"]), decimal(&case["factor"]));
        assert_eq!(by_price["worst"]["kind"], "maturity");
        let ytm = case["ytm_mid"].as_str().unwrap();
        let (status, by_yield) = calculate(json!({"kind": "yield"}), ytm).await;
        assert_eq!(status, StatusCode::OK, "{by_yield}");
        assert!(
            (number(&by_yield["clean_price_mid"]) - number(&case["clean_price_mid"])).abs() < 1e-8
        );
    }
}

/// A bond flagged sinkable with no schedule, or carrying a schedule it is not flagged
/// for, is refused at the reference route, and so is a two-way width on a sinker's rule:
/// a sinker is quoted mid only.
#[tokio::test]
async fn a_schedule_that_disagrees_with_its_bond_or_a_width_is_refused() {
    let (setup, engine, _) = desk().await;
    let app = router(engine);
    let (mut unflagged, _) = sinker();
    unflagged.instrument_id = convex_core::ids::InstrumentId::new("SINKER-002");
    unflagged.isin = Some("fixture-isin-sinker-2".into());
    unflagged.cusip = Some("fixture-cusip-sinker-2".into());
    unflagged.is_sinkable = false;
    let mut flagged = fixture::bond();
    flagged.instrument_id = convex_core::ids::InstrumentId::new("BULLET-FLAGGED");
    flagged.isin = Some("fixture-isin-flagged".into());
    flagged.cusip = Some("fixture-cusip-flagged".into());
    flagged.is_sinkable = true;
    for bond in [unflagged, flagged] {
        let (status, refused) = call(
            &app,
            "POST",
            "/api/v1/bonds",
            Some(serde_json::to_value(&bond).unwrap()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
        assert!(
            refused.to_string().contains("sinkable exactly"),
            "{refused}"
        );
    }
    let width = json!({"unit": "price_points", "buckets": [{"width": "0.25"}]});
    let refused = setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("sinking-fund-width").unwrap(),
                changes: vec![],
                master_changes: vec![{
                    let mut change = sinker_rule(Some(width));
                    change.expected_version = Some(fixture::version(1));
                    change
                }],
            },
            fixture::time("2025-06-02T00:00:00Z"),
        )
        .await
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains(convex_ports::level_pricing::WIDTH_NEEDS_TWO_WAY),
        "{refused}"
    );
}
