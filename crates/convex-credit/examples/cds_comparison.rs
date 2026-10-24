//! Actual native outputs for all original grid inputs. No expected results read.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[path = "../tests/common/mod.rs"]
mod common;
use convex_credit::{convert_cds, CdsQuote, ConversionSettings, InitialPaymentState};
use serde_json::json;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let inputs = common::inputs()?;
    let curves = common::market(&inputs)?;
    let calendar = common::calendar(&inputs);
    let mut values = Vec::with_capacity(inputs.cases.len());
    for case in &inputs.cases {
        let t = common::terms(&inputs, case)?;
        let contract = common::build(&t, &calendar)?;
        let context = t.trade_context(InitialPaymentState::NotContracted);
        let conversion = convert_cds(
            &contract,
            &context,
            &curves,
            CdsQuote::RunningSpread {
                decimal_rate: case.spread_bps.parse::<f64>()? / 10000.0,
            },
            ConversionSettings::default(),
        )?;
        let v = conversion.valuation;
        values.push(json!({"row":case.row,"trade_date":t.trade_date,"maturity_date":t.premium_schedule.end_date,"currency":case.currency,"coupon_bps":case.coupon_bps,"spread_bps":case.spread_bps,"recovery":case.recovery,"clean_upfront_fraction":v.clean_upfront_fraction,"dirty_upfront_fraction":v.dirty_upfront_fraction,"cash_settlement_amount":v.cash_settlement_amount,"clean_price_per100":v.clean_price_per100,"accrued_days":v.accrued_days,"accrued_premium":v.accrued_premium,"cash_settlement_date":v.cash_settlement_date,"step_in_date":v.step_in_date,"hazard_per_act365f_year":conversion.hazard_per_act365f_year,"native_quote_residual":conversion.native_quote_residual,"iterations":conversion.iterations}));
    }
    println!(
        "{}",
        json!({"schema_version":1,"engine":"Convex","model_version":"1.8.3","inputs_sha256":"3d850ab14c7bf2e8c7aea98f16129e7a2a24ee3bc2b8ebce3458ee20f40704d2","notice":convex_credit::STANDARD_MODEL_NOTICE,"cases":values})
    );
    Ok(())
}
