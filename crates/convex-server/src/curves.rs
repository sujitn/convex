//! Curve routes over the single durable owner, the recipe `CurveStore`, plus
//! the stateless bootstrap calculator. No curve is cached in the server.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use convex_core::contracts::UtcTimestamp;
use convex_core::ids::CurveId;
use convex_core::Date;
use convex_curves::{Compounding, RateCurveDyn};
use convex_engine::curve_resolution::resolve_curves;
use convex_engine::ports::snapshot::SnapshotCurveRecipe;
use convex_engine::ports::storage::MAX_CURVE_LISTING;
use convex_engine::BuiltCurve;
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::handlers::AppState;

/// Instruments accepted by one bootstrap request: the calibrators' own bound.
const MAX_BOOTSTRAP_INSTRUMENTS: usize = 128;
/// Curve heads returned when the request names no limit.
const DEFAULT_CURVE_LISTING: usize = 100;
/// How far a stored revision's `recorded_at` may lead the server clock: D70's clock-step
/// bound (D73). A later time would hide the revision until then, and the store's
/// recording order would hold every later revision of the curve behind it.
const MAX_RECORDED_AHEAD_MS: i64 = crate::market::MAX_CLOCK_STEP_MS;

/// Explicit information cutoff; the current time when absent. Every response
/// echoes the cutoff it used in `x-convex-known-at`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KnownAtQuery {
    known_at: Option<UtcTimestamp>,
}

pub(crate) fn known_at_or_now(known_at: Option<UtcTimestamp>) -> Result<UtcTimestamp, ApiError> {
    match known_at {
        Some(time) => Ok(time),
        None => UtcTimestamp::from_datetime(chrono::Utc::now())
            .map_err(|_| ApiError::unavailable("clock unavailable")),
    }
}

/// The wire form of a cutoff, as echoed in `x-convex-known-at`.
pub(crate) fn rfc3339(time: UtcTimestamp) -> String {
    time.datetime()
        .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
}

pub(crate) fn with_header(
    mut response: Response,
    name: &'static str,
    value: &str,
) -> Result<Response, ApiError> {
    let value =
        HeaderValue::from_str(value).map_err(|_| ApiError::unavailable("response header"))?;
    response.headers_mut().insert(name, value);
    Ok(response)
}

fn query<T>(query: Result<Query<T>, QueryRejection>) -> Result<T, ApiError> {
    query
        .map(|Query(value)| value)
        .map_err(|e| ApiError::invalid(e.body_text()))
}

/// Store one immutable recipe revision. `POST /api/v1/curves` and
/// `POST /api/v1/analytics/curves` are the same write. Its `recorded_at` is the
/// writer's, at most [`MAX_RECORDED_AHEAD_MS`] after the server clock; the store
/// refuses one out of the curve's recording order (D73). An identical retry of a stored
/// revision is `204` whatever its time, since one loaded offline or before the bound
/// can be later; a different revision beyond the bound is `400`, not `409`.
pub(crate) async fn save_curve(
    State(state): State<Arc<AppState>>,
    payload: Result<Json<SnapshotCurveRecipe>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(recipe) = payload.map_err(|e| ApiError::invalid(e.body_text()))?;
    let store = state.engine.curve_store();
    let bound = chrono::TimeDelta::milliseconds(MAX_RECORDED_AHEAD_MS);
    if recipe.recorded_at.datetime() - chrono::Utc::now() > bound {
        let id = CurveId::new(recipe.curve_id.as_str());
        if store.get_recipe(&id, recipe.version).await?.as_ref() != Some(&recipe) {
            return Err(ApiError::invalid(
                "recorded_at is more than 5 s after the server clock",
            ));
        }
        return Ok(StatusCode::NO_CONTENT);
    }
    store.save_recipe(&recipe).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    after: Option<String>,
    limit: Option<usize>,
}

