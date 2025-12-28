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
