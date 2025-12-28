//! Bond endpoints.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use convex_analytics::spreads::{i_spread, z_spread};
use convex_bonds::traits::{Bond, BondAnalytics, FixedCouponBond};
use convex_bonds::types::BondIdentifiers;
use convex_bonds::FixedRateBond;
use convex_core::types::{Compounding, Yield};
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
        today.month(),
        today.day(),
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
    let stored = bonds
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("Bond '{}' not found", id)))?;

    let curves = state.curves.read().unwrap();
    let curve = curves
        .get(&req.curve_id)
        .ok_or_else(|| ApiError::NotFound(format!("Curve '{}' not found", req.curve_id)))?;

    let settlement = req.settlement.to_date()?;

    match stored {
        StoredBond::Fixed(bond) => {
            let frequency = bond.frequency();

            // Calculate dirty price from clean price
            let price_decimal = Decimal::from_f64_retain(req.clean_price)
                .ok_or_else(|| ApiError::Validation("Invalid price".to_string()))?;
            let accrued = bond.accrued_interest(settlement);
            let accrued_f64: f64 = accrued.try_into().unwrap_or(0.0);
            let dirty_price_f64 = req.clean_price + accrued_f64;
            let dirty_price = Decimal::from_f64_retain(dirty_price_f64)
                .ok_or_else(|| ApiError::Validation("Invalid dirty price".to_string()))?;

            // Calculate YTM from price (needed for I-spread)
            let ytm_result = bond.yield_to_maturity(settlement, price_decimal, frequency)?;
            let ytm = ytm_result.yield_value;

            // Calculate Z-spread
            // RateCurve<T> implements RateCurveDyn, so we can use it directly
            let z_spread_result = z_spread(bond, dirty_price, curve, settlement);
            let z_spread_bps = z_spread_result.ok().map(|s| {
                s.as_decimal()
                    .try_into()
                    .unwrap_or(0.0)
            });

            // Calculate I-spread (need to create a Yield type)
            let bond_yield = Yield::new(
                Decimal::from_f64_retain(ytm).unwrap_or_default(),
                Compounding::SemiAnnual,
            );
            let i_spread_result = i_spread(bond, bond_yield, curve, settlement);
            let i_spread_bps = i_spread_result.ok().map(|s| {
                s.as_decimal()
                    .try_into()
                    .unwrap_or(0.0)
            });

            // G-spread requires GovernmentCurve type which has benchmark bonds
            // For now, return None - would need separate government curve setup
            let g_spread_bps: Option<f64> = None;

            Ok(Json(SpreadResponse {
                bond_id: id,
                curve_id: req.curve_id,
                settlement: settlement.to_string(),
                clean_price: req.clean_price,
                z_spread_bps,
                i_spread_bps,
                g_spread_bps,
            }))
        }
        _ => Err(ApiError::BadRequest(
            "Spread calculations only supported for fixed rate bonds".to_string(),
        )),
    }
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

#[cfg(test)]
mod tests {
    use axum_test::TestServer;

    use crate::dto::*;
    use crate::server::create_router;
    use crate::state::AppState;

    fn create_test_server() -> TestServer {
        let state = AppState::with_demo_mode();
        let router = create_router(state);
        TestServer::new(router).unwrap()
    }

    #[tokio::test]
    async fn test_list_bonds_empty() {
        let state = AppState::new();
        let router = create_router(state);
        let server = TestServer::new(router).unwrap();

        let response = server.get("/api/v1/bonds").await;
        response.assert_status_ok();

        let body: BondListResponse = response.json();
        assert_eq!(body.count, 0);
        assert!(body.bonds.is_empty());
    }

    #[tokio::test]
    async fn test_list_bonds_with_demo_data() {
        let server = create_test_server();

        let response = server.get("/api/v1/bonds").await;
        response.assert_status_ok();

        let body: BondListResponse = response.json();
        assert!(body.count >= 4);
        assert!(body.bonds.iter().any(|b| b.id == "UST.10Y"));
        assert!(body.bonds.iter().any(|b| b.id == "UST.5Y"));
    }

    #[tokio::test]
    async fn test_create_bond() {
        let state = AppState::new();
        let router = create_router(state);
        let server = TestServer::new(router).unwrap();

        let req = CreateBondRequest {
            id: "TEST.BOND".to_string(),
            coupon_rate: 5.0,
            maturity: DateInput {
                year: 2030,
                month: 6,
                day: 15,
            },
            issue_date: DateInput {
                year: 2020,
                month: 6,
                day: 15,
            },
            frequency: FrequencyCode::SemiAnnual,
            day_count: DayCountCode::Thirty360Us,
            currency: CurrencyCode::Usd,
            face_value: 100.0,
        };

        let response = server.post("/api/v1/bonds").json(&req).await;
        response.assert_status(axum::http::StatusCode::CREATED);

        let body: BondResponse = response.json();
        assert_eq!(body.id, "TEST.BOND");
        assert!((body.coupon_rate - 5.0).abs() < 0.0001);
    }

