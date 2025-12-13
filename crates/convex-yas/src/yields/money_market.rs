//! Money Market Yield Calculations.
//!
//! Provides:
//! - T-Bill discount yield
//! - T-Bill bond equivalent yield
//! - CD equivalent yield
//! - Money Market Equivalent Yield for coupon bonds
//! - Newton-Raphson solver for money market yield from price

use crate::YasError;
use convex_bonds::traits::BondCashFlow;
use convex_core::daycounts::DayCount;
use convex_core::types::Date;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use super::{DEFAULT_MAX_ITERATIONS, DEFAULT_TOLERANCE};

// ============================================================================
// Money Market Equivalent Yield (Roll-Forward)
// ============================================================================

/// Calculate money market equivalent yield using roll-forward method.
///
/// This converts a bond's YTM to a money market equivalent yield by simulating
/// the investment over a horizon period, accounting for coupon reinvestment.
///
/// # Formula
///
/// ```text
/// MMY = (Total Value at Horizon / Initial Investment - 1) / (Days / Days per Year)
/// ```
///
/// # Arguments
///
/// * `cash_flows` - Bond cash flows from settlement date forward
/// * `dirty_price` - Dirty price (clean + accrued) as percentage of par
/// * `ytm` - Yield to maturity as decimal (e.g., 0.05 for 5%)
/// * `settlement` - Settlement date
/// * `maturity` - Bond maturity date
/// * `frequency` - Coupon frequency (1=annual, 2=semi-annual, 4=quarterly)
/// * `days_per_year` - Day count basis (360 for USD/EUR, 365 for GBP/AUD)
///
/// # Returns
///
/// Money market yield as decimal (e.g., 0.048 for 4.8%)
///
/// # Example
///
/// ```ignore
/// let mmy = money_market_yield(
///     &cash_flows,
///     dec!(110.5),  // dirty price
///     dec!(0.05),   // 5% YTM
///     settlement,
///     maturity,
///     2,            // semi-annual
///     360,          // USD convention
/// )?;
/// ```
pub fn money_market_yield(
    cash_flows: &[BondCashFlow],
    dirty_price: Decimal,
    ytm: Decimal,
    settlement: Date,
    maturity: Date,
    frequency: u32,
    days_per_year: u32,
) -> Result<Decimal, YasError> {
    money_market_yield_with_horizon(
        cash_flows,
        dirty_price,
        ytm,
        settlement,
        maturity,
        frequency,
        days_per_year,
        days_per_year, // Default horizon = 1 year in the relevant convention
    )
}

/// Calculate money market yield with custom horizon.
///
/// Same as `money_market_yield` but allows specifying a custom horizon in days.
#[allow(clippy::too_many_arguments)]
pub fn money_market_yield_with_horizon(
    cash_flows: &[BondCashFlow],
    dirty_price: Decimal,
    ytm: Decimal,
    settlement: Date,
    maturity: Date,
    frequency: u32,
    days_per_year: u32,
    max_horizon_days: u32,
) -> Result<Decimal, YasError> {
    // Validate inputs
    if cash_flows.is_empty() {
        return Err(YasError::InvalidInput("no cash flows provided".to_string()));
    }

    if dirty_price <= Decimal::ZERO {
        return Err(YasError::InvalidInput(
            "dirty price must be positive".to_string(),
        ));
    }

    // Calculate horizon date (earlier of max_horizon or maturity)
    let max_horizon = settlement.add_days(max_horizon_days as i64);
    let horizon = if maturity < max_horizon {
        maturity
    } else {
        max_horizon
    };

    let days_to_horizon = settlement.days_between(&horizon);
    if days_to_horizon <= 0 {
        return Err(YasError::InvalidInput(
            "horizon must be after settlement".to_string(),
        ));
    }

    // Roll forward to calculate total value at horizon
    let total_value = roll_forward(
        dirty_price,
        cash_flows,
        ytm,
        ytm, // reinvestment rate = YTM
        settlement,
        horizon,
        maturity,
        frequency,
    );

    // MMY = (FV/PV - 1) / t where t = days / days_per_year
    let t = Decimal::from(days_to_horizon) / Decimal::from(days_per_year);

    if t <= Decimal::ZERO {
        return Ok(Decimal::ZERO);
    }

    let mmy = (total_value / dirty_price - Decimal::ONE) / t;
    Ok(mmy)
}

