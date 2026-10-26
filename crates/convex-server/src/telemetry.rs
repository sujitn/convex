//! The process-wide Prometheus recorder behind `GET /metrics`. The engine records
//! stage timings through the `metrics` facade (`convex_engine::telemetry`); this
//! module installs the one recorder that keeps them and renders them on request.
//! The exporter runs no listener of its own.
#![forbid(unsafe_code)]

use std::sync::OnceLock;

use axum::http::header;
use axum::response::{IntoResponse, Response};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use tracing::warn;

use crate::error::ApiError;

/// Histogram upper bounds in seconds, 1 µs to 10 s, three per decade.
pub const LATENCY_BUCKETS: [f64; 22] = [
    1e-6, 2.5e-6, 5e-6, 1e-5, 2.5e-5, 5e-5, 1e-4, 2.5e-4, 5e-4, 1e-3, 2.5e-3, 5e-3, 1e-2, 2.5e-2,
    5e-2, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// The Prometheus text exposition format.
const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// `None` once installation has failed: another recorder owns the process.
static HANDLE: OnceLock<Option<PrometheusHandle>> = OnceLock::new();

/// Install the recorder once per process; later calls return the same handle.
/// Returns `None` when another recorder already owns the process. Metrics only
/// observe, so the caller continues without them.
pub fn install() -> Option<&'static PrometheusHandle> {
    HANDLE
        .get_or_init(|| {
            let handle = PrometheusBuilder::new()
                .set_buckets_for_metric(Matcher::Suffix("_seconds".into()), &LATENCY_BUCKETS)
                .and_then(PrometheusBuilder::install_recorder);
            match handle {
                Ok(handle) => {
                    for (name, help) in convex_engine::telemetry::HISTOGRAMS {
                        metrics::describe_histogram!(name, metrics::Unit::Seconds, help);
                    }
                    Some(handle)
                }
                Err(error) => {
                    warn!(%error, "Metrics recorder not installed; /metrics is unavailable");
                    None
                }
            }
        })
        .as_ref()
}

/// `GET /metrics`: every recorded histogram in the Prometheus text format.
pub async fn render() -> Response {
    let Some(handle) = HANDLE.get().and_then(Option::as_ref) else {
        return ApiError::unavailable("this process has no metrics recorder").into_response();
    };
    handle.run_upkeep();
    ([(header::CONTENT_TYPE, CONTENT_TYPE)], handle.render()).into_response()
}
