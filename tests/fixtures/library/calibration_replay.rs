#![allow(dead_code)]
#[path = "../packed.rs"]
#[allow(clippy::duplicate_mod)]
mod packed;
use convex::library::*;
use serde_json::{json, Value};

pub fn original_cases() -> Vec<Value> {
    serde_json::from_str::<Value>(packed::text(
        "tests/fixtures/library/calibration-replay.json",
    ))
    .unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
pub fn original_inputs(request: &Value) -> Value {
    let mut inputs = request["inputs"].clone();
    inputs["calibration"] = request["operation"]["input"].clone();
    inputs
}
/// Explicit new fictional full two-deposit graph for the actual runtime fixture.
pub fn runtime_inputs() -> Value {
    let curve = json!({"key":{"id":"USD-FIXTURE","version":"deposits.v1"},"role":{"kind":"Discount","collateral_currency":"USD"},"currency":"USD","reference_date":"2026-10-02","time_basis":"Act360","representation":{"ZeroRates":{"compounding":"Continuous"}},"interpolation":"Linear","extrapolation":"Flat","dependencies":[],"calibration":"Piecewise","source":{"Helpers":["deposit.1","deposit.2"]}});
    let helpers:Vec<Value> = [("deposit.1","2027-10-04",0.02),("deposit.2","2028-10-02",0.03)].into_iter().map(|(id,date,rate)| json!({"id":id,"pillar":date,"quote":{"unit":"DecimalRate","value":rate},"tolerance":1e-12,"terms":{"kind":"money_market","curve":curve["key"],"calendar":{"id":"actual-calendar","version":17},"terms":{"kind":"Deposit","start_date":"2026-10-02","maturity_date":date,"day_count":"Act360"}}})).collect();
    json!({"calendars":[{"pin":{"id":"actual-calendar","version":17},"holidays":[],"weekend_days":[5,6]}],"helper_calendars":[],"curves":[],"fixings":[],"publications":[],"calibration":{"rates":[{"recipe":{"curve":curve,"pillars":["2027-10-04","2028-10-02"],"helpers":helpers,"bootstrap":{"lower_zero":-0.08,"upper_zero":0.15,"max_iterations":100},"overnight_projection":null,"overlap_policy":"Reject"},"quote_versions":[21,22],"futures_calendar_stamps":[null,null]}],"credit":[],"cash":[],"rate_context":{"as_of":"2026-10-02T15:00:00Z","cashflows":{"fixings":{"valuation_date":"2026-10-02","known_at":"2026-10-02T15:00:00Z","unpublished":"Forecast","final_only":true},"on_payment":"Include"}},"controls":{"jacobian_step":1e-6,"minimum_singular_ratio":1e-10,"svd_iterations":1000}}})
}
pub fn pure(inputs: &Value) -> LibraryResult<CalibrationReplay> {
    CalibrationReplay::build(&decode_calibration_inputs(inputs)?)
}
