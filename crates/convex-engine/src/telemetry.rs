//! Stage timings for the runtime and request-time pricing. Each site records one
//! histogram observation, in seconds, through the `metrics` facade. Recording does
//! nothing until a process installs a recorder. Names and label values are fixed
//! here; none carries an instrument, curve or request id.
#![forbid(unsafe_code)]

use std::time::Instant;

/// One parsed HTTP quote batch through ingress validation and queue admission, the
/// whole batch or nothing. It starts after JSON extraction and excludes runtime work.
pub const INGRESS_SECONDS: &str = "convex_ingress_seconds";
/// One snapshot resolved from the held quotes (Live) or the retained trace.
pub const SNAPSHOT_ASSEMBLE_SECONDS: &str = "convex_snapshot_assemble_seconds";
/// One instrument valued against one snapshot.
pub const VALUATION_SECONDS: &str = "convex_valuation_seconds";
/// Every instrument of one snapshot, including commit to the calculation graph.
pub const CALCULATE_SECONDS: &str = "convex_calculate_seconds";
/// One result record handed to the publisher.
pub const PUBLISH_SECONDS: &str = "convex_publish_seconds";
/// One curve graph built from stored recipes, labelled by [`CurvePath`].
pub const CURVE_BUILD_SECONDS: &str = "convex_curve_build_seconds";
/// One request-time valuation context: activation read and curve resolution.
pub const REQUEST_CONTEXT_SECONDS: &str = "convex_request_context_seconds";
/// One `POST /api/v1/quote`, from handler entry to response.
pub const QUOTE_SECONDS: &str = "convex_quote_seconds";

/// Every histogram this crate and the server record, with its help text.
pub const HISTOGRAMS: [(&str, &str); 8] = [
    (
        INGRESS_SECONDS,
        "Parsed HTTP quote batch through validation and queue admission",
    ),
    (
        SNAPSHOT_ASSEMBLE_SECONDS,
        "Snapshot resolution for one tick",
    ),
    (
        VALUATION_SECONDS,
        "One instrument valued against one snapshot",
    ),
    (CALCULATE_SECONDS, "Every instrument of one snapshot"),
    (PUBLISH_SECONDS, "One result record published"),
    (CURVE_BUILD_SECONDS, "One curve graph built from recipes"),
    (REQUEST_CONTEXT_SECONDS, "Request-time valuation context"),
    (QUOTE_SECONDS, "POST /api/v1/quote"),
];

/// Where a curve graph was built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurvePath {
    /// Request-time curve resolution.
    Request,
    /// The build check on records a Live snapshot reads from the curve store.
    Stream,
    /// The one build of a snapshot at graph activation, shared by all its valuations.
    Graph,
    /// A single snapshot valuation that missed the cache, such as a supplied context.
    Valuation,
}
impl CurvePath {
    /// The bounded `path` label value.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Stream => "stream",
            Self::Graph => "graph",
            Self::Valuation => "valuation",
        }
    }
}

/// Record the seconds elapsed since `started` under `name`.
pub fn observe(name: &'static str, started: Instant) {
    metrics::histogram!(name).record(started.elapsed().as_secs_f64());
}

/// Record one curve graph build on `path`, started at `started`.
pub fn observe_curve_build(path: CurvePath, started: Instant) {
    metrics::histogram!(CURVE_BUILD_SECONDS, "path" => path.label())
        .record(started.elapsed().as_secs_f64());
}
