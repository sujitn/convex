//! Solve-for-price entrypoints: price_from_yield, price_from_spread, price_from_g_spread, price_from_benchmark_spread.

use wasm_bindgen::prelude::*;

use convex_analytics::spreads::ZSpreadCalculator;
use convex_bonds::pricing::{StandardYieldEngine, YieldEngine};
use convex_bonds::traits::Bond;

use crate::bond::{create_bond, create_curve, create_discount_curve, get_yield_rules};
use crate::convert::{decimal_to_f64, parse_date, parse_tenor_to_years};
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

/// Calculate clean price from target G-spread.
///
/// Given a target G-spread (in basis points), calculates the clean price.
/// G-spread = YTM - interpolated benchmark rate at maturity.
#[wasm_bindgen]
pub fn price_from_g_spread(
    params: JsValue,
    target_g_spread_bps: f64,
    curve_points: JsValue,
) -> JsValue {
    let result = price_from_g_spread_impl(params, target_g_spread_bps, curve_points);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn price_from_g_spread_impl(
    params: JsValue,
    target_g_spread_bps: f64,
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

    let curve = match create_curve(settlement, &points) {
        Ok(c) => c,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    let maturity = match bond.maturity() {
        Some(m) => m,
        None => {
            return PriceFromYieldResult {
                error: Some("Bond has no maturity date".to_string()),
                ..Default::default()
            }
        }
    };

    let benchmark_rate = match curve.zero_rate(maturity, convex_curves::Compounding::SemiAnnual) {
        Ok(r) => r,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(format!("Failed to get benchmark rate: {:?}", e)),
                ..Default::default()
            }
        }
    };

    // YTM = G-spread + benchmark_rate. G-spread is in bps, benchmark_rate is decimal.
    let target_ytm = (target_g_spread_bps / 100.0) + (benchmark_rate * 100.0);

    let cash_flows = bond.cash_flows(settlement);
    let accrued = bond.accrued_interest(settlement);

    // MUST use the same rules as analyze_bond.
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

/// Calculate clean price from target benchmark spread.
///
/// Given a target benchmark spread (in basis points), calculates the clean price.
/// Benchmark spread = YTM - nearest on-the-run tenor rate.
#[wasm_bindgen]
pub fn price_from_benchmark_spread(
    params: JsValue,
    target_benchmark_spread_bps: f64,
    benchmark_tenor: String,
    curve_points: JsValue,
) -> JsValue {
    let result = price_from_benchmark_spread_impl(
        params,
        target_benchmark_spread_bps,
        benchmark_tenor,
        curve_points,
    );
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn price_from_benchmark_spread_impl(
    params: JsValue,
    target_benchmark_spread_bps: f64,
    benchmark_tenor: String,
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

    let curve = match create_curve(settlement, &points) {
        Ok(c) => c,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    let tenor_years = match parse_tenor_to_years(&benchmark_tenor) {
        Ok(t) => t,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    let benchmark_days = (tenor_years * 365.25) as i64;
    let benchmark_date = settlement.add_days(benchmark_days);

    let benchmark_rate =
        match curve.zero_rate(benchmark_date, convex_curves::Compounding::SemiAnnual) {
            Ok(r) => r,
            Err(e) => {
                return PriceFromYieldResult {
                    error: Some(format!("Failed to get benchmark rate: {:?}", e)),
                    ..Default::default()
                }
            }
        };

    // YTM = benchmark_spread + benchmark_tenor_rate.
    let target_ytm = (target_benchmark_spread_bps / 100.0) + (benchmark_rate * 100.0);

    let cash_flows = bond.cash_flows(settlement);
    let accrued = bond.accrued_interest(settlement);

    // MUST use the same rules as analyze_bond.
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
