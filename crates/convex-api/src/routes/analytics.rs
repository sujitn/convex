//! Batch analytics endpoints.

use axum::{extract::State, Json};
use convex_bonds::traits::BondAnalytics;
use rust_decimal::Decimal;

use crate::dto::{
    BatchAnalyticsRequest, BatchAnalyticsResponse, BatchAnalyticsResult, BatchYieldRequest,
    BatchYieldResponse, BatchYieldResult,
};
use crate::error::ApiResult;
use crate::state::{AppState, StoredBond};

/// Batch yield calculation.
pub async fn batch_yield(
    State(state): State<AppState>,
    Json(req): Json<BatchYieldRequest>,
) -> ApiResult<Json<BatchYieldResponse>> {
    let settlement = req.settlement.to_date()?;
    let bonds = state.bonds.read().unwrap();

    let mut results = Vec::new();
    let mut success_count = 0;
    let mut error_count = 0;

    for (i, bond_id) in req.bond_ids.iter().enumerate() {
        let price = req.clean_prices.get(i).copied().unwrap_or(100.0);

        match bonds.get(bond_id) {
            Some(StoredBond::Fixed(bond)) => {
                let price_decimal = Decimal::from_f64_retain(price);
                match price_decimal {
                    Some(pd) => {
                        let frequency = bond.frequency();
                        match bond.yield_to_maturity(settlement, pd, frequency) {
                            Ok(ytm_result) => {
                                results.push(BatchYieldResult {
                                    bond_id: bond_id.clone(),
                                    clean_price: price,
                                    yield_to_maturity_pct: Some(ytm_result.yield_value * 100.0),
                                    error: None,
                                });
                                success_count += 1;
                            }
                            Err(e) => {
                                results.push(BatchYieldResult {
                                    bond_id: bond_id.clone(),
                                    clean_price: price,
                                    yield_to_maturity_pct: None,
                                    error: Some(e.to_string()),
                                });
                                error_count += 1;
                            }
                        }
                    }
                    None => {
                        results.push(BatchYieldResult {
                            bond_id: bond_id.clone(),
                            clean_price: price,
                            yield_to_maturity_pct: None,
                            error: Some("Invalid price".to_string()),
                        });
                        error_count += 1;
                    }
                }
            }
            Some(_) => {
                results.push(BatchYieldResult {
                    bond_id: bond_id.clone(),
                    clean_price: price,
                    yield_to_maturity_pct: None,
                    error: Some("Unsupported bond type".to_string()),
                });
                error_count += 1;
            }
            None => {
                results.push(BatchYieldResult {
                    bond_id: bond_id.clone(),
                    clean_price: price,
                    yield_to_maturity_pct: None,
                    error: Some("Bond not found".to_string()),
                });
                error_count += 1;
            }
        }
    }

    Ok(Json(BatchYieldResponse {
        settlement: settlement.to_string(),
        results,
        success_count,
        error_count,
    }))
}

/// Batch analytics calculation.
pub async fn batch_analytics(
    State(state): State<AppState>,
    Json(req): Json<BatchAnalyticsRequest>,
) -> ApiResult<Json<BatchAnalyticsResponse>> {
    let settlement = req.settlement.to_date()?;
    let bonds = state.bonds.read().unwrap();

    let mut results = Vec::new();
    let mut success_count = 0;
    let mut error_count = 0;

    for (i, bond_id) in req.bond_ids.iter().enumerate() {
        let price = req.clean_prices.get(i).copied().unwrap_or(100.0);

        match bonds.get(bond_id) {
            Some(StoredBond::Fixed(bond)) => {
                let price_decimal = Decimal::from_f64_retain(price);
                match price_decimal {
                    Some(pd) => {
                        let frequency = bond.frequency();
                        match bond.yield_to_maturity(settlement, pd, frequency) {
                            Ok(ytm_result) => {
                                let ytm = ytm_result.yield_value;
                                let mod_dur = bond.modified_duration(settlement, ytm, frequency).ok();
                                let conv = bond.convexity(settlement, ytm, frequency).ok();
                                let dv01 = bond.dv01(settlement, ytm, 100.0, frequency).ok();

                                results.push(BatchAnalyticsResult {
                                    bond_id: bond_id.clone(),
                                    yield_to_maturity_pct: Some(ytm * 100.0),
                                    modified_duration: mod_dur,
                                    convexity: conv,
                                    dv01,
                                    error: None,
                                });
                                success_count += 1;
                            }
                            Err(e) => {
                                results.push(BatchAnalyticsResult {
                                    bond_id: bond_id.clone(),
                                    yield_to_maturity_pct: None,
                                    modified_duration: None,
                                    convexity: None,
                                    dv01: None,
                                    error: Some(e.to_string()),
                                });
                                error_count += 1;
                            }
                        }
                    }
                    None => {
                        results.push(BatchAnalyticsResult {
                            bond_id: bond_id.clone(),
                            yield_to_maturity_pct: None,
                            modified_duration: None,
                            convexity: None,
                            dv01: None,
                            error: Some("Invalid price".to_string()),
                        });
                        error_count += 1;
                    }
                }
            }
            Some(_) => {
                results.push(BatchAnalyticsResult {
                    bond_id: bond_id.clone(),
                    yield_to_maturity_pct: None,
                    modified_duration: None,
                    convexity: None,
                    dv01: None,
                    error: Some("Unsupported bond type".to_string()),
                });
                error_count += 1;
            }
            None => {
                results.push(BatchAnalyticsResult {
                    bond_id: bond_id.clone(),
                    yield_to_maturity_pct: None,
                    modified_duration: None,
                    convexity: None,
                    dv01: None,
                    error: Some("Bond not found".to_string()),
                });
                error_count += 1;
            }
        }
    }

    Ok(Json(BatchAnalyticsResponse {
        settlement: settlement.to_string(),
        results,
        success_count,
        error_count,
    }))
}
