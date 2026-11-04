//! Exact financial amounts in risk-profile and hedge-report wire contracts.
use convex_analytics::risk::hedging::types::{
    BondFuture, CashBondLeg, ComparisonReport, ComparisonRow, HedgeProposal, InterestRateSwap,
};
use convex_analytics::risk::RiskProfile;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};

fn check<T: DeserializeOwned + Serialize>(mut input: Value, field: &str) {
    const EXACT: &str = "1234567.1234567890123456789012";
    input[field] = json!(EXACT);
    let decoded: T = serde_json::from_value(input.clone()).unwrap();
    let serialized = serde_json::to_value(decoded).unwrap();
    assert_eq!(serialized[field], EXACT);
    let second: T = serde_json::from_value(serialized.clone()).unwrap();
    assert_eq!(serde_json::to_value(second).unwrap()[field], EXACT);
    for invalid in [
        "1e-29",
        "1234567.12345678901234567890123",
        "79228162514264337593543950336",
    ] {
        let mut rejected = input.clone();
        rejected[field] = json!(invalid);
        assert!(
            serde_json::from_value::<T>(rejected).is_err(),
            "{field}: {invalid}"
        );
    }
    input["unconsumed_recovery"] = json!(0.4);
    assert!(serde_json::from_value::<T>(input).is_err());
    println!("{}", json!({"field":field,"serialized":serialized}));
}

#[test]
fn risk_profile_amounts_retain_exact_digits_and_reject_nonrepresentable_input() {
    let input = json!({"currency":"USD","settlement":"2025-04-15","notional_face":"1000000","clean_price_per_100":100,"dirty_price_per_100":101.25,"accrued_per_100":1.25,"market_value":"1012500","ytm_decimal":0.05,"modified_duration_years":7,"macaulay_duration_years":7.175,"convexity":50,"dv01":700});
    check::<RiskProfile>(input.clone(), "notional_face");
    check::<RiskProfile>(input, "market_value");
}

#[test]
fn hedge_and_comparison_amounts_share_the_exact_wire_owner() {
    check::<BondFuture>(
        json!({"contract_code":"SUPPLIED","underlying_tenor_years":10,"deliverable_basket":[],"delivery_months":3,"repo_rate_decimal":0.04,"contract_size_face":"100000","currency":"USD"}),
        "contract_size_face",
    );
    check::<CashBondLeg>(
        json!({"tenor_years":10,"coupon_rate_decimal":0.04,"currency":"USD","face_amount":"100000"}),
        "face_amount",
    );
    check::<InterestRateSwap>(
        json!({"tenor_years":10,"fixed_rate_decimal":0.04,"fixed_frequency":"SemiAnnual","fixed_day_count":"Thirty360US","floating_index":"SOFR","side":"pay_fixed","notional":"100000","currency":"USD"}),
        "notional",
    );
    check::<HedgeProposal>(
        json!({"strategy":"supplied","trades":[],"residual":{"residual_dv01":0,"residual_krd_l1_norm":0},"cost_bps":1,"cost_total":"100"}),
        "cost_total",
    );
    check::<ComparisonRow>(
        json!({"strategy":"supplied","hedge_dv01":0,"residual_dv01":0,"residual_krd_l1_norm":0,"cost_bps":1,"cost_total":"100","cost_source":"supplied"}),
        "cost_total",
    );
    check::<ComparisonReport>(
        json!({"currency":"USD","position_market_value":"1000000","position_dv01":700,"rows":[],"recommendation":{"strategy":"supplied","row_index":0,"reasons":[]}}),
        "position_market_value",
    );
}
