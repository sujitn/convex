//! Thin dispatch over checked native supplied-cash discount laws and inverses.
use super::*;
use convex_bonds::pricing::{yield_cash, YieldSolver};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Mathematical supplied-cash operations; no instrument or date context is inferred.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CashYieldOperation {
    /// Price in the unchanged input cash units.
    Price {
        /// Explicit positive discount years and signed cash amounts, in input order.
        cash: Vec<(f64, f64)>,
        /// Explicit mathematical discount law.
        compounding: yield_cash::CashYieldCompounding,
        /// Annual decimal yield.
        annual_yield: f64,
    },
    /// Unique inverse for nonnegative cash, retaining actual native diagnostics.
    Solve {
        /// Explicit positive years and nonnegative amounts with positive exposure.
        cash: Vec<(f64, f64)>,
        /// Same mathematical law used for every trial and final repricing.
        compounding: yield_cash::CashYieldCompounding,
        /// Positive target in the input cash units.
        target_dirty_price: f64,
        /// Actual finite starting annual decimal rate in the discount-law domain.
        initial_guess: f64,
        /// Positive finite absolute native price-residual bound.
        tolerance: f64,
        /// Native per-attempt iteration bound, checked against its hard ceiling.
        max_iterations: u32,
    },
}

pub(super) fn execute(action: &CashYieldOperation) -> LibraryResult<Value> {
    let result = match action {
        CashYieldOperation::Price { cash, compounding, annual_yield } =>
            yield_cash::price_cash(cash, *annual_yield, *compounding)
                .map(|price| json!({"unit":"input_cash", "present_value":price})),
        CashYieldOperation::Solve { cash, compounding, target_dirty_price,
            initial_guess, tolerance, max_iterations } => YieldSolver::new()
                .with_tolerance(*tolerance).with_max_iterations(*max_iterations)
                .solve_cash(cash, *target_dirty_price, *compounding, *initial_guess)
                .map(|result| json!({"unit":"annual_decimal_rate", "yield_value":result.yield_value,
                    "iterations":result.iterations, "residual":result.residual, "residual_unit":"input_cash"})),
    };
    result.map_err(|error| LibraryError {
        code: LibraryErrorCode::InvalidInput,
        message: error.to_string(),
        details: json!({"native_kind":format!("{error:?}")}),
    })
}
