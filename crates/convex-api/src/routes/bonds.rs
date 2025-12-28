//! Bond endpoints.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use convex_bonds::traits::{Bond, BondAnalytics, FixedCouponBond};
use convex_bonds::types::BondIdentifiers;
use convex_bonds::FixedRateBond;
use rust_decimal::Decimal;

use crate::dto::{
    AnalyticsRequest, AnalyticsResponse, BondListResponse, BondResponse, CalculatePriceRequest,
    CalculateYieldRequest, CashflowEntry, CashflowResponse, CreateBondRequest, PriceResponse,
    SpreadRequest, SpreadResponse, YieldResponse,
};
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, StoredBond};

/// List all bonds.
pub async fn list(State(state): State<AppState>) -> Json<BondListResponse> {
    let bonds = state.bonds.read().unwrap();
    let bond_list: Vec<BondResponse> = bonds
        .iter()
        .map(|(id, bond)| bond_to_response(id, bond))
        .collect();
    let count = bond_list.len();

    Json(BondListResponse {
        bonds: bond_list,
        count,
    })
}

/// Create a new bond.
pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateBondRequest>,
) -> ApiResult<(StatusCode, Json<BondResponse>)> {
    // Check if bond already exists
    if state.bonds.read().unwrap().contains_key(&req.id) {
        return Err(ApiError::BadRequest(format!(
            "Bond '{}' already exists",
            req.id
        )));
    }

    // Parse dates
    let maturity = req.maturity.to_date()?;
    let issue_date = req.issue_date.to_date()?;

    // Convert coupon rate from percentage to decimal
    let coupon_decimal = Decimal::from_f64_retain(req.coupon_rate / 100.0)
        .ok_or_else(|| ApiError::Validation("Invalid coupon rate".to_string()))?;
    let face_decimal = Decimal::from_f64_retain(req.face_value)
        .ok_or_else(|| ApiError::Validation("Invalid face value".to_string()))?;

    // Build bond
    let bond = FixedRateBond::builder()
        .identifiers(BondIdentifiers::new())
        .coupon_rate(coupon_decimal)
        .maturity(maturity)
        .issue_date(issue_date)
        .frequency(req.frequency.into())
        .day_count(req.day_count.into())
        .currency(req.currency.into())
        .face_value(face_decimal)
        .build()?;

    let stored = StoredBond::Fixed(bond);
    let response = bond_to_response(&req.id, &stored);

    state
        .bonds
        .write()
        .unwrap()
        .insert(req.id.clone(), stored);

    Ok((StatusCode::CREATED, Json(response)))
}

/// Get a bond by ID.
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<BondResponse>> {
    let bonds = state.bonds.read().unwrap();
    let bond = bonds
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("Bond '{}' not found", id)))?;

    Ok(Json(bond_to_response(&id, bond)))
}

/// Delete a bond.
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let mut bonds = state.bonds.write().unwrap();
    if bonds.remove(&id).is_some() {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound(format!("Bond '{}' not found", id)))
    }
}

/// Calculate yield to maturity from price.
pub async fn calculate_yield(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CalculateYieldRequest>,
) -> ApiResult<Json<YieldResponse>> {
    let bonds = state.bonds.read().unwrap();
    let stored = bonds
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("Bond '{}' not found", id)))?;

    let settlement = req.settlement.to_date()?;
    let price_decimal = Decimal::from_f64_retain(req.clean_price)
        .ok_or_else(|| ApiError::Validation("Invalid price".to_string()))?;

    match stored {
        StoredBond::Fixed(bond) => {
            let frequency = bond.frequency();
            let ytm_result = bond.yield_to_maturity(settlement, price_decimal, frequency)?;
            let accrued = bond.accrued_interest(settlement);
            let accrued_f64: f64 = accrued.try_into().unwrap_or(0.0);
            let dirty_price = req.clean_price + accrued_f64;

            Ok(Json(YieldResponse {
                bond_id: id,
                settlement: settlement.to_string(),
                clean_price: req.clean_price,
                dirty_price,
                accrued_interest: accrued.to_string(),
                yield_to_maturity_pct: ytm_result.yield_value * 100.0,
            }))
        }
        _ => Err(ApiError::BadRequest(
            "Yield calculation not supported for this bond type".to_string(),
        )),
    }
}