/// Perform roll-forward calculation to get total value at horizon.
#[allow(clippy::too_many_arguments)]
fn roll_forward(
    initial: Decimal,
    cash_flows: &[BondCashFlow],
    ytm: Decimal,
    reinvest_rate: Decimal,
    settlement: Date,
    horizon: Date,
    maturity: Date,
    frequency: u32,
) -> Decimal {
    let ppy = if frequency == 0 {
        Decimal::ONE
    } else {
        Decimal::from(frequency)
    };

    let ytm_per_period = ytm / ppy;
    let reinvest_per_period = reinvest_rate / ppy;

    let mut bond_value = initial;
    let mut reinvested = Decimal::ZERO;
    let mut current_date = settlement;

    // Process cash flows between settlement and horizon
    for cf in cash_flows
        .iter()
        .filter(|c| c.date > settlement && c.date <= horizon)
    {
        let periods = periods_between(current_date, cf.date, frequency);

        // Compound bond value at YTM
        let growth = pow_decimal(Decimal::ONE + ytm_per_period, periods);
        let bond_at_cf = bond_value * growth;

        // Compound reinvested coupons
        let reinvest_growth = pow_decimal(Decimal::ONE + reinvest_per_period, periods);
        reinvested *= reinvest_growth;

        // Update state: subtract CF from bond, add to reinvested
        bond_value = bond_at_cf - cf.amount;
        reinvested += cf.amount;
        current_date = cf.date;
    }

    // Compound to horizon if needed
    if current_date < horizon {
        let periods = periods_between(current_date, horizon, frequency);

        bond_value *= pow_decimal(Decimal::ONE + ytm_per_period, periods);
        reinvested *= pow_decimal(Decimal::ONE + reinvest_per_period, periods);
    }

    // Residual bond value is zero if matured
    let residual = if horizon >= maturity {
        Decimal::ZERO
    } else {
        bond_value
    };

    residual + reinvested
}

/// Calculate periods between two dates.
#[inline]
fn periods_between(from: Date, to: Date, frequency: u32) -> Decimal {
    let days = from.days_between(&to) as f64;
    let years = days / 365.0;
    let periods = years * frequency as f64;
    Decimal::from_f64_retain(periods).unwrap_or(Decimal::ZERO)
}

/// Decimal exponentiation via f64.
#[inline]
fn pow_decimal(base: Decimal, exp: Decimal) -> Decimal {
    let b = base.to_f64().unwrap_or(1.0);
    let e = exp.to_f64().unwrap_or(0.0);
    Decimal::from_f64_retain(b.powf(e)).unwrap_or(Decimal::ONE)
}

// ============================================================================
// Newton-Raphson Solver for Money Market Yield
// ============================================================================

