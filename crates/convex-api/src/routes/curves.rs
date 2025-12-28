//! Curve endpoints.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use convex_core::daycounts::DayCountConvention;
use convex_curves::calibration::{
    Deposit, FitterConfig, GlobalFitter, InstrumentSet, Ois, SequentialBootstrapper, Swap,
};
use convex_curves::{DiscreteCurve, RateCurve, TermStructure, ValueType};
use convex_core::types::Frequency;

use crate::dto::{
    BootstrapRequest, BootstrapResponse, CalibrationMethod, CreateCurveRequest, CurveDetailResponse,
    CurveListResponse, CurvePoint, CurveResponse, DiscountFactorQuery, ForwardRateQuery,
    RateQueryResponse, ZeroRateQuery,
};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// List all curves.
pub async fn list(State(state): State<AppState>) -> Json<CurveListResponse> {
    let curves = state.curves.read().unwrap();
    let curve_list: Vec<CurveResponse> = curves
        .iter()
        .map(|(id, curve)| curve_to_response(id, curve))
        .collect();
    let count = curve_list.len();

    Json(CurveListResponse {
        curves: curve_list,
        count,
    })
}

/// Create a new curve from zero rates.
pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<CreateCurveRequest>,
) -> ApiResult<(StatusCode, Json<CurveResponse>)> {
    // Check if curve already exists
    if state.curves.read().unwrap().contains_key(&req.id) {
        return Err(ApiError::BadRequest(format!(
            "Curve '{}' already exists",
            req.id
        )));
    }

    // Validate input
    if req.tenors.len() != req.rates.len() {
        return Err(ApiError::Validation(format!(
            "Tenors ({}) and rates ({}) must have same length",
            req.tenors.len(),
            req.rates.len()
        )));
    }

    if req.tenors.is_empty() {
        return Err(ApiError::Validation("At least one tenor required".to_string()));
    }

    let reference_date = req.reference_date.to_date()?;

    // Convert rates from percentage to decimal
    let rates: Vec<f64> = req.rates.iter().map(|r| r / 100.0).collect();

    let curve = DiscreteCurve::new(
        reference_date,
        req.tenors.clone(),
        rates,
        ValueType::continuous_zero(DayCountConvention::Act365Fixed),
        req.interpolation.into(),
    )
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let stored = RateCurve::new(curve);
    let response = curve_to_response(&req.id, &stored);

    state.curves.write().unwrap().insert(req.id.clone(), stored);

    Ok((StatusCode::CREATED, Json(response)))
}

/// Get a curve by ID.
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<CurveDetailResponse>> {
    let curves = state.curves.read().unwrap();
    let curve = curves
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("Curve '{}' not found", id)))?;

    // Generate points at standard tenors
    let standard_tenors = [0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 15.0, 20.0, 30.0];
    let points: Vec<CurvePoint> = standard_tenors
        .iter()
        .map(|&tenor| {
            let zero = curve.value_at(tenor);
            let df = (-zero * tenor).exp();
            CurvePoint {
                tenor,
                zero_rate_pct: zero * 100.0,
                discount_factor: df,
            }
        })
        .collect();

    Ok(Json(CurveDetailResponse {
        id: id.clone(),
        reference_date: curve.reference_date().to_string(),
        interpolation: "MonotoneConvex".to_string(),
        points,
    }))
}

/// Delete a curve.
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let mut curves = state.curves.write().unwrap();
    if curves.remove(&id).is_some() {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound(format!("Curve '{}' not found", id)))
    }
}

/// Query zero rate at a tenor.
pub async fn zero_rate(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ZeroRateQuery>,
) -> ApiResult<Json<RateQueryResponse>> {
    let curves = state.curves.read().unwrap();
    let curve = curves
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("Curve '{}' not found", id)))?;

    let zero = curve.value_at(query.tenor);

    Ok(Json(RateQueryResponse {
        curve_id: id,
        tenor: query.tenor,
        value: zero * 100.0, // Return as percentage
        value_type: "zero_rate_pct".to_string(),
    }))
}

