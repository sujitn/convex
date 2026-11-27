//! A fixed-coupon callable perpetual over the routes, against the independent reference
//! `tests/scripts/perpetual_quote_reference.py`. It is stored with its perpetual terms and
//! no maturity date (a dated bond's stored terms carry neither change), valued in Live and
//! at request time on each call through its horizon and to perpetuity, and calculated from
//! a price or a yield to worst. Its rule reads the G-spread at the worst call. A perpetual
//! the library does not admit, a dated bond with perpetual terms, write-down terms, and a
//! rule the perpetual does not take are refused.
#[path = "../../convex-engine/tests/support/configuration_setup.rs"]
mod configuration;
#[path = "../../convex-engine/tests/support/products.rs"]
mod products;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use configuration::runtime::*;
use convex_core::contracts::*;
use convex_engine::perpetual_valuation::{
    PERPETUAL_MEASURES, PERPETUAL_NOT_ADMITTED, PERPETUAL_SNAPSHOT_MODEL,
};
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

const PERPETUAL: &str = "PERP-001";
/// The Live runtime's settlement.
const SETTLEMENT: &str = "2026-10-06";
/// Its Live price: a premium, at which the first call is worst.
const PRICE: &str = "104.25";

fn decimal(value: &Value) -> Decimal {
    match value {
        Value::String(text) => text.parse().unwrap(),
        other => other.to_string().parse().unwrap(),
    }
}

fn number(value: &Value) -> f64 {
    decimal(value).to_f64().unwrap()
}

/// The fixture bond under `id`, with identifiers of its own.
fn named(id: &str) -> BondReferenceData {
    let mut bond = fixture::bond();
    bond.instrument_id = convex_core::ids::InstrumentId::new(id);
    bond.isin = Some(format!("fixture-isin-{id}"));
    bond.cusip = Some(format!("fixture-cusip-{id}"));
    bond
}

/// The reference's fixed perpetual under `PERPETUAL`, with the calendar and convention
/// pack its commit carries.
fn perpetual() -> (BondReferenceData, Vec<MasterChange>) {
    let bond = products::perpetual(named(PERPETUAL));
    let resources = fixture::reference_terms::resources(&bond)
        .into_iter()
        .filter(|change| {
            matches!(
                change.definition,
                MasterDefinition::Calendar(_) | MasterDefinition::Convention(_)
            )
        })
        .collect();
    (bond, resources)
}

