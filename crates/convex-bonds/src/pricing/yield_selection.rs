//! Explicit branch selection for complete signed-cash yield inverses.
//! Reuses the native cash discount owner; no seed chooses an economic root.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::yield_cash::{self, CashYieldCompounding};
use crate::{BondError, BondResult};
use convex_math::{portable, solvers::SolverConfig};
use serde::{Deserialize, Serialize};

/// Caller-selected closed rate branch and required absolute rate precision.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(try_from = "BranchInput", deny_unknown_fields)]
pub struct YieldBranch {
    lower: f64,
    upper: f64,
    rate_tolerance: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
struct BranchInput {
    lower: f64,
    upper: f64,
    rate_tolerance: f64,
}

impl TryFrom<BranchInput> for YieldBranch {
    type Error = BondError;
    fn try_from(value: BranchInput) -> BondResult<Self> {
        Self::new(value.lower, value.upper, value.rate_tolerance)
    }
}

impl YieldBranch {
    /// Admit only finite ordered endpoints and positive finite rate precision.
    pub fn new(lower: f64, upper: f64, rate_tolerance: f64) -> BondResult<Self> {
        if !lower.is_finite()
            || !upper.is_finite()
            || lower >= upper
            || !rate_tolerance.is_finite()
            || rate_tolerance <= 0.0
        {
            return Err(BondError::invalid_spec(
                "finite ordered yield branch and positive rate tolerance required",
            ));
        }
        Ok(Self {
            lower,
            upper,
            rate_tolerance,
        })
    }
}

/// Original cash-unit residual and numerical branch/precision diagnostics.
#[derive(Debug, Clone, Serialize)]
pub struct SelectedYieldResult {
    /// Annual decimal rate in the caller's branch.
    pub yield_value: f64,
    /// Actual bisection evaluations, excluding endpoint/conditioning checks.
    pub iterations: u32,
    /// Final full original-cash price minus the original target.
    pub residual: f64,
    /// Caller-supplied branch, without extension or guessed selection.
    pub branch: YieldBranch,
    /// Conservative first-derivative bounds across the complete branch.
    pub derivative_bounds: [f64; 2],
    /// Numerical full-price error allowance in original cash units.
    pub price_error_bound: f64,
    /// Absolute rate error bounded by residual and minimum branch slope.
    pub rate_error_bound: f64,
    /// Actual calls to the native cash-value owner, including final repricing.
    pub value_evaluations: u32,
    /// Actual discount terms evaluated by those calls, with no nominal proxy.
    pub discount_term_evaluations: u64,
}

fn unresolved(reason: &str) -> BondError {
    BondError::YieldSelectionUnresolved {
        reason: reason.into(),
    }
}

fn finite(value: f64) -> BondResult<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(unresolved("nonfinite branch arithmetic"))
    }
}

fn value(
    cash: &[(f64, f64)],
    rate: f64,
    basis: CashYieldCompounding,
    dated: bool,
) -> BondResult<(f64, f64)> {
    if dated {
        yield_cash::value_dated_cash(cash, rate, basis)
    } else {
        yield_cash::value_cash(cash, rate, basis)
    }
}

// The locked portable kernels retain one arithmetic owner. Bound sensitivity
// to input/base rounding as well as exp/log and accumulation error. Reject
// subnormal or ill-conditioned terms instead of claiming precise cancellation.
fn relative_allowance(
    time: f64,
    amount: f64,
    price: f64,
    rate: f64,
    basis: CashYieldCompounding,
) -> BondResult<f64> {
    if !price.is_normal() || !amount.is_normal() {
        return Err(unresolved(
            "subnormal branch term is below numerical resolution",
        ));
    }
    let condition = match basis {
        CashYieldCompounding::Annual => (1.0 + rate.abs()) / (1.0 + rate),
        CashYieldCompounding::Periodic { frequency } => {
            (frequency + rate.abs()) / (frequency + rate)
        }
        CashYieldCompounding::Simple => {
            let product = finite(rate * time)?;
            (1.0 + product.abs()) / (1.0 + product)
        }
        CashYieldCompounding::Continuous => 1.0,
    };
    let exponent = (portable::log(price) - portable::log(amount)).abs();
    let relative = finite(32.0 * f64::EPSILON * (4.0 + exponent) * (1.0 + condition))?;
    if !(0.0..=1e-6).contains(&relative) {
        return Err(unresolved(
            "branch term conditioning cannot establish precision",
        ));
    }
    Ok(relative)
}

