//! CVX-017 supplied REST contracts and actual native/C ABI parity fixtures.
use super::*;

fn supplied_quote_fixture(case: &Value) -> (Arc<convex_engine::PricingEngine>, BondReferenceData) {
    use convex_core::contracts::{BusinessDate, ExactDecimal, ReferenceDataId};
    use convex_ports::reference_master::{BondCalendar, BondStub};
    let id = case["id"].as_str().unwrap();
    let mut bond = production_readiness_bond(id);
    bond.issue_date = Date::parse(case["issue"].as_str().unwrap()).unwrap();
    bond.maturity_date = Date::parse(case["maturity"].as_str().unwrap()).unwrap();
    bond.first_coupon_date = case["terms"]["first_coupon_date"]
        .as_str()
        .map(|s| Date::parse(s).unwrap());
    bond.day_count = serde_json::from_value::<convex_core::daycounts::DayCountConvention>(
        case["day_count"].clone(),
    )
    .unwrap()
    .to_string();
    bond.frequency = case["quote_frequency_count"].as_u64().unwrap() as u32;
    bond.currency = serde_json::from_value(case["currency"].clone()).unwrap();
    bond.face_value = case["face_value"].as_str().unwrap().parse().unwrap();
    bond.coupon_rate = Some(case["coupon_rate"].as_str().unwrap().parse().unwrap());
    let mut terms = reference_terms::terms(&bond);
    terms.dated_date = BusinessDate::parse(case["terms"]["dated_date"].as_str().unwrap()).unwrap();
    terms.redemption_value =
        ExactDecimal::parse(case["terms"]["redemption_value"].as_str().unwrap()).unwrap();
    terms.settings.end_of_month = case["terms"]["end_of_month"].as_bool().unwrap();
    terms.settings.business_day_convention =
        serde_json::from_value(case["business_day_convention"].clone()).unwrap();
    terms.settings.settlement_days = 0;
    terms.calendar.id = ReferenceDataId::new(format!("supplied-calendar-{id}")).unwrap();
    terms.calendar_code = if case["terms"]["calendar"] == "SIFMA" {
        BondCalendar::Sifma
    } else {
        BondCalendar::WeekendsOnly
    };
    terms.convention.id = ReferenceDataId::new(format!("supplied-convention-{id}")).unwrap();
    terms.stub = if bond.first_coupon_date.is_some() {
        BondStub::ShortFirst
    } else {
        BondStub::None
    };
    bond.contractual_terms = Some(terms);
    // The existing engine fixture owns the common issuer; add only these exact
    // calendar/convention revisions, so no guessed convention reaches pricing.
    let storage = convex_ext_redb::create_test_storage().unwrap();
    let mut resources = reference_terms::resources(&bond);
    resources.remove(0);
    use futures_util::FutureExt;
    storage
        .bonds
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new(format!("supplied-{id}")).unwrap(),
                changes: vec![],
                master_changes: resources,
            },
            UtcTimestamp::parse("2000-01-01T00:00:00Z").unwrap(),
        )
        .now_or_never()
        .unwrap()
        .unwrap();
    (create_test_engine_with_storage(storage), bond)
}

fn supplied_curve(currency: Currency, rate: f64) -> Value {
    json!({"currency":currency,"curve":{"curve_id":format!("SUPPLIED-017-{rate}"),"version":7,"reference_date":"2024-01-15","source_time":"2024-01-15T00:00:00Z","recorded_at":"2024-01-15T00:00:00Z","source":"supplied-contract","model_id":"linear-continuous-zero","input_versions":[],"quality":"Indicative","max_age_ms":86400000,"interpolation":"linear","extrapolation":"flat_zero","points":[{"tenor_years":0.01,"continuous_zero":rate},{"tenor_years":1.0,"continuous_zero":rate},{"tenor_years":5.0,"continuous_zero":rate},{"tenor_years":10.0,"continuous_zero":rate}]}})
}