    #[tokio::test]
    async fn test_create_duplicate_bond() {
        let server = create_test_server();

        let req = CreateBondRequest {
            id: "UST.10Y".to_string(), // Already exists in demo
            coupon_rate: 5.0,
            maturity: DateInput {
                year: 2030,
                month: 6,
                day: 15,
            },
            issue_date: DateInput {
                year: 2020,
                month: 6,
                day: 15,
            },
            frequency: FrequencyCode::SemiAnnual,
            day_count: DayCountCode::Thirty360Us,
            currency: CurrencyCode::Usd,
            face_value: 100.0,
        };

        let response = server.post("/api/v1/bonds").json(&req).await;
        response.assert_status(axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_get_bond() {
        let server = create_test_server();

        let response = server.get("/api/v1/bonds/UST.10Y").await;
        response.assert_status_ok();

        let body: BondResponse = response.json();
        assert_eq!(body.id, "UST.10Y");
        assert_eq!(body.bond_type, "Fixed Rate");
    }

    #[tokio::test]
    async fn test_get_bond_not_found() {
        let server = create_test_server();

        let response = server.get("/api/v1/bonds/NONEXISTENT").await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_bond() {
        let server = create_test_server();

        // First verify the bond exists
        let response = server.get("/api/v1/bonds/UST.5Y").await;
        response.assert_status_ok();

        // Delete it
        let response = server.delete("/api/v1/bonds/UST.5Y").await;
        response.assert_status(axum::http::StatusCode::NO_CONTENT);

        // Verify it's gone
        let response = server.get("/api/v1/bonds/UST.5Y").await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_bond_not_found() {
        let server = create_test_server();

        let response = server.delete("/api/v1/bonds/NONEXISTENT").await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_calculate_yield() {
        let server = create_test_server();

        let req = CalculateYieldRequest {
            settlement: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            clean_price: 100.0,
        };

        let response = server
            .post("/api/v1/bonds/UST.10Y/yield")
            .json(&req)
            .await;
        response.assert_status_ok();

        let body: YieldResponse = response.json();
        assert_eq!(body.bond_id, "UST.10Y");
        assert!(body.yield_to_maturity_pct > 0.0);
    }

    #[tokio::test]
    async fn test_calculate_yield_bond_not_found() {
        let server = create_test_server();

        let req = CalculateYieldRequest {
            settlement: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            clean_price: 100.0,
        };

        let response = server
            .post("/api/v1/bonds/NONEXISTENT/yield")
            .json(&req)
            .await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_calculate_price() {
        let server = create_test_server();

        let req = CalculatePriceRequest {
            settlement: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            yield_pct: 4.25,
        };

        let response = server
            .post("/api/v1/bonds/UST.10Y/price")
            .json(&req)
            .await;
        response.assert_status_ok();

        let body: PriceResponse = response.json();
        assert_eq!(body.bond_id, "UST.10Y");
        assert!(body.clean_price > 0.0);
        assert!(body.dirty_price > 0.0);
    }

    #[tokio::test]
    async fn test_analytics_with_price() {
        let server = create_test_server();

        let req = AnalyticsRequest {
            settlement: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            clean_price: Some(100.0),
            yield_pct: None,
        };

        let response = server
            .post("/api/v1/bonds/UST.10Y/analytics")
            .json(&req)
            .await;
        response.assert_status_ok();

        let body: AnalyticsResponse = response.json();
        assert_eq!(body.bond_id, "UST.10Y");
        assert!(body.macaulay_duration > 0.0);
        assert!(body.modified_duration > 0.0);
        assert!(body.convexity > 0.0);
        assert!(body.dv01 > 0.0);
    }

    #[tokio::test]
    async fn test_analytics_with_yield() {
        let server = create_test_server();

        let req = AnalyticsRequest {
            settlement: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            clean_price: None,
            yield_pct: Some(4.25),
        };

        let response = server
            .post("/api/v1/bonds/UST.10Y/analytics")
            .json(&req)
            .await;
        response.assert_status_ok();

        let body: AnalyticsResponse = response.json();
        assert_eq!(body.bond_id, "UST.10Y");
        assert!((body.yield_to_maturity_pct - 4.25).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_analytics_missing_input() {
        let server = create_test_server();

        let req = AnalyticsRequest {
            settlement: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            clean_price: None,
            yield_pct: None,
        };

        let response = server
            .post("/api/v1/bonds/UST.10Y/analytics")
            .json(&req)
            .await;
        response.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_cashflows() {
        let server = create_test_server();

        let response = server.get("/api/v1/bonds/UST.10Y/cashflows").await;
        response.assert_status_ok();

        let body: CashflowResponse = response.json();
        assert_eq!(body.bond_id, "UST.10Y");
        assert!(!body.cashflows.is_empty());
    }

    #[tokio::test]
    async fn test_spreads() {
        let server = create_test_server();

        let req = SpreadRequest {
            curve_id: "UST".to_string(),
            settlement: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            clean_price: 100.0,
        };

        let response = server
            .post("/api/v1/bonds/CORP.AAPL/spreads")
            .json(&req)
            .await;
        response.assert_status_ok();

        let body: SpreadResponse = response.json();
        assert_eq!(body.bond_id, "CORP.AAPL");
        assert_eq!(body.curve_id, "UST");
    }

    #[tokio::test]
    async fn test_spreads_bond_not_found() {
        let server = create_test_server();

        let req = SpreadRequest {
            curve_id: "UST".to_string(),
            settlement: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            clean_price: 100.0,
        };

        let response = server
            .post("/api/v1/bonds/NONEXISTENT/spreads")
            .json(&req)
            .await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_spreads_curve_not_found() {
        let server = create_test_server();

        let req = SpreadRequest {
            curve_id: "NONEXISTENT".to_string(),
            settlement: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            clean_price: 100.0,
        };

        let response = server
            .post("/api/v1/bonds/CORP.AAPL/spreads")
            .json(&req)
            .await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }
}