/// Solve for money market yield from dirty price using Newton-Raphson iteration.
///
/// This finds the yield `y` such that the present value of all cash flows,
/// discounted using the money market (add-on) method, equals the dirty price.
///
/// # Formula
///
/// For money market yield, we discount using simple interest:
/// ```text
/// PV = Σ CF_i / (1 + y × t_i)
/// ```
///
/// where `t_i` is the year fraction to each cash flow using the bond's day count.
///
/// # Arguments
///
/// * `cash_flows` - Bond cash flows from settlement forward
/// * `dirty_price` - Target dirty price (clean + accrued)
/// * `settlement` - Settlement date
/// * `day_count` - Day count convention from the bond
/// * `tolerance` - Convergence tolerance (default: 1e-10)
/// * `max_iterations` - Maximum Newton-Raphson iterations (default: 100)
///
/// # Returns
///
/// Money market yield as a decimal (e.g., 0.05 for 5%)
#[allow(clippy::too_many_arguments)]
pub fn solve_money_market_yield(
    cash_flows: &[BondCashFlow],
    dirty_price: Decimal,
    settlement: Date,
    day_count: &dyn DayCount,
    tolerance: Option<f64>,
    max_iterations: Option<u32>,
) -> Result<Decimal, YasError> {
    let tol = tolerance.unwrap_or(DEFAULT_TOLERANCE);
    let max_iter = max_iterations.unwrap_or(DEFAULT_MAX_ITERATIONS);

    if cash_flows.is_empty() {
        return Err(YasError::InvalidInput("no cash flows provided".to_string()));
    }

    if dirty_price <= Decimal::ZERO {
        return Err(YasError::InvalidInput(
            "dirty price must be positive".to_string(),
        ));
    }

    // Filter cash flows to those after settlement
    let future_cfs: Vec<_> = cash_flows
        .iter()
        .filter(|cf| cf.date > settlement)
        .collect();

    if future_cfs.is_empty() {
        return Err(YasError::InvalidInput(
            "no cash flows after settlement".to_string(),
        ));
    }

    // Precompute year fractions for each cash flow
    let year_fracs: Vec<f64> = future_cfs
        .iter()
        .map(|cf| {
            day_count
                .year_fraction(settlement, cf.date)
                .to_f64()
                .unwrap_or(0.0)
        })
        .collect();

    let cf_amounts: Vec<f64> = future_cfs
        .iter()
        .map(|cf| cf.amount.to_f64().unwrap_or(0.0))
        .collect();

    let target = dirty_price.to_f64().unwrap_or(100.0);

    // Initial guess: simple yield approximation
    let total_cf: f64 = cf_amounts.iter().sum();
    let avg_t: f64 = year_fracs.iter().sum::<f64>() / year_fracs.len() as f64;
    let mut y = if avg_t > 0.0 && target > 0.0 {
        (total_cf / target - 1.0) / avg_t
    } else {
        0.05 // fallback
    };

    // Newton-Raphson iteration
    for _ in 0..max_iter {
        let (pv, dpv_dy) = pv_and_derivative_mm(&cf_amounts, &year_fracs, y);

        let f = pv - target;

        if f.abs() < tol {
            return Ok(Decimal::from_f64_retain(y).unwrap_or(Decimal::ZERO));
        }

        if dpv_dy.abs() < 1e-15 {
            return Err(YasError::CalculationFailed(
                "derivative too small in Newton-Raphson".to_string(),
            ));
        }

        let dy = f / dpv_dy;
        y -= dy;

        // Prevent negative yields going too extreme
        if y < -0.99 {
            y = -0.99;
        }
    }

    Err(YasError::CalculationFailed(format!(
        "Newton-Raphson did not converge after {} iterations",
        max_iter
    )))
}

/// Calculate PV and its derivative for money market yield.
///
/// For money market yield with simple discounting:
/// ```text
/// PV = Σ CF_i / (1 + y × t_i)
/// dPV/dy = Σ -CF_i × t_i / (1 + y × t_i)²
/// ```
#[inline]
fn pv_and_derivative_mm(cf_amounts: &[f64], year_fracs: &[f64], y: f64) -> (f64, f64) {
    let mut pv = 0.0;
    let mut dpv = 0.0;

    for (cf, t) in cf_amounts.iter().zip(year_fracs.iter()) {
        let disc = 1.0 + y * t;
        if disc.abs() > 1e-10 {
            pv += cf / disc;
            dpv -= cf * t / (disc * disc);
        }
    }

    (pv, dpv)
}

