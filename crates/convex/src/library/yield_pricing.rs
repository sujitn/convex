//! Thin dispatch over checked native supplied-cash discount laws and inverses.
use super::*;
use convex_bonds::pricing::yield_selection::YieldBranch;
use convex_bonds::pricing::{
    dated_cash::dated_cash_yield_risk_with_basis,
    discount_time::{DatedYieldCash, DiscountTimeBasis, PreparedDatedCash},
};
use convex_bonds::pricing::{yield_cash, YieldSolver};
use convex_core::types::Compounding;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Mathematical supplied-cash operations; no instrument or date context is inferred.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CashYieldOperation {
    /// Signed-cash inverse on an explicitly selected, numerically unique branch.
    SolveSelected {
        /// Complete positive discount years and original signed cash units.
        cash: Vec<(f64, f64)>,
        /// Original mathematical law; no quotation convention is inferred.
        compounding: yield_cash::CashYieldCompounding,
        /// Positive target in original cash units.
        target_dirty_price: f64,
        /// Explicit closed branch and absolute annual-rate precision.
        branch: YieldBranch,
        /// Positive absolute full original-cash price-residual ceiling.
        tolerance: f64,
        /// Bounded native bisection iterations, with no seed or fallback branch.
        max_iterations: u32,
    },
    /// Selected signed inverse retaining complete original dated cash/context.
    DatedSolveSelected {
        /// Actual strictly future cash, including valid zero discount time.
        cash: Vec<DatedYieldCash>,
        /// Original purchase/valuation boundary.
        settlement: Date,
        /// Complete original reference/coupon/termination context.
        basis: DiscountTimeBasis,
        /// Mathematical annual-rate discount law.
        compounding: yield_cash::CashYieldCompounding,
        /// Positive original cash-unit target, without constant-cash subtraction.
        target_dirty_price: f64,
        /// Actual caller-selected closed branch and annual-rate precision.
        branch: YieldBranch,
        /// Full original-cash price-residual ceiling.
        tolerance: f64,
        /// Bounded native bisection iterations.
        max_iterations: u32,
    },
    /// Price actual entitled dated cash on its complete supplied discount basis.
    DatedPrice {
        /// Sorted strictly future cash in original input units.
        cash: Vec<DatedYieldCash>,
        /// Original purchase/valuation boundary.
        settlement: Date,
        /// Complete explicit discount-time context, distinct from compounding.
        basis: DiscountTimeBasis,
        /// Original mathematical discount law.
        compounding: yield_cash::CashYieldCompounding,
        /// Annual decimal yield.
        annual_yield: f64,
    },
    /// Unique nonnegative-cash inverse using the same actual dated basis.
    DatedSolve {
        /// Original sorted strictly future nonnegative cash.
        cash: Vec<DatedYieldCash>,
        /// Original purchase/valuation boundary.
        settlement: Date,
        /// Complete original reference/coupon/termination context.
        basis: DiscountTimeBasis,
        /// Same mathematical discount law for every trial and final reprice.
        compounding: yield_cash::CashYieldCompounding,
        /// Positive target in original cash units.
        target_dirty_price: f64,
        /// Actual finite domain-admitted annual-rate seed.
        initial_guess: f64,
        /// Original positive finite absolute price-residual ceiling.
        tolerance: f64,
        /// Original bounded per-attempt native iteration limit.
        max_iterations: u32,
    },
    /// Analytical price and first/second yield risk over the actual dated cash.
    DatedRisk {
        /// Original sorted strictly future nonnegative cash.
        cash: Vec<DatedYieldCash>,
        /// Original purchase/valuation boundary.
        settlement: Date,
        /// Complete original discount-time context.
        basis: DiscountTimeBasis,
        /// Explicit mathematical annual-rate compounding.
        compounding: Compounding,
        /// Actual annual decimal yield.
        annual_yield: f64,
    },
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
    match action {
        CashYieldOperation::SolveSelected {
            cash,
            compounding,
            target_dirty_price,
            branch,
            tolerance,
            max_iterations,
        } => {
            return selected_result(
                YieldSolver::new()
                    .with_tolerance(*tolerance)
                    .with_max_iterations(*max_iterations)
                    .solve_cash_selected(cash, *target_dirty_price, *compounding, *branch),
                None,
            )
        }
        CashYieldOperation::DatedSolveSelected {
            cash,
            settlement,
            basis,
            compounding,
            target_dirty_price,
            branch,
            tolerance,
            max_iterations,
        } => {
            let prepared =
                PreparedDatedCash::new(cash, *settlement, basis).map_err(selected_error)?;
            return selected_result(
                YieldSolver::new()
                    .with_tolerance(*tolerance)
                    .with_max_iterations(*max_iterations)
                    .solve_dated_cash_selected(
                        &prepared,
                        *target_dirty_price,
                        *compounding,
                        *branch,
                    ),
                Some(prepared.discount_cash()),
            );
        }
        _ => {}
    }
    let result = match action {
        CashYieldOperation::SolveSelected { .. } | CashYieldOperation::DatedSolveSelected { .. } =>
            return Err(LibraryError::invalid("action", "selected yield action was not dispatched")),
        CashYieldOperation::DatedPrice { cash, settlement, basis, compounding, annual_yield } =>
            PreparedDatedCash::new(cash,*settlement,basis).and_then(|prepared|
                prepared.price(*annual_yield,*compounding)
                    .map(|price| json!({"unit":"input_cash", "present_value":price, "discount_cash":prepared.discount_cash()}))),
        CashYieldOperation::DatedSolve { cash, settlement, basis, compounding,
            target_dirty_price, initial_guess, tolerance, max_iterations } =>
            PreparedDatedCash::new(cash,*settlement,basis).and_then(|prepared| YieldSolver::new()
                .with_tolerance(*tolerance).with_max_iterations(*max_iterations)
                .solve_dated_cash(&prepared,*target_dirty_price,*compounding,*initial_guess)
                .map(|result| json!({"unit":"annual_decimal_rate", "yield_value":result.yield_value,
                    "iterations":result.iterations, "residual":result.residual, "residual_unit":"input_cash",
                    "discount_cash":prepared.discount_cash()}))),
        CashYieldOperation::DatedRisk { cash, settlement, basis, compounding, annual_yield } =>
            dated_cash_yield_risk_with_basis(cash,*settlement,*annual_yield,basis,*compounding)
                .map(|risk| json!({"unit":"input_cash", "risk":risk})),
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

fn selected_error(error: convex_bonds::BondError) -> LibraryError {
    use convex_bonds::BondError;
    let (code, scope) = match &error {
        BondError::YieldBranchNoRoot => (LibraryErrorCode::NoRoot, "selected_branch"),
        BondError::YieldNoRoot => (LibraryErrorCode::NoRoot, "constant_cash"),
        BondError::YieldNonUnique => (LibraryErrorCode::NonUnique, "constant_cash"),
        BondError::YieldSelectionUnresolved { .. } | BondError::YieldConvergenceFailed { .. } => {
            (LibraryErrorCode::NoConvergence, "selected_branch")
        }
        _ => (LibraryErrorCode::InvalidInput, "input"),
    };
    LibraryError {
        code,
        message: error.to_string(),
        details: json!({"native_kind":format!("{error:?}"),"root_scope":scope}),
    }
}

fn selected_result(
    result: convex_bonds::BondResult<convex_bonds::pricing::yield_selection::SelectedYieldResult>,
    cash: Option<&[(f64, f64)]>,
) -> LibraryResult<Value> {
    let result = result.map_err(selected_error)?;
    let mut data =
        json!({"unit":"annual_decimal_rate","residual_unit":"input_cash","selection":result});
    if let Some(cash) = cash {
        data["discount_cash"] = json!(cash);
    }
    Ok(data)
}
