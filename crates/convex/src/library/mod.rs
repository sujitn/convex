//! Closed supplied-input API. Adapters share serialization and native dispatch;
//! financial formulas remain in their original pure product/curve modules.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod cash;
mod cash_hedges;
mod credit;
mod curves;
mod errors;
mod events;
mod floating_workout;
mod graph_hedges;
mod hedges;
mod helpers;
mod icma;
mod icma_government;
mod ingress;
mod inputs;
mod products;
mod reconciliation;
mod replay;
mod requests;
mod risk;
mod scenarios;
mod workout;
mod zero;

pub use cash::*;
pub use cash_hedges::*;
use convex_core::{Currency, Date};
use convex_curves::contracts::*;
pub use credit::*;
pub use curves::*;
pub use events::*;
pub use floating_workout::*;
pub use hedges::*;
pub use helpers::*;
pub use icma::*;
pub use inputs::*;
pub use products::*;
pub use reconciliation::*;
pub use replay::*;
pub use requests::*;
pub use risk::*;
pub use scenarios::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use workout::*;
pub use zero::*;

/// Wire contract version, independent of server API and native term revisions.
pub const LIBRARY_SCHEMA_VERSION: u32 = 1;
/// Typed errors keep missing, unsupported, inapplicable and numerical failures distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LibraryErrorCode {
    /// Malformed terms, identity, units or input JSON.
    InvalidInput,
    /// Supplied immutable data is absent.
    MissingInput,
    /// The operation has no financial meaning for this product.
    NotApplicable,
    /// Full economics/model are outside the admitted native implementation.
    Unsupported,
    /// No parameter root in the original admitted domain.
    NoRoot,
    /// Original native inverse is not identified.
    NonUnique,
    /// Native solver/grid/risk postcondition was not met.
    NoConvergence,
    /// Bounded input/output/work admission exhausted.
    ResourceLimit,
    /// Other retained native financial error.
    Analytics,
    /// Output serialization failed; no accepted partial result.
    Internal,
}
/// Complete diagnostic. The original native message/kind remains available.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
#[error("{code:?}: {message}")]
pub struct LibraryError {
    /// Stable category, never a successful zero.
    pub code: LibraryErrorCode,
    /// Original native explanation.
    pub message: String,
    /// Exact missing manifest or native kind; no invented provenance.
    pub details: Value,
}
/// Fallible result with no partial accepted output.
pub type LibraryResult<T> = Result<T, LibraryError>;
impl LibraryError {
    pub(super) fn invalid(field: &str, reason: &str) -> Self {
        Self {
            code: LibraryErrorCode::InvalidInput,
            message: reason.into(),
            details: serde_json::json!({"field":field}),
        }
    }
    pub(super) fn missing(field: &str, details: Value) -> Self {
        Self {
            code: LibraryErrorCode::MissingInput,
            message: format!("missing supplied {field}"),
            details,
        }
    }
    pub(super) fn limit(reason: &str) -> Self {
        Self {
            code: LibraryErrorCode::ResourceLimit,
            message: reason.into(),
            details: Value::Null,
        }
    }
}
pub(super) fn value<T: Serialize>(v: T) -> LibraryResult<Value> {
    serde_json::to_value(v).map_err(|e| LibraryError {
        code: LibraryErrorCode::Internal,
        message: e.to_string(),
        details: Value::Null,
    })
}
/// Execute a complete versioned request against only its supplied values.
pub fn execute(request: &LibraryRequest) -> LibraryResult<Value> {
    if request.schema_version != LIBRARY_SCHEMA_VERSION {
        return Err(LibraryError::invalid(
            "schema_version",
            "unsupported library schema version",
        ));
    }
    let inputs = inputs::ResolvedInputs::new(&request.inputs)?;
    let data = products::execute(&inputs, &request.operation)?;
    Ok(serde_json::json!({"data":data,"market_calibration":inputs.calibration}))
}
/// Parse with duplicate/depth/size bounds, reconstruct native objects and return
/// the existing C envelope convention. Output cash is exact decimal strings.
pub fn execute_json(input: &str) -> String {
    let result = ingress::decode(input).and_then(|r| execute(&r));
    let envelope = match result {
        Ok(result) => {
            serde_json::json!({"ok":"true","schema_version":LIBRARY_SCHEMA_VERSION,"result":result})
        }
        Err(error) => {
            serde_json::json!({"ok":"false","schema_version":LIBRARY_SCHEMA_VERSION,"error":error})
        }
    };
    // Value contains only serializable wire values; to_string is infallible.
    let output = envelope.to_string();
    if output.len() > MAX_OUTPUT_BYTES {
        serde_json::json!({"ok":"false","schema_version":LIBRARY_SCHEMA_VERSION,
            "error":LibraryError::limit("serialized response exceeds16MiB")})
        .to_string()
    } else {
        output
    }
}
/// Decode the complete economic request with the same strict bounded ingress as
/// every client. Construction/financial validation happens only in `execute`.
pub fn decode_request(input: &str) -> LibraryResult<LibraryRequest> {
    ingress::decode(input)
}
/// Serialized response ceiling; rejected output never appears as partial success.
pub const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
/// Exact generated schema of the actual complete request enum and its native terms.
#[cfg(feature = "schemars")]
pub fn request_schema() -> Value {
    schemars::schema_for!(LibraryRequest).to_value()
}

/// Actual dispatch availability, conditional on full native input/model guards.
/// This is a capability report, never an independent library/release certificate.
pub fn capabilities() -> Value {
    match serde_json::from_str::<Value>(include_str!(
        "../../../../tools/library/api-capabilities.json"
    )) {
        Ok(mut report) => {
            report["library_schema_version"] = serde_json::json!(LIBRARY_SCHEMA_VERSION);
            report["bounds"] = serde_json::json!({"input_bytes":2097152,"depth":64,"nodes":32768,"array_items":4096,"text_bytes":8192,"calendars":256,"curves":256,"replay_events":4096,"output_bytes":MAX_OUTPUT_BYTES});
            report
        }
        Err(error) => {
            serde_json::json!({"certified":false,"status":"unavailable","reason":error.to_string()})
        }
    }
}