/// Calculate price from money market yield.
///
/// Given a money market yield, calculate the present value (dirty price)
/// using simple interest discounting.
///
/// # Formula
///
/// ```text
/// PV = Σ CF_i / (1 + y × t_i)
/// ```
pub fn price_from_money_market_yield(
    cash_flows: &[BondCashFlow],
    yield_decimal: Decimal,
    settlement: Date,
    day_count: &dyn DayCount,
) -> Result<Decimal, YasError> {
    if cash_flows.is_empty() {
        return Err(YasError::InvalidInput("no cash flows provided".to_string()));
    }

    let y = yield_decimal.to_f64().unwrap_or(0.0);
    let mut pv = 0.0;

    for cf in cash_flows.iter().filter(|cf| cf.date > settlement) {
        let t = day_count.year_fraction(settlement, cf.date).to_f64().unwrap_or(0.0);
        let disc = 1.0 + y * t;

        if disc.abs() < 1e-10 {
            return Err(YasError::CalculationFailed(
                "discount factor too small".to_string(),
            ));
        }

        pv += cf.amount.to_f64().unwrap_or(0.0) / disc;
    }

    Ok(Decimal::from_f64_retain(pv).unwrap_or(Decimal::ZERO))
}

// ============================================================================
// T-Bill Yield Functions
// ============================================================================

/// Calculate discount yield (bank discount basis).
///
/// Used for T-Bills and other discount instruments.
///
/// # Formula
///
/// ```text
/// Discount Yield = (Face - Price) / Face × (360 / Days) × 100
/// ```
///
/// # Returns
///
/// Discount yield as percentage (e.g., 6.0 for 6%)
pub fn discount_yield(
    price: Decimal,
    face_value: Decimal,
    days_to_maturity: u32,
) -> Result<Decimal, YasError> {
    if days_to_maturity == 0 {
        return Err(YasError::InvalidInput(
            "days to maturity must be positive".to_string(),
        ));
    }

    if face_value <= Decimal::ZERO {
        return Err(YasError::InvalidInput(
            "face value must be positive".to_string(),
        ));
    }

    let discount = face_value - price;
    let days = Decimal::from(days_to_maturity);
    let dy = discount / face_value * (dec!(360) / days) * dec!(100);
    Ok(dy)
}

/// Calculate bond equivalent yield (BEY).
///
/// Converts discount yield to a yield comparable with coupon bonds.
///
/// # Formula (for instruments ≤ 182 days)
///
/// ```text
/// BEY = (Face - Price) / Price × (365 / Days) × 100
/// ```
///
/// # Returns
///
/// Bond equivalent yield as percentage
pub fn bond_equivalent_yield(
    price: Decimal,
    face_value: Decimal,
    days_to_maturity: u32,
) -> Result<Decimal, YasError> {
    if days_to_maturity == 0 {
        return Err(YasError::InvalidInput(
            "days to maturity must be positive".to_string(),
        ));
    }

    if price <= Decimal::ZERO {
        return Err(YasError::InvalidInput("price must be positive".to_string()));
    }

    let discount = face_value - price;
    let days = Decimal::from(days_to_maturity);

    if days_to_maturity <= 182 {
        // Simple formula for short-dated instruments
        let bey = discount / price * (dec!(365) / days) * dec!(100);
        Ok(bey)
    } else {
        // Complex formula for longer instruments
        let d = days_to_maturity as f64;
        let p = price.to_f64().unwrap_or(100.0);
        let f = face_value.to_f64().unwrap_or(100.0);

        let term = d / 365.0;
        let price_factor = 1.0 - f / p;
        let discriminant = term * term + (2.0 * term - 1.0) * price_factor;

        if discriminant < 0.0 {
            return Err(YasError::CalculationFailed(
                "negative discriminant in BEY calculation".to_string(),
            ));
        }

        let bey = (-2.0 * term + 2.0 * discriminant.sqrt()) / (2.0 * term - 1.0);
        Ok(Decimal::from_f64_retain(bey * 100.0).unwrap_or(Decimal::ZERO))
    }
}

