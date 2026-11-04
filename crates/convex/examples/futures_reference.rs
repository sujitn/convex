//! Standalone native consumer: archived actual facts, exact units, explicit eligibility.
use convex::contracts::{BusinessDate, ExactDecimal, UtcTimestamp};
use convex::futures_reference::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let inputs: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/futures/cme-2026-09-24.json"
    ))?;
    let bond: FuturesContract = serde_json::from_value(inputs["contract"].clone())?;
    let basket: DeliveryBasket = serde_json::from_value(inputs["basket"].clone())?;
    let rate: FuturesContract = serde_json::from_value(inputs["rate_contract"].clone())?;
    bond.validate()?;
    basket.validate_contract(&bond)?;
    rate.validate()?;
    let context = EligibilityContext {
        as_of: UtcTimestamp::parse("2026-10-07T12:00:00Z")?,
        known_at: UtcTimestamp::parse("2026-10-07T12:00:00Z")?,
    };
    let eligible = assess_futures_reference(&bond, Some(&basket), context)?;
    if eligible.eligible
        || !eligible
            .issues
            .iter()
            .any(|i| i.reason == HedgeInputBlock::Historical)
    {
        return Err("archived source must remain non-executable".into());
    }
    let invoice = bond_delivery_invoice(
        &bond,
        &basket,
        &basket.bonds[0].instrument,
        BusinessDate::parse("2026-12-01")?,
        ExactDecimal::parse("100")?,
        ExactDecimal::parse("123.45")?,
    )?;
    if invoice != ExactDecimal::parse("90813.45")?
        || rate.rate_from_price(ExactDecimal::parse("96.155")?)? != ExactDecimal::parse("0.03845")?
        || rate
            .calibration_rate(ExactDecimal::parse("96.155")?)
            .is_ok()
    {
        return Err("native invoice/rate/adjustment contract failed".into());
    }
    println!(
        "{}",
        serde_json::json!({"status":"passed","contract":bond.contract_code,
        "eligible_bonds":basket.bonds.len(),"invoice_currency":invoice,
        "reference_eligibility":eligible,"certified":false})
    );
    Ok(())
}