/// Calculate price from yield.
pub async fn calculate_price(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CalculatePriceRequest>,
) -> ApiResult<Json<PriceResponse>> {
    let bonds = state.bonds.read().unwrap();
    let stored = bonds
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("Bond '{}' not found", id)))?;

    let settlement = req.settlement.to_date()?;
    let ytm = req.yield_pct / 100.0;

    match stored {
        StoredBond::Fixed(bond) => {
            let frequency = bond.frequency();
            let clean_price = bond.clean_price_from_yield(settlement, ytm, frequency)?;
            let dirty_price = bond.dirty_price_from_yield(settlement, ytm, frequency)?;
            let accrued = bond.accrued_interest(settlement);

            Ok(Json(PriceResponse {
                bond_id: id,
                settlement: settlement.to_string(),
                yield_pct: req.yield_pct,
                clean_price,
                dirty_price,
                accrued_interest: accrued.to_string(),
            }))
        }
        _ => Err(ApiError::BadRequest(
            "Price calculation not supported for this bond type".to_string(),
        )),
    }
}

/// Get bond analytics.
pub async fn analytics(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<AnalyticsRequest>,
) -> ApiResult<Json<AnalyticsResponse>> {
    let bonds = state.bonds.read().unwrap();
    let stored = bonds
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("Bond '{}' not found", id)))?;

    let settlement = req.settlement.to_date()?;

    match stored {
        StoredBond::Fixed(bond) => {
            let frequency = bond.frequency();

            // Determine yield
            let ytm = if let Some(price) = req.clean_price {
                let price_decimal = Decimal::from_f64_retain(price)
                    .ok_or_else(|| ApiError::Validation("Invalid price".to_string()))?;
                let ytm_result = bond.yield_to_maturity(settlement, price_decimal, frequency)?;
                ytm_result.yield_value
            } else if let Some(yield_pct) = req.yield_pct {
                yield_pct / 100.0
            } else {
                return Err(ApiError::Validation(
                    "Either clean_price or yield_pct must be provided".to_string(),
                ));
            };

            let clean_price = bond.clean_price_from_yield(settlement, ytm, frequency)?;
            let dirty_price = bond.dirty_price_from_yield(settlement, ytm, frequency)?;
            let accrued = bond.accrued_interest(settlement);
            let mac_duration = bond.macaulay_duration(settlement, ytm, frequency)?;
            let mod_duration = bond.modified_duration(settlement, ytm, frequency)?;
            let convexity = bond.convexity(settlement, ytm, frequency)?;
            let dv01 = bond.dv01(settlement, ytm, 100.0, frequency)?;

            Ok(Json(AnalyticsResponse {
                bond_id: id,
                settlement: settlement.to_string(),
                yield_to_maturity_pct: ytm * 100.0,
                clean_price,
                dirty_price,
                accrued_interest: accrued.to_string(),
                macaulay_duration: mac_duration,
                modified_duration: mod_duration,
                convexity,
                dv01,
            }))
        }
        _ => Err(ApiError::BadRequest(
            "Analytics not supported for this bond type".to_string(),
        )),
    }
}

