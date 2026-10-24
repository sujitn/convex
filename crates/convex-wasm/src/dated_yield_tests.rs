//! Direct native wrapper checks against independently declared cash references.
use crate::{
    bond::{calculate_dated_yield_quote, create_price_bond, get_yield_rules},
    dto::BondParams,
};
use serde_json::Value;

fn params() -> BondParams {
    serde_json::from_value(
        serde_json::json!({"coupon_rate":5.0,"maturity_date":"2028-06-15",
        "issue_date":"2026-06-15","settlement_date":"2026-06-15","face_value":100.0,
        "frequency":1,"day_count":"30/360","currency":"USD"}),
    )
    .unwrap()
}

#[test]
fn quoted_yield_and_all_risk_share_the_independent_dated_basis() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../convex-bonds/tests/fixtures/dated-cash-risk.json"
    ))
    .unwrap();
    for row in fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["compounding"] != "Daily")
    {
        let mut p = params();
        p.compounding = Some(row["compounding"].as_str().unwrap().into());
        let price = row["present_value"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        let rate = row["rate"].as_str().unwrap().parse::<f64>().unwrap();
        let bond = create_price_bond(&p).unwrap();
        let rules = get_yield_rules(&p).unwrap();
        let (actual, risk) = calculate_dated_yield_quote(
            &bond,
            crate::convert::parse_date(&p.settlement_date).unwrap(),
            price,
            &rules,
        )
        .unwrap();
        assert!((actual - rate).abs() <= 1e-10, "{} rate", row["id"]);
        assert!(
            (risk.present_value - price).abs() <= 1e-8,
            "{} repricing",
            row["id"]
        );
        for (field, actual) in [
            ("macaulay_duration", risk.macaulay_duration),
            ("modified_duration", risk.modified_duration),
            ("convexity", risk.convexity),
            ("dv01_loss", risk.dv01_loss),
        ] {
            let expected = row[field].as_str().unwrap().parse::<f64>().unwrap();
            assert!((actual - expected).abs() <= 1e-8, "{} {field}", row["id"]);
        }
    }
}

#[test]
fn context_free_adapter_rejects_reference_entitlement_and_bill_substitutes() {
    for update in [
        serde_json::json!({"day_count":"ACT/ACT ICMA"}),
        serde_json::json!({"market":"UK","instrument_type":"GovernmentBond"}),
        serde_json::json!({"compounding":"Discount"}),
        serde_json::json!({"ex_dividend_days":7}),
    ] {
        let mut source = serde_json::to_value(params()).unwrap();
        for (k, v) in update.as_object().unwrap() {
            source[k] = v.clone();
        }
        let p: BondParams = serde_json::from_value(source).unwrap();
        let bond = create_price_bond(&p).unwrap();
        let rules = get_yield_rules(&p).unwrap();
        assert!(calculate_dated_yield_quote(
            &bond,
            crate::convert::parse_date(&p.settlement_date).unwrap(),
            100.0,
            &rules
        )
        .is_err());
    }
}