#[tokio::test]
async fn cvx017_rest_matches_supplied_native_and_c_abi_quote_goldens() {
    let pack: Value = serde_json::from_str(include_str!(
        "../../../convex-analytics/tests/fixtures/bond-quotes.json"
    ))
    .unwrap();
    let selected = [
        "coupon-date-negative-yield",
        "leap-eom-icma-non100-face",
        "leap-act365f",
        "short-first-icma",
        "weekend-payment-adjustment-30us",
        "one-day-negative-yield",
        "annual-redemption-premium",
        "sifma-holiday-payment",
    ];
    for case in pack["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| selected.contains(&c["id"].as_str().unwrap()))
    {
        let (engine, bond) = supplied_quote_fixture(case);
        let (status,response)=post_json(create_router(engine),"/api/v1/quote",json!({"bond":bond,"settlement_date":case["settlement"],"market_price":case["expected"]["clean_price"]})).await;
        assert_eq!(status, StatusCode::OK, "{} {response}", case["id"]);
        for (field, expected) in [
            ("clean_price_mid", "clean_price"),
            ("dirty_price_mid", "dirty_price"),
            ("accrued_interest", "accrued"),
            ("ytm_mid", "ytm_decimal"),
            ("modified_duration", "modified_duration"),
            ("macaulay_duration", "macaulay_duration"),
            ("convexity", "convexity"),
            ("dv01", "dv01"),
        ] {
            let actual = response[field]
                .as_f64()
                .unwrap_or_else(|| panic!("{field}: {response}"));
            let expected: f64 = case["expected"][expected]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
            assert!(
                (actual - expected).abs() <= 1e-9 + 1e-11 * expected.abs(),
                "{field}: {actual} != {expected}"
            );
        }
        let errors = response["measure_errors"].as_array().unwrap();
        assert_eq!(errors.len(), 5);
        assert!(errors.iter().all(|e| e["code"] == "MISSING_MARKET_DATA"));
        for field in [
            "z_spread_mid",
            "i_spread_mid",
            "g_spread_mid",
            "asw_mid",
            "pv01",
            "cs01",
            "zero_pv01_signed",
            "z_pv01_signed",
        ] {
            assert!(response[field].is_null(), "{field}: {response}");
        }
        assert!(response.get("curve_versions").is_none());
    }
}

#[tokio::test]
async fn cvx017_rest_curve_roles_currency_policy_and_missing_inputs_are_explicit() {
    use convex_analytics::bond_quote::{quote_fixed, BondCurves};
    use convex_bonds::instruments::FixedRateBond;
    use convex_core::types::{Frequency, Mark, PriceKind};
    use convex_engine::curve_builder::BuiltCurve;
    let case = json!({"id":"rest-explicit-roles","issue":"2024-01-15","maturity":"2029-01-15","day_count":"Thirty360US","quote_frequency_count":2,"currency":"USD","face_value":"100","coupon_rate":"0.05","business_day_convention":"Unadjusted","terms":{"dated_date":"2024-01-15","redemption_value":"100","end_of_month":false}});
    let (engine, bond) = supplied_quote_fixture(&case);
    let app = create_router(engine);
    let settle = Date::parse("2025-02-17").unwrap();
    let discount = supplied_curve(Currency::USD, 0.035);
    let swap = supplied_curve(Currency::USD, 0.045);
    let request = json!({"bond":bond,"settlement_date":settle,"market_price":"100","discount_curve":discount,"swap_curve":swap,"volatility":null});
    let (status, response) = post_json(app.clone(), "/api/v1/quote", request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let native_bond = FixedRateBond::builder()
        .cusip_unchecked("REST-SAME-TERMS")
        .issue_date(bond.issue_date)
        .dated_date(bond.issue_date)
        .maturity(bond.maturity_date)
        .coupon_rate(dec!(0.05))
        .frequency(Frequency::SemiAnnual)
        .day_count(convex_core::daycounts::DayCountConvention::Thirty360US)
        .currency(Currency::USD)
        .face_value(dec!(100))
        .redemption_value(dec!(100))
        .calendar(convex_bonds::types::CalendarId::weekend_only())
        .business_day_convention(convex_core::calendars::BusinessDayConvention::Unadjusted)
        .settlement_days(0)
        .end_of_month(false)
        .build()
        .unwrap();
    let d = BuiltCurve::from_snapshot(&serde_json::from_value(discount["curve"].clone()).unwrap())
        .unwrap();
    let s =
        BuiltCurve::from_snapshot(&serde_json::from_value(swap["curve"].clone()).unwrap()).unwrap();
    let native = quote_fixed(
        &native_bond,
        settle,
        &Mark::Price {
            value: dec!(100),
            kind: PriceKind::Clean,
        },
        Frequency::SemiAnnual,
        BondCurves {
            discount: Some(&d),
            swap: Some(&s),
            government: None,
        },
        10.0,
        convex_analytics::risk::STANDARD_KEY_RATE_TENORS,
    )
    .unwrap();
    let risk = native.curve_risk.unwrap();
    for (field, expected) in [
        ("z_spread_mid", native.z_spread_bps.unwrap()),
        ("i_spread_mid", native.i_spread_bps.unwrap()),
        ("asw_mid", native.asw_bps.unwrap()),
        ("zero_pv01_signed", risk.zero_pv01_signed),
        ("z_pv01_signed", risk.z_pv01_signed),
        ("pv01", -risk.zero_pv01_signed),
        ("cs01", -risk.z_pv01_signed),
    ] {
        assert!(
            (response[field].as_f64().unwrap() - expected).abs() < 1e-9,
            "{field}: {response}"
        );
    }
    assert_eq!(response["measure_errors"].as_array().unwrap().len(), 1);
    assert_eq!(response["curve_versions"][0]["version"], 7);
    assert_eq!(response["curve_versions"][0]["role"], "discount");
    assert_eq!(response["curve_versions"][1]["role"], "swap");
    for (pointer, value) in [
        ("/discount_curve/currency", json!("EUR")),
        ("/discount_curve/curve/reference_date", json!("2026-01-01")),
        ("/discount_curve/curve/version", json!(0)),
        ("/discount_curve/curve/quality", json!("Stale")),
        ("/discount_curve/curve/quality", json!("Invalid")),
        ("/discount_curve/curve/max_age_ms", json!(0)),
        (
            "/discount_curve/curve/recorded_at",
            json!("2024-01-17T00:00:00Z"),
        ),
        ("/discount_curve/curve/points/1/tenor_years", json!(0.01)),
        (
            "/discount_curve/curve/source_time",
            json!("2026-01-01T00:00:00Z"),
        ),
        ("/market_price", Value::Null),
        ("/volatility", json!(0.01)),
    ] {
        let mut bad = request.clone();
        *bad.pointer_mut(pointer)
            .unwrap_or_else(|| panic!("{pointer}")) = value;
        let (status, result) = post_json(app.clone(), "/api/v1/quote", bad).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{pointer}: {result}"
        );
        assert!(!result["error"].is_null());
    }
    let mut bad = request.clone();
    bad["swap_curve"]["curve"]["curve_id"] = bad["discount_curve"]["curve"]["curve_id"].clone();
    let (status, result) = post_json(app.clone(), "/api/v1/quote", bad).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{result}");
    assert!(result["error"].as_str().unwrap().contains("conflicting"));
    use convex_analytics::spreads::{GovernmentBenchmark, Sovereign};
    use convex_bonds::types::Tenor;
    let benchmark = GovernmentBenchmark::with_cusip_unchecked(
        Sovereign::UST,
        Tenor::Y5,
        "SUPPLIED-REST-017",
        bond.maturity_date,
        dec!(0.04),
        convex_core::types::Yield::new(dec!(0.04), convex_core::types::Compounding::SemiAnnual),
    );
    let mut actual = request.clone();
    actual["government_curve"] =
        json!({"reference_date":settle,"sovereign":"UST","benchmarks":[benchmark]});
    let (status, result) = post_json(app.clone(), "/api/v1/quote", actual.clone()).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert!(result.get("measure_errors").is_none(), "{result}");
    let expected = (native.pricing.ytm_decimal - 0.04) * 10000.0;
    assert!((result["g_spread_mid"].as_f64().unwrap() - expected.round()).abs() < 1e-9);
    for (gov, expected_status) in [
        (Value::Null, StatusCode::OK),
        (
            json!({"reference_date":settle,"sovereign":"UST","benchmarks":[]}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let mut bad = actual.clone();
        bad["government_curve"] = gov;
        let (status, result) = post_json(app.clone(), "/api/v1/quote", bad).await;
        assert_eq!(status, expected_status, "{result}");
        if status == StatusCode::OK {
            assert!(result["g_spread_mid"].is_null());
            assert_eq!(result["measure_errors"].as_array().unwrap().len(), 1);
        }
    }
    let mut bad = request;
    bad["bond"]["bond_type"] = json!(BondType::FloatingRate);
    let (status, result) = post_json(app, "/api/v1/quote", bad).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{result}");
    assert!(!result["error"].is_null());
}

#[tokio::test]
async fn cvx017_rest_zero_consumes_explicit_day_count_compounding_and_redemption() {
    use convex_analytics::bond_quote::quote_zero;
    use convex_bonds::instruments::{Compounding, ZeroCouponBond};
    use convex_core::types::{Mark, PriceKind};
    let case = json!({"id":"rest-zero-017","issue":"2024-01-15","maturity":"2029-01-15","day_count":"Act360","quote_frequency_count":2,"currency":"EUR","face_value":"1000","coupon_rate":"0","business_day_convention":"Unadjusted","terms":{"dated_date":"2024-01-15","redemption_value":"1050","end_of_month":false}});
    let (engine, mut bond) = supplied_quote_fixture(&case);
    bond.bond_type = BondType::ZeroCoupon;
    bond.coupon_rate = None;
    let settle = Date::parse("2025-01-15").unwrap();
    let native = ZeroCouponBond::builder()
        .cusip_unchecked("REST-ZERO")
        .issue_date(bond.issue_date)
        .maturity(bond.maturity_date)
        .currency(Currency::EUR)
        .day_count(convex_core::daycounts::DayCountConvention::Act360)
        .face_value(dec!(1000))
        .redemption_value(dec!(1050))
        .compounding(Compounding::SemiAnnual)
        .build()
        .unwrap();
    let (p, r) = quote_zero(
        &native,
        settle,
        &Mark::Price {
            value: dec!(99),
            kind: PriceKind::Clean,
        },
    )
    .unwrap();
    let (status, response) = post_json(
        create_router(engine),
        "/api/v1/quote",
        json!({"bond":bond,"settlement_date":settle,"market_price":"99"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    for (field, value) in [
        ("clean_price_mid", p.clean_price_per_100),
        ("accrued_interest", p.accrued_per_100),
        ("ytm_mid", p.ytm_decimal),
        ("modified_duration", r.modified_duration),
        ("convexity", r.convexity),
        ("dv01", r.dv01_loss),
    ] {
        assert!(
            (response[field].as_f64().unwrap() - value).abs() < 1e-10,
            "{field}: {response}"
        );
    }
    assert!(response["pv01"].is_null());
    assert!(response["cs01"].is_null());
    assert!(response["measure_errors"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["code"] == "UNSUPPORTED_CONVENTION"));
}