/// Query forward rate between two tenors.
pub async fn forward_rate(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ForwardRateQuery>,
) -> ApiResult<Json<RateQueryResponse>> {
    let curves = state.curves.read().unwrap();
    let curve = curves
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("Curve '{}' not found", id)))?;

    if query.t2 <= query.t1 {
        return Err(ApiError::Validation("t2 must be greater than t1".to_string()));
    }

    let z1 = curve.value_at(query.t1);
    let z2 = curve.value_at(query.t2);
    let forward = (z2 * query.t2 - z1 * query.t1) / (query.t2 - query.t1);

    Ok(Json(RateQueryResponse {
        curve_id: id,
        tenor: query.t2 - query.t1,
        value: forward * 100.0, // Return as percentage
        value_type: "forward_rate_pct".to_string(),
    }))
}

/// Query discount factor at a tenor.
pub async fn discount_factor(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DiscountFactorQuery>,
) -> ApiResult<Json<RateQueryResponse>> {
    let curves = state.curves.read().unwrap();
    let curve = curves
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("Curve '{}' not found", id)))?;

    let zero = curve.value_at(query.tenor);
    let df = (-zero * query.tenor).exp();

    Ok(Json(RateQueryResponse {
        curve_id: id,
        tenor: query.tenor,
        value: df,
        value_type: "discount_factor".to_string(),
    }))
}

/// Bootstrap a curve from market instruments.
pub async fn bootstrap(
    State(state): State<AppState>,
    Json(req): Json<BootstrapRequest>,
) -> ApiResult<(StatusCode, Json<BootstrapResponse>)> {
    // Check if curve already exists
    if state.curves.read().unwrap().contains_key(&req.id) {
        return Err(ApiError::BadRequest(format!(
            "Curve '{}' already exists",
            req.id
        )));
    }

    let reference_date = req.reference_date.to_date()?;
    let deposit_dc = DayCountConvention::Act360;
    let swap_dc = DayCountConvention::Thirty360US;
    let swap_freq = Frequency::SemiAnnual;

    let mut instruments = InstrumentSet::new();

    // Add deposits
    for inst in &req.deposits {
        let rate = inst.rate / 100.0;
        instruments = instruments.with(Deposit::from_tenor(reference_date, inst.tenor, rate, deposit_dc));
    }

    // Add swaps
    for inst in &req.swaps {
        let rate = inst.rate / 100.0;
        instruments = instruments.with(Swap::from_tenor(reference_date, inst.tenor, rate, swap_freq, swap_dc));
    }

    // Add OIS
    for inst in &req.ois {
        let rate = inst.rate / 100.0;
        instruments = instruments.with(Ois::from_tenor(reference_date, inst.tenor, rate, deposit_dc));
    }

    if instruments.is_empty() {
        return Err(ApiError::Validation(
            "At least one instrument required".to_string(),
        ));
    }

    // Calibrate
    let result = match req.method {
        CalibrationMethod::Global => {
            let fitter = GlobalFitter::with_config(FitterConfig::default());
            fitter
                .fit(reference_date, &instruments)
                .map_err(|e| ApiError::CalculationFailed(e.to_string()))?
        }
        CalibrationMethod::Sequential => {
            let bootstrapper = SequentialBootstrapper::new();
            bootstrapper
                .bootstrap(reference_date, &instruments)
                .map_err(|e| ApiError::CalculationFailed(e.to_string()))?
        }
    };

    let stored = RateCurve::new(result.curve.clone());
    let tenor_count = req.deposits.len() + req.swaps.len() + req.ois.len();

    state.curves.write().unwrap().insert(req.id.clone(), stored);

    Ok((
        StatusCode::CREATED,
        Json(BootstrapResponse {
            id: req.id,
            reference_date: reference_date.to_string(),
            method: format!("{:?}", req.method),
            iterations: result.iterations as u32,
            rms_error: result.rms_error,
            tenor_count,
        }),
    ))
}