/// Bounded listing of each curve's highest stored revision, in identity order.
pub(crate) async fn list_curves(
    State(state): State<Arc<AppState>>,
    params: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let params = query(params)?;
    let limit = params.limit.unwrap_or(DEFAULT_CURVE_LISTING);
    if limit == 0 || limit > MAX_CURVE_LISTING {
        return Err(ApiError::invalid(format!(
            "limit must be 1..={MAX_CURVE_LISTING}"
        )));
    }
    let after = params.after.map(CurveId::new);
    let heads = state
        .engine
        .curve_store()
        .recipe_heads(after.as_ref(), limit)
        .await?;
    let next = (heads.len() == limit)
        .then(|| heads.last().map(|h| h.curve_id.as_str().to_owned()))
        .flatten();
    let curves: Vec<_> = heads
        .iter()
        .map(|h| {
            serde_json::json!({
                "curve_id": h.curve_id.as_str(),
                "version": h.version.get(),
                "recorded_at": h.recorded_at,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "curves": curves, "next": next })).into_response())
}

/// The stored recipe revision current at the cutoff.
pub(crate) async fn get_curve(
    State(state): State<Arc<AppState>>,
    Path(curve_id): Path<String>,
    params: Result<Query<KnownAtQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let known_at = known_at_or_now(query(params)?.known_at)?;
    let record = state
        .engine
        .curve_store()
        .recipe_at(&CurveId::new(&curve_id), known_at)
        .await
        .map_err(ApiError::stored)?
        .ok_or_else(|| ApiError::not_found(format!("curve {curve_id} at {}", rfc3339(known_at))))?;
    let version = record.version.get().to_string();
    let response = with_header(
        Json(record).into_response(),
        "x-convex-curve-version",
        &version,
    )?;
    with_header(response, "x-convex-known-at", &rfc3339(known_at))
}

/// Resolve one curve and its dependencies at the cutoff, fresh, for queries.
async fn query_curve(
    state: &AppState,
    curve_id: &str,
    known_at: UtcTimestamp,
) -> Result<(BuiltCurve, String), ApiError> {
    let id = CurveId::new(curve_id);
    let resolved = resolve_curves(
        state.engine.curve_store().as_ref(),
        std::slice::from_ref(&id),
        known_at,
    )
    .await?;
    let version = resolved.record(&id)?.version.get().to_string();
    Ok((resolved.curve(&id)?, version))
}

