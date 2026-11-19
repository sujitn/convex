//! Pricing from a quoted level over the routes.
//!
//! A bond's pricing rule and a benchmark set are master resources, committed and
//! read through the reference routes like any other, and linked to the bonds they name.
//!
//! `GET /quotes/:id` prices a stored bond from a level in its rule's unit, with
//! each benchmark's yield read from the Live runtime's price of it, and the level comes
//! back unchanged through the router.
#[path = "support/access_fixture.rs"]
#[allow(dead_code)] // Only the issuer, the policy and the request helper.
mod access;
#[path = "../../convex-engine/tests/support/configuration_setup.rs"]
mod configuration;
#[path = "../../convex-engine/tests/support/curve_replay.rs"]
mod recipes;
use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use configuration::runtime::*;
use convex_core::contracts::*;
use convex_engine::{PricingEngine, RuntimeInput};
use convex_ports::{
    configuration::InstrumentPin,
    quote_event::{FieldUpdate, QuoteChange, QuoteEventKind, QuoteField, QuoteValue},
    reference_data::{BondReferenceData, IssuerType},
    reference_master::{MasterChange, MasterDefinition},
    reference_repository::{ReferenceChange, ReferenceCommand},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

/// A US Treasury under `id`: the fixture bond with a sovereign issuer.
fn treasury(id: &str) -> BondReferenceData {
    let mut bond = fixture::bond();
    bond.instrument_id = convex_core::ids::InstrumentId::new(id);
    bond.isin = Some(format!("fixture-isin-{id}"));
    bond.cusip = Some(format!("fixture-cusip-{id}"));
    bond.issuer_type = IssuerType::Sovereign;
    bond
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
    let (status, _, body) = call_with_headers(router, method, uri, body).await;
    (status, body)
}

async fn call_with_headers(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(key) = body.as_ref().and_then(|b| b["idempotency_key"].as_str()) {
        request = request.header("idempotency-key", key);
    }
    let request = match body {
        Some(body) => request.body(Body::from(serde_json::to_vec(&body).unwrap())),
        None => request.body(Body::empty()),
    }
    .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn provenance() -> Value {
    json!({"source": "desk", "evidence_reference": "level-pricing-test"})
}

fn effective() -> Value {
    json!({"from": "2025-01-01T00:00:00Z", "until": null})
}

fn benchmark_set(id: &str, securities: Value) -> Value {
    json!({"expected_version": null, "definition": {"kind": "benchmark_set", "definition": {
        "benchmark_set_id": id,
        "currency": "USD",
        "securities": securities,
        "effective": effective(),
        "provenance": provenance(),
    }}})
}

fn pricing_rule(instrument: &str, method: Value, set: Option<&str>) -> Value {
    let mut definition = json!({
        "instrument_id": instrument,
        "method": method,
        "effective": effective(),
        "provenance": provenance(),
    });
    if let Some(set) = set {
        definition["benchmark_set_id"] = set.into();
    }
    json!({"expected_version": null, "definition": {"kind": "pricing_rule", "definition": definition}})
}

/// The reference routes commit a benchmark set and a rule quoting the fixture corporate
/// as a spread to its 10-year security, list them by kind, and refuse a set holding a
/// corporate bond, a rule on a bond that is not stored, and an unknown field.
#[tokio::test]
async fn rules_and_benchmark_sets_commit_and_list_through_the_reference_routes() {
    let setup = setup().await;
    let engine = make_engine(
        &setup,
        archive(&setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    let router = router(engine);
    let corporate = fixture::bond().instrument_id.as_str().to_owned();
    let spread = json!({"kind": "benchmark_spread", "tenor": "10Y"});
    let (status, body) = call(
        &router,
        "POST",
        "/api/v1/reference/transactions",
        Some(json!({
            "idempotency_key": "level-seed",
            "changes": [{"kind": "create", "bond": treasury("UST-10Y")}],
            "master_changes": [
                benchmark_set(
                    "UST-OTR",
                    json!([{"tenor": "10Y", "instrument_id": "UST-10Y", "version": 1}]),
                ),
                pricing_rule(&corporate, spread.clone(), Some("UST-OTR")),
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, sets) = call(
        &router,
        "GET",
        "/api/v1/reference/masters/benchmark_set",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sets}");
    assert_eq!(sets["items"].as_array().map(Vec::len), Some(1), "{sets}");
    let (status, rules) = call(
        &router,
        "GET",
        "/api/v1/reference/masters/pricing_rule",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rules}");
    let rule = &rules["items"][0]["definition"]["definition"];
    assert_eq!(rule["method"], spread, "{rules}");
    assert_eq!(rule["benchmark_set_id"], "UST-OTR");

    let refusals = [
        (
            "level-corporate-set",
            benchmark_set(
                "CORPORATES",
                json!([{"tenor": "5Y", "instrument_id": corporate, "version": 1}]),
            ),
        ),
        (
            "level-missing-bond",
            pricing_rule("NOT-STORED", json!({"kind": "yield"}), None),
        ),
        (
            "level-unknown-field",
            pricing_rule(
                &corporate,
                json!({"kind": "yield", "curve": "USD-SOFR"}),
                None,
            ),
        ),
        (
            "level-width-unit",
            with_width(
                pricing_rule(&corporate, json!({"kind": "price"}), None),
                json!({"unit": "basis_points", "buckets": [{"width": "4"}]}),
            ),
        ),
        (
            "level-width-bounds",
            with_width(
                pricing_rule(&corporate, json!({"kind": "yield"}), None),
                json!({"unit": "basis_points", "buckets": [
                    {"width": "4"},
                    {"max_face": "1000000", "width": "6"},
                ]}),
            ),
        ),
        (
            "level-width-inexact",
            with_width(
                pricing_rule(&corporate, json!({"kind": "yield"}), None),
                json!({"unit": "basis_points", "buckets": [{"width": 4}]}),
            ),
        ),
    ];
    for (key, change) in refusals {
        let (status, body) = call(
            &router,
            "POST",
            "/api/v1/reference/transactions",
            Some(json!({"idempotency_key": key, "changes": [], "master_changes": [change]})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{key}: {body}");
        assert_eq!(body["code"], "INVALID_INPUT", "{key}: {body}");
    }
}

const SETTLEMENT: &str = "2026-10-06";

/// A Treasury note under `id`, with its own maturity, coupon and a valid CUSIP, accruing
/// ACT/ACT ICMA as Treasuries do.
fn note(id: &str, maturity: &str, coupon: &str, cusip: &str) -> BondReferenceData {
    let mut bond = treasury(id);
    bond.maturity_date = convex_core::Date::parse(maturity).unwrap();
    bond.coupon_rate = Some(coupon.parse().unwrap());
    bond.cusip = Some(cusip.into());
    bond.day_count = "ACT/ACT ICMA".into();
    bond.contractual_terms = Some(fixture::reference_terms::terms(&bond));
    bond
}

fn master(definition: Value, expected: Option<u64>) -> MasterChange {
    let mut change = definition;
    change["expected_version"] = expected.into();
    serde_json::from_value(change).unwrap()
}

/// Commit through the repository, recorded at `at`, as the reference routes do.
async fn commit(setup: &Setup, key: &str, at: &str, changes: Vec<MasterChange>) {
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new(key).unwrap(),
                changes: vec![],
                master_changes: changes,
            },
            fixture::time(at),
        )
        .await
        .unwrap();
}

/// The `UST-OTR` set's securities: the 2- and 10-year Treasuries.
fn ust_otr_securities() -> Value {
    json!([
        {"tenor": "2Y", "instrument_id": "UST-2Y", "version": 1},
        {"tenor": "10Y", "instrument_id": "UST-10Y", "version": 1},
    ])
}

/// The runtime fixture, with a 2-year and a 10-year Treasury valued beside the fixture
/// corporate (`BOND-001`, maturing 2030) at 99.50 and 98.25, and a 30-year Treasury stored
/// but not valued. The `UST-OTR` set holds the 2- and 10-year; the configuration is
/// activated over the stored curve recipe.
async fn desk() -> (Setup, Arc<PricingEngine>, String) {
    let mut setup = setup().await;
    let notes = [
        note("UST-2Y", "2028-01-15", "0.035", "91282CAA9"),
        note("UST-10Y", "2035-01-15", "0.04125", "91282CBB6"),
        note("UST-30Y", "2050-01-15", "0.045", "91282CCC3"),
    ];
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("level-notes").unwrap(),
                changes: notes
                    .iter()
                    .map(|bond| ReferenceChange::Create {
                        bond: Box::new(bond.clone()),
                    })
                    .collect(),
                // The notes' ACT/ACT ICMA convention pack; the issuer and calendar exist.
                master_changes: fixture::reference_terms::resources(&notes[0])
                    .into_iter()
                    .filter(|change| matches!(change.definition, MasterDefinition::Convention(_)))
                    .collect(),
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
    for (bond, price) in notes[..2].iter().zip(["99.50", "98.25"]) {
        plan.assembly.instruments.push(InstrumentPin {
            instrument_id: InstrumentId::new(bond.instrument_id.as_str()).unwrap(),
            version: fixture::version(1),
        });
        let mut quote = plan.assembly.quotes[0].clone();
        quote.key.instrument_id = bond.instrument_id.clone();
        plan.assembly.quotes.push(quote);
        let mut event = fixture::event(1, 0, 0, price);
        event.source_time_ms = plan.seed_events[0].source_time_ms;
        event.receive_time_ms = plan.seed_events[0].receive_time_ms;
        event.key.instrument_id = bond.instrument_id.clone();
        plan.seed_events.push(event);
    }
    // The corporate is quoted 125 bp over the 10-year, so the runtime prices it from that
    // level against the notes' prices in the same snapshot.
    plan.assembly.quotes[0].fields = vec![QuoteField::MidSpread];
    plan.seed_events[0].event = QuoteEventKind::Image {
        fields: vec![QuoteChange {
            field: QuoteField::MidSpread,
            update: FieldUpdate::Set(QuoteValue {
                value: 125.into(),
                unit: QuoteField::MidSpread.unit(),
            }),
        }],
    };
    plan.measures = convex_engine::snapshot::RUNTIME_MEASURES
        .iter()
        .map(|m| MeasureId::new(*m).unwrap())
        .collect();
    commit(
        &setup,
        "level-desk",
        "2025-06-01T00:00:00Z",
        vec![
            master(benchmark_set("UST-OTR", ust_otr_securities()), None),
            master(
                benchmark_set(
                    "UST-10",
                    json!([{"tenor": "10Y", "instrument_id": "UST-10Y", "version": 1}]),
                ),
                None,
            ),
            master(
                benchmark_set(
                    "UST-ALL",
                    json!([
                        {"tenor": "10Y", "instrument_id": "UST-10Y", "version": 1},
                        {"tenor": "30Y", "instrument_id": "UST-30Y", "version": 1},
                    ]),
                ),
                None,
            ),
            master(
                pricing_rule(
                    "BOND-001",
                    json!({"kind": "benchmark_spread", "tenor": "10Y"}),
                    Some("UST-OTR"),
                ),
                None,
            ),
        ],
    )
    .await;
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
    // Priced from its level; the configuration has no swap curve, so its I-spread and
    // asset swap are not.
    let corporate = &engine.runtime_results_of(["BOND-001"])[0];
    assert!(
        corporate
            .result
            .results()
            .keys()
            .any(|measure| measure.as_str() == "clean_price_mid"),
        "{:?}",
        corporate.result.errors()
    );
    let known_at = engine.runtime_results()[0].result.provenance().known_at;
    let cutoff = known_at
        .datetime()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    (setup, engine, cutoff)
}

fn number(value: &Value) -> f64 {
    match value {
        Value::String(text) => text.parse().unwrap(),
        other => other.as_f64().unwrap(),
    }
}

/// A corporate quoted 125 bp over the 10-year prices from the Treasury's Live price: its
/// yield is the 10-year's street yield plus 125 bp, and its price quoted back gives the
/// same yield. The G-spread interpolates the 2- and 10-year. Yield and Z-spread rules
/// round-trip the same way; a bond with no rule is quoted by its price alone.
#[tokio::test]
async fn a_level_prices_from_its_rule_and_comes_back_through_the_router() {
    let (setup, engine, cutoff) = desk().await;
    let app = router(engine);
    let quote = |id: &str, input: &str| {
        format!("/api/v1/quotes/{id}?{input}&known_at={cutoff}&settlement_date={SETTLEMENT}")
    };
    let (status, headers, treasury) =
        call_with_headers(&app, "GET", &quote("UST-10Y", "market_price=98.25"), None).await;
    assert_eq!(status, StatusCode::OK, "{treasury}");
    assert!(treasury.get("pricing_rule").is_none());
    assert!(headers.get("x-convex-pricing-rule-version").is_none());
    let ten_year = number(&treasury["ytm_mid"]);

    let (status, headers, spread) =
        call_with_headers(&app, "GET", &quote("BOND-001", "level=125"), None).await;
    assert_eq!(status, StatusCode::OK, "{spread}");
    let rule = &spread["pricing_rule"];
    assert_eq!(rule["model"], "convex-level-price-v1");
    assert_eq!(rule["benchmark"]["instrument_id"], "UST-10Y");
    assert!(
        (number(&rule["benchmark"]["ytm"]) - ten_year).abs() < 1e-12,
        "{spread}"
    );
    assert!(
        (number(&spread["ytm_mid"]) - (ten_year + 0.0125)).abs() < 1e-9,
        "{spread}"
    );
    assert!(spread["g_spread_mid"].is_string() || spread["g_spread_mid"].is_number());
    assert_eq!(rule["benchmark_set"]["version"], 1);
    assert_eq!(
        number(&rule["benchmark_set"]["securities"][1]["clean_price_mid"]),
        98.25
    );
    assert_eq!(headers["x-convex-pricing-rule-version"], "1");
    assert_eq!(headers["x-convex-benchmark-set-version"], "1");
    // A level-quoted bond has no bid/ask width yet, and none is taken as a percent of price.
    assert!(spread["clean_price_bid"].is_null());

    // Its price quoted back gives the same yield and G-spread.
    let clean = spread["clean_price_mid"].to_string().replace('"', "");
    let (status, back) = call(
        &app,
        "GET",
        &quote("BOND-001", &format!("market_price={clean}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{back}");
    assert!((number(&back["ytm_mid"]) - number(&spread["ytm_mid"])).abs() < 1e-12);
    assert!((number(&back["g_spread_mid"]) - number(&spread["g_spread_mid"])).abs() < 1e-9);
    assert!(back["pricing_rule"].get("level").is_none());

    // A yield rule and a Z-spread rule round-trip their own levels.
    commit(
        &setup,
        "level-yield",
        "2025-06-02T00:00:00Z",
        vec![master(
            pricing_rule("BOND-001", json!({"kind": "yield"}), None),
            Some(1),
        )],
    )
    .await;
    let (status, headers, by_yield) =
        call_with_headers(&app, "GET", &quote("BOND-001", "level=0.0612"), None).await;
    assert_eq!(status, StatusCode::OK, "{by_yield}");
    assert!(
        (number(&by_yield["ytm_mid"]) - 0.0612).abs() < 1e-10,
        "{by_yield}"
    );
    assert!(headers.get("x-convex-benchmark-set-version").is_none());
    commit(
        &setup,
        "level-z",
        "2025-06-03T00:00:00Z",
        vec![master(
            pricing_rule("BOND-001", json!({"kind": "z_spread"}), None),
            Some(2),
        )],
    )
    .await;
    let (status, by_z) = call(&app, "GET", &quote("BOND-001", "level=150"), None).await;
    assert_eq!(status, StatusCode::OK, "{by_z}");
    assert!(
        (number(&by_z["z_spread_mid"]) - 150.0).abs() < 1e-6,
        "{by_z}"
    );

    // Against a set holding only the 10-year, the 2030 corporate is still priced off it,
    // but its G-spread would be an extrapolation, so it is not published, and says why.
    // The rule names the tenor as 120M; the set labels it 10Y, the same tenor.
    commit(
        &setup,
        "level-ten-only",
        "2025-06-04T00:00:00Z",
        vec![master(
            pricing_rule(
                "BOND-001",
                json!({"kind": "benchmark_spread", "tenor": "120M"}),
                Some("UST-10"),
            ),
            Some(3),
        )],
    )
    .await;
    let (status, outside) = call(&app, "GET", &quote("BOND-001", "level=125"), None).await;
    assert_eq!(status, StatusCode::OK, "{outside}");
    assert_eq!(outside["pricing_rule"]["benchmark"]["tenor"], "10Y");
    assert!(outside["g_spread_mid"].is_null(), "{outside}");
    assert_eq!(
        outside["pricing_rule"]["g_spread_refused"],
        "the bond matures outside its benchmark set's maturities"
    );
    assert!(
        (number(&outside["ytm_mid"]) - (ten_year + 0.0125)).abs() < 1e-9,
        "{outside}"
    );

    // Nor past a set's longest security: against the 2028 2-year alone.
    commit(
        &setup,
        "level-two-only",
        "2025-06-05T00:00:00Z",
        vec![
            master(
                benchmark_set(
                    "UST-2",
                    json!([{"tenor": "2Y", "instrument_id": "UST-2Y", "version": 1}]),
                ),
                None,
            ),
            master(
                pricing_rule(
                    "BOND-001",
                    json!({"kind": "benchmark_spread", "tenor": "2Y"}),
                    Some("UST-2"),
                ),
                Some(4),
            ),
        ],
    )
    .await;
    let (status, past) = call(&app, "GET", &quote("BOND-001", "level=125"), None).await;
    assert_eq!(status, StatusCode::OK, "{past}");
    assert!(past["g_spread_mid"].is_null(), "{past}");
    assert_eq!(
        past["pricing_rule"]["g_spread_refused"],
        "the bond matures outside its benchmark set's maturities"
    );
    let two_year = number(&past["pricing_rule"]["benchmark"]["ytm"]);
    assert!(
        (number(&past["ytm_mid"]) - (two_year + 0.0125)).abs() < 1e-9,
        "{past}"
    );
}

/// `rule` with a two-way width.
fn with_width(mut rule: Value, width: Value) -> Value {
    rule["definition"]["definition"]["width"] = width;
    rule
}

/// A rule's width gives the two-way in the unit the rule quotes, at its first size bucket,
/// since a request carries no trade size. A corporate quoted 125 bp over the 10-year, 4 bp
/// wide, is bid at 127 and offered at 123, each side priced as the mid is; a yield and a
/// Z-spread rule the same. A Treasury quoted by price, a 32nd wide, is bid and offered
/// half a 32nd either side of its price, exactly, in place of the configuration's
/// percent-of-price width. A price sent for a level-quoted bond is quoted mid only.
#[tokio::test]
async fn a_rule_width_quotes_the_two_way_in_the_rules_unit() {
    let (setup, engine, cutoff) = desk().await;
    let app = router(engine);
    let quote = |id: &str, input: &str| {
        format!("/api/v1/quotes/{id}?{input}&known_at={cutoff}&settlement_date={SETTLEMENT}")
    };
    let (status, treasury) = call(&app, "GET", &quote("UST-10Y", "market_price=98.25"), None).await;
    assert_eq!(status, StatusCode::OK, "{treasury}");
    let ten_year = number(&treasury["ytm_mid"]);
    // With no rule, the configuration's 2 bp of price either side.
    assert!(
        (number(&treasury["clean_price_bid"]) - 98.25 * (1.0 - 0.0002)).abs() < 1e-9,
        "{treasury}"
    );
    // The first bucket's width; the larger trades' is never quoted without a size.
    let basis_points = |first: &str| {
        json!({"unit": "basis_points", "buckets": [
            {"max_face": "1000000", "width": first},
            {"width": "9"},
        ]})
    };

    commit(
        &setup,
        "width-spread",
        "2025-06-02T00:00:00Z",
        vec![master(
            with_width(
                pricing_rule(
                    "BOND-001",
                    json!({"kind": "benchmark_spread", "tenor": "10Y"}),
                    Some("UST-OTR"),
                ),
                basis_points("4"),
            ),
            Some(1),
        )],
    )
    .await;
    let (status, spread) = call(&app, "GET", &quote("BOND-001", "level=125"), None).await;
    assert_eq!(status, StatusCode::OK, "{spread}");
    let two_way = &spread["pricing_rule"]["two_way"];
    assert_eq!(two_way["unit"], "basis_points", "{spread}");
    assert_eq!(
        [&two_way["width"], &two_way["bid"], &two_way["ask"]].map(number),
        [4.0, 127.0, 123.0]
    );
    // The corporate pays semi-annually, so each side's yield is the 10-year's plus its
    // spread.
    for (field, over) in [
        ("ytm_bid", 0.0127),
        ("ytm_mid", 0.0125),
        ("ytm_ask", 0.0123),
    ] {
        assert!(
            (number(&spread[field]) - (ten_year + over)).abs() < 1e-9,
            "{field}: {spread}"
        );
    }
    let [bid, mid, ask] = ["clean_price_bid", "clean_price_mid", "clean_price_ask"]
        .map(|field| number(&spread[field]));
    assert!(bid < mid && mid < ask, "{spread}");
    // Its inline terms, equal to the stored ones, read the rule's width with the rule.
    let posted = json!({"bond": stored_terms(&setup, "BOND-001").await,
        "settlement_date": SETTLEMENT, "known_at": cutoff, "level": 125});
    let (status, posted) = call(&app, "POST", "/api/v1/quote", Some(posted)).await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    assert_eq!(posted["pricing_rule"]["two_way"], *two_way);
    assert_eq!(posted["clean_price_bid"], spread["clean_price_bid"]);

    // A price sent for it is quoted mid only: there is no level to widen.
    let clean = spread["clean_price_mid"].to_string().replace('"', "");
    let (status, back) = call(
        &app,
        "GET",
        &quote("BOND-001", &format!("market_price={clean}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{back}");
    assert!(back["clean_price_bid"].is_null(), "{back}");
    assert!(back["pricing_rule"].get("two_way").is_none(), "{back}");

    // A yield 3 bp wide, and a Z-spread 10 bp wide.
    let levels = [
        (
            "yield",
            "3",
            "0.0612",
            ["ytm_bid", "ytm_ask"],
            [0.06135, 0.06105],
            1e-10,
        ),
        (
            "z_spread",
            "10",
            "150",
            ["z_spread_bid", "z_spread_ask"],
            [155.0, 145.0],
            1e-6,
        ),
    ];
    for (version, (kind, width, level, fields, expected, tolerance)) in (2..).zip(levels) {
        commit(
            &setup,
            &format!("width-{kind}"),
            &format!("2025-06-0{}T00:00:00Z", version + 1),
            vec![master(
                with_width(
                    pricing_rule("BOND-001", json!({"kind": kind}), None),
                    basis_points(width),
                ),
                Some(version),
            )],
        )
        .await;
        let (status, body) = call(
            &app,
            "GET",
            &quote("BOND-001", &format!("level={level}")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        for (field, expected) in fields.into_iter().zip(expected) {
            assert!(
                (number(&body[field]) - expected).abs() < tolerance,
                "{field}: {body}"
            );
        }
    }

    // The 10-year quoted by price, one 32nd wide: 98-08 bid at 98-07+, offered at 98-08+.
    commit(
        &setup,
        "width-treasury",
        "2025-06-05T00:00:00Z",
        vec![master(
            with_width(
                pricing_rule("UST-10Y", json!({"kind": "price"}), None),
                json!({"unit": "thirty_seconds", "buckets": [{"width": "1"}]}),
            ),
            None,
        )],
    )
    .await;
    let (status, treasury) = call(&app, "GET", &quote("UST-10Y", "market_price=98.25"), None).await;
    assert_eq!(status, StatusCode::OK, "{treasury}");
    assert_eq!(
        ["clean_price_bid", "clean_price_ask"].map(|field| number(&treasury[field])),
        [98.234375, 98.265625],
        "{treasury}"
    );
    assert_eq!(
        treasury["pricing_rule"]["two_way"]["unit"],
        "thirty_seconds"
    );
}

/// A benchmark's Live record is read only when it values the pinned version, is no newer
/// than the cutoff, is still live, is of a quality the books accept from Live results
/// (`Valid`, or `Demo` when demonstration data is accepted), and carries a clean price. A
/// stale tick publishes a failed `Stale` result with the last good one kept beside it; the
/// stage reads only the current result, never `last_good`.
#[tokio::test]
async fn a_benchmark_live_record_is_read_only_when_current_and_exact() {
    use convex_core::contracts::{ErrorCode, ResultEnvelope, ResultStatus, StructuredError};
    use convex_engine::level_pricing::{live_clean_price_mid, LevelError};
    use convex_ports::level_pricing::BenchmarkSecurity;
    let (_setup, engine, cutoff) = desk().await;
    let cutoff = fixture::time(&cutoff);
    let record = engine
        .runtime_results_of(["UST-10Y"])
        .first()
        .unwrap()
        .as_ref()
        .clone();
    assert_eq!(record.result.quality(), DataQuality::Valid);
    let read = |record: Option<&_>, version, accepts_demo| {
        let security = BenchmarkSecurity {
            tenor: "10Y".into(),
            instrument_id: InstrumentId::new("UST-10Y").unwrap(),
            version: fixture::version(version),
        };
        live_clean_price_mid(record, &security, cutoff, accepts_demo)
    };
    let reason = |outcome: Result<_, LevelError>| match outcome {
        Err(LevelError::BenchmarkUnavailable {
            tenor,
            instrument_id,
            reason,
        }) => {
            assert_eq!(tenor, "10Y");
            assert_eq!(instrument_id.as_str(), "UST-10Y");
            reason
        }
        other => panic!("expected a benchmark refusal, got {other:?}"),
    };
    let live = read(Some(&record), 1, false).unwrap();
    assert_eq!(live.clean_price_mid, "98.25".parse().unwrap());
    assert!(reason(read(None, 1, false)).contains("does not value it"));
    assert!(reason(read(Some(&record), 2, false)).contains("another version"));
    let mut expired = record.clone();
    expired.live_valid_until = Some(cutoff);
    assert!(reason(read(Some(&expired), 1, false)).contains("stale"));
    let republished = |status, quality, measures, errors| {
        let mut copy = record.clone();
        copy.last_good = Some(record.result.clone());
        copy.result = ResultEnvelope::new(
            record.result.request_id().clone(),
            status,
            quality,
            measures,
            errors,
            record.result.provenance().clone(),
        )
        .unwrap();
        copy
    };
    // A stale tick, as the worker publishes one: failed, stale, no measures.
    let stale = republished(
        ResultStatus::Failed,
        DataQuality::Stale,
        Default::default(),
        vec![StructuredError::new(
            ErrorCode::StaleInput,
            None,
            "the quote is older than its age limit",
            false,
        )
        .unwrap()],
    );
    assert!(reason(read(Some(&stale), 1, false)).contains("stale"));
    // Of a quality the books do not accept either.
    let measures = record.result.results().clone();
    let indicative = republished(
        record.result.status(),
        DataQuality::Indicative,
        measures.clone(),
        record.result.errors().to_vec(),
    );
    assert!(reason(read(Some(&indicative), 1, true)).contains("not accepted"));
    let demo = republished(
        record.result.status(),
        DataQuality::Demo,
        measures.clone(),
        record.result.errors().to_vec(),
    );
    assert!(reason(read(Some(&demo), 1, false)).contains("not accepted"));
    assert_eq!(
        read(Some(&demo), 1, true).unwrap().clean_price_mid,
        live.clean_price_mid
    );
    let mut without_price = measures;
    without_price.remove(&MeasureId::new("clean_price_mid").unwrap());
    let without_price = republished(
        record.result.status(),
        DataQuality::Valid,
        without_price,
        record.result.errors().to_vec(),
    );
    assert!(reason(read(Some(&without_price), 1, false)).contains("no clean price"));
}

/// Each refusal names what is missing, and nothing is substituted: two inputs, a level on
/// a bond with no rule, no input, a benchmark the set does not hold, a benchmark the Live
/// runtime does not value, and a cutoff before the Live price.
#[tokio::test]
async fn a_level_is_refused_by_name_when_its_inputs_are_not_there() {
    let (setup, engine, cutoff) = desk().await;
    let app = router(engine);
    let at = |input: &str, cutoff: &str| {
        format!("/api/v1/quotes/BOND-001?{input}&known_at={cutoff}&settlement_date={SETTLEMENT}")
    };
    let refused = |uri: String, status: StatusCode, text: &'static str| {
        let app = app.clone();
        async move {
            let (actual, body) = call(&app, "GET", &uri, None).await;
            assert_eq!(actual, status, "{uri}: {body}");
            assert!(
                body["error"].as_str().unwrap().contains(text),
                "{uri}: {body}"
            );
            body
        }
    };
    refused(
        at("level=125&market_price=99", &cutoff),
        StatusCode::BAD_REQUEST,
        "not both",
    )
    .await;
    // A spread sent as a price is outside its unit's range, and refused, not priced.
    refused(
        at("level=99999", &cutoff),
        StatusCode::BAD_REQUEST,
        "basis points of spread",
    )
    .await;
    refused(
        format!("/api/v1/quotes/UST-10Y?level=0.04&known_at={cutoff}&settlement_date={SETTLEMENT}"),
        StatusCode::BAD_REQUEST,
        "no pricing rule",
    )
    .await;
    refused(
        format!("/api/v1/quotes/BOND-001?known_at={cutoff}&settlement_date={SETTLEMENT}"),
        StatusCode::UNPROCESSABLE_ENTITY,
        "market_price or level is required",
    )
    .await;
    // A minute before the Live prices, they cannot be read: Live prices are not stored.
    let earlier = fixture::time(&cutoff).datetime() - chrono::Duration::minutes(1);
    let earlier = earlier.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    refused(
        at("level=125", &earlier),
        StatusCode::UNPROCESSABLE_ENTITY,
        "newer than the cutoff",
    )
    .await;
    commit(
        &setup,
        "level-5y",
        "2025-06-02T00:00:00Z",
        vec![master(
            pricing_rule(
                "BOND-001",
                json!({"kind": "benchmark_spread", "tenor": "5Y"}),
                Some("UST-OTR"),
            ),
            Some(1),
        )],
    )
    .await;
    refused(
        at("level=125", &cutoff),
        StatusCode::BAD_REQUEST,
        "no 5Y security",
    )
    .await;
    commit(
        &setup,
        "level-30y",
        "2025-06-03T00:00:00Z",
        vec![master(
            pricing_rule(
                "BOND-001",
                json!({"kind": "benchmark_spread", "tenor": "10Y"}),
                Some("UST-ALL"),
            ),
            Some(2),
        )],
    )
    .await;
    let body = refused(
        at("level=125", &cutoff),
        StatusCode::UNPROCESSABLE_ENTITY,
        "the 30Y benchmark has no usable Live price",
    )
    .await;
    // By tenor, not instrument: a refusal is not checked against the caller's entitlements.
    assert!(!body.to_string().contains("UST-30Y"), "{body}");
}

/// A rule or set is ended by a version with an end date, as the reference routes commit
/// it. After the end the bond has no rule: it is quoted by price with no report, and a
/// level for it is refused. A later start revives the rule; once its set ends, the bond is
/// refused, naming the set, whether asked by level or by price.
#[tokio::test]
async fn a_rule_or_set_with_an_end_date_is_not_in_force_after_it() {
    let (setup, engine, cutoff) = desk().await;
    let app = router(engine);
    let quote = |input: &str| {
        format!("/api/v1/quotes/BOND-001?{input}&known_at={cutoff}&settlement_date={SETTLEMENT}")
    };
    let interval = |mut change: Value, from: &str, until: Option<&str>| {
        change["definition"]["definition"]["effective"] = json!({"from": from, "until": until});
        change
    };
    let spread = || {
        pricing_rule(
            "BOND-001",
            json!({"kind": "benchmark_spread", "tenor": "10Y"}),
            Some("UST-OTR"),
        )
    };
    let ends = "2025-06-04T00:00:00Z";
    commit(
        &setup,
        "rule-ended",
        ends,
        vec![master(
            interval(spread(), "2025-01-01T00:00:00Z", Some(ends)),
            Some(1),
        )],
    )
    .await;
    let (status, headers, body) =
        call_with_headers(&app, "GET", &quote("market_price=99"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.get("pricing_rule").is_none(), "{body}");
    assert!(headers.get("x-convex-pricing-rule-version").is_none());
    let (status, body) = call(&app, "GET", &quote("level=125"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("no pricing rule"),
        "{body}"
    );

    let revived = "2025-06-05T00:00:00Z";
    commit(
        &setup,
        "rule-revived",
        revived,
        vec![master(interval(spread(), revived, None), Some(2))],
    )
    .await;
    let (status, headers, body) = call_with_headers(&app, "GET", &quote("level=125"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(headers["x-convex-pricing-rule-version"], "3");

    let set_ends = "2025-06-06T00:00:00Z";
    commit(
        &setup,
        "set-ended",
        set_ends,
        vec![master(
            interval(
                benchmark_set("UST-OTR", ust_otr_securities()),
                "2025-01-01T00:00:00Z",
                Some(set_ends),
            ),
            Some(1),
        )],
    )
    .await;
    for input in ["level=125", "market_price=99"] {
        let (status, body) = call(&app, "GET", &quote(input), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{input}: {body}");
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("benchmark set UST-OTR is not in force"),
            "{input}: {body}"
        );
    }
}

/// Under real access control the report names each benchmark security, so a caller
/// entitled to the corporate but not its benchmarks is refused the quote, and one entitled
/// to all of them is served, although the route itself is scoped to the one bond.
#[tokio::test]
async fn a_level_quote_needs_entitlement_to_its_benchmarks() {
    use access::{IssuerFixture, HUMAN_TOKEN};
    use convex_server::access::{AccessControl, ResourceKind};
    let (setup, engine, cutoff) = desk().await;
    let issuer = IssuerFixture::start().await;
    let uri =
        format!("/api/v1/quotes/BOND-001?level=125&known_at={cutoff}&settlement_date={SETTLEMENT}");
    let corporate = stored_terms(&setup, "BOND-001").await;
    let single = |mark: Value| {
        let mut body =
            json!({"bond": corporate, "settlement_date": SETTLEMENT, "known_at": cutoff});
        body.as_object_mut()
            .unwrap()
            .extend(mark.as_object().unwrap().clone());
        body
    };
    let batch = |mark: Value| {
        let mut item = json!({"bond": corporate});
        item.as_object_mut()
            .unwrap()
            .extend(mark.as_object().unwrap().clone());
        json!({"bonds": [item], "settlement_date": SETTLEMENT, "known_at": cutoff})
    };
    let by_price = format!(
        "/api/v1/quotes/BOND-001?market_price=99.5&known_at={cutoff}&settlement_date={SETTLEMENT}"
    );
    for (instruments, expected) in [
        (vec!["BOND-001"], StatusCode::FORBIDDEN),
        (vec!["BOND-001", "UST-2Y", "UST-10Y"], StatusCode::OK),
    ] {
        let mut policy = access::policy();
        policy.grants[0].resources.insert(
            ResourceKind::Instrument,
            instruments.into_iter().map(String::from).collect(),
        );
        let router = convex_server::routes::create_router_with_access(
            engine.clone(),
            true,
            AccessControl::new(issuer.verifier.clone(), policy).unwrap(),
        );
        for (method, path, body) in [
            // By price too: the rule's report names the set's securities.
            ("GET", uri.as_str(), None),
            ("GET", by_price.as_str(), None),
            ("POST", "/api/v1/quote", Some(single(json!({"level": 125})))),
            (
                "POST",
                "/api/v1/quote",
                Some(single(json!({"market_price": 99.5}))),
            ),
            (
                "POST",
                "/api/v1/batch/price",
                Some(batch(json!({"level": 125}))),
            ),
            (
                "POST",
                "/api/v1/batch/price",
                Some(batch(json!({"market_price": 99.5}))),
            ),
        ] {
            let (status, _, response) =
                access::request(router.clone(), method, path, Some(HUMAN_TOKEN), body).await;
            assert_eq!(status, expected, "{method} {path}: {response}");
        }
    }
    issuer.finish().await;
}

/// A stored bond's terms, as a request sends them inline.
async fn stored_terms(setup: &Setup, id: &str) -> Value {
    let record = setup
        .storage
        .bonds
        .get_version(
            &convex_core::ids::InstrumentId::new(id),
            fixture::version(1),
        )
        .await
        .unwrap()
        .unwrap();
    serde_json::to_value(record.bond.unwrap()).unwrap()
}

/// `POST /quote` and `/batch/price` price inline terms under the stored rule, as
/// `GET /quotes` prices the stored bond: the same clean price and the same report. The
/// rule applies only to terms equal to the stored ones, and only under a configuration.
#[tokio::test]
async fn inline_terms_equal_to_the_stored_ones_are_priced_under_the_stored_rule() {
    let (setup, engine, cutoff) = desk().await;
    let app = router(engine);
    let (status, stored) = call(
        &app,
        "GET",
        &format!(
            "/api/v1/quotes/BOND-001?level=125&known_at={cutoff}&settlement_date={SETTLEMENT}"
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    let corporate = stored_terms(&setup, "BOND-001").await;
    let treasury = stored_terms(&setup, "UST-10Y").await;
    let quote = |extra: Value| {
        let mut body =
            json!({"bond": corporate, "settlement_date": SETTLEMENT, "known_at": cutoff});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        body
    };

    let (status, headers, single) = call_with_headers(
        &app,
        "POST",
        "/api/v1/quote",
        Some(quote(json!({"level": 125}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{single}");
    assert_eq!(single["clean_price_mid"], stored["clean_price_mid"]);
    assert_eq!(single["ytm_mid"], stored["ytm_mid"]);
    assert_eq!(single["g_spread_mid"], stored["g_spread_mid"]);
    assert_eq!(single["pricing_rule"], stored["pricing_rule"]);
    assert_eq!(headers["x-convex-pricing-rule-version"], "1");
    assert_eq!(headers["x-convex-benchmark-set-version"], "1");
    assert!(
        single["clean_price_bid"].is_null(),
        "no percent-of-price width"
    );

    // By price, the rule's set still gives the G-spread, and the report has no level.
    let clean = single["clean_price_mid"].clone();
    let (status, by_price) = call(
        &app,
        "POST",
        "/api/v1/quote",
        Some(quote(json!({"market_price": clean}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{by_price}");
    assert_eq!(by_price["g_spread_mid"], single["g_spread_mid"]);
    assert!(
        by_price["pricing_rule"].get("level").is_none(),
        "{by_price}"
    );
    // A bond quoted by a level takes no percent-of-price width, even sent a price.
    assert!(by_price["clean_price_bid"].is_null(), "{by_price}");

    // Refused: both marks; terms that differ from the stored ones; supplied curves.
    let mut edited = corporate.clone();
    edited["description"] = "edited for a what-if".into();
    for (body, reason) in [
        (quote(json!({"level": 125, "market_price": 99})), "not both"),
        (
            json!({"bond": edited, "settlement_date": SETTLEMENT, "known_at": cutoff,
                "level": 125}),
            "no pricing rule in force for its terms",
        ),
        (
            quote(json!({"level": 125, "volatility": 0.01})),
            "only a configuration context reads",
        ),
    ] {
        let (status, refusal) = call(&app, "POST", "/api/v1/quote", Some(body)).await;
        assert!(status.is_client_error(), "{status}: {refusal}");
        assert!(refusal.to_string().contains(reason), "{reason}: {refusal}");
    }

    // A batch keeps its order: a Treasury by price, the corporate by its level, and the
    // edited terms refused on their own.
    let (status, batch) = call(
        &app,
        "POST",
        "/api/v1/batch/price",
        Some(json!({
            "bonds": [
                {"bond": treasury, "market_price": 98.25},
                {"bond": corporate, "level": 125},
                {"bond": edited, "level": 125},
            ],
            "settlement_date": SETTLEMENT,
            "known_at": cutoff,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    let quotes = batch["quotes"].as_array().unwrap();
    assert_eq!(quotes.len(), 2, "{batch}");
    assert_eq!(quotes[0]["instrument_id"], "UST-10Y");
    assert!(quotes[0].get("pricing_rule").is_none());
    assert_eq!(quotes[1]["instrument_id"], "BOND-001");
    assert_eq!(quotes[1]["clean_price_mid"], stored["clean_price_mid"]);
    assert_eq!(quotes[1]["pricing_rule"], stored["pricing_rule"]);
    assert_eq!(batch["errors"][0]["instrument_id"], "BOND-001");
    assert!(
        batch["errors"][0]["error"]
            .as_str()
            .unwrap()
            .contains("no pricing rule in force for its terms"),
        "{batch}"
    );
}

/// Measurement: `/batch/price` of 1,000 inline bonds with no rule, and `GET /quotes` of
/// one bond by price and by level, the medians of 20 rounds. Under a rule the batch is
/// also timed by level and by price.
#[tokio::test]
#[ignore = "measurement; run with --ignored --nocapture under the ci profile"]
async fn request_time_pricing_time_at_the_batch_bound() {
    const ROUNDS: usize = 20;
    let (setup, engine, cutoff) = desk().await;
    let app = router(engine);
    let treasury = stored_terms(&setup, "UST-10Y").await;
    let corporate = stored_terms(&setup, "BOND-001").await;
    let batch = |bond: &Value, mark: Value| {
        let mut item = json!({ "bond": bond });
        item.as_object_mut()
            .unwrap()
            .extend(mark.as_object().unwrap().clone());
        json!({"bonds": vec![item; 1000], "settlement_date": SETTLEMENT, "known_at": cutoff})
    };
    let cases = [
        ("batch, no rule, by price", "POST", "/api/v1/batch/price".to_owned(),
            Some(batch(&treasury, json!({"market_price": 98.25})))),
        ("batch, rule, by level", "POST", "/api/v1/batch/price".to_owned(),
            Some(batch(&corporate, json!({"level": 125})))),
        ("batch, rule, by price", "POST", "/api/v1/batch/price".to_owned(),
            Some(batch(&corporate, json!({"market_price": 99.5})))),
        ("GET, no rule, by price", "GET",
            format!("/api/v1/quotes/UST-10Y?market_price=98.25&known_at={cutoff}&settlement_date={SETTLEMENT}"),
            None),
        ("GET, rule, by level", "GET",
            format!("/api/v1/quotes/BOND-001?level=125&known_at={cutoff}&settlement_date={SETTLEMENT}"),
            None),
    ];
    for (name, method, uri, body) in cases {
        let mut times = Vec::with_capacity(ROUNDS);
        for round in 0..ROUNDS + 3 {
            let started = std::time::Instant::now();
            let (status, response) = call(&app, method, &uri, body.clone()).await;
            let elapsed = started.elapsed();
            assert_eq!(status, StatusCode::OK, "{name}: {response}");
            if let Some(errors) = response.get("errors") {
                assert_eq!(errors, &json!([]), "{name}");
            }
            if round >= 3 {
                times.push(elapsed);
            }
        }
        times.sort();
        println!("{name}: median {:?}, min {:?}", times[ROUNDS / 2], times[0]);
    }
}
