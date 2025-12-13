//! Street convention yield calculation.
//!
//! The street convention yield is the standard market quote for bond yields.
//! It assumes reinvestment at the yield rate and standard day count conventions.
//!
//! This module uses `GenericYieldSolver` from `convex-pricing` for the core
//! calculations, providing a thin wrapper for backward compatibility.

use crate::YasError;
use convex_core::types::{CashFlow, Date};
use convex_pricing::GenericYieldSolver;
use rust_decimal::Decimal;

/// Calculate street convention yield using GenericYieldSolver.
///
/// This is the standard yield-to-maturity calculation using market conventions.
///
/// # Arguments
///
/// * `dirty_price` - Dirty price (clean + accrued) as percentage of par
/// * `cash_flows` - Vector of cash flow amounts
/// * `times` - Vector of times to each cash flow (in years)
/// * `frequency` - Compounding frequency per year
/// * `_initial_guess` - Initial yield guess (ignored, solver uses adaptive guessing)
///
/// # Returns
///
/// Street convention yield as decimal (e.g., 0.05 for 5%)
pub fn street_convention_yield(
    dirty_price: f64,
    cash_flows: &[f64],
    times: &[f64],
    frequency: u32,
    _initial_guess: f64,
) -> Result<Decimal, YasError> {
    if cash_flows.len() != times.len() {
        return Err(YasError::InvalidInput(
            "cash_flows and times must have same length".to_string(),
        ));
    }

    if cash_flows.is_empty() {
        return Err(YasError::InvalidInput("no cash flows provided".to_string()));
    }

    // Convert to CashFlow format for GenericYieldSolver
    // We use a reference date and create cash flows at the appropriate times
    let ref_date = Date::from_ymd(2000, 1, 1).unwrap();
    let core_cash_flows = convert_to_cash_flows(cash_flows, times, ref_date);

    let solver = GenericYieldSolver::new();

    solver
        .solve_yield_f64(&core_cash_flows, dirty_price, ref_date, frequency)
        .map(|y| Decimal::from_f64_retain(y).unwrap_or(Decimal::ZERO))
        .map_err(|e| YasError::SolverNoConvergence {
            context: format!("street convention yield: {e}"),
            iterations: 100,
        })
}

/// Calculate street convention yield from CashFlow slice.
///
/// This is the preferred method when you already have `CashFlow` objects.
pub fn street_convention_yield_from_cash_flows(
    dirty_price: f64,
    cash_flows: &[CashFlow],
    settlement: Date,
    frequency: u32,
) -> Result<Decimal, YasError> {
    if cash_flows.is_empty() {
        return Err(YasError::InvalidInput("no cash flows provided".to_string()));
    }

    let solver = GenericYieldSolver::new();

    solver
        .solve_yield_f64(cash_flows, dirty_price, settlement, frequency)
        .map(|y| Decimal::from_f64_retain(y).unwrap_or(Decimal::ZERO))
        .map_err(|e| YasError::SolverNoConvergence {
            context: format!("street convention yield: {e}"),
            iterations: 100,
        })
}

/// Calculate price from yield using GenericYieldSolver.
pub fn price_from_yield(
    yield_value: f64,
    cash_flows: &[CashFlow],
    settlement: Date,
    frequency: u32,
) -> Result<f64, YasError> {
    if cash_flows.is_empty() {
        return Err(YasError::InvalidInput("no cash flows provided".to_string()));
    }

    let solver = GenericYieldSolver::new();

    solver
        .price_from_yield_f64(cash_flows, yield_value, settlement, frequency)
        .map_err(|e| YasError::CalculationFailed(format!("price from yield: {e}")))
}

/// Convert raw cash flow arrays to CashFlow objects.
fn convert_to_cash_flows(amounts: &[f64], times: &[f64], ref_date: Date) -> Vec<CashFlow> {
    use convex_core::types::CashFlowType;

    amounts
        .iter()
        .zip(times.iter())
        .map(|(amount, time)| {
            let days = (time * 365.0).round() as i64;
            let cf_date = ref_date + days;
            CashFlow::new(
                cf_date,
                Decimal::from_f64_retain(*amount).unwrap_or(Decimal::ZERO),
                CashFlowType::Coupon,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_street_convention_par_bond() {
        // 5% coupon, 2-year, semi-annual, priced at par
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];
        let dirty_price = 100.0;

        let ytm = street_convention_yield(dirty_price, &cash_flows, &times, 2, 0.05).unwrap();

        // At par, YTM should equal coupon rate
        assert_relative_eq!(
            ytm.to_string().parse::<f64>().unwrap(),
            0.05,
            epsilon = 0.0001
        );
    }

    #[test]
    fn test_street_convention_premium_bond() {
        // 5% coupon, 2-year, semi-annual, priced at 102
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];
        let dirty_price = 102.0;

        let ytm = street_convention_yield(dirty_price, &cash_flows, &times, 2, 0.05).unwrap();

        // Premium bond should have YTM < coupon
        assert!(ytm.to_string().parse::<f64>().unwrap() < 0.05);
    }

    #[test]
    fn test_street_convention_discount_bond() {
        // 5% coupon, 2-year, semi-annual, priced at 98
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];
        let dirty_price = 98.0;

        let ytm = street_convention_yield(dirty_price, &cash_flows, &times, 2, 0.05).unwrap();

        // Discount bond should have YTM > coupon
        assert!(ytm.to_string().parse::<f64>().unwrap() > 0.05);
    }

    #[test]
    fn test_street_convention_from_cash_flows() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();

        let cash_flows = vec![
            CashFlow::new(
                settlement.add_months(6).unwrap(),
                Decimal::from_f64_retain(2.5).unwrap(),
                convex_core::types::CashFlowType::Coupon,
            ),
            CashFlow::new(
                settlement.add_months(12).unwrap(),
                Decimal::from_f64_retain(2.5).unwrap(),
                convex_core::types::CashFlowType::Coupon,
            ),
            CashFlow::new(
                settlement.add_months(18).unwrap(),
                Decimal::from_f64_retain(2.5).unwrap(),
                convex_core::types::CashFlowType::Coupon,
            ),
            CashFlow::new(
                settlement.add_months(24).unwrap(),
                Decimal::from_f64_retain(102.5).unwrap(),
                convex_core::types::CashFlowType::CouponAndPrincipal,
            ),
        ];

        let ytm =
            street_convention_yield_from_cash_flows(100.0, &cash_flows, settlement, 2).unwrap();

        // At par, YTM should be close to coupon rate
        assert_relative_eq!(
            ytm.to_string().parse::<f64>().unwrap(),
            0.05,
            epsilon = 0.001
        );
    }
}
