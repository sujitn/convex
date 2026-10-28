//! Request-time callables and floating rate notes over the routes (G18, D57, D61–D65): a
//! stored callable and note valued through the runtime as the stream values them, beside
//! a bullet the router prices.
#[path = "../../convex-engine/tests/support/configuration_setup.rs"]
mod configuration;
#[path = "../../convex-engine/tests/support/products.rs"]
mod products;
#[path = "../../convex-engine/tests/support/curve_replay.rs"]
mod recipes;
use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use configuration::runtime::*;
use convex_core::contracts::*;
use convex_engine::{snapshot::RUNTIME_MEASURES, PricingEngine, RuntimeInput};
use convex_ports::{
    configuration::InstrumentPin,
    fixing::FixingRepository,
    reference_data::BondReferenceData,
    reference_master::CallStyle,
    reference_repository::{ReferenceChange, ReferenceCommand},
    storage::OptionModelAssumption,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

const SETTLEMENT: &str = "2026-10-06";

/// The fixture bond under another identity.
fn renamed(id: &str) -> BondReferenceData {
    let mut bond = fixture::bond();
    bond.instrument_id = convex_core::ids::InstrumentId::new(id);
    bond.isin = Some(format!("fixture-isin-{id}"));
    bond.cusip = Some(format!("fixture-cusip-{id}"));
    bond
}

/// The runtime fixture with a stored callable (`CALL-001`) and floating rate note
/// (`FRN-001`) beside the bullet `BOND-001`. Configuration version 2 carries the callable
/// model assumption and the projection curve; both recipes are in the curve store and the
/// note's fixings are published. The runtime values all three at 99.25.
async fn products_setup() -> (Setup, BondReferenceData, BondReferenceData) {
    let mut setup = setup().await;
    let callable = products::callable(renamed("CALL-001"), CallStyle::Bermudan);
    let note = products::frn(renamed("FRN-001"));
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("request-time-products").unwrap(),
                changes: [&callable, &note]
                    .into_iter()
                    .map(|bond| ReferenceChange::Create {
                        bond: Box::new(bond.clone()),
                    })
                    .collect(),
                master_changes: vec![products::convention(&note), products::index(&note)],
            },
            fixture::time("2025-06-01T00:00:00Z"),
        )
        .await
        .unwrap();
    setup.fixings.publish(&products::published()).await.unwrap();
    let mut config = fixture::config();
    config.version = 2;
    config.option_model = Some(OptionModelAssumption {
        mean_reversion: 0.03,
        volatility: 0.01,
        reason: "fictional fixture assumption".into(),
    });
    config.analytics_curves.frn_reference_curve =
        Some(convex_core::ids::CurveId::new("USD-SOFR-PROJECTION"));
    setup.storage.configs.save(&config).await.unwrap();
    let records = [
        recipes::record(recipes::recipe()),
        recipes::record(products::projection(recipes::recipe())),
    ];
    for record in &records {
        setup.storage.curves.save_recipe(record).await.unwrap();
    }
    let plan = &mut setup.plan;
    plan.assembly.configuration.version = fixture::version(2);
    plan.assembly.curves.clear();
    plan.assembly.curve_recipes = records.to_vec();
    plan.assembly
        .required_curves
        .push(CurveId::new("USD-SOFR-PROJECTION").unwrap());
    for bond in [&callable, &note] {
        plan.assembly.instruments.push(InstrumentPin {
            instrument_id: InstrumentId::new(bond.instrument_id.as_str()).unwrap(),
            version: fixture::version(1),
        });
        let mut quote = plan.assembly.quotes[0].clone();
        quote.key.instrument_id = bond.instrument_id.clone();
        plan.assembly.quotes.push(quote);
        let mut event = plan.seed_events[0].clone();
        event.key.instrument_id = bond.instrument_id.clone();
        plan.seed_events.push(event);
    }
    plan.measures = RUNTIME_MEASURES
        .iter()
        .chain(["oas_mid", "discount_margin_mid"].iter())
        .map(|m| MeasureId::new(*m).unwrap())
        .collect();
    (setup, callable, note)
}

