//! Exactly representable independent window arithmetic; no model reference claim.
use convex_analytics::workflow::*;
use serde_json::{json, Value};
fn row(basis: &str, currency: &str, amount: f64) -> PnlWindowTotal {
    let money = |v| json!({"value":v,"currency":currency,"unit":"currency"});
    serde_json::from_value(json!({"basis":basis,"currency":currency,"total":{
        "marking":{"current":money(amount),"pending":money(amount/2.),"projected":money(amount*1.5)},
        "cash":money(-amount/4.),"current_model_pnl":money(amount*0.75)}})).unwrap()
}
#[test]
fn independent_binary_exact_signed_addition_keeps_basis_and_currency() {
    let old = vec![
        row("CurrencyPresentValue", "USD", 8.),
        row("FuturesQuoteLevel", "USD", 32.),
    ];
    let now = vec![
        row("FuturesQuoteLevel", "USD", 16.),
        row("CurrencyPresentValue", "USD", -4.),
    ];
    let result = accumulate_pnl_windows(&old, &now).unwrap();
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!([
            serde_json::to_value(row("FuturesQuoteLevel", "USD", 48.)).unwrap(),
            serde_json::to_value(row("CurrencyPresentValue", "USD", 4.)).unwrap()
        ])
    );
}
#[test]
fn incomplete_duplicate_currency_and_basis_substitution_fail() {
    let old = vec![row("CurrencyPresentValue", "USD", 8.)];
    for new in [
        vec![],
        vec![old[0].clone(), old[0].clone()],
        vec![row("CurrencyPresentValue", "EUR", 8.)],
        vec![row("FuturesQuoteLevel", "USD", 8.)],
    ] {
        assert!(accumulate_pnl_windows(&old, &new).is_err());
    }
    let mut bad = serde_json::to_value(&old[0]).unwrap();
    bad["total"]["cash"]["currency"] = json!("EUR");
    assert!(validate_pnl_window_totals(&[serde_json::from_value(bad).unwrap()]).is_err());
}
#[test]
fn finite_input_overflow_rejected_without_clamping() {
    let mut v: Value = serde_json::to_value(row("CurrencyPresentValue", "USD", 8.)).unwrap();
    v["total"]["cash"]["value"] = json!(f64::MAX);
    let row: PnlWindowTotal = serde_json::from_value(v).unwrap();
    assert!(accumulate_pnl_windows(&[row.clone()], &[row]).is_err());
}

fn fx_point(id: &str, rate: &str) -> PnlPointInput {
    serde_json::from_value(json!({"snapshot_id":id,"manifest":{"id":id,"version":1},"valuation_date":"2026-10-09",
        "known_at":"2026-10-09T12:00:00Z","positions":[],"fills":[],"fx":[{"from":"EUR","to":"USD","rate":rate,
        "observation":{"id":"actual.fx","version":1},"snapshot_id":id}]})).unwrap()
}
#[test]
fn prior_cash_and_futures_consideration_keep_exact_binary_fx_revaluation() {
    let before = fx_point("before", "1.5");
    let after = fx_point("after", "2");
    let local = vec![
        row("CurrencyPresentValue", "EUR", 8.),
        row("FuturesQuoteLevel", "EUR", 16.),
    ];
    let previous = vec![
        row("CurrencyPresentValue", "USD", 12.),
        row("FuturesQuoteLevel", "USD", 24.),
    ];
    let current = vec![
        row("CurrencyPresentValue", "USD", 0.),
        row("FuturesQuoteLevel", "USD", 0.),
    ];
    let output = continue_pnl_reporting(
        &local,
        &previous,
        &current,
        &before,
        &after,
        convex_core::Currency::USD,
    )
    .unwrap();
    // Prior cash -2/-4 EUR at FX1.5 becomes -4/-8 USD at FX2.
    assert_eq!(output[0].total.cash.value().get(), -4.);
    assert_eq!(output[1].total.cash.value().get(), -8.);
    // Prior model-minus-marking adjustment -2/-4 EUR has the same FX change;
    // marking itself already telescopes through full point valuations.
    assert_eq!(output[0].total.current_model_pnl.value().get(), 8.);
    assert_eq!(output[1].total.current_model_pnl.value().get(), 16.);
    let mut missing = after.clone();
    missing.fx.clear();
    assert!(continue_pnl_reporting(
        &local,
        &previous,
        &current,
        &before,
        &missing,
        convex_core::Currency::USD
    )
    .is_err());
    let mut duplicate = after.clone();
    duplicate.fx.push(duplicate.fx[0].clone());
    assert!(continue_pnl_reporting(
        &local,
        &previous,
        &current,
        &before,
        &duplicate,
        convex_core::Currency::USD
    )
    .is_err());
}
