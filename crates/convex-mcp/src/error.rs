//! Typed errors for MCP tools.
//!
//! Three categories worth distinguishing on the wire:
//!
//! - `InvalidInput`        → JSON-RPC -32602. Client retry after fixing the call.
//! - `ConvergenceFailure`  → JSON-RPC -32603. Math diverged; client may retry with different inputs.
//! - `CalculationFailed`   → JSON-RPC -32603. Other math/curve errors.
//!
//! The variant tag is mirrored as a snake_case `code` on the JSON-RPC `data`
//! field so agents can program against it without parsing messages.

use rmcp::ErrorData as McpError;
use thiserror::Error;

use convex::AnalyticsError;

#[derive(Debug, Error)]
#[allow(missing_docs)]
pub enum McpToolError {
    /// Caller-side problem: missing id, bad date, NaN, length mismatch, build failure,
    /// unsupported instrument for this tool, etc. Anything the caller can fix and retry.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// A numerical solver did not converge.
    #[error("solver did not converge: {0}")]
    ConvergenceFailure(String),

    /// Catch-all for math/curve failures that are not solver convergence.
    #[error("calculation failed: {0}")]
    CalculationFailed(String),
}

impl From<McpToolError> for McpError {
    fn from(err: McpToolError) -> Self {
        let (code, is_caller) = match &err {
            McpToolError::InvalidInput(_) => ("invalid_input", true),
            McpToolError::ConvergenceFailure(_) => ("convergence_failure", false),
            McpToolError::CalculationFailed(_) => ("calculation_failed", false),
        };
        let data = Some(serde_json::json!({ "code": code }));
        let msg = err.to_string();
        if is_caller {
            McpError::invalid_params(msg, data)
        } else {
            McpError::internal_error(msg, data)
        }
    }
}

impl From<AnalyticsError> for McpToolError {
    fn from(err: AnalyticsError) -> Self {
        match err {
            AnalyticsError::SolverConvergenceFailed { .. }
            | AnalyticsError::YieldSolverFailed { .. } => Self::ConvergenceFailure(err.to_string()),
            AnalyticsError::InvalidInput(msg) => Self::InvalidInput(msg),
            AnalyticsError::InvalidSettlement {
                settlement,
                maturity,
            } => Self::InvalidInput(format!("settlement {settlement} >= maturity {maturity}")),
            other => Self::CalculationFailed(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analytics_solver_failure_surfaces_as_convergence() {
        let analytic = AnalyticsError::SolverConvergenceFailed {
            solver: "Z-spread Brent".into(),
            iterations: 100,
            residual: 1e-3,
        };
        let mcp: McpError = McpToolError::from(analytic).into();
        assert_eq!(mcp.data.as_ref().unwrap()["code"], "convergence_failure");
    }
}