#[derive(Clone, Copy)]
struct Problem<'a> {
    cash: &'a [(f64, f64)],
    target: f64,
    basis: CashYieldCompounding,
    branch: YieldBranch,
    config: &'a SolverConfig,
    dated: bool,
}
#[derive(Clone, Copy)]
struct BranchBounds {
    derivative_lower: f64,
    derivative_upper: f64,
    price_error: f64,
    slope: f64,
    evaluations: u32,
    terms: u64,
}
pub(crate) fn solve(
    cash: &[(f64, f64)],
    target: f64,
    basis: CashYieldCompounding,
    branch: YieldBranch,
    config: &SolverConfig,
    dated: bool,
) -> BondResult<SelectedYieldResult> {
    config
        .validate()
        .map_err(|error| BondError::invalid_spec(error.to_string()))?;
    if dated {
        yield_cash::validate_dated_cash(cash, false)?;
    } else {
        yield_cash::validate_cash(cash, false)?;
    }
    if !target.is_finite() || target <= 0.0 {
        return Err(BondError::invalid_spec(
            "positive finite yield target required",
        ));
    }
    Problem {
        cash,
        target,
        basis,
        branch,
        config,
        dated,
    }
    .solve()
}

impl Problem<'_> {
    fn bounds(&self) -> BondResult<BranchBounds> {
        let cash = self.cash;
        let basis = self.basis;
        let branch = self.branch;
        let dated = self.dated;
        let mut evaluations = 2_u32;
        let mut terms = 2 * cash.len() as u64;
        let (
            mut derivative_lower,
            mut derivative_upper,
            mut price_error,
            mut sum_prices,
            mut sum_slopes,
        ) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for &(time, amount) in cash {
            if amount == 0.0 {
                continue;
            }
            let absolute = amount.abs();
            let low = value(&[(time, absolute)], branch.lower, basis, dated)?;
            let high = value(&[(time, absolute)], branch.upper, basis, dated)?;
            evaluations += 2;
            terms += 2;
            let allowance = relative_allowance(time, absolute, low.0, branch.lower, basis)?.max(
                relative_allowance(time, absolute, high.0, branch.upper, basis)?,
            );
            let price = low.0.max(high.0);
            let slope = low.1.abs().max(high.1.abs());
            if time > 0.0 && !slope.is_normal() {
                return Err(unresolved("subnormal derivative cannot establish a branch"));
            }
            let (a, b) = if amount > 0.0 {
                (low.1, high.1)
            } else {
                (-high.1, -low.1)
            };
            derivative_lower = finite(derivative_lower + a.min(b) - slope * allowance)?;
            derivative_upper = finite(derivative_upper + a.max(b) + slope * allowance)?;
            // Price excludes the derivative's additional product/base division.
            // Keep its16EPS allowance separate from the32EPS slope allowance.
            price_error = finite(price_error + price * (0.5 * allowance))?;
            sum_prices = finite(sum_prices + price)?;
            sum_slopes = finite(sum_slopes + slope)?;
        }
        let accumulation = (4.0 * cash.len() as f64 + 16.0) * f64::EPSILON;
        price_error = finite(finite(price_error + sum_prices * accumulation)?.next_up())?;
        derivative_lower =
            finite(finite(derivative_lower - sum_slopes * accumulation)?.next_down())?;
        derivative_upper = finite(finite(derivative_upper + sum_slopes * accumulation)?.next_up())?;
        let slope = if derivative_lower > 0.0 {
            derivative_lower
        } else if derivative_upper < 0.0 {
            -derivative_upper
        } else {
            return Err(unresolved(
                "complete branch does not have a resolved strict derivative sign",
            ));
        };
        Ok(BranchBounds {
            derivative_lower,
            derivative_upper,
            price_error,
            slope,
            evaluations,
            terms,
        })
    }
    fn finish(
        &self,
        rate: f64,
        iterations: u32,
        bounds: &BranchBounds,
        evaluations: u32,
        terms: u64,
    ) -> BondResult<SelectedYieldResult> {
        let Self {
            cash,
            target,
            basis,
            branch,
            config,
            dated,
        } = *self;
        let BranchBounds {
            derivative_lower,
            derivative_upper,
            price_error,
            slope,
            ..
        } = *bounds;
        let residual = finite(value(cash, rate, basis, dated)?.0 - target)?;
        let rate_error_bound = finite(finite((residual.abs() + price_error) / slope)?.next_up())?;
        if residual.abs() >= config.tolerance || rate_error_bound > branch.rate_tolerance {
            return Err(unresolved(
                "final full original-cash residual or rate precision is unresolved",
            ));
        }
        Ok(SelectedYieldResult {
            yield_value: rate,
            iterations,
            residual,
            branch,
            derivative_bounds: [derivative_lower, derivative_upper],
            price_error_bound: price_error,
            rate_error_bound,
            value_evaluations: evaluations + 1,
            discount_term_evaluations: terms + cash.len() as u64,
        })
    }
    fn solve(&self) -> BondResult<SelectedYieldResult> {
        let Self {
            cash,
            target,
            basis,
            branch,
            config,
            dated,
        } = *self;
        let left = value(cash, branch.lower, basis, dated)?.0;
        let right = value(cash, branch.upper, basis, dated)?.0;
        if cash
            .iter()
            .all(|(time, amount)| *time == 0.0 || *amount == 0.0)
        {
            return Err(if left == target {
                BondError::YieldNonUnique
            } else {
                BondError::YieldNoRoot
            });
        }
        let bounds = self.bounds()?;
        let price_error = bounds.price_error;
        let slope = bounds.slope;
        if left.min(right) <= price_error
            || price_error >= config.tolerance
            || price_error / slope >= branch.rate_tolerance
        {
            return Err(unresolved(
                "complete branch cannot establish requested price/rate precision",
            ));
        }
        let mut evaluations = bounds.evaluations;
        let mut terms = bounds.terms;
        for (rate, price) in [(branch.lower, left), (branch.upper, right)] {
            if price == target {
                return self.finish(rate, 0, &bounds, evaluations, terms);
            }
        }
        let mut lower = branch.lower;
        let mut upper = branch.upper;
        let mut low_residual = finite(left - target)?;
        let high_residual = finite(right - target)?;
        if low_residual.is_sign_positive() == high_residual.is_sign_positive() {
            if low_residual.abs().min(high_residual.abs()) <= price_error {
                return Err(unresolved(
                    "endpoint price uncertainty cannot establish branch root existence",
                ));
            }
            return Err(BondError::YieldBranchNoRoot);
        }
        for iteration in 1..=config.max_iterations {
            let middle = lower * 0.5 + upper * 0.5;
            if middle <= lower || middle >= upper {
                return Err(unresolved("branch rate resolution exhausted"));
            }
            let residual = finite(value(cash, middle, basis, dated)?.0 - target)?;
            evaluations += 1;
            terms += cash.len() as u64;
            if residual.abs() < config.tolerance
                && (residual.abs() + price_error) / slope <= branch.rate_tolerance
            {
                return self.finish(middle, iteration, &bounds, evaluations, terms);
            }
            if residual.abs() <= price_error {
                return Err(unresolved(
                    "price uncertainty cannot refine the selected branch",
                ));
            }
            if residual.is_sign_positive() == low_residual.is_sign_positive() {
                lower = middle;
                low_residual = residual;
            } else {
                upper = middle;
            }
        }
        Err(BondError::YieldConvergenceFailed {
            iterations: config.max_iterations,
        })
    }
}
