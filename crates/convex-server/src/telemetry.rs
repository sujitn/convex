//! The process-wide Prometheus recorder behind `GET /metrics`. The engine records
//! stage timings through the `metrics` facade (`convex_engine::telemetry`); this
//! module installs the one recorder that keeps them and renders them on request.
//! The exporter runs no listener of its own.
#![forbid(unsafe_code)]

use std::sync::OnceLock;
use std::time::Duration;

use axum::http::header;
use axum::response::{IntoResponse, Response};
use metrics_exporter_prometheus::{BuildError, Matcher, PrometheusBuilder, PrometheusHandle};
use tracing::warn;

use crate::error::ApiError;

/// Histogram upper bounds in seconds, 1 µs to 10 s, three per decade.
pub const LATENCY_BUCKETS: [f64; 22] = [
    1e-6, 2.5e-6, 5e-6, 1e-5, 2.5e-5, 5e-5, 1e-4, 2.5e-4, 5e-4, 1e-3, 2.5e-3, 5e-3, 1e-2, 2.5e-2,
    5e-2, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// The Prometheus text exposition format.
const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// How often recorded observations are drained into the histograms. Between
/// drains they accumulate, so without upkeep an unscraped process grows without
/// bound.
const UPKEEP_INTERVAL: Duration = Duration::from_secs(5);

/// `None` once installation has failed: another recorder owns the process, or
/// the upkeep thread could not start.
static HANDLE: OnceLock<Option<PrometheusHandle>> = OnceLock::new();

/// Install the recorder once per process; later calls return the same handle.
/// Returns `None` when it cannot be installed. Metrics only observe, so the
/// caller continues without them.
pub fn install() -> Option<&'static PrometheusHandle> {
    HANDLE
        .get_or_init(|| match build() {
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
        })
        .as_ref()
}

/// Build the recorder, start its upkeep thread for the life of the process, then
/// install it. The thread starts first, so an installed recorder always has
/// upkeep; if installation then fails, the thread only drains an unused handle.
fn build() -> Result<PrometheusHandle, Box<dyn std::error::Error + Send + Sync>> {
    let recorder = PrometheusBuilder::new()
        .set_buckets_for_metric(Matcher::Suffix("_seconds".into()), &LATENCY_BUCKETS)?
        .build_recorder();
    let handle = recorder.handle();
    let upkeep = handle.clone();
    std::thread::Builder::new()
        .name("convex-metrics-upkeep".into())
        .spawn(move || loop {
            std::thread::sleep(UPKEEP_INTERVAL);
            upkeep.run_upkeep();
        })?;
    metrics::set_global_recorder(recorder).map_err(BuildError::from)?;
    Ok(handle)
}

/// `GET /metrics`: every recorded histogram in the Prometheus text format.
pub async fn render() -> Response {
    let Some(handle) = HANDLE.get().and_then(Option::as_ref) else {
        return ApiError::unavailable("this process has no metrics recorder").into_response();
    };
    handle.run_upkeep();
    ([(header::CONTENT_TYPE, CONTENT_TYPE)], handle.render()).into_response()
}