fn queried(
    value: serde_json::Value,
    version: &str,
    known_at: UtcTimestamp,
) -> Result<Response, ApiError> {
    let response = with_header(
        Json(value).into_response(),
        "x-convex-curve-version",
        version,
    )?;
    with_header(response, "x-convex-known-at", &rfc3339(known_at))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ZeroRateQuery {
    #[serde(default = "default_compounding")]
    compounding: String,
    known_at: Option<UtcTimestamp>,
}

fn default_compounding() -> String {
    "continuous".to_string()
}

fn compounding(name: &str) -> Result<Compounding, ApiError> {
    Ok(match name.to_lowercase().as_str() {
        "continuous" => Compounding::Continuous,
        "simple" => Compounding::Simple,
        "annual" => Compounding::Annual,
        "semiannual" | "semi_annual" => Compounding::SemiAnnual,
        "quarterly" => Compounding::Quarterly,
        "monthly" => Compounding::Monthly,
        "daily" => Compounding::Daily,
        _ => return Err(ApiError::invalid(format!("unknown compounding {name}"))),
    })
}

/// Zero rate at a tenor (years) on the curve current at the cutoff.
pub(crate) async fn get_curve_zero_rate(
    State(state): State<Arc<AppState>>,
    Path((curve_id, tenor)): Path<(String, f64)>,
    params: Result<Query<ZeroRateQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let params = query(params)?;
    let method = compounding(&params.compounding)?;
    let known_at = known_at_or_now(params.known_at)?;
    let (curve, version) = query_curve(&state, &curve_id, known_at).await?;
    let rate = curve
        .zero_rate(tenor, method)
        .map_err(|e| ApiError::invalid(e.to_string()))?;
    queried(
        serde_json::json!({
            "curve_id": curve_id,
            "tenor": tenor,
            "zero_rate": rate,
            "compounding": params.compounding,
        }),
        &version,
        known_at,
    )
}

/// Discount factor at a tenor (years) on the curve current at the cutoff.
pub(crate) async fn get_curve_discount_factor(
    State(state): State<Arc<AppState>>,
    Path((curve_id, tenor)): Path<(String, f64)>,
    params: Result<Query<KnownAtQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let known_at = known_at_or_now(query(params)?.known_at)?;
    let (curve, version) = query_curve(&state, &curve_id, known_at).await?;
    let df = curve
        .discount_factor(tenor)
        .map_err(|e| ApiError::invalid(e.to_string()))?;
    queried(
        serde_json::json!({"curve_id": curve_id, "tenor": tenor, "discount_factor": df}),
        &version,
        known_at,
    )
}

/// Forward rate between two tenors (years) on the curve current at the cutoff.
pub(crate) async fn get_curve_forward_rate(
    State(state): State<Arc<AppState>>,
    Path((curve_id, t1, t2)): Path<(String, f64, f64)>,
    params: Result<Query<KnownAtQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let known_at = known_at_or_now(query(params)?.known_at)?;
    let (curve, version) = query_curve(&state, &curve_id, known_at).await?;
    let rate = curve
        .forward_rate(t1, t2)
        .map_err(|e| ApiError::invalid(e.to_string()))?;
    queried(
        serde_json::json!({"curve_id": curve_id, "t1": t1, "t2": t2, "forward_rate": rate}),
        &version,
        known_at,
    )
}

// ============================================================================
// CURVE BOOTSTRAPPING (stateless calculator)
// ============================================================================

/// Input instrument for curve bootstrapping.
#[derive(Debug, Clone, Deserialize)]
pub struct BootstrapInstrumentInput {
    /// Instrument type: "Deposit", "FRA", "Swap", "OIS"
    #[serde(rename = "type")]
    pub instrument_type: String,
    /// Tenor in years (for Deposit, Swap, OIS)
    pub tenor: Option<f64>,
    /// Start tenor for FRA
    pub start_tenor: Option<f64>,
    /// End tenor for FRA
    pub end_tenor: Option<f64>,
    /// Market quote (rate as decimal, e.g., 0.04 for 4%)
    pub quote: f64,
}

/// Request for curve bootstrapping. Other fields (such as `interpolation` or an
/// instrument `description`) are ignored: each calibrator uses its own interpolation.
#[derive(Debug, Deserialize)]
pub struct BootstrapCurveRequest {
    /// Label echoed in the response; nothing is stored under it.
    pub curve_id: String,
    /// Reference date (YYYY-MM-DD); today when absent.
    pub reference_date: Option<String>,
    /// Instruments to calibrate, at most 128.
    pub instruments: Vec<BootstrapInstrumentInput>,
    /// Calibration method: "GlobalFit", "Piecewise" (default: GlobalFit)
    pub method: Option<String>,
}

/// Response from curve bootstrapping.
#[derive(Debug, Serialize)]
pub struct BootstrapCurveResponse {
    /// Label from the request.
    pub curve_id: String,
    /// Reference date
    pub reference_date: String,
    /// Calibrated curve points: [(tenor, zero_rate), ...]
    pub points: Vec<(f64, f64)>,
    /// Calibration statistics
    pub calibration: CalibrationStats,
}

/// Calibration statistics.
#[derive(Debug, Serialize)]
pub struct CalibrationStats {
    /// Whether calibration converged
    pub converged: bool,
    /// Number of iterations
    pub iterations: usize,
    /// RMS error (in rate terms)
    pub rms_error: f64,
    /// RMS error in basis points
    pub rms_error_bps: f64,
    /// Max error in basis points
    pub max_error_bps: f64,
    /// Per-instrument residuals in basis points
    pub residuals_bps: Vec<f64>,
    /// Method used
    pub method: String,
}

/// Calibrate a zero curve from market instruments and return it. Nothing is
/// stored; persist a result as a recipe with `POST /api/v1/curves`.
pub(crate) async fn bootstrap_curve(
    payload: Result<Json<BootstrapCurveRequest>, JsonRejection>,
) -> Result<Json<BootstrapCurveResponse>, ApiError> {
    use convex_curves::calibration::{CalibrationResult, GlobalFitter, PiecewiseBootstrapper};

    let Json(request) = payload.map_err(|e| ApiError::invalid(e.body_text()))?;
    if request.instruments.is_empty() || request.instruments.len() > MAX_BOOTSTRAP_INSTRUMENTS {
        return Err(ApiError::invalid(format!(
            "1..={MAX_BOOTSTRAP_INSTRUMENTS} instruments required"
        )));
    }
    let reference_date = match &request.reference_date {
        Some(text) => Date::parse(text)
            .map_err(|_| ApiError::invalid("Invalid reference_date format. Use YYYY-MM-DD"))?,
        None => Date::today(),
    };
    let instruments = instrument_set(reference_date, &request.instruments)?;
    let method = request.method.as_deref().unwrap_or("GlobalFit");
    let result: CalibrationResult = match method.to_uppercase().as_str() {
        "PIECEWISE" => PiecewiseBootstrapper::new().bootstrap(reference_date, &instruments),
        "GLOBALFIT" => GlobalFitter::default().fit(reference_date, &instruments),
        _ => {
            return Err(ApiError::invalid(format!(
                "unknown calibration method {method}"
            )))
        }
    }
    .map_err(|e| ApiError::calculation(format!("Calibration failed: {e}")))?;

    let points: Vec<(f64, f64)> = result
        .curve
        .tenors()
        .iter()
        .zip(result.curve.values().iter())
        .map(|(&t, &v)| (t, v))
        .collect();
    Ok(Json(BootstrapCurveResponse {
        curve_id: request.curve_id,
        reference_date: format!(
            "{}-{:02}-{:02}",
            reference_date.year(),
            reference_date.month(),
            reference_date.day()
        ),
        points,
        calibration: CalibrationStats {
            converged: result.converged,
            iterations: result.iterations,
            rms_error: result.rms_error,
            rms_error_bps: result.rms_error * 10_000.0,
            max_error_bps: result.max_error() * 10_000.0,
            residuals_bps: result.errors_bps(),
            method: method.to_string(),
        },
    }))
}

/// Every supplied instrument must be complete; none is skipped or guessed.
fn instrument_set(
    reference_date: Date,
    inputs: &[BootstrapInstrumentInput],
) -> Result<convex_curves::calibration::InstrumentSet, ApiError> {
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::Frequency;
    use convex_curves::calibration::{Deposit, Fra, InstrumentSet, Ois, Swap};

    let dc = DayCountConvention::Act360;
    let swap_dc = DayCountConvention::Thirty360US;
    let mut instruments = InstrumentSet::new();
    for (row, input) in inputs.iter().enumerate() {
        let incomplete = || ApiError::invalid(format!("instrument {row}: missing tenor"));
        let positive = |tenor: Option<f64>| match tenor {
            Some(years) if years.is_finite() && years > 0.0 => Ok(years),
            Some(_) => Err(ApiError::invalid(format!(
                "instrument {row}: tenor must be finite and positive"
            ))),
            None => Err(incomplete()),
        };
        if !input.quote.is_finite() {
            return Err(ApiError::invalid(format!("instrument {row}: quote")));
        }
        match input.instrument_type.to_uppercase().as_str() {
            "DEPOSIT" => {
                let tenor = positive(input.tenor)?;
                instruments.add(Deposit::from_tenor(reference_date, tenor, input.quote, dc));
            }
            "FRA" => {
                let (start, end) = input
                    .start_tenor
                    .zip(input.end_tenor)
                    .ok_or_else(incomplete)?;
                let months = |years: f64| (years * 12.0).round() as i32;
                if !(start.is_finite() && end.is_finite())
                    || months(start) < 0
                    || months(end) <= months(start)
                {
                    return Err(ApiError::invalid(format!(
                        "instrument {row}: FRA needs 0 <= start_tenor < end_tenor"
                    )));
                }
                let (Ok(start_date), Ok(end_date)) = (
                    reference_date.add_months(months(start)),
                    reference_date.add_months(months(end)),
                ) else {
                    return Err(ApiError::invalid(format!("instrument {row}: FRA dates")));
                };
                instruments.add(Fra::new(
                    reference_date,
                    start_date,
                    end_date,
                    input.quote,
                    dc,
                ));
            }
            "SWAP" => {
                let tenor = positive(input.tenor)?;
                instruments.add(Swap::from_tenor(
                    reference_date,
                    tenor,
                    input.quote,
                    Frequency::SemiAnnual,
                    swap_dc,
                ));
            }
            "OIS" => {
                let tenor = positive(input.tenor)?;
                instruments.add(Ois::from_tenor(reference_date, tenor, input.quote, dc));
            }
            other => {
                return Err(ApiError::invalid(format!(
                    "instrument {row}: unknown type {other}"
                )))
            }
        }
    }
    Ok(instruments)
}
