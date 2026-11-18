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
            master(
                benchmark_set(
                    "UST-OTR",
                    json!([
                        {"tenor": "2Y", "instrument_id": "UST-2Y", "version": 1},
                        {"tenor": "10Y", "instrument_id": "UST-10Y", "version": 1},
                    ]),
                ),
                None,
            ),
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

/// Under real access control the report names each benchmark security, so a caller
/// entitled to the corporate but not its benchmarks is refused the quote, and one entitled
/// to all of them is served, although the route itself is scoped to the one bond.
#[tokio::test]
async fn a_level_quote_needs_entitlement_to_its_benchmarks() {
    use access::{IssuerFixture, HUMAN_TOKEN};
    use convex_server::access::{AccessControl, ResourceKind};
    let (_setup, engine, cutoff) = desk().await;
    let issuer = IssuerFixture::start().await;
    let uri =
        format!("/api/v1/quotes/BOND-001?level=125&known_at={cutoff}&settlement_date={SETTLEMENT}");
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
        let (status, _, body) = access::request(router, "GET", &uri, Some(HUMAN_TOKEN), None).await;
        assert_eq!(status, expected, "{body}");
    }
    issuer.finish().await;
}