/// Approve configuration version 2, as the operator would.
async fn activate(setup: &Setup) {
    let config = setup
        .storage
        .configs
        .get_version("CONFIG-RESTORE", 2)
        .await
        .unwrap()
        .unwrap();
    configuration::activate_pricing(setup, &config).await;
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
) -> (StatusCode, HeaderMap, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    let request = match body {
        Some(body) => request.body(Body::from(serde_json::to_vec(&body).unwrap())),
        None => request.body(Body::empty()),
    }
    .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, serde_json::from_slice(&bytes).unwrap())
}

fn cutoff(time: UtcTimestamp) -> String {
    time.datetime()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// D57: at the stream's cutoff and settlement and with its mark, `GET /quotes/:id` on the
/// stored callable and note returns the stream's values, read from an unpublished
/// one-instrument snapshot. `POST /quote` answers per measure without a mark, refuses
/// inline terms that differ from the stored ones (D62), and a batch prices a bullet, a
/// callable and a note together (D64).
#[tokio::test]
async fn stored_callables_and_notes_price_at_request_time_as_the_stream_values_them() {
    let (setup, callable, note) = products_setup().await;
    let engine = make_engine(
        &setup,
        archive(&setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    // A runtime without configuration use cases starts only with nothing activated.
    activate(&setup).await;
    let app = router(engine.clone());
    let published = engine.runtime_results();
    let settlement = BusinessDate::parse(SETTLEMENT).unwrap();
    let stream = |id: &str| {
        let valuation = published
            .iter()
            .find(|p| match &p.result.provenance().subject {
                ValuationSubject::InstrumentVersion { instrument_id, .. } => {
                    instrument_id.as_str() == id
                }
                _ => false,
            })
            .unwrap();
        let quote = convex_engine::runtime_bond_quote(
            valuation,
            settlement.date(),
            convex_core::Currency::USD,
        )
        .unwrap();
        (
            valuation.result.provenance().known_at,
            serde_json::to_value(quote).unwrap(),
        )
    };
    let mut priced = Vec::new();
    for (id, fields, curves) in [
        (
            "CALL-001",
            &[
                "clean_price_mid",
                "accrued_interest",
                "oas_mid",
                "effective_duration",
                "effective_convexity",
                "pv01",
            ][..],
            "USD-FIXTURE@7",
        ),
        (
            "FRN-001",
            &["clean_price_mid", "accrued_interest", "discount_margin_mid"][..],
            "USD-FIXTURE@7,USD-SOFR-PROJECTION@7",
        ),
    ] {
        let (known_at, expected) = stream(id);
        let uri = format!(
            "/api/v1/quotes/{id}?market_price=99.25&known_at={}&settlement_date={SETTLEMENT}",
            cutoff(known_at)
        );
        let (status, headers, body) = call(&app, "GET", &uri, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        for field in fields {
            assert!(!expected[field].is_null(), "{id} {field}: {expected}");
            assert_eq!(body[field], expected[field], "{id} {field}");
        }
        assert_eq!(body["source"], "request");
        assert!(body.get("measure_errors").is_none(), "{body}");
        assert_eq!(body["snapshot"]["published"], false);
        assert!(body["snapshot"]["id"].is_string());
        assert_eq!(headers["x-convex-curve-versions"], curves);
        assert_eq!(headers["x-convex-reference-version"], "1");
        assert_eq!(headers["x-convex-pricing-config"], "CONFIG-RESTORE@2");
        assert_eq!(headers["x-convex-activation"], "1");
        assert_eq!(
            body["context"]["curves"].as_array().unwrap().len(),
            curves.split(',').count()
        );
        priced.push((known_at, body));
    }
    let known_at = cutoff(priced[0].0);

    // Without a mark every measure says so, as the stream says it of an unquoted bond.
    let posted = json!({"bond": callable, "settlement_date": SETTLEMENT, "known_at": known_at});
    let (status, _, body) = call(&app, "POST", "/api/v1/quote", Some(posted.clone())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let errors = body["measure_errors"].as_array().unwrap();
    assert_eq!(
        errors.len(),
        convex_engine::optional_valuation::OPTIONAL_MEASURES.len()
    );
    assert!(
        errors.iter().all(|e| e["code"] == "MISSING_MARKET_DATA"),
        "{body}"
    );
    assert!(body["clean_price_mid"].is_null());
    assert_eq!(body["context"]["kind"], "configuration");

    // Inline terms must be the stored ones (D62).
    let mut changed = callable.clone();
    changed.coupon_rate = Some(rust_decimal::Decimal::new(7, 2));
    let mut refused = posted.clone();
    refused["bond"] = json!(changed);
    refused["market_price"] = json!("99.25");
    let (status, _, body) = call(&app, "POST", "/api/v1/quote", Some(refused)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "INVALID_REFERENCE", "{body}");

    // A batch prices a bullet, a callable and a note under one context.
    let batch = json!({
        "bonds": [
            {"bond": fixture::bond(), "market_price": "99.25"},
            {"bond": callable, "market_price": "99.25"},
            {"bond": note, "market_price": "99.25"},
        ],
        "settlement_date": SETTLEMENT,
        "known_at": known_at,
    });
    let (status, _, body) = call(&app, "POST", "/api/v1/batch/price", Some(batch)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["errors"], json!([]), "{body}");
    let quotes = body["quotes"].as_array().unwrap();
    assert_eq!(quotes.len(), 3);
    assert_eq!(quotes[0]["instrument_id"], "BOND-001");
    assert!(quotes[0].get("snapshot").is_none());
    assert_eq!(quotes[1]["oas_mid"], priced[0].1["oas_mid"]);
    assert_eq!(
        quotes[2]["discount_margin_mid"],
        priced[1].1["discount_margin_mid"]
    );
    engine.shutdown().await.unwrap();
}

/// D65: a server with no runtime refuses callables and notes, named configuration or
/// not, and still prices a bullet under the activated configuration.
#[tokio::test]
async fn a_server_without_a_runtime_refuses_callables_and_still_prices_bullets() {
    let (setup, callable, _) = products_setup().await;
    activate(&setup).await;
    let engine = Arc::new(
        convex_engine::PricingEngineBuilder::new()
            .with_storage(setup.storage.clone())
            .build()
            .unwrap(),
    );
    let app = router(engine);
    let known_at = "2026-10-02T15:00:00Z";
    let quote = |bond: &BondReferenceData| {
        json!({"bond": bond, "settlement_date": SETTLEMENT, "known_at": known_at,
            "market_price": "99.25", "configuration": "CONFIG-RESTORE"})
    };
    let (status, _, body) = call(&app, "POST", "/api/v1/quote", Some(quote(&callable))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "UNSUPPORTED_PRODUCT", "{body}");
    let uri = format!(
        "/api/v1/quotes/CALL-001?market_price=99.25&known_at={known_at}&configuration=CONFIG-RESTORE"
    );
    let (status, _, body) = call(&app, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "UNSUPPORTED_PRODUCT", "{body}");
    let (status, _, body) =
        call(&app, "POST", "/api/v1/quote", Some(quote(&fixture::bond()))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["clean_price_mid"].is_number(), "{body}");
    let batch = json!({"bonds": [{"bond": callable, "market_price": "99.25"}],
        "settlement_date": SETTLEMENT, "known_at": known_at});
    let (status, _, body) = call(&app, "POST", "/api/v1/batch/price", Some(batch)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["errors"][0]["error"]
            .as_str()
            .unwrap()
            .contains("UNSUPPORTED_PRODUCT"),
        "{body}"
    );
}