/// A US Treasury note, as the `UST-OTR` set holds it.
fn note(id: &str, maturity: &str, coupon: &str, cusip: &str) -> BondReferenceData {
    let mut bond = fixture::bond();
    bond.instrument_id = convex_core::ids::InstrumentId::new(id);
    bond.isin = None;
    bond.cusip = Some(cusip.into());
    bond.issuer_type = IssuerType::Sovereign;
    bond.maturity_date = Some(convex_core::Date::parse(maturity).unwrap());
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

/// The perpetual's rule: quoted by `method` against `UST-OTR`, its G-spread read at
/// `tenor_basis`.
fn perpetual_rule(method: Value, tenor_basis: &str) -> MasterChange {
    master(
        "pricing_rule",
        json!({
            "instrument_id": PERPETUAL,
            "method": method,
            "benchmark_set_id": "UST-OTR",
            "tenor_basis": tenor_basis,
            "effective": {"from": "2025-01-01T00:00:00Z", "until": null},
            "provenance": {"source": "desk", "evidence_reference": "perpetual-test"},
        }),
    )
}

/// The runtime fixture with the perpetual and a 2- and 10-year Treasury, each valued in
/// Live from its price: the perpetual at `price`, the notes at 99.50 and 98.25. The
/// `UST-OTR` set holds the notes, and the perpetual's rule quotes it by price with its
/// G-spread at the worst call. The configuration is activated.
async fn desk(price: &str) -> (Setup, Arc<PricingEngine>, String) {
    let mut setup = setup().await;
    let (bond, mut master_changes) = perpetual();
    let notes = [
        note("UST-2Y", "2028-01-15", "0.035", "91282CAA9"),
        note("UST-10Y", "2035-01-15", "0.04125", "91282CBB6"),
    ];
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
            "effective": {"from": "2025-01-01T00:00:00Z", "until": null},
            "provenance": {"source": "desk", "evidence_reference": "perpetual-test"},
        }),
    ));
    let at = fixture::time("2025-06-01T00:00:00Z");
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("perpetual-desk").unwrap(),
                changes: std::iter::once(&bond)
                    .chain(&notes)
                    .map(|bond| ReferenceChange::Create {
                        bond: Box::new(bond.clone()),
                    })
                    .collect(),
                master_changes,
            },
            at,
        )
        .await
        .unwrap();
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("perpetual-rule").unwrap(),
                changes: vec![],
                master_changes: vec![perpetual_rule(json!({"kind": "price"}), "workout")],
            },
            at,
        )
        .await
        .unwrap();
    let plan = &mut setup.plan;
    for (id, price) in [
        (PERPETUAL, price),
        ("UST-2Y", "99.50"),
        ("UST-10Y", "98.25"),
    ] {
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
    plan.measures = PERPETUAL_MEASURES
        .iter()
        .chain(["ytm_mid", "g_spread_mid"].iter())
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
        .header("idempotency-key", format!("perpetual-{method}-{uri}"));
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

/// Each Live measure of `id`'s record, as JSON, and the record's model.
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

/// The perpetual is stored as sent, with no maturity date, and a dated bond's stored
/// terms are unchanged. Live values it under its own model from its price: its yield to
/// worst is the first call's, and its G-spread is that yield less the notes' yields
/// interpolated at the call date, in whole basis points. At request time the price gives
/// the same yield, and so does the calculator, with each call through the horizon, the
/// perpetuity and the worst; from the yield to worst it prices back.
#[tokio::test]
async fn a_perpetual_is_stored_valued_in_live_and_calculated_on_its_workouts() {
    let (_setup, engine, cutoff) = desk(PRICE).await;
    let app = router(engine.clone());

    let (status, stored) = call(&app, "GET", &format!("/api/v1/bonds/{PERPETUAL}"), None).await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    assert!(stored.get("maturity_date").is_none(), "{stored}");
    assert_eq!(
        stored["contractual_terms"]["perpetual"],
        json!({"first_call_date": "2030-01-15", "call_interval_months": 6,
               "call_price": "100", "notice_days": 30})
    );
    let (status, bullet) = call(&app, "GET", "/api/v1/bonds/BOND-001", None).await;
    assert_eq!(status, StatusCode::OK, "{bullet}");
    assert_eq!(bullet["maturity_date"], "2030-01-15");
    assert!(bullet["contractual_terms"].get("perpetual").is_none());

    let (model, results) = live(&engine, PERPETUAL);
    assert_eq!(model, PERPETUAL_SNAPSHOT_MODEL);
    assert_eq!(
        decimal(&results["clean_price_mid"]["value"]["value"]),
        decimal(&json!(PRICE))
    );
    // A perpetual has no yield to maturity.
    assert!(results.get("ytm_mid").is_none(), "{results}");
    let ytw = measure(&results, "ytw_mid");

    let calculate = |method: Value, level: String| {
        let body = json!({"method": method, "level": level, "settlement_date": SETTLEMENT, "known_at": cutoff});
        let app = app.clone();
        async move {
            call(
                &app,
                "POST",
                &format!("/api/v1/bonds/{PERPETUAL}/calculate"),
                Some(body),
            )
            .await
        }
    };
    let (status, by_price) = calculate(json!({"kind": "price"}), PRICE.into()).await;
    assert_eq!(status, StatusCode::OK, "{by_price}");
    assert_eq!(by_price["pricing_rule"]["model"], PERPETUAL_SNAPSHOT_MODEL);
    assert_eq!(by_price["horizon"], "2093-07-15");
    let workouts = by_price["workouts"].as_array().unwrap();
    assert_eq!(workouts.len(), 129);
    assert_eq!(workouts[0]["kind"], "call");
    assert_eq!(workouts[0]["date"], "2030-01-15");
    assert_eq!(decimal(&workouts[0]["call_price"]), Decimal::ONE_HUNDRED);
    assert_eq!(workouts[128]["kind"], "perpetuity");
    assert!(workouts[128].get("date").is_none());
    assert_eq!(by_price["worst"], workouts[0]);
    assert_eq!(number(&by_price["ytw"]), ytw);
    assert_eq!(number(&workouts[0]["yield"]), ytw);

    let years = |date: &str| {
        convex_core::Date::parse(SETTLEMENT)
            .unwrap()
            .days_between(&convex_core::Date::parse(date).unwrap()) as f64
            / 365.0
    };
    let (_, two) = live(&engine, "UST-2Y");
    let (_, ten) = live(&engine, "UST-10Y");
    let (y2, y10) = (measure(&two, "ytm_mid"), measure(&ten, "ytm_mid"));
    let (t2, t10) = (years("2028-01-15"), years("2035-01-15"));
    let benchmark = y2 + (y10 - y2) * (years("2030-01-15") - t2) / (t10 - t2);
    let g_spread = ((ytw - benchmark) * 10_000.0).round();
    assert_eq!(measure(&results, "g_spread_mid"), g_spread);
    assert_eq!(number(&by_price["g_spread_mid"]), g_spread);
    assert!(by_price["pricing_rule"].get("g_spread_refused").is_none());

    let (status, by_yield) = calculate(json!({"kind": "yield"}), ytw.to_string()).await;
    assert_eq!(status, StatusCode::OK, "{by_yield}");
    assert!(
        (number(&by_yield["clean_price_mid"]) - 104.25).abs() < 1e-8,
        "{by_yield}"
    );
    assert_eq!(by_yield["worst"]["date"], "2030-01-15");
    // Below par the worst is the perpetuity: its G-spread is over the long bond.
    let (status, at_discount) = calculate(json!({"kind": "price"}), "92.5".into()).await;
    assert_eq!(status, StatusCode::OK, "{at_discount}");
    assert_eq!(at_discount["worst"]["kind"], "perpetuity");
    assert_eq!(
        number(&at_discount["g_spread_mid"]),
        ((number(&at_discount["ytw"]) - y10) * 10_000.0).round()
    );
    assert!(at_discount["pricing_rule"]
        .get("g_spread_refused")
        .is_none());
    // A spread does not quote a perpetual.
    let (status, refused) = calculate(json!({"kind": "z_spread"}), "150".into()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");

    let (status, quote) = call(
        &app,
        "GET",
        &format!(
            "/api/v1/quotes/{PERPETUAL}?market_price={PRICE}&known_at={cutoff}&settlement_date={SETTLEMENT}"
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{quote}");
    assert_eq!(number(&quote["ytw"]), ytw, "{quote}");
    assert!(quote["clean_price_bid"].is_null());

    // Inline terms are valued as stored ones are; a what-if curve values only a dated bond.
    let (bond, _) = perpetual();
    let inline = json!({"bond": bond, "settlement_date": SETTLEMENT, "market_price": PRICE,
                        "known_at": cutoff});
    let (status, quote) = call(&app, "POST", "/api/v1/quote", Some(inline)).await;
    assert_eq!(status, StatusCode::OK, "{quote}");
    assert_eq!(number(&quote["ytw"]), ytw, "{quote}");
    let curve = json!({"currency": "USD", "curve": {
        "curve_id": "SUPPLIED-PERPETUAL", "version": 1, "reference_date": SETTLEMENT,
        "source_time": "2026-10-06T00:00:00Z", "recorded_at": "2026-10-06T00:00:00Z",
        "source": "supplied-contract", "model_id": "linear-continuous-zero",
        "input_versions": [], "quality": "Indicative", "max_age_ms": 86400000,
        "interpolation": "linear", "extrapolation": "flat_zero",
        "points": [{"tenor_years": 0.01, "continuous_zero": 0.04},
                   {"tenor_years": 10.0, "continuous_zero": 0.04}],
    }});
    let what_if = json!({"bond": bond, "settlement_date": SETTLEMENT, "market_price": PRICE,
                         "discount_curve": curve});
    let (status, refused) = call(&app, "POST", "/api/v1/quote", Some(what_if)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
}

/// Below par, Live values the perpetual to perpetuity and its G-spread over the long bond,
/// so its record is complete, as a book reads it.
#[tokio::test]
async fn a_discounted_perpetual_is_complete_in_live() {
    let (_setup, engine, _) = desk("92.5").await;
    let record = &engine.runtime_results_of([PERPETUAL])[0];
    assert_eq!(
        record.result.status(),
        ResultStatus::Complete,
        "{:?}",
        record.result.errors()
    );
    let (_, results) = live(&engine, PERPETUAL);
    let (_, ten) = live(&engine, "UST-10Y");
    let ytw = measure(&results, "ytw_mid");
    assert!((0.065..0.075).contains(&ytw), "{ytw}");
    assert_eq!(
        measure(&results, "g_spread_mid"),
        ((ytw - measure(&ten, "ytm_mid")) * 10_000.0).round()
    );
}

/// A perpetual the library does not admit, a perpetual with a maturity date, write-down
/// or coupon-cancellation terms, and a rule the perpetual does not take are refused, and
/// nothing is stored.
#[tokio::test]
async fn what_a_fixed_coupon_perpetual_does_not_carry_is_refused() {
    let (setup, engine, _) = desk(PRICE).await;
    let app = router(engine);
    let post = |bond: Value| {
        let app = app.clone();
        async move { call(&app, "POST", "/api/v1/bonds", Some(bond)).await }
    };
    // Month end on a semiannual 30/360 US grid: February's last day counts short.
    let month_end = products::month_end_perpetual(named("PERP-MONTH-END"));
    let (status, refused) = post(serde_json::to_value(&month_end).unwrap()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert!(
        refused.to_string().contains(PERPETUAL_NOT_ADMITTED),
        "{refused}"
    );
    let mut dated = products::perpetual(named("PERP-DATED"));
    dated.maturity_date = Some(convex_core::Date::parse("2040-01-15").unwrap());
    let (status, refused) = post(serde_json::to_value(&dated).unwrap()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert!(refused.to_string().contains("maturity date"), "{refused}");
    for field in ["write_down_trigger", "coupon_cancellation"] {
        let mut sent = serde_json::to_value(products::perpetual(named("PERP-AT1"))).unwrap();
        sent["contractual_terms"]["perpetual"][field] = json!({});
        let (status, refused) = post(sent).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{field}: {refused}");
    }
    for id in ["PERP-MONTH-END", "PERP-DATED", "PERP-AT1"] {
        let (status, _) = call(&app, "GET", &format!("/api/v1/bonds/{id}"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{id}");
    }
    // A spread rule, or a G-spread read at maturity, does not apply to a perpetual.
    for rule in [
        perpetual_rule(json!({"kind": "z_spread"}), "workout"),
        perpetual_rule(json!({"kind": "price"}), "maturity"),
    ] {
        let mut rule = rule;
        rule.expected_version = Some(fixture::version(1));
        let refused = setup
            .storage
            .bonds
            .commit(
                &ReferenceCommand {
                    idempotency_key: RequestId::new("perpetual-rule-revision").unwrap(),
                    changes: vec![],
                    master_changes: vec![rule],
                },
                fixture::time("2025-06-02T00:00:00Z"),
            )
            .await
            .unwrap_err();
        assert!(
            refused
                .to_string()
                .contains(convex_ports::level_pricing::RULE_DOES_NOT_APPLY),
            "{refused}"
        );
    }
}
