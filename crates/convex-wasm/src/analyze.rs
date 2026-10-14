//! Checked fixed bond conveniences; prices/risk/accrued are per100, cashflows absolute.
use crate::bond::{
    calculate_convention_yield, convert_yas_result, create_bond, create_curve, create_price_bond,
    get_yield_rules,
};
use crate::convert::{date_to_naive, decimal_to_f64, f64_to_decimal, finite, parse_date, positive};
use crate::dto::{AnalysisResult, CashFlowEntry};
use crate::ingress::{decode_bond_params, decode_curve_points};
use convex_analytics::yas::YASCalculator;
use convex_bonds::traits::{Bond, BondCashFlow, CashFlowType, FixedCouponBond};
use rust_decimal::Decimal;
use wasm_bindgen::prelude::*;

fn failure(error: String) -> AnalysisResult {
    AnalysisResult {
        error: Some(error),
        ..Default::default()
    }
}

/// Checked quote and analytics with explicit positive per100 clean price.
#[wasm_bindgen]
pub fn analyze_bond(params: JsValue, clean_price: f64, curve_points: JsValue) -> JsValue {
    let result = analyze_bond_impl(params, clean_price, curve_points).unwrap_or_else(failure);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}
fn analyze_bond_impl(
    params: JsValue,
    clean_price: f64,
    curve_points: JsValue,
) -> Result<AnalysisResult, String> {
    positive(clean_price, "clean price")?;
    let params = decode_bond_params(params)?;
    let points = decode_curve_points(curve_points)?;
    let bond = create_price_bond(&params)?;
    let settlement = parse_date(&params.settlement_date)?;
    let curve = create_curve(settlement, &points)?;
    let rules = get_yield_rules(&params)?;
    let yas = YASCalculator::new(&curve)
        .analyze(
            &bond,
            date_to_naive(settlement),
            f64_to_decimal(clean_price)?,
        )
        .map_err(|e| format!("Analysis failed: {e}"))?;
    let mut result = convert_yas_result(&yas, &bond, settlement, &rules, &params)?;
    result.ytm = Some(finite(
        calculate_convention_yield(&bond, settlement, clean_price, &rules)? * 100.0,
        "percent yield",
    )?);
    Ok(result)
}

/// Projected future cashflows in absolute bond currency; errors are explicit objects.
#[wasm_bindgen]
pub fn get_cash_flows(params: JsValue) -> JsValue {
    match get_cash_flows_impl(params) {
        Ok(result) => serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL),
        Err(error) => serde_wasm_bindgen::to_value(&failure(error)).unwrap_or(JsValue::NULL),
    }
}
fn get_cash_flows_impl(params: JsValue) -> Result<Vec<CashFlowEntry>, String> {
    let params = decode_bond_params(params)?;
    let bond = create_bond(&params)?;
    let settlement = parse_date(&params.settlement_date)?;
    let redemption = bond.redemption_value();
    bond.try_cash_flows(settlement)
        .map_err(|e| e.to_string())?
        .iter()
        .map(|cf| fixed_cash_flow_entry(cf, redemption))
        .collect()
}

/// Restricted fixed-bond cash components use native identity and actual redemption.
pub(crate) fn fixed_cash_flow_entry(
    cf: &BondCashFlow,
    redemption: Decimal,
) -> Result<CashFlowEntry, String> {
    if redemption <= Decimal::ZERO {
        return Err("Positive actual fixed redemption required".into());
    }
    let zero = Decimal::ZERO;
    let (kind, coupon, principal, fee) = match cf.flow_type {
        CashFlowType::Coupon => ("coupon", cf.amount, zero, zero),
        CashFlowType::Principal => ("principal", zero, cf.amount, zero),
        CashFlowType::CouponAndPrincipal => (
            "coupon_and_principal",
            cf.amount
                .checked_sub(redemption)
                .ok_or("Cash component subtraction overflow")?,
            redemption,
            zero,
        ),
        CashFlowType::Fee => ("fee", zero, zero, cf.amount),
    };
    Ok(CashFlowEntry {
        date: cf.date.to_string(),
        amount: decimal_to_f64(cf.amount)?,
        cf_type: kind.into(),
        coupon_amount: decimal_to_f64(coupon)?,
        principal_amount: decimal_to_f64(principal)?,
        fee_amount: decimal_to_f64(fee)?,
    })
}

/// Accrued per100 original face, matching the price quote units.
#[wasm_bindgen]
pub fn calculate_accrued(params: JsValue) -> JsValue {
    let result = match calculate_accrued_impl(params) {
        Ok(accrued) => AnalysisResult {
            accrued_interest: Some(accrued),
            ..Default::default()
        },
        Err(error) => failure(error),
    };
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}
fn calculate_accrued_impl(params: JsValue) -> Result<f64, String> {
    let params = decode_bond_params(params)?;
    let bond = create_price_bond(&params)?;
    decimal_to_f64(bond.accrued_interest(parse_date(&params.settlement_date)?))
}

/// Basic metrics in per100 price/accrual units and percentage current yield.
#[wasm_bindgen]
pub fn calculate_simple_metrics(params: JsValue, clean_price: f64) -> JsValue {
    let result = calculate_simple_metrics_impl(params, clean_price).unwrap_or_else(failure);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}
fn calculate_simple_metrics_impl(
    params: JsValue,
    clean_price: f64,
) -> Result<AnalysisResult, String> {
    positive(clean_price, "clean price")?;
    let params = decode_bond_params(params)?;
    let bond = create_price_bond(&params)?;
    let settlement = parse_date(&params.settlement_date)?;
    let accrued = decimal_to_f64(bond.accrued_interest(settlement))?;
    let dirty_price = finite(clean_price + accrued, "dirty price")?;
    let maturity = bond.maturity().ok_or("Missing fixed maturity")?;
    let days = settlement.days_between(&maturity);
    let years = days as f64 / 365.0;
    let annual_coupon = decimal_to_f64(bond.coupon_rate())? * decimal_to_f64(bond.face_value())?;
    let current_yield = finite(annual_coupon / clean_price * 100.0, "current yield")?;
    Ok(AnalysisResult {
        clean_price: Some(clean_price),
        dirty_price: Some(dirty_price),
        accrued_interest: Some(accrued),
        current_yield: Some(current_yield),
        days_to_maturity: Some(days),
        years_to_maturity: Some(years),
        error: None,
        ..Default::default()
    })
}
