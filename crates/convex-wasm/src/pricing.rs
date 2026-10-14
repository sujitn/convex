//! Checked per100 price inverses with explicit numeric domains and native errors.
use crate::{
    bond::{create_discount_curve, create_price_bond, dated_yield_compounding, get_yield_rules},
    convert::{decimal_to_f64, finite, parse_date, percentage_to_decimal, positive},
    dto::PriceFromYieldResult,
    ingress::{decode_bond_params, decode_curve_points},
};
use convex_analytics::spreads::ZSpreadCalculator;
use convex_bonds::{pricing::dated_cash::dated_cash_yield_risk, traits::Bond};
use wasm_bindgen::prelude::*;

fn failure(error: String) -> PriceFromYieldResult {
    PriceFromYieldResult {
        error: Some(error),
        ..Default::default()
    }
}

/// Dirty/clean/accrued prices are per100 face; target YTM is percentage.
#[wasm_bindgen]
pub fn price_from_yield(params: JsValue, target_ytm: f64) -> JsValue {
    let result = price_from_yield_impl(params, target_ytm).unwrap_or_else(failure);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}
fn price_from_yield_impl(params: JsValue, target_ytm: f64) -> Result<PriceFromYieldResult, String> {
    let params = decode_bond_params(params)?;
    let bond = create_price_bond(&params)?;
    let settlement = parse_date(&params.settlement_date)?;
    let rules = get_yield_rules(&params)?;
    let yield_decimal = decimal_to_f64(percentage_to_decimal(target_ytm)?)?;
    let cash = bond.try_cash_flows(settlement).map_err(|e| e.to_string())?;
    let accrued = decimal_to_f64(bond.accrued_interest(settlement))?;
    let dirty = dated_cash_yield_risk(
        &cash,
        settlement,
        yield_decimal,
        rules.discount_day_count,
        dated_yield_compounding(&rules)?,
    )
    .map_err(|e| e.to_string())?
    .present_value;
    let clean = finite(dirty - accrued, "clean price")?;
    Ok(PriceFromYieldResult {
        clean_price: Some(clean),
        dirty_price: Some(dirty),
        accrued_interest: Some(accrued),
        error: None,
    })
}

/// Target Z-spread is bps; cash price outputs are per100 face.
#[wasm_bindgen]
pub fn price_from_spread(
    params: JsValue,
    target_spread_bps: f64,
    curve_points: JsValue,
) -> JsValue {
    let result =
        price_from_spread_impl(params, target_spread_bps, curve_points).unwrap_or_else(failure);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}
fn price_from_spread_impl(
    params: JsValue,
    target_spread_bps: f64,
    curve_points: JsValue,
) -> Result<PriceFromYieldResult, String> {
    let target = finite(target_spread_bps, "Z-spread")? / 10000.0;
    let params = decode_bond_params(params)?;
    let points = decode_curve_points(curve_points)?;
    let bond = create_price_bond(&params)?;
    let settlement = parse_date(&params.settlement_date)?;
    let curve = create_discount_curve(settlement, &points)?;
    let accrued = decimal_to_f64(bond.accrued_interest(settlement))?;
    let dirty = positive(
        ZSpreadCalculator::new(&curve)
            .try_price_with_spread(&bond, target, settlement)
            .map_err(|e| e.to_string())?,
        "dirty price",
    )?;
    Ok(PriceFromYieldResult {
        clean_price: Some(finite(dirty - accrued, "clean price")?),
        dirty_price: Some(dirty),
        accrued_interest: Some(accrued),
        error: None,
    })
}