/// Calculate CD equivalent yield.
///
/// Compares discount instruments with CDs on add-on interest basis.
///
/// # Formula
///
/// ```text
/// CD Equivalent = (Face - Price) / Price × (360 / Days) × 100
/// ```
pub fn cd_equivalent_yield(
    price: Decimal,
    face_value: Decimal,
    days_to_maturity: u32,
) -> Result<Decimal, YasError> {
    if days_to_maturity == 0 {
        return Err(YasError::InvalidInput(
            "days to maturity must be positive".to_string(),
        ));
    }

    if price <= Decimal::ZERO {
        return Err(YasError::InvalidInput("price must be positive".to_string()));
    }

    let discount = face_value - price;
    let days = Decimal::from(days_to_maturity);
    let cd = discount / price * (dec!(360) / days) * dec!(100);
    Ok(cd)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    // ========================================================================
    // Money Market Yield Tests
    // ========================================================================

    #[test]
    fn test_mmy_single_cash_flow() {
        let settlement = date(2020, 4, 29);
        let maturity = date(2020, 10, 29); // ~6 months

        let cash_flows = vec![BondCashFlow::coupon_and_principal(
            maturity,
            dec!(3.75),
            dec!(100),
        )];

        let mmy = money_market_yield(
            &cash_flows,
            dec!(101.5), // dirty price
            dec!(0.05),  // 5% YTM
            settlement,
            maturity,
            2,   // semi-annual
            360, // USD
        )
        .unwrap();

        assert!(mmy > Decimal::ZERO);
    }

    #[test]
    fn test_mmy_multiple_coupons() {
        let settlement = date(2020, 1, 15);
        let maturity = date(2021, 1, 15);

        let cash_flows = vec![
            BondCashFlow::coupon(date(2020, 7, 15), dec!(2.5)),
            BondCashFlow::coupon_and_principal(maturity, dec!(2.5), dec!(100)),
        ];

        let mmy = money_market_yield(
            &cash_flows,
            dec!(100.0),
            dec!(0.05),
            settlement,
            maturity,
            2,
            360,
        )
        .unwrap();

        // MMY should be positive and reasonable
        assert!(mmy > Decimal::ZERO);
        assert!(mmy < dec!(0.10)); // Less than 10%
    }

    #[test]
    fn test_mmy_with_custom_horizon() {
        let settlement = date(2020, 4, 29);
        let maturity = date(2025, 6, 15); // Long bond

        let cash_flows = vec![
            BondCashFlow::coupon(date(2020, 6, 15), dec!(3.75)),
            BondCashFlow::coupon(date(2020, 12, 15), dec!(3.75)),
        ];

        let mmy = money_market_yield_with_horizon(
            &cash_flows,
            dec!(110.5),
            dec!(0.049),
            settlement,
            maturity,
            2,
            360,
            180, // 6-month horizon
        )
        .unwrap();

        assert!(mmy > Decimal::ZERO);
    }

    #[test]
    fn test_mmy_different_day_bases() {
        let settlement = date(2020, 4, 29);
        let maturity = date(2020, 10, 29);

        let cash_flows = vec![BondCashFlow::coupon_and_principal(
            maturity,
            dec!(3.75),
            dec!(100),
        )];

        let mmy_360 = money_market_yield(
            &cash_flows,
            dec!(100.0),
            dec!(0.05),
            settlement,
            maturity,
            2,
            360, // USD/EUR convention
        )
        .unwrap();

        let mmy_365 = money_market_yield(
            &cash_flows,
            dec!(100.0),
            dec!(0.05),
            settlement,
            maturity,
            2,
            365, // GBP/AUD convention
        )
        .unwrap();

        // Both should be positive and close to YTM
        assert!(mmy_360 > Decimal::ZERO);
        assert!(mmy_365 > Decimal::ZERO);

        // MMY = return / (days/days_per_year)
        // With 365 days_per_year, time fraction is smaller, so MMY is larger
        assert!(mmy_365 > mmy_360);
    }

    #[test]
    fn test_mmy_invalid_inputs() {
        let settlement = date(2020, 4, 29);
        let maturity = date(2020, 10, 29);

        // Empty cash flows
        assert!(
            money_market_yield(&[], dec!(100), dec!(0.05), settlement, maturity, 2, 360).is_err()
        );

        // Zero price
        let cfs = vec![BondCashFlow::coupon(maturity, dec!(5))];
        assert!(
            money_market_yield(&cfs, dec!(0), dec!(0.05), settlement, maturity, 2, 360).is_err()
        );

        // Negative price
        assert!(
            money_market_yield(&cfs, dec!(-100), dec!(0.05), settlement, maturity, 2, 360).is_err()
        );
    }

    // ========================================================================
    // T-Bill Tests
    // ========================================================================

    #[test]
    fn test_discount_yield() {
        let dy = discount_yield(dec!(98.5), dec!(100.0), 90).unwrap();
        // (100 - 98.5) / 100 × (360/90) × 100 = 6.0%
        assert_relative_eq!(dy.to_f64().unwrap(), 6.0, epsilon = 0.01);
    }

    #[test]
    fn test_bond_equivalent_yield_short() {
        let bey = bond_equivalent_yield(dec!(98.5), dec!(100.0), 90).unwrap();
        // (100 - 98.5) / 98.5 × (365/90) × 100 ≈ 6.17%
        assert_relative_eq!(bey.to_f64().unwrap(), 6.17, epsilon = 0.02);
    }

    #[test]
    fn test_cd_equivalent_yield() {
        let cd = cd_equivalent_yield(dec!(98.5), dec!(100.0), 90).unwrap();
        // (100 - 98.5) / 98.5 × (360/90) × 100 ≈ 6.09%
        assert_relative_eq!(cd.to_f64().unwrap(), 6.09, epsilon = 0.02);
    }

    #[test]
    fn test_discount_yield_invalid() {
        assert!(discount_yield(dec!(98.5), dec!(100), 0).is_err());
        assert!(discount_yield(dec!(98.5), dec!(0), 90).is_err());
    }

    #[test]
    fn test_bey_invalid() {
        assert!(bond_equivalent_yield(dec!(0), dec!(100), 90).is_err());
        assert!(bond_equivalent_yield(dec!(98.5), dec!(100), 0).is_err());
    }

    // ========================================================================
    // Newton-Raphson Solver Tests
    // ========================================================================

    #[test]
    fn test_solve_mm_yield_simple_case() {
        use convex_core::daycounts::Act360;

        let settlement = date(2020, 4, 29);
        let maturity = date(2020, 10, 29); // ~6 months

        // Single cash flow: coupon + principal at maturity
        let cash_flows = vec![BondCashFlow::coupon_and_principal(
            maturity,
            dec!(2.5),   // 5% annual coupon, semi-annual = 2.5%
            dec!(100.0), // principal
        )];

        let day_count = Act360;

        // At par, yield should be close to the coupon rate
        let yield_result =
            solve_money_market_yield(&cash_flows, dec!(100.0), settlement, &day_count, None, None);

        assert!(yield_result.is_ok());
        let y = yield_result.unwrap().to_f64().unwrap();

        // Should converge to a reasonable yield
        assert!(y > 0.0);
        assert!(y < 0.15); // Less than 15%
    }

    #[test]
    fn test_solve_mm_yield_discount_bond() {
        use convex_core::daycounts::Act360;

        let settlement = date(2020, 4, 29);
        let maturity = date(2020, 10, 29);

        let cash_flows =
            vec![BondCashFlow::coupon_and_principal(maturity, dec!(2.5), dec!(100.0))];

        let day_count = Act360;

        // Bond trading at discount - yield should be higher
        let yield_result =
            solve_money_market_yield(&cash_flows, dec!(98.0), settlement, &day_count, None, None);

        assert!(yield_result.is_ok());
        let y = yield_result.unwrap().to_f64().unwrap();

        // At discount, yield > coupon rate
        assert!(y > 0.05);
    }

    #[test]
    fn test_solve_mm_yield_premium_bond() {
        use convex_core::daycounts::Act360;

        let settlement = date(2020, 4, 29);
        let maturity = date(2020, 10, 29);

        let cash_flows =
            vec![BondCashFlow::coupon_and_principal(maturity, dec!(2.5), dec!(100.0))];

        let day_count = Act360;

        // Bond trading at premium - yield should be lower
        let yield_result =
            solve_money_market_yield(&cash_flows, dec!(102.0), settlement, &day_count, None, None);

        assert!(yield_result.is_ok());
        let y = yield_result.unwrap().to_f64().unwrap();

        // At premium, yield < coupon rate
        assert!(y < 0.05);
        assert!(y > 0.0);
    }

    #[test]
    fn test_solve_mm_yield_roundtrip() {
        use convex_core::daycounts::Act360;

        let settlement = date(2020, 4, 29);
        let maturity = date(2020, 10, 29);

        let cash_flows =
            vec![BondCashFlow::coupon_and_principal(maturity, dec!(2.5), dec!(100.0))];

        let day_count = Act360;
        let original_price = dec!(99.5);

        // Solve for yield
        let yield_result = solve_money_market_yield(
            &cash_flows,
            original_price,
            settlement,
            &day_count,
            None,
            None,
        )
        .unwrap();

        // Calculate price from yield
        let recovered_price =
            price_from_money_market_yield(&cash_flows, yield_result, settlement, &day_count)
                .unwrap();

        // Should roundtrip within tolerance
        let diff = (original_price - recovered_price).abs();
        assert!(diff < dec!(0.0001), "Roundtrip error: {}", diff);
    }

    #[test]
    fn test_solve_mm_yield_multiple_coupons() {
        use convex_core::daycounts::Act365Fixed;

        let settlement = date(2020, 1, 15);
        let maturity = date(2021, 1, 15);

        let cash_flows = vec![
            BondCashFlow::coupon(date(2020, 7, 15), dec!(2.5)),
            BondCashFlow::coupon_and_principal(maturity, dec!(2.5), dec!(100)),
        ];

        let day_count = Act365Fixed;

        let yield_result =
            solve_money_market_yield(&cash_flows, dec!(100.0), settlement, &day_count, None, None);

        assert!(yield_result.is_ok());
        let y = yield_result.unwrap().to_f64().unwrap();

        // At par with 5% coupon, yield should be close to 5%
        assert_relative_eq!(y, 0.05, epsilon = 0.005);
    }

    #[test]
    fn test_solve_mm_yield_invalid_inputs() {
        use convex_core::daycounts::Act360;

        let settlement = date(2020, 4, 29);
        let maturity = date(2020, 10, 29);
        let day_count = Act360;

        // Empty cash flows
        assert!(
            solve_money_market_yield(&[], dec!(100), settlement, &day_count, None, None).is_err()
        );

        // Zero price
        let cfs = vec![BondCashFlow::coupon(maturity, dec!(5))];
        assert!(
            solve_money_market_yield(&cfs, dec!(0), settlement, &day_count, None, None).is_err()
        );

        // Negative price
        assert!(
            solve_money_market_yield(&cfs, dec!(-100), settlement, &day_count, None, None).is_err()
        );
    }

    #[test]
    fn test_price_from_mm_yield() {
        use convex_core::daycounts::Act360;

        let settlement = date(2020, 4, 29);
        let maturity = date(2020, 10, 29);

        let cash_flows =
            vec![BondCashFlow::coupon_and_principal(maturity, dec!(2.5), dec!(100.0))];

        let day_count = Act360;

        // Calculate price at 5% yield
        let price =
            price_from_money_market_yield(&cash_flows, dec!(0.05), settlement, &day_count).unwrap();

        // PV = CF / (1 + y * t)
        // For ~6 months (183 days), t ≈ 183/360 = 0.5083
        // PV = 102.5 / (1 + 0.05 * 0.5083) ≈ 99.97
        assert!(price > dec!(99.0));
        assert!(price < dec!(103.0));
    }
}
