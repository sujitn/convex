//! Public WASM analytics surface: analyze_bond, get_cash_flows, calculate_accrued, calculate_simple_metrics.

use wasm_bindgen::prelude::*;

use convex_analytics::spreads::OASCalculator;
use convex_analytics::yas::YASCalculator;
use convex_bonds::instruments::CallableBond;
use convex_bonds::traits::{Bond, EmbeddedOptionBond, FixedCouponBond};
use convex_bonds::types::{CallEntry, CallSchedule, CallType};

use crate::bond::{
    calculate_convention_yield, convert_yas_result, create_bond, create_curve,
    create_discount_curve, get_yield_rules,
};
use crate::convert::{date_to_naive, decimal_to_f64, f64_to_decimal, parse_date};
use crate::dto::{AnalysisResult, BondParams, CashFlowEntry, CurvePoint};

/// Calculate bond analytics given price and yield curve.
///
/// Takes bond parameters, a clean price, and curve points, returns comprehensive analytics.
#[wasm_bindgen]
pub fn analyze_bond(params: JsValue, clean_price: f64, curve_points: JsValue) -> JsValue {
    let result = analyze_bond_impl(params, clean_price, curve_points);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn analyze_bond_impl(params: JsValue, clean_price: f64, curve_points: JsValue) -> AnalysisResult {
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return AnalysisResult {
                error: Some(format!("Failed to parse bond parameters: {:?}", e)),
                ..Default::default()
            }
        }
    };

    let points: Vec<CurvePoint> = match serde_wasm_bindgen::from_value(curve_points) {
        Ok(p) => p,
        Err(e) => {
            return AnalysisResult {
                error: Some(format!("Failed to parse curve points: {:?}", e)),
                ..Default::default()
            }
        }
    };

    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(e) => {
            return AnalysisResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(e) => {
            return AnalysisResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    let curve = match create_curve(settlement, &points) {
        Ok(c) => c,
        Err(e) => {
            return AnalysisResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    let yield_rules = get_yield_rules(&bond_params);

    let calculator = YASCalculator::new(&curve);
    let settlement_naive = date_to_naive(settlement);

    let yas_result = match calculator.analyze(&bond, settlement_naive, f64_to_decimal(clean_price))
    {
        Ok(result) => result,
        Err(e) => {
            return AnalysisResult {
                error: Some(format!("Analysis failed: {:?}", e)),
                ..Default::default()
            }
        }
    };

    let mut result = convert_yas_result(&yas_result, &bond, settlement, &yield_rules, &bond_params);

    // Convention-aware YTM via StandardYieldEngine — same engine the bond was priced with.
    if let Some(convention_ytm) =
        calculate_convention_yield(&bond, settlement, clean_price, &yield_rules)
    {
        result.ytm = Some(convention_ytm * 100.0);
    }

    if let Some(ref call_entries) = bond_params.call_schedule {
        if !call_entries.is_empty() {
            // Parse all call dates up front: a single bad date should fail the whole call,
            // not silently flag the bond callable with a partial / empty schedule.
            let parsed: Result<Vec<(convex_core::types::Date, f64)>, String> = call_entries
                .iter()
                .map(|entry| parse_date(&entry.date).map(|d| (d, entry.price)))
                .collect();
            let parsed = match parsed {
                Ok(v) => v,
                Err(e) => {
                    return AnalysisResult {
                        error: Some(format!("Invalid call schedule entry: {}", e)),
                        ..Default::default()
                    }
                }
            };

            result.is_callable = Some(true);

            let mut call_schedule = CallSchedule::new(CallType::American);
            for (call_date, price) in parsed {
                call_schedule = call_schedule.with_entry(CallEntry::new(call_date, price));
            }

            let callable = CallableBond::new(bond.clone(), call_schedule);
            let price_decimal = f64_to_decimal(clean_price);

            if let Ok(ytc) = callable.yield_to_first_call(price_decimal, settlement) {
                result.ytc = Some(decimal_to_f64(ytc) * 100.0);
            }

            if let Ok((ytw, workout_date)) =
                callable.yield_to_worst_with_date(price_decimal, settlement)
            {
                result.ytw = Some(decimal_to_f64(ytw) * 100.0);
                result.workout_date = Some(format!("{}", workout_date));

                if let Some(maturity) = bond.maturity() {
                    if workout_date == maturity {
                        result.workout_price = Some(100.0);
                    } else if let Some(call_schedule) = callable.call_schedule() {
                        result.workout_price = call_schedule.call_price_on(workout_date);
                    }
                }
            }

            // OAS via Hull-White (default 1% vol if not provided).
            let volatility = bond_params.volatility.unwrap_or(1.0) / 100.0;
            let oas_calc = OASCalculator::default_hull_white(volatility);
            let accrued = decimal_to_f64(bond.accrued_interest(settlement));
            let dirty_price_f64 = clean_price + accrued;
            let dirty_price = f64_to_decimal(dirty_price_f64);

            match create_discount_curve(settlement, &points) {
                Ok(discount_curve) => {
                    match oas_calc.calculate(&callable, dirty_price, &discount_curve, settlement) {
                        Ok(oas) => {
                            result.oas = Some(decimal_to_f64(oas.as_bps()));

                            let oas_decimal = decimal_to_f64(oas.as_bps()) / 10000.0;
                            if let Ok(eff_dur) = oas_calc.effective_duration(
                                &callable,
                                &discount_curve,
                                oas_decimal,
                                settlement,
                            ) {
                                result.effective_duration = Some(eff_dur);
                            }
                            if let Ok(eff_conv) = oas_calc.effective_convexity(
                                &callable,
                                &discount_curve,
                                oas_decimal,
                                settlement,
                            ) {
                                result.effective_convexity = Some(eff_conv);
                            }
                            if let Ok(opt_val) = oas_calc.option_value(
                                &callable,
                                &discount_curve,
                                oas_decimal,
                                settlement,
                            ) {
                                result.option_value = Some(opt_val);
                            }
                        }
                        Err(_e) => {
                            // OAS calc failed (model price can't bracket market price);
                            // fall back to Z-spread so the UI gets a sensible spread.
                            result.oas = result.z_spread;
                        }
                    }
                }
                Err(_e) => {
                    result.oas = None;
                }
            }
        }
    } else {
        result.is_callable = Some(false);
    }

    result
}

/// Get bond cash flows.
///
/// Returns all future cash flows from settlement date.
#[wasm_bindgen]
pub fn get_cash_flows(params: JsValue) -> JsValue {
    let result = get_cash_flows_impl(params);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn get_cash_flows_impl(params: JsValue) -> Vec<CashFlowEntry> {
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(_) => return vec![],
    };

    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(_) => return vec![],
    };

    let face = decimal_to_f64(bond.face_value());
    bond.cash_flows(settlement)
        .iter()
        .map(|cf| {
            let amount = decimal_to_f64(cf.amount);
            // Classify against the bond's face value, not a hardcoded 100, so face_value != 100
            // (e.g. 1000 for institutional notional) doesn't misclassify maturities.
            let cf_type = if cf.is_principal() && amount >= face / 2.0 {
                if amount > face {
                    "coupon_and_principal"
                } else {
                    "principal"
                }
            } else {
                "coupon"
            };

            CashFlowEntry {
                date: format!("{}", cf.date),
                amount,
                cf_type: cf_type.to_string(),
            }
        })
        .collect()
}

/// Calculate accrued interest.
///
/// Returns an `AnalysisResult`-shaped object with `accrued_interest` populated on success
/// or `error` populated on failure — consistent with the other analytics entrypoints.
#[wasm_bindgen]
pub fn calculate_accrued(params: JsValue) -> JsValue {
    let result = match calculate_accrued_impl(params) {
        Ok(v) => AnalysisResult {
            accrued_interest: Some(v),
            ..Default::default()
        },
        Err(e) => AnalysisResult {
            error: Some(e),
            ..Default::default()
        },
    };
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn calculate_accrued_impl(params: JsValue) -> Result<f64, String> {
    let bond_params: BondParams = serde_wasm_bindgen::from_value(params)
        .map_err(|e| format!("Failed to parse bond parameters: {:?}", e))?;

    let bond = create_bond(&bond_params)?;

    let settlement = parse_date(&bond_params.settlement_date)?;

    let accrued = bond.accrued_interest(settlement);
    Ok(decimal_to_f64(accrued))
}

/// Simple yield calculation without curve (only basic metrics).
#[wasm_bindgen]
pub fn calculate_simple_metrics(params: JsValue, clean_price: f64) -> JsValue {
    let result = calculate_simple_metrics_impl(params, clean_price);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn calculate_simple_metrics_impl(params: JsValue, clean_price: f64) -> AnalysisResult {
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return AnalysisResult {
                error: Some(format!("Failed to parse bond parameters: {:?}", e)),
                ..Default::default()
            }
        }
    };

    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(e) => {
            return AnalysisResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(e) => {
            return AnalysisResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    let accrued = decimal_to_f64(bond.accrued_interest(settlement));
    let dirty_price = clean_price + accrued;

    let (days_to_mat, years_to_mat) = match bond.maturity() {
        Some(maturity) => {
            let days = settlement.days_between(&maturity);
            (days, days as f64 / 365.0)
        }
        None => (0, 0.0),
    };

    // Current yield = annual coupon / clean price.
    // coupon_rate() is decimal (0.05 for 5%), face_value() is per-100-face (100), so
    // their product is the annual coupon amount; the percent conversion happens in current_yield.
    let annual_coupon = decimal_to_f64(bond.coupon_rate()) * decimal_to_f64(bond.face_value());
    let current_yield = if clean_price > 0.0 {
        Some(annual_coupon / clean_price * 100.0)
    } else {
        None
    };

    AnalysisResult {
        clean_price: Some(clean_price),
        dirty_price: Some(dirty_price),
        accrued_interest: Some(accrued),
        current_yield,
        days_to_maturity: Some(days_to_mat),
        years_to_maturity: Some(years_to_mat),
        error: None,
        ..Default::default()
    }
}