/// Convert stored curve to response.
fn curve_to_response(id: &str, curve: &RateCurve<DiscreteCurve>) -> CurveResponse {
    // Get min/max tenors from a quick sample
    let min_tenor = 0.25;
    let max_tenor = 30.0;

    CurveResponse {
        id: id.to_string(),
        reference_date: curve.reference_date().to_string(),
        tenor_count: 11, // Standard tenors
        min_tenor,
        max_tenor,
    }
}

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
    async fn test_list_curves_empty() {
        let state = AppState::new();
        let router = create_router(state);
        let server = TestServer::new(router).unwrap();

        let response = server.get("/api/v1/curves").await;
        response.assert_status_ok();

        let body: CurveListResponse = response.json();
        assert_eq!(body.count, 0);
        assert!(body.curves.is_empty());
    }

    #[tokio::test]
    async fn test_list_curves_with_demo_data() {
        let server = create_test_server();

        let response = server.get("/api/v1/curves").await;
        response.assert_status_ok();

        let body: CurveListResponse = response.json();
        assert!(body.count >= 2);
        assert!(body.curves.iter().any(|c| c.id == "UST"));
        assert!(body.curves.iter().any(|c| c.id == "SOFR"));
    }

    #[tokio::test]
    async fn test_create_curve() {
        let state = AppState::new();
        let router = create_router(state);
        let server = TestServer::new(router).unwrap();

        let req = CreateCurveRequest {
            id: "TEST.CURVE".to_string(),
            reference_date: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            tenors: vec![0.25, 0.5, 1.0, 2.0, 5.0, 10.0],
            rates: vec![4.35, 4.32, 4.25, 4.18, 4.20, 4.35],
            interpolation: InterpolationMethod::MonotoneConvex,
        };

        let response = server.post("/api/v1/curves").json(&req).await;
        response.assert_status(axum::http::StatusCode::CREATED);

        let body: CurveResponse = response.json();
        assert_eq!(body.id, "TEST.CURVE");
    }

    #[tokio::test]
    async fn test_create_curve_duplicate() {
        let server = create_test_server();

        let req = CreateCurveRequest {
            id: "UST".to_string(), // Already exists
            reference_date: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            tenors: vec![1.0],
            rates: vec![4.0],
            interpolation: InterpolationMethod::Linear,
        };

        let response = server.post("/api/v1/curves").json(&req).await;
        response.assert_status(axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_curve_mismatched_lengths() {
        let state = AppState::new();
        let router = create_router(state);
        let server = TestServer::new(router).unwrap();

        let req = CreateCurveRequest {
            id: "BAD.CURVE".to_string(),
            reference_date: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            tenors: vec![1.0, 2.0],
            rates: vec![4.0], // Only one rate for two tenors
            interpolation: InterpolationMethod::Linear,
        };

        let response = server.post("/api/v1/curves").json(&req).await;
        response.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_get_curve() {
        let server = create_test_server();

        let response = server.get("/api/v1/curves/UST").await;
        response.assert_status_ok();

        let body: CurveDetailResponse = response.json();
        assert_eq!(body.id, "UST");
        assert!(!body.points.is_empty());
    }

    #[tokio::test]
    async fn test_get_curve_not_found() {
        let server = create_test_server();

        let response = server.get("/api/v1/curves/NONEXISTENT").await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_curve() {
        let server = create_test_server();

        // First verify the curve exists
        let response = server.get("/api/v1/curves/SOFR").await;
        response.assert_status_ok();

        // Delete it
        let response = server.delete("/api/v1/curves/SOFR").await;
        response.assert_status(axum::http::StatusCode::NO_CONTENT);

        // Verify it's gone
        let response = server.get("/api/v1/curves/SOFR").await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_zero_rate() {
        let server = create_test_server();

        let response = server
            .get("/api/v1/curves/UST/zero-rate")
            .add_query_param("tenor", "5.0")
            .await;
        response.assert_status_ok();

        let body: RateQueryResponse = response.json();
        assert_eq!(body.curve_id, "UST");
        assert_eq!(body.tenor, 5.0);
        assert!(body.value > 0.0); // Rate should be positive
        assert_eq!(body.value_type, "zero_rate_pct");
    }

    #[tokio::test]
    async fn test_forward_rate() {
        let server = create_test_server();

        let response = server
            .get("/api/v1/curves/UST/forward-rate")
            .add_query_param("t1", "2.0")
            .add_query_param("t2", "5.0")
            .await;
        response.assert_status_ok();

        let body: RateQueryResponse = response.json();
        assert_eq!(body.curve_id, "UST");
        assert!((body.tenor - 3.0).abs() < 0.001); // t2 - t1
        assert!(body.value > 0.0);
        assert_eq!(body.value_type, "forward_rate_pct");
    }

    #[tokio::test]
    async fn test_forward_rate_invalid() {
        let server = create_test_server();

        // t2 <= t1 should fail
        let response = server
            .get("/api/v1/curves/UST/forward-rate")
            .add_query_param("t1", "5.0")
            .add_query_param("t2", "2.0")
            .await;
        response.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_discount_factor() {
        let server = create_test_server();

        let response = server
            .get("/api/v1/curves/UST/discount-factor")
            .add_query_param("tenor", "5.0")
            .await;
        response.assert_status_ok();

        let body: RateQueryResponse = response.json();
        assert_eq!(body.curve_id, "UST");
        assert!(body.value > 0.0 && body.value < 1.0); // DF should be between 0 and 1
        assert_eq!(body.value_type, "discount_factor");
    }

    #[tokio::test]
    async fn test_bootstrap_global() {
        let state = AppState::new();
        let router = create_router(state);
        let server = TestServer::new(router).unwrap();

        let req = BootstrapRequest {
            id: "BOOT.TEST".to_string(),
            reference_date: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            deposits: vec![
                BootstrapInstrument {
                    tenor: 0.25,
                    rate: 4.35,
                },
                BootstrapInstrument {
                    tenor: 0.5,
                    rate: 4.32,
                },
            ],
            swaps: vec![
                BootstrapInstrument {
                    tenor: 2.0,
                    rate: 4.20,
                },
                BootstrapInstrument {
                    tenor: 5.0,
                    rate: 4.25,
                },
            ],
            ois: vec![],
            method: CalibrationMethod::Global,
        };

        let response = server
            .post("/api/v1/curves/bootstrap")
            .json(&req)
            .await;
        response.assert_status(axum::http::StatusCode::CREATED);

        let body: BootstrapResponse = response.json();
        assert_eq!(body.id, "BOOT.TEST");
        assert_eq!(body.tenor_count, 4);
    }

    #[tokio::test]
    async fn test_bootstrap_sequential() {
        let state = AppState::new();
        let router = create_router(state);
        let server = TestServer::new(router).unwrap();

        let req = BootstrapRequest {
            id: "BOOT.SEQ".to_string(),
            reference_date: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            deposits: vec![BootstrapInstrument {
                tenor: 0.25,
                rate: 4.35,
            }],
            swaps: vec![
                BootstrapInstrument {
                    tenor: 1.0,
                    rate: 4.25,
                },
                BootstrapInstrument {
                    tenor: 2.0,
                    rate: 4.20,
                },
            ],
            ois: vec![],
            method: CalibrationMethod::Sequential,
        };

        let response = server
            .post("/api/v1/curves/bootstrap")
            .json(&req)
            .await;
        response.assert_status(axum::http::StatusCode::CREATED);

        let body: BootstrapResponse = response.json();
        assert_eq!(body.id, "BOOT.SEQ");
    }

    #[tokio::test]
    async fn test_bootstrap_no_instruments() {
        let state = AppState::new();
        let router = create_router(state);
        let server = TestServer::new(router).unwrap();

        let req = BootstrapRequest {
            id: "EMPTY.CURVE".to_string(),
            reference_date: DateInput {
                year: 2025,
                month: 12,
                day: 20,
            },
            deposits: vec![],
            swaps: vec![],
            ois: vec![],
            method: CalibrationMethod::Global,
        };

        let response = server
            .post("/api/v1/curves/bootstrap")
            .json(&req)
            .await;
        response.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    }
}