/// Get bond cashflows.
pub async fn cashflows(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<CashflowResponse>> {
    let bonds = state.bonds.read().unwrap();
    let stored = bonds
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("Bond '{}' not found", id)))?;

    // Use today as settlement for cashflow listing
    let today = chrono::Utc::now().date_naive();
    let settlement = convex_core::types::Date::from_ymd(
        today.year(),
        today.month() as u32,
        today.day() as u32,
    )
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    match stored {
        StoredBond::Fixed(bond) => {
            let cfs = bond.cash_flows(settlement);
            let cashflows: Vec<CashflowEntry> = cfs
                .iter()
                .map(|cf| CashflowEntry {
                    date: cf.date.to_string(),
                    amount: cf.amount.try_into().unwrap_or(0.0),
                    flow_type: format!("{:?}", cf.flow_type),
                })
                .collect();

            Ok(Json(CashflowResponse {
                bond_id: id,
                settlement: settlement.to_string(),
                cashflows,
            }))
        }
        _ => Err(ApiError::BadRequest(
            "Cashflows not supported for this bond type".to_string(),
        )),
    }
}

/// Calculate spreads.
pub async fn spreads(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SpreadRequest>,
) -> ApiResult<Json<SpreadResponse>> {
    let bonds = state.bonds.read().unwrap();
    let _stored = bonds
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("Bond '{}' not found", id)))?;

    let curves = state.curves.read().unwrap();
    let _curve = curves
        .get(&req.curve_id)
        .ok_or_else(|| ApiError::NotFound(format!("Curve '{}' not found", req.curve_id)))?;

    let settlement = req.settlement.to_date()?;

    // TODO: Implement actual spread calculations using convex-analytics
    // For now, return placeholder response
    Ok(Json(SpreadResponse {
        bond_id: id,
        curve_id: req.curve_id,
        settlement: settlement.to_string(),
        clean_price: req.clean_price,
        z_spread_bps: Some(50.0), // Placeholder
        i_spread_bps: Some(45.0), // Placeholder
        g_spread_bps: Some(55.0), // Placeholder
    }))
}

/// Convert stored bond to response.
fn bond_to_response(id: &str, bond: &StoredBond) -> BondResponse {
    match bond {
        StoredBond::Fixed(b) => BondResponse {
            id: id.to_string(),
            bond_type: "Fixed Rate".to_string(),
            coupon_rate: b.coupon_rate_decimal().try_into().unwrap_or(0.0) * 100.0,
            maturity: b.maturity().map(|d| d.to_string()).unwrap_or_else(|| "N/A".to_string()),
            issue_date: b.issue_date().to_string(),
            frequency: format!("{:?}", b.frequency()),
            currency: format!("{:?}", b.currency()),
            face_value: b.face_value().try_into().unwrap_or(100.0),
        },
        StoredBond::Zero(b) => BondResponse {
            id: id.to_string(),
            bond_type: "Zero Coupon".to_string(),
            coupon_rate: 0.0,
            maturity: b.maturity().map(|d| d.to_string()).unwrap_or_else(|| "N/A".to_string()),
            issue_date: b.issue_date().to_string(),
            frequency: "None".to_string(),
            currency: format!("{:?}", b.currency()),
            face_value: b.face_value().try_into().unwrap_or(100.0),
        },
        StoredBond::Callable(b) => BondResponse {
            id: id.to_string(),
            bond_type: "Callable".to_string(),
            coupon_rate: b.coupon_rate().try_into().unwrap_or(0.0) * 100.0,
            maturity: b.maturity().map(|d| d.to_string()).unwrap_or_else(|| "N/A".to_string()),
            issue_date: b.issue_date().to_string(),
            frequency: format!("{:?}", b.frequency()),
            currency: format!("{:?}", b.currency()),
            face_value: b.face_value().try_into().unwrap_or(100.0),
        },
        StoredBond::Floating(b) => BondResponse {
            id: id.to_string(),
            bond_type: "Floating Rate Note".to_string(),
            coupon_rate: 0.0, // FRN doesn't have fixed coupon
            maturity: b.maturity().map(|d| d.to_string()).unwrap_or_else(|| "N/A".to_string()),
            issue_date: b.issue_date().to_string(),
            frequency: format!("{:?}", b.frequency()),
            currency: format!("{:?}", b.currency()),
            face_value: b.face_value().try_into().unwrap_or(100.0),
        },
    }
}

// Needed for chrono date parsing
use chrono::Datelike;
