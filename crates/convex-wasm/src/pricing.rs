//! Solve-for-price entrypoints: price_from_yield and price_from_spread.

use wasm_bindgen::prelude::*;

use convex_analytics::spreads::ZSpreadCalculator;
use convex_bonds::pricing::{StandardYieldEngine, YieldEngine};
use convex_bonds::traits::Bond;

use crate::bond::{create_bond, create_discount_curve, get_yield_rules};
use crate::convert::{decimal_to_f64, parse_date};
use crate::dto::{BondParams, CurvePoint, PriceFromYieldResult};

/// Calculate clean price from target yield.
///
/// Given a target YTM, calculates the clean price that would produce that yield.
#[wasm_bindgen]
pub fn price_from_yield(params: JsValue, target_ytm: f64, curve_points: JsValue) -> JsValue {
    let result = price_from_yield_impl(params, target_ytm, curve_points);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn price_from_yield_impl(
    params: JsValue,
    target_ytm: f64,
    _curve_points: JsValue,
) -> PriceFromYieldResult {
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(format!("Failed to parse bond parameters: {:?}", e)),
                ..Default::default()
            }
        }
    };

    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    let cash_flows = bond.cash_flows(settlement);
    let accrued = bond.accrued_interest(settlement);

    // MUST use the same rules as analyze_bond to keep YTM round-trip consistent.
    let yield_rules = get_yield_rules(&bond_params);

    let yield_decimal = target_ytm / 100.0;

    let engine = StandardYieldEngine::default();
    let dirty_price = engine.price_from_yield(&cash_flows, yield_decimal, settlement, &yield_rules);
    let clean_price = dirty_price - decimal_to_f64(accrued);

    PriceFromYieldResult {
        clean_price: Some(clean_price),
        dirty_price: Some(dirty_price),
        accrued_interest: Some(decimal_to_f64(accrued)),
        error: None,
    }
}

/// Calculate clean price from target Z-spread.
///
/// Given a target Z-spread (in basis points), calculates the clean price.
#[wasm_bindgen]
pub fn price_from_spread(
    params: JsValue,
    target_spread_bps: f64,
    curve_points: JsValue,
) -> JsValue {
    let result = price_from_spread_impl(params, target_spread_bps, curve_points);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn price_from_spread_impl(
    params: JsValue,
    target_spread_bps: f64,
    curve_points: JsValue,
) -> PriceFromYieldResult {
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(format!("Failed to parse bond parameters: {:?}", e)),
                ..Default::default()
            }
        }
    };

    let points: Vec<CurvePoint> = match serde_wasm_bindgen::from_value(curve_points) {
        Ok(p) => p,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(format!("Failed to parse curve points: {:?}", e)),
                ..Default::default()
            }
        }
    };

    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    let curve = match create_discount_curve(settlement, &points) {
        Ok(c) => c,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    let accrued = bond.accrued_interest(settlement);

    let spread_decimal = target_spread_bps / 10000.0;

    let calculator = ZSpreadCalculator::new(&curve);
    let dirty_price = calculator.price_with_spread(&bond, spread_decimal, settlement);
    let clean_price = dirty_price - decimal_to_f64(accrued);

    PriceFromYieldResult {
        clean_price: Some(clean_price),
        dirty_price: Some(dirty_price),
        accrued_interest: Some(decimal_to_f64(accrued)),
        error: None,
    }
}
