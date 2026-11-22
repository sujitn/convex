//! Request-time callables and floating rate notes over the routes: a
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

/// Commit `changes` and `master_changes` as one reference command under `key`, known at
/// `at`.
async fn commit(
    setup: &Setup,
    key: &str,
    at: UtcTimestamp,
    changes: Vec<ReferenceChange>,
    master_changes: Vec<convex_ports::reference_master::MasterChange>,
) {
    let command = ReferenceCommand {
        idempotency_key: RequestId::new(key).unwrap(),
        changes,
        master_changes,
    };
    setup.storage.bonds.commit(&command, at).await.unwrap();
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
        .chain(
            [
                "oas_mid",
                "discount_margin_mid",
                "ytw_mid",
                "years_to_worst_mid",
            ]
            .iter(),
        )
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

/// Approve configuration version 3: version 2 with a swap benchmark, `USD-SWAP`, whose
/// one stored revision has `quality`. Only bullets read it.
async fn activate_with_swap(setup: &Setup, quality: DataQuality) {
    let mut config = setup
        .storage
        .configs
        .get_version("CONFIG-RESTORE", 2)
        .await
        .unwrap()
        .unwrap();
    config.version = 3;
    config.analytics_curves.swap_curve = Some(convex_core::ids::CurveId::new("USD-SWAP"));
    setup.storage.configs.save(&config).await.unwrap();
    let mut swap = products::projection(recipes::recipe());
    swap["key"]["id"] = "USD-SWAP".into();
    let mut record = recipes::record(swap);
    record.quality = quality;
    setup.storage.curves.save_recipe(&record).await.unwrap();
    configuration::activate_pricing(setup, &config).await;
}

/// A started runtime over the products fixture and the cutoff of its first result.
async fn started(setup: &Setup) -> (Arc<PricingEngine>, String) {
    let engine = make_engine(
        setup,
        archive(setup),
        setup.plan.clone(),
        setup.store.clone(),
        RuntimeInput::Captured,
    );
    engine.start().await.unwrap();
    let known_at = engine.runtime_results()[0].result.provenance().known_at;
    (engine, cutoff(known_at))
}

/// An engine over the setup's storage with no runtime composed.
fn without_runtime(setup: &Setup) -> Arc<PricingEngine> {
    Arc::new(
        convex_engine::PricingEngineBuilder::new()
            .with_storage(setup.storage.clone())
            .build()
            .unwrap(),
    )
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

/// At the stream's cutoff and settlement and with its mark, `GET /quotes/:id` on the
/// stored callable and note returns the stream's values, read from an unpublished
/// one-instrument snapshot. `POST /quote` answers per measure without a mark, refuses
/// inline terms that differ from the stored ones, and a batch prices a bullet, a
/// callable and a note together.
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
        // Only a reply on HW1F trees names an option model.
        assert_eq!(
            body.get("option_model").is_some(),
            id == "CALL-001",
            "{body}"
        );
        assert_eq!(
            body["context"]["curves"].as_array().unwrap().len(),
            curves.split(',').count()
        );
        priced.push((known_at, body));
    }
    let known_at = cutoff(priced[0].0);

    // A callable quoted by price is valued on its workouts, with no option model; its
    // quote reports the rule as a bullet's does, in the body and the headers.
    let rule_wire = json!({
        "expected_version": null,
        "definition": {"kind": "pricing_rule", "definition": {
            "instrument_id": "CALL-001",
            "method": {"kind": "price"},
            "effective": {"from": "2025-01-01T00:00:00Z", "until": null},
            "provenance": {"source": "desk", "evidence_reference": "request-time-products"},
        }},
    });
    // It is quoted mid only, so its rule takes no width.
    let mut with_width = rule_wire.clone();
    with_width["definition"]["definition"]["width"] =
        json!({"unit": "price_points", "buckets": [{"width": "0.25"}]});
    let with_width = serde_json::from_value(with_width).unwrap();
    let rule = serde_json::from_value(rule_wire.clone()).unwrap();
    let commit_rule =
        |key: &str, rule: convex_ports::reference_master::MasterChange| ReferenceCommand {
            idempotency_key: RequestId::new(key).unwrap(),
            changes: vec![],
            master_changes: vec![rule],
        };
    assert_eq!(
        setup
            .storage
            .bonds
            .commit(&commit_rule("call-rule-width", with_width), priced[0].0)
            .await
            .err(),
        Some(
            convex_ports::reference_repository::ReferenceError::InvalidInput(
                convex_ports::level_pricing::WIDTH_NEEDS_TWO_WAY
            )
        )
    );
    setup
        .storage
        .bonds
        .commit(&commit_rule("call-rule", rule), priced[0].0)
        .await
        .unwrap();
    let uri = format!(
        "/api/v1/quotes/CALL-001?market_price=99.25&known_at={known_at}&settlement_date={SETTLEMENT}"
    );
    let (status, headers, body) = call(&app, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["clean_price_mid"], priced[0].1["clean_price_mid"]);
    assert_eq!(body["pricing_rule"]["method"]["kind"], "price", "{body}");
    assert_eq!(body["pricing_rule"]["version"], 1);
    assert_eq!(
        body["pricing_rule"]["model"],
        convex_engine::workout_valuation::WORKOUT_SNAPSHOT_MODEL
    );
    assert_eq!(headers["x-convex-pricing-rule-version"], "1");
    assert!(body["oas_mid"].is_null(), "{body}");
    // It reads no curve.
    assert_eq!(body["snapshot"]["curves"], json!([]), "{body}");
    // Below its call prices its worst is maturity, so its yield to worst is the yield of
    // the bullet with its terms at the same price.
    let ytw = number(&body["ytw"]);
    let bullet = published
        .iter()
        .find(|p| {
            matches!(&p.result.provenance().subject,
                ValuationSubject::InstrumentVersion { instrument_id, .. } if instrument_id.as_str() == "BOND-001")
        })
        .unwrap();
    let MeasureValue::ModelRate(ytm) =
        &bullet.result.results()[&MeasureId::new("ytm_mid").unwrap()].value
    else {
        panic!("the bullet's yield")
    };
    assert!((ytw - ytm.value().get()).abs() < 1e-10, "{body}");
    // Its inline terms, equal to the stored ones, get the same quote and report on POST.
    let by_price = json!({"bond": callable, "settlement_date": SETTLEMENT, "known_at": known_at,
        "market_price": "99.25"});
    let (status, headers, posted) = call(&app, "POST", "/api/v1/quote", Some(by_price)).await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    assert_eq!(posted["clean_price_mid"], body["clean_price_mid"]);
    assert_eq!(posted["pricing_rule"], body["pricing_rule"]);
    assert_eq!(headers["x-convex-pricing-rule-version"], "1");

    // Without a mark every measure says so, as the stream says it of an unquoted bond.
    let posted = json!({"bond": callable, "settlement_date": SETTLEMENT, "known_at": known_at});
    let (status, _, body) = call(&app, "POST", "/api/v1/quote", Some(posted.clone())).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let errors = body["measure_errors"].as_array().unwrap();
    assert_eq!(
        errors.len(),
        convex_engine::workout_valuation::WORKOUT_MEASURES.len()
    );
    assert!(
        errors.iter().all(|e| e["code"] == "MISSING_MARKET_DATA"),
        "{body}"
    );
    assert!(body["clean_price_mid"].is_null());
    assert_eq!(body["context"]["kind"], "configuration");

    // Inline terms must be the stored ones.
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
    // Every item has the single-quote shape, bullets included.
    for quote in quotes {
        assert!(!quote["dirty_price_mid"].is_null(), "{quote}");
    }
    assert_eq!(quotes[0]["instrument_id"], "BOND-001");
    assert!(quotes[0].get("snapshot").is_none());
    assert_eq!(number(&quotes[1]["ytw"]), ytw);
    assert_eq!(
        quotes[1]["pricing_rule"]["method"]["kind"], "price",
        "{body}"
    );
    assert_eq!(
        quotes[2]["discount_margin_mid"],
        priced[1].1["discount_margin_mid"]
    );

    // Quoted by its yield to worst, the callable is priced to worst from its level, and
    // still takes a price.
    let mut by_yield = rule_wire.clone();
    by_yield["expected_version"] = json!(1);
    by_yield["definition"]["definition"]["method"] = json!({"kind": "yield"});
    setup
        .storage
        .bonds
        .commit(
            &commit_rule("call-rule-yield", serde_json::from_value(by_yield).unwrap()),
            priced[0].0,
        )
        .await
        .unwrap();
    let uri = format!(
        "/api/v1/quotes/CALL-001?level={ytw}&known_at={known_at}&settlement_date={SETTLEMENT}"
    );
    let (status, headers, body) = call(&app, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        (number(&body["clean_price_mid"]) - 99.25).abs() < 1e-8,
        "{body}"
    );
    assert_eq!(number(&body["ytw"]), ytw);
    assert_eq!(body["pricing_rule"]["method"]["kind"], "yield", "{body}");
    assert_eq!(number(&body["pricing_rule"]["level"]), ytw);
    assert_eq!(headers["x-convex-pricing-rule-version"], "2");
    let uri = format!(
        "/api/v1/quotes/CALL-001?market_price=99.25&known_at={known_at}&settlement_date={SETTLEMENT}"
    );
    let (status, _, body) = call(&app, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(number(&body["ytw"]), ytw);

    // The calculator gives the quote's values, by the rule's method or by a price, and
    // reports the rule it read.
    let by_level = format!(
        "/api/v1/quotes/CALL-001?level={ytw}&known_at={known_at}&settlement_date={SETTLEMENT}"
    );
    for (method, level, uri) in [
        (json!({"kind": "yield"}), ytw.to_string(), by_level),
        (json!({"kind": "price"}), "99.25".to_string(), uri),
    ] {
        let (_, _, quoted) = call(&app, "GET", &uri, None).await;
        let calculation = json!({"method": method, "level": level,
            "settlement_date": SETTLEMENT, "known_at": known_at});
        let (status, headers, calculated) = call(
            &app,
            "POST",
            "/api/v1/bonds/CALL-001/calculate",
            Some(calculation),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{calculated}");
        for measure in [
            "clean_price_mid",
            "accrued_interest",
            "dirty_price_mid",
            "ytw",
        ] {
            assert_eq!(
                calculated[measure], quoted[measure],
                "{measure}: {calculated}"
            );
        }
        assert_eq!(
            calculated["pricing_rule"],
            json!({"model": convex_engine::workout_valuation::WORKOUT_SNAPSHOT_MODEL,
                "version": 2, "method": {"kind": "yield"}})
        );
        assert_eq!(headers["x-convex-pricing-rule-version"], "2");
    }

    // The calculator values a note by price as the quote route does, with no rule and no
    // workout table, over the note's discount and projection curves.
    let note_curves = "USD-FIXTURE@7,USD-SOFR-PROJECTION@7";
    let calculate_note = |method: Value, level: String| {
        let body = json!({"method": method, "level": level, "settlement_date": SETTLEMENT,
            "known_at": known_at});
        let app = app.clone();
        async move { call(&app, "POST", "/api/v1/bonds/FRN-001/calculate", Some(body)).await }
    };
    let margin = || json!({"kind": "discount_margin"});
    let (status, headers, by_price) =
        calculate_note(json!({"kind": "price"}), "99.25".into()).await;
    assert_eq!(status, StatusCode::OK, "{by_price}");
    for measure in convex_engine::floating_valuation::FLOATING_MEASURES {
        assert_eq!(
            by_price[measure], priced[1].1[measure],
            "{measure}: {by_price}"
        );
    }
    assert!(by_price["pricing_rule"].is_null(), "{by_price}");
    assert!(by_price.get("workouts").is_none(), "{by_price}");
    assert_eq!(headers["x-convex-curve-versions"], note_curves);
    // Its margin is read in its rule's field, so with no margin rule a margin is refused.
    let dm = number(&by_price["discount_margin_mid"]);
    let (status, _, refused) = calculate_note(margin(), dm.to_string()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .unwrap()
            .contains("discount_margin pricing rule"),
        "{refused}"
    );

    // Quoted by its discount margin, the note is priced at it by the quote route and the
    // calculator alike, and still takes a price.
    let mut by_margin = rule_wire.clone();
    by_margin["definition"]["definition"]["instrument_id"] = json!("FRN-001");
    by_margin["definition"]["definition"]["method"] = margin();
    setup
        .storage
        .bonds
        .commit(
            &commit_rule(
                "note-rule-margin",
                serde_json::from_value(by_margin).unwrap(),
            ),
            priced[1].0,
        )
        .await
        .unwrap();
    let uri = format!(
        "/api/v1/quotes/FRN-001?level={dm}&known_at={known_at}&settlement_date={SETTLEMENT}"
    );
    let (status, headers, quoted) = call(&app, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{quoted}");
    assert!(
        (number(&quoted["clean_price_mid"]) - 99.25).abs() < 1e-9,
        "{quoted}"
    );
    assert_eq!(number(&quoted["discount_margin_mid"]), dm);
    assert_eq!(quoted["accrued_interest"], priced[1].1["accrued_interest"]);
    assert_eq!(
        quoted["pricing_rule"],
        json!({"model": convex_engine::floating_valuation::FLOATING_SNAPSHOT_MODEL,
            "version": 1, "method": margin(), "level": dm})
    );
    assert_eq!(headers["x-convex-pricing-rule-version"], "1");
    assert_eq!(headers["x-convex-curve-versions"], note_curves);
    let (status, _, calculated) = calculate_note(margin(), dm.to_string()).await;
    assert_eq!(status, StatusCode::OK, "{calculated}");
    for measure in convex_engine::floating_valuation::FLOATING_MEASURES {
        assert_eq!(
            calculated[measure], quoted[measure],
            "{measure}: {calculated}"
        );
    }
    assert_eq!(
        calculated["pricing_rule"],
        json!({"model": convex_engine::floating_valuation::FLOATING_SNAPSHOT_MODEL,
            "version": 1, "method": margin()})
    );
    assert_eq!(
        calculated["input"],
        json!({"method": margin(), "level": dm})
    );
    let uri = format!(
        "/api/v1/quotes/FRN-001?market_price=99.25&known_at={known_at}&settlement_date={SETTLEMENT}"
    );
    let (status, _, quoted) = call(&app, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{quoted}");
    assert_eq!(
        quoted["discount_margin_mid"],
        priced[1].1["discount_margin_mid"]
    );
    // A margin out of range, or a method the note is not quoted by, is refused.
    let uri = format!(
        "/api/v1/quotes/FRN-001?level=6000&known_at={known_at}&settlement_date={SETTLEMENT}"
    );
    let (status, _, refused) = call(&app, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    for (method, level) in [(margin(), "6000"), (json!({"kind": "yield"}), "0.05")] {
        let (status, _, refused) = calculate_note(method, level.into()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
        assert_eq!(refused["code"], "INVALID_INPUT", "{refused}");
    }
    engine.shutdown().await.unwrap();
}

/// Quoted by its OAS, a callable is priced at it on HW1F trees by the quote route and the
/// calculator alike, and still takes a price; by a price the calculator still values it on
/// its workouts. With no `oas` rule, or on a server without a runtime, the calculator
/// refuses an OAS, and an OAS out of range is refused on both routes.
#[tokio::test]
async fn a_callable_under_an_oas_rule_is_priced_at_its_oas_by_the_quote_route_and_the_calculator() {
    use convex_engine::optional_valuation::{OPTIONAL_MEASURES, OPTIONAL_SNAPSHOT_MODEL};
    let (setup, callable, _) = products_setup().await;
    let (engine, known_at) = started(&setup).await;
    activate(&setup).await;
    let app = router(engine.clone());
    let quote = |query: &str| {
        let uri = format!(
            "/api/v1/quotes/CALL-001?{query}&known_at={known_at}&settlement_date={SETTLEMENT}"
        );
        let app = app.clone();
        async move { call(&app, "GET", &uri, None).await }
    };
    let calculate = |app: axum::Router, method: Value, level: String| {
        let body = json!({"method": method, "level": level, "settlement_date": SETTLEMENT,
            "known_at": known_at});
        async move { call(&app, "POST", "/api/v1/bonds/CALL-001/calculate", Some(body)).await }
    };
    let oas_method = || json!({"kind": "oas"});
    let (status, _, by_price) = quote("market_price=99.25").await;
    assert_eq!(status, StatusCode::OK, "{by_price}");
    let oas = number(&by_price["oas_mid"]);
    // A reply on HW1F trees names the model its OAS was taken on.
    let option_model = json!({"mean_reversion": 0.03, "volatility": 0.01,
        "reason": "fictional fixture assumption", "discount_curve": "USD-FIXTURE"});
    assert_eq!(by_price["option_model"], option_model, "{by_price}");
    // An OAS is read in its rule's field, so with no `oas` rule it is refused.
    let (status, _, refused) = calculate(app.clone(), oas_method(), oas.to_string()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .unwrap()
            .contains("oas pricing rule"),
        "{refused}"
    );

    let rule = products::pricing_rule("CALL-001", oas_method());
    let committed_at = engine.runtime_results()[0].result.provenance().known_at;
    commit(&setup, "call-rule-oas", committed_at, vec![], vec![rule]).await;
    let (status, headers, quoted) = quote(&format!("level={oas}")).await;
    assert_eq!(status, StatusCode::OK, "{quoted}");
    assert!(
        (number(&quoted["clean_price_mid"]) - 99.25).abs() < 1e-6,
        "{quoted}"
    );
    assert_eq!(number(&quoted["oas_mid"]), oas);
    assert_eq!(quoted["accrued_interest"], by_price["accrued_interest"]);
    assert_eq!(
        quoted["pricing_rule"],
        json!({"model": OPTIONAL_SNAPSHOT_MODEL, "version": 1, "method": oas_method(),
            "level": oas})
    );
    assert_eq!(headers["x-convex-pricing-rule-version"], "1");
    assert_eq!(headers["x-convex-curve-versions"], "USD-FIXTURE@7");
    assert_eq!(quoted["option_model"], option_model, "{quoted}");
    // Inline terms equal to the stored ones, alone or in a batch, take the OAS too.
    let posted = json!({"bond": callable, "settlement_date": SETTLEMENT, "known_at": known_at,
        "level": oas});
    let (status, _, posted) = call(&app, "POST", "/api/v1/quote", Some(posted)).await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    assert_eq!(posted["clean_price_mid"], quoted["clean_price_mid"]);
    assert_eq!(posted["pricing_rule"], quoted["pricing_rule"]);
    let batch = json!({"bonds": [{"bond": callable, "level": oas}],
        "settlement_date": SETTLEMENT, "known_at": known_at});
    let (status, _, batch) = call(&app, "POST", "/api/v1/batch/price", Some(batch)).await;
    assert_eq!(status, StatusCode::OK, "{batch}");
    assert_eq!(batch["errors"], json!([]), "{batch}");
    assert_eq!(batch["quotes"][0]["oas_mid"], quoted["oas_mid"]);
    assert_eq!(batch["quotes"][0]["option_model"], option_model, "{batch}");
    let (status, _, calculated) = calculate(app.clone(), oas_method(), oas.to_string()).await;
    assert_eq!(status, StatusCode::OK, "{calculated}");
    // The model's measures, as the quote body names them.
    let measures = [
        "clean_price_mid",
        "dirty_price_mid",
        "accrued_interest",
        "oas_mid",
        "effective_duration",
        "effective_convexity",
        "pv01",
    ];
    assert_eq!(measures.len(), OPTIONAL_MEASURES.len());
    for measure in measures {
        assert!(!quoted[measure].is_null(), "{measure}: {quoted}");
        assert_eq!(
            calculated[measure], quoted[measure],
            "{measure}: {calculated}"
        );
    }
    assert_eq!(
        calculated["pricing_rule"],
        json!({"model": OPTIONAL_SNAPSHOT_MODEL, "version": 1, "method": oas_method()})
    );
    assert_eq!(
        calculated["input"],
        json!({"method": oas_method(), "level": oas})
    );
    assert!(calculated.get("workouts").is_none(), "{calculated}");
    assert_eq!(calculated["option_model"], option_model, "{calculated}");
    // By a price, the quote route values it on its trees, and the calculator on its
    // workouts.
    let (status, _, quoted) = quote("market_price=99.25").await;
    assert_eq!(status, StatusCode::OK, "{quoted}");
    assert_eq!(quoted["oas_mid"], by_price["oas_mid"]);
    let (status, _, on_workouts) =
        calculate(app.clone(), json!({"kind": "price"}), "99.25".into()).await;
    assert_eq!(status, StatusCode::OK, "{on_workouts}");
    assert!(on_workouts["workouts"].is_array(), "{on_workouts}");
    assert!(on_workouts.get("option_model").is_none(), "{on_workouts}");
    assert_eq!(
        on_workouts["pricing_rule"]["model"],
        convex_engine::workout_valuation::WORKOUT_SNAPSHOT_MODEL
    );
    // An OAS out of range is refused on both routes.
    let (status, _, refused) = quote("level=5001").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    let (status, _, refused) = calculate(app.clone(), oas_method(), "5001".into()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_eq!(refused["code"], "INVALID_INPUT", "{refused}");
    engine.shutdown().await.unwrap();
    // A server without a runtime refuses an OAS, naming the runtime.
    let (status, _, refused) = calculate(
        router(without_runtime(&setup)),
        oas_method(),
        oas.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["code"], "UNSUPPORTED_PRODUCT", "{refused}");
    assert!(
        refused["error"].as_str().unwrap().contains("runtime"),
        "{refused}"
    );
}

/// A callable revised after its `oas` rule was committed, to American calls and then to a
/// bullet, so that the rule no longer applies, is refused at request time before anything
/// is valued: by its OAS or a price, on the quote routes, alone and in a batch, and in the
/// calculator.
#[tokio::test]
async fn a_bond_whose_rule_no_longer_applies_is_refused_at_request_time() {
    use convex_ports::level_pricing::RULE_DOES_NOT_APPLY;
    let (setup, _, _) = products_setup().await;
    let (engine, known_at) = started(&setup).await;
    activate(&setup).await;
    let app = router(engine.clone());
    let committed_at = engine.runtime_results()[0].result.provenance().known_at;
    let oas_method = || json!({"kind": "oas"});
    let rule = products::pricing_rule("CALL-001", oas_method());
    commit(&setup, "call-rule-oas", committed_at, vec![], vec![rule]).await;
    let is_rule_refusal = |message: &str| message.contains(RULE_DOES_NOT_APPLY);
    let refuses = |(status, _, refused): (StatusCode, HeaderMap, Value)| {
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
        assert_eq!(refused["code"], "INVALID_INPUT", "{refused}");
        assert!(
            is_rule_refusal(refused["error"].as_str().unwrap()),
            "{refused}"
        );
    };
    let revisions = [
        products::callable(renamed("CALL-001"), CallStyle::American),
        renamed("CALL-001"),
    ];
    for (version, revision) in (1..).zip(revisions) {
        let update = ReferenceChange::Update {
            bond: Box::new(revision.clone()),
            expected_version: Version::new(version).unwrap(),
        };
        let key = format!("call-revision-{version}");
        commit(&setup, &key, committed_at, vec![update], vec![]).await;
        for (field, value, method) in [
            ("level", "50", oas_method()),
            ("market_price", "99.25", json!({"kind": "price"})),
        ] {
            let uri = format!(
                "/api/v1/quotes/CALL-001?{field}={value}&known_at={known_at}&settlement_date={SETTLEMENT}"
            );
            refuses(call(&app, "GET", &uri, None).await);
            // Inline terms equal to the stored ones, alone or in a batch.
            let posted = json!({"bond": revision, field: value, "settlement_date": SETTLEMENT,
                "known_at": known_at});
            refuses(call(&app, "POST", "/api/v1/quote", Some(posted)).await);
            let batch = json!({"bonds": [{"bond": revision, field: value}],
                "settlement_date": SETTLEMENT, "known_at": known_at});
            let (status, _, batch) = call(&app, "POST", "/api/v1/batch/price", Some(batch)).await;
            assert_eq!(status, StatusCode::OK, "{batch}");
            assert!(
                is_rule_refusal(batch["errors"][0]["error"].as_str().unwrap()),
                "{batch}"
            );
            // The calculator by the rule's method or by price, ahead of any refusal by method.
            let body = json!({"method": method, "level": value, "settlement_date": SETTLEMENT,
                "known_at": known_at});
            refuses(call(&app, "POST", "/api/v1/bonds/CALL-001/calculate", Some(body)).await);
        }
    }
    engine.shutdown().await.unwrap();
}

/// A JSON number or numeric string.
fn number(value: &Value) -> f64 {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        .unwrap_or_else(|| panic!("a number: {value}"))
}

/// A server with no runtime refuses callables and notes, named configuration or
/// not, and still prices a bullet under the activated configuration.
#[tokio::test]
async fn a_server_without_a_runtime_refuses_callables_and_still_prices_bullets() {
    let (setup, callable, _) = products_setup().await;
    activate(&setup).await;
    let engine = without_runtime(&setup);
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

/// The calculator values a stored callable on its workouts, with no runtime and no curve.
/// It refuses an American callable before any context is read, and a floating rate note,
/// which the runtime values, on a server without one. Without a configuration activated,
/// the callable and the bullet beside it are refused for want of a context.
#[tokio::test]
async fn the_calculator_values_callables_on_their_workouts_without_a_runtime() {
    let (setup, _, _) = products_setup().await;
    let american = products::callable(renamed("CALL-AMERICAN"), CallStyle::American);
    // A dated yield needs a context-free day count, which ACT/ACT ICMA is not.
    let mut icma = products::callable(renamed("CALL-ICMA"), CallStyle::Bermudan);
    icma.day_count = "ACT/ACT ICMA".into();
    let terms = icma.contractual_terms.as_mut().unwrap();
    terms.settings.day_count = convex_core::daycounts::DayCountConvention::ActActIcma;
    terms.convention.id = ReferenceDataId::new("fixture-convention-icma").unwrap();
    let icma_convention = products::convention(&icma);
    setup
        .storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new("calculator-callables").unwrap(),
                changes: [american, icma]
                    .into_iter()
                    .map(|bond| ReferenceChange::Create {
                        bond: Box::new(bond),
                    })
                    .collect(),
                master_changes: vec![icma_convention],
            },
            fixture::time("2025-06-01T00:00:00Z"),
        )
        .await
        .unwrap();
    let engine = without_runtime(&setup);
    let app = router(engine);
    let calculate = |id: &str, method: Value, level: &str, settlement: &str| {
        let body = json!({"method": method, "level": level, "settlement_date": settlement,
            "known_at": "2026-10-02T15:00:00Z", "configuration": "CONFIG-RESTORE"});
        let (app, uri) = (app.clone(), format!("/api/v1/bonds/{id}/calculate"));
        async move { call(&app, "POST", &uri, Some(body)).await }
    };
    let price = || json!({"kind": "price"});
    // An American callable is not valued on its workouts, and a note needs the runtime.
    for (id, reason) in [
        ("CALL-AMERICAN", "callables on their workouts"),
        ("FRN-001", "runtime"),
    ] {
        let (status, _, refused) = calculate(id, price(), "99.25", SETTLEMENT).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{id}: {refused}");
        assert_eq!(refused["code"], "UNSUPPORTED_PRODUCT", "{refused}");
        assert!(
            refused["error"].as_str().unwrap().contains(reason),
            "{id}: {refused}"
        );
    }
    for id in ["CALL-001", "BOND-001"] {
        let (status, _, refused) = calculate(id, price(), "99.25", SETTLEMENT).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
        assert_eq!(refused["code"], "NO_VALUATION_CONTEXT", "{refused}");
    }
    activate(&setup).await;
    let (status, _, bullet) = calculate("BOND-001", price(), "99.25", SETTLEMENT).await;
    assert_eq!(status, StatusCode::OK, "{bullet}");
    assert_eq!(bullet["clean_price_mid"], json!(99.25), "{bullet}");

    // From a price: each call at its price, then maturity, each at its yield; the worst
    // is the lowest, with the years to it. Below every call price, maturity is worst.
    let (status, headers, by_price) = calculate("CALL-001", price(), "99.25", SETTLEMENT).await;
    assert_eq!(status, StatusCode::OK, "{by_price}");
    assert_eq!(by_price["clean_price_mid"], json!(99.25), "{by_price}");
    assert_eq!(
        by_price["pricing_spec"],
        convex_engine::workout_valuation::WORKOUT_SNAPSHOT_MODEL
    );
    assert!(by_price["pricing_rule"].is_null(), "{by_price}");
    assert_eq!(headers["x-convex-curve-versions"], "", "it reads no curve");
    assert_eq!(headers["x-convex-pricing-config"], "CONFIG-RESTORE@2");
    let workouts = by_price["workouts"].as_array().unwrap();
    let table: Vec<_> = workouts
        .iter()
        .map(|w| (w["kind"].as_str().unwrap(), w["date"].as_str().unwrap()))
        .collect();
    assert_eq!(
        table,
        [
            ("call", "2027-01-15"),
            ("call", "2028-01-15"),
            ("call", "2029-01-15"),
            ("maturity", "2030-01-15"),
        ]
    );
    let call_prices: Vec<_> = workouts[..3]
        .iter()
        .map(|w| number(&w["call_price"]))
        .collect();
    assert_eq!(call_prices, [101.0, 100.5, 100.0]);
    let ytw = number(&by_price["ytw"]);
    assert!(
        workouts.iter().all(|w| number(&w["yield"]) >= ytw),
        "{by_price}"
    );
    assert_eq!(by_price["worst"]["kind"], "maturity", "{by_price}");
    assert_eq!(number(&by_price["worst"]["yield"]), ytw);
    assert!(number(&by_price["worst"]["years"]) > 3.0, "{by_price}");
    assert_eq!(
        by_price["input"],
        json!({"method": price(), "level": 99.25})
    );

    // From that yield to worst: the price, and the same table to the solver's tolerance.
    let level = by_price["ytw"].to_string();
    let (status, _, by_yield) =
        calculate("CALL-001", json!({"kind": "yield"}), &level, SETTLEMENT).await;
    assert_eq!(status, StatusCode::OK, "{by_yield}");
    assert!(
        (number(&by_yield["clean_price_mid"]) - 99.25).abs() < 1e-8,
        "{by_yield}"
    );
    assert_eq!(number(&by_yield["ytw"]), ytw);
    for (from_yield, from_price) in by_yield["workouts"]
        .as_array()
        .unwrap()
        .iter()
        .zip(workouts)
    {
        assert_eq!(from_yield["date"], from_price["date"]);
        assert!((number(&from_yield["yield"]) - number(&from_price["yield"])).abs() < 1e-9);
    }

    // A callable whose conventions the workout model does not value is unsupported.
    let (status, _, refused) = calculate("CALL-ICMA", price(), "99.25", SETTLEMENT).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["code"], "UNSUPPORTED_PRODUCT", "{refused}");

    // A spread does not quote a callable on its workouts, a yield out of range is
    // refused, and nothing settles on a Saturday.
    for (method, level, settlement) in [
        (json!({"kind": "z_spread"}), "150", SETTLEMENT),
        (json!({"kind": "yield"}), "1.5", SETTLEMENT),
        (price(), "99.25", "2026-10-10"),
    ] {
        let (status, _, refused) = calculate("CALL-001", method, level, settlement).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
        assert_eq!(refused["code"], "INVALID_INPUT", "{refused}");
    }
}

/// A request reads only the curves its product reads. Under a configuration whose
/// swap curve is stale at the cutoff, a bullet is refused, alone or in a batch. A
/// callable, which reads only the discount curve, prices alone and in the same batch,
/// and the batch lists only the curves it read.
#[tokio::test]
async fn a_stale_swap_curve_refuses_bullets_and_still_prices_callables() {
    let (setup, callable, _) = products_setup().await;
    let (engine, known_at) = started(&setup).await;
    // A runtime without configuration use cases starts only with nothing activated.
    activate_with_swap(&setup, DataQuality::Stale).await;
    let app = router(engine.clone());

    let bullet = json!({"bond": fixture::bond(), "settlement_date": SETTLEMENT,
        "known_at": known_at, "market_price": "99.25"});
    let (status, _, body) = call(&app, "POST", "/api/v1/quote", Some(bullet)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "MISSING_INPUT", "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("USD-SWAP"),
        "{body}"
    );

    let uri = format!(
        "/api/v1/quotes/CALL-001?market_price=99.25&known_at={known_at}&settlement_date={SETTLEMENT}"
    );
    let (status, headers, body) = call(&app, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["oas_mid"].is_number(), "{body}");
    assert_eq!(headers["x-convex-pricing-config"], "CONFIG-RESTORE@3");
    assert_eq!(headers["x-convex-curve-versions"], "USD-FIXTURE@7");

    let batch = json!({
        "bonds": [
            {"bond": fixture::bond(), "market_price": "99.25"},
            {"bond": callable, "market_price": "99.25"},
        ],
        "settlement_date": SETTLEMENT,
        "known_at": known_at,
    });
    let (status, headers, body) = call(&app, "POST", "/api/v1/batch/price", Some(batch)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["errors"].as_array().unwrap().len(), 1, "{body}");
    assert_eq!(body["errors"][0]["instrument_id"], "BOND-001");
    assert!(
        body["errors"][0]["error"]
            .as_str()
            .unwrap()
            .contains("MISSING_INPUT"),
        "{body}"
    );
    assert_eq!(body["quotes"].as_array().unwrap().len(), 1, "{body}");
    assert_eq!(body["quotes"][0]["instrument_id"], "CALL-001");
    assert!(body["quotes"][0]["oas_mid"].is_number(), "{body}");
    assert_eq!(headers["x-convex-curve-versions"], "USD-FIXTURE@7");
    assert_eq!(body["context"]["curves"], json!(["USD-FIXTURE@7"]));
    engine.shutdown().await.unwrap();
}

/// A batch of a bullet, a callable and a note reads three curve sets, each once,
/// and lists every curve it read once, in first-use order: the bullet's discount and
/// swap curves, then the note's projection curve.
#[tokio::test]
async fn a_batch_lists_each_curve_it_read_once() {
    let (setup, callable, note) = products_setup().await;
    let (engine, known_at) = started(&setup).await;
    activate_with_swap(&setup, DataQuality::Valid).await;
    let app = router(engine.clone());
    let batch = json!({
        "bonds": [
            {"bond": fixture::bond(), "market_price": "99.25"},
            {"bond": callable, "market_price": "99.25"},
            {"bond": note, "market_price": "99.25"},
        ],
        "settlement_date": SETTLEMENT,
        "known_at": known_at,
    });
    let (status, headers, body) = call(&app, "POST", "/api/v1/batch/price", Some(batch)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["errors"], json!([]), "{body}");
    assert_eq!(body["quotes"].as_array().unwrap().len(), 3);
    assert!(body["quotes"][0]["i_spread_mid"].is_number(), "{body}");
    let read = ["USD-FIXTURE@7", "USD-SWAP@7", "USD-SOFR-PROJECTION@7"];
    assert_eq!(headers["x-convex-curve-versions"], read.join(",").as_str());
    assert_eq!(body["context"]["curves"], json!(read));
    // Each item's own snapshot lists the curves that product read.
    assert_eq!(
        body["quotes"][1]["snapshot"]["curves"],
        json!(["USD-FIXTURE@7"])
    );
    assert_eq!(
        body["quotes"][2]["snapshot"]["curves"],
        json!(["USD-FIXTURE@7", "USD-SOFR-PROJECTION@7"])
    );
    engine.shutdown().await.unwrap();
}
