//! Sinking fund bond implementation.
//!
//! Provides sinking fund bonds with:
//! - Scheduled mandatory principal redemptions
//! - Delivery option (issuer can deliver bonds instead of cash)
//! - Acceleration/double-up provisions
//! - Average life and yield-to-average-life calculations
//! - Factor-adjusted cash flows

use rust_decimal::Decimal;

use convex_core::types::{Currency, Date};
use convex_math::solvers::{newton_raphson, SolverConfig};

use crate::error::{BondError, BondResult};
use crate::instruments::FixedRateBond;
use crate::traits::{AmortizingBond, Bond, BondCashFlow, CashFlowType, FixedCouponBond};
use crate::types::{AmortizationSchedule, AmortizationType, BondIdentifiers, BondType, CalendarId};

/// A single sinking fund payment.
///
/// Represents a mandatory redemption where the issuer must retire
/// a specified amount of bonds at the sinking fund price.
#[derive(Debug, Clone, PartialEq)]
pub struct SinkingFundPayment {
    /// Date of the sinking fund payment
    pub date: Date,
    /// Par amount to be retired (as percentage of original face)
    pub amount_pct: f64,
    /// Price paid for redemption (percentage of par, usually 100)
    pub price: f64,
}

impl SinkingFundPayment {
    /// Creates a new sinking fund payment.
    #[must_use]
    pub fn new(date: Date, amount_pct: f64) -> Self {
        Self {
            date,
            amount_pct,
            price: 100.0, // Par redemption by default
        }
    }

    /// Creates a sinking fund payment with custom price.
    #[must_use]
    pub fn with_price(date: Date, amount_pct: f64, price: f64) -> Self {
        Self {
            date,
            amount_pct,
            price,
        }
    }

    /// Returns the amount as a decimal fraction.
    #[must_use]
    pub fn amount_decimal(&self) -> f64 {
        self.amount_pct / 100.0
    }
}

/// Acceleration option for sinking funds (double-up provision).
///
/// Allows the issuer to redeem additional bonds beyond the
/// mandatory sinking fund amount.
#[derive(Debug, Clone, PartialEq)]
pub struct AccelerationOption {
    /// Multiple of regular sinking fund (e.g., 2.0 = double-up)
    pub multiple: f64,
    /// Maximum total that can be accelerated (as pct of original)
    pub max_amount_pct: Option<f64>,
}

impl AccelerationOption {
    /// Creates a double-up provision (2x the regular amount).
    #[must_use]
    pub fn double_up() -> Self {
        Self {
            multiple: 2.0,
            max_amount_pct: None,
        }
    }

    /// Creates a triple-up provision.
    #[must_use]
    pub fn triple_up() -> Self {
        Self {
            multiple: 3.0,
            max_amount_pct: None,
        }
    }

    /// Creates a custom acceleration with maximum.
    #[must_use]
    pub fn custom(multiple: f64, max_pct: Option<f64>) -> Self {
        Self {
            multiple,
            max_amount_pct: max_pct,
        }
    }
}

/// Sinking fund schedule defining mandatory redemptions.
///
/// A sinking fund requires the issuer to periodically retire
/// a portion of the outstanding bonds, reducing credit risk
/// over time.
///
/// # Features
///
/// - **Mandatory Payments**: Required principal repayments
/// - **Delivery Option**: Issuer can deliver bonds purchased in market
/// - **Acceleration**: Double-up or higher provisions
///
/// # Example
///
/// ```rust,ignore
/// use convex_bonds::instruments::SinkingFundSchedule;
///
/// // 20% annual sinking fund starting year 5
/// let schedule = SinkingFundSchedule::new()
///     .with_payment(SinkingFundPayment::new(date!(2025-06-15), 20.0))
///     .with_payment(SinkingFundPayment::new(date!(2026-06-15), 20.0))
///     .with_payment(SinkingFundPayment::new(date!(2027-06-15), 20.0))
///     .with_payment(SinkingFundPayment::new(date!(2028-06-15), 20.0))
///     .with_delivery_option()
///     .with_double_up();
/// ```
#[derive(Debug, Clone)]
pub struct SinkingFundSchedule {
    /// Individual sinking fund payments
    payments: Vec<SinkingFundPayment>,
    /// Whether sinking fund is mandatory (true) or optional (false)
    is_mandatory: bool,
    /// Whether issuer can deliver bonds instead of cash
    delivery_option: bool,
    /// Acceleration provision (double-up, etc.)
    acceleration: Option<AccelerationOption>,
}

impl SinkingFundSchedule {
    /// Creates a new empty sinking fund schedule.
    #[must_use]
    pub fn new() -> Self {
        Self {
            payments: Vec::new(),
            is_mandatory: true,
            delivery_option: false,
            acceleration: None,
        }
    }

    /// Creates a schedule from a vector of payments.
    #[must_use]
    pub fn from_payments(payments: Vec<SinkingFundPayment>) -> Self {
        Self {
            payments,
            is_mandatory: true,
            delivery_option: false,
            acceleration: None,
        }
    }

    /// Adds a payment to the schedule.
    #[must_use]
    pub fn with_payment(mut self, payment: SinkingFundPayment) -> Self {
        self.payments.push(payment);
        self
    }

    /// Sets the schedule as optional (not mandatory).
    #[must_use]
    pub fn as_optional(mut self) -> Self {
        self.is_mandatory = false;
        self
    }

    /// Enables delivery option.
    #[must_use]
    pub fn with_delivery_option(mut self) -> Self {
        self.delivery_option = true;
        self
    }

    /// Enables double-up provision.
    #[must_use]
    pub fn with_double_up(mut self) -> Self {
        self.acceleration = Some(AccelerationOption::double_up());
        self
    }

    /// Enables custom acceleration.
    #[must_use]
    pub fn with_acceleration(mut self, acceleration: AccelerationOption) -> Self {
        self.acceleration = Some(acceleration);
        self
    }

    /// Returns true if this schedule has delivery option.
    #[must_use]
    pub fn has_delivery_option(&self) -> bool {
        self.delivery_option
    }

    /// Returns true if this schedule is mandatory.
    #[must_use]
    pub fn is_mandatory(&self) -> bool {
        self.is_mandatory
    }

    /// Returns the acceleration option if any.
    #[must_use]
    pub fn acceleration(&self) -> Option<&AccelerationOption> {
        self.acceleration.as_ref()
    }

    /// Returns remaining principal after sinking fund payments.
    ///
    /// # Arguments
    ///
    /// * `settlement` - Date to calculate remaining principal as of
    ///
    /// # Returns
    ///
    /// Remaining principal as a percentage of original face (0.0 to 100.0)
    #[must_use]
    pub fn remaining_principal_pct(&self, settlement: Date) -> f64 {
        let retired: f64 = self
            .payments
            .iter()
            .filter(|p| p.date <= settlement)
            .map(|p| p.amount_pct)
            .sum();

        (100.0 - retired).max(0.0)
    }

    /// Returns the factor (remaining / original) as of a date.
    #[must_use]
    pub fn factor(&self, settlement: Date) -> f64 {
        self.remaining_principal_pct(settlement) / 100.0
    }

    /// Calculates the average life of the sinking fund.
    ///
    /// Average life = `sum(time_i` * `principal_i`) / `total_principal`
    ///
    /// # Arguments
    ///
    /// * `settlement` - Settlement date
    /// * `maturity` - Final maturity date for remaining principal
    ///
    /// # Returns
    ///
    /// Weighted average life in years
    #[must_use]
    pub fn average_life(&self, settlement: Date, maturity: Date) -> f64 {
        let remaining_pct = self.remaining_principal_pct(settlement);

        if remaining_pct <= 0.0 {
            return 0.0;
        }

        let mut weighted_time = 0.0_f64;
        let mut total_principal = 0.0_f64;

        // Weight by sinking fund payments after settlement
        for payment in &self.payments {
            if payment.date > settlement {
                let years = settlement.days_between(&payment.date) as f64 / 365.0;
                weighted_time += years * payment.amount_pct;
                total_principal += payment.amount_pct;
            }
        }

        // Add final maturity for remaining principal
        let final_principal = remaining_pct - total_principal;
        if final_principal > 0.0 {
            let years = settlement.days_between(&maturity) as f64 / 365.0;
            weighted_time += years * final_principal;
        }

        weighted_time / remaining_pct
    }

    /// Returns all payment dates after settlement.
    #[must_use]
    pub fn payment_dates_from(&self, settlement: Date) -> Vec<Date> {
        self.payments
            .iter()
            .filter(|p| p.date > settlement)
            .map(|p| p.date)
            .collect()
    }

    /// Returns the payment on a specific date, if any.
    #[must_use]
    pub fn payment_on(&self, date: Date) -> Option<&SinkingFundPayment> {
        self.payments.iter().find(|p| p.date == date)
    }

    /// Returns total percentage to be sunk.
    #[must_use]
    pub fn total_sinking_pct(&self) -> f64 {
        self.payments.iter().map(|p| p.amount_pct).sum()
    }

    /// Converts to an `AmortizationSchedule` for use with standard calculations.
    #[must_use]
    pub fn to_amortization_schedule(&self) -> AmortizationSchedule {
        let mut schedule = AmortizationSchedule::new(AmortizationType::SinkingFund);
        for payment in &self.payments {
            schedule = schedule.with_entry(crate::types::AmortizationEntry::new(
                payment.date,
                payment.amount_pct,
            ));
        }
        schedule.compute_remaining_factors();
        schedule
    }

    /// Sorts payments by date.
    pub fn sort_payments(&mut self) {
        self.payments.sort_by_key(|p| p.date);
    }
}

impl Default for SinkingFundSchedule {
    fn default() -> Self {
        Self::new()
    }
}

/// A sinking fund bond wrapping a fixed rate bond with mandatory redemptions.
///
/// Sinking fund bonds have scheduled principal repayments that reduce
/// the outstanding amount over time, providing credit protection to holders.
///
/// # Features
///
/// - Factor-adjusted cash flows based on remaining principal
/// - Average life calculation
/// - Yield-to-average-life calculation
/// - Delivery option and acceleration support
///
/// # Performance
///
/// - Average life: < 500ns
/// - Factor calculation: < 100ns
/// - Cash flow generation: < 2μs
///
/// # Example
///
/// ```rust,ignore
/// use convex_bonds::instruments::{SinkingFundBond, SinkingFundSchedule, SinkingFundPayment};
///
/// let schedule = SinkingFundSchedule::new()
///     .with_payment(SinkingFundPayment::new(date!(2025-06-15), 25.0))
///     .with_payment(SinkingFundPayment::new(date!(2026-06-15), 25.0))
///     .with_payment(SinkingFundPayment::new(date!(2027-06-15), 25.0))
///     .with_payment(SinkingFundPayment::new(date!(2028-06-15), 25.0))
///     .with_double_up();
///
/// let sf_bond = SinkingFundBond::new(base_bond, schedule);
///
/// // Get current factor
/// let factor = sf_bond.factor(settlement);
///
/// // Calculate average life
/// let avg_life = sf_bond.average_life(settlement);
///
/// // Yield to average life
/// let ytal = sf_bond.yield_to_average_life(dec!(100), settlement)?;
/// ```
#[derive(Debug, Clone)]
pub struct SinkingFundBond {
    /// Underlying fixed-rate bond
    base: FixedRateBond,
    /// Sinking fund schedule
    sinking_schedule: SinkingFundSchedule,
    /// Original face value (for factor calculations)
    original_face: Decimal,
    /// Cached amortization schedule
    amortization: AmortizationSchedule,
}

impl SinkingFundBond {
    /// Creates a new sinking fund bond.
    #[must_use]
    pub fn new(base: FixedRateBond, sinking_schedule: SinkingFundSchedule) -> Self {
        let original_face = base.face_value();
        let amortization = sinking_schedule.to_amortization_schedule();

        Self {
            base,
            sinking_schedule,
            original_face,
            amortization,
        }
    }

    /// Creates a builder for sinking fund bonds.
    #[must_use]
    pub fn builder() -> SinkingFundBondBuilder {
        SinkingFundBondBuilder::default()
    }

    /// Returns a reference to the underlying bond.
    #[must_use]
    pub fn base_bond(&self) -> &FixedRateBond {
        &self.base
    }

    /// Returns a reference to the sinking fund schedule.
    #[must_use]
    pub fn sinking_schedule(&self) -> &SinkingFundSchedule {
        &self.sinking_schedule
    }

    /// Returns the original face value.
    #[must_use]
    pub fn original_face(&self) -> Decimal {
        self.original_face
    }

    /// Returns the current factor (remaining principal / original).
    #[must_use]
    pub fn current_factor(&self, settlement: Date) -> f64 {
        self.sinking_schedule.factor(settlement)
    }

    /// Returns the average life from settlement.
    #[must_use]
    pub fn average_life(&self, settlement: Date) -> f64 {
        let maturity = self.base.maturity().unwrap();
        self.sinking_schedule.average_life(settlement, maturity)
    }

    /// Returns the remaining principal at settlement.
    #[must_use]
    pub fn remaining_principal(&self, settlement: Date) -> Decimal {
        let factor = Decimal::try_from(self.current_factor(settlement)).unwrap_or(Decimal::ONE);
        self.original_face * factor
    }

    /// Calculates yield to average life.
    ///
    /// This calculates the yield assuming the bond is retired at its
    /// average life date.
    ///
    /// # Arguments
    ///
    /// * `clean_price` - Market clean price (percentage of par)
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// Yield to average life as a decimal.
    pub fn yield_to_average_life(
        &self,
        clean_price: Decimal,
        settlement: Date,
    ) -> BondResult<Decimal> {
        let avg_life_years = self.average_life(settlement);
        let avg_life_days = (avg_life_years * 365.0) as i64;
        let avg_life_date = settlement + avg_life_days;

        // Generate cash flows to average life
        let flows = self.cash_flows_to_date(settlement, avg_life_date);

        if flows.is_empty() {
            return Err(BondError::invalid_spec("no cash flows to average life"));
        }

        self.solve_yield(&flows, clean_price, settlement)
    }

    /// Generates factor-adjusted cash flows to a specific date.
    fn cash_flows_to_date(&self, settlement: Date, end_date: Date) -> Vec<BondCashFlow> {
        let mut flows = Vec::new();
        let maturity = self.base.maturity().unwrap();
        let coupon_rate = self.base.coupon_rate();
        let freq = self.base.coupon_frequency();

        // Get all base cash flow dates
        let base_flows = self.base.cash_flows(settlement);

        // Track remaining principal
        let mut remaining_factor = self.current_factor(settlement);

        for cf in base_flows {
            if cf.date > end_date {
                break;
            }

            // Check for sinking fund payment on this date
            if let Some(sf_payment) = self.sinking_schedule.payment_on(cf.date) {
                let sf_amount = self.original_face
                    * Decimal::try_from(sf_payment.amount_pct / 100.0).unwrap_or(Decimal::ZERO)
                    * Decimal::try_from(sf_payment.price / 100.0).unwrap_or(Decimal::ONE);

                // Add sinking fund redemption
                flows.push(BondCashFlow {
                    date: cf.date,
                    amount: sf_amount,
                    flow_type: CashFlowType::Principal,
                    accrual_start: cf.accrual_start,
                    accrual_end: cf.accrual_end,
                    factor: Decimal::try_from(remaining_factor).unwrap_or(Decimal::ONE),
                    reference_rate: None,
                });

                // Update remaining factor
                remaining_factor -= sf_payment.amount_pct / 100.0;
                remaining_factor = remaining_factor.max(0.0);
            }

            // Add factor-adjusted coupon
            if cf.is_coupon() || cf.is_principal() {
                let coupon_factor = Decimal::try_from(remaining_factor).unwrap_or(Decimal::ONE);
                let coupon_amount = if freq > 0 {
                    self.original_face * coupon_rate * coupon_factor / Decimal::from(freq)
                } else {
                    Decimal::ZERO
                };

                if coupon_amount > Decimal::ZERO {
                    flows.push(
                        BondCashFlow::coupon(cf.date, coupon_amount)
                            .with_accrual(
                                cf.accrual_start.unwrap_or(settlement),
                                cf.accrual_end.unwrap_or(cf.date),
                            )
                            .with_factor(coupon_factor),
                    );
                }
            }
        }

        // Add final redemption at end_date if there's remaining principal
        if remaining_factor > 0.0 && end_date <= maturity {
            let final_amount =
                self.original_face * Decimal::try_from(remaining_factor).unwrap_or(Decimal::ZERO);
            flows.push(BondCashFlow::principal(end_date, final_amount));
        }

        flows
    }

    /// Solves for yield given cash flows and target price.
    fn solve_yield(
        &self,
        flows: &[BondCashFlow],
        clean_price: Decimal,
        settlement: Date,
    ) -> BondResult<Decimal> {
        let factor = self.current_factor(settlement);
        let adjusted_price = clean_price * Decimal::try_from(factor).unwrap_or(Decimal::ONE);
        let accrued = self.accrued_interest(settlement);
        let target_dirty = adjusted_price + accrued;
        let target = target_dirty.to_string().parse::<f64>().unwrap_or(100.0);

        let freq = f64::from(self.base.coupon_frequency());
        let coupon_rate = self
            .base
            .coupon_rate()
            .to_string()
            .parse::<f64>()
            .unwrap_or(0.05);
        let initial_guess = coupon_rate;

        // Collect flow data for closures
        let flow_data: Vec<(f64, f64)> = flows
            .iter()
            .map(|cf| {
                let t = settlement.days_between(&cf.date) as f64 / 365.0;
                let amount = cf.amount.to_string().parse::<f64>().unwrap_or(0.0);
                (t, amount)
            })
            .collect();

        if flow_data.is_empty() {
            return Err(BondError::invalid_spec(
                "no cash flows for yield calculation",
            ));
        }

        let objective = |y: f64| {
            let mut pv = 0.0;
            for &(t, amount) in &flow_data {
                let df = 1.0 / (1.0 + y / freq).powf(freq * t);
                pv += amount * df;
            }
            pv - target
        };

        let derivative = |y: f64| {
            let mut dpv = 0.0;
            for &(t, amount) in &flow_data {
                let df = 1.0 / (1.0 + y / freq).powf(freq * t);
                dpv += amount * (-t) * df / (1.0 + y / freq);
            }
            dpv
        };

        let config = SolverConfig::new(1e-10, 100);
        let result = newton_raphson(objective, derivative, initial_guess, &config)
            .map_err(|_| BondError::YieldConvergenceFailed { iterations: 100 })?;

        Ok(Decimal::from_f64_retain(result.root).unwrap_or(Decimal::ZERO))
    }
}

// Implement Bond trait
impl Bond for SinkingFundBond {
    fn identifiers(&self) -> &BondIdentifiers {
        self.base.identifiers()
    }

    fn bond_type(&self) -> BondType {
        BondType::SinkingFund
    }

    fn currency(&self) -> Currency {
        self.base.currency()
    }

    fn maturity(&self) -> Option<Date> {
        self.base.maturity()
    }

    fn issue_date(&self) -> Date {
        self.base.issue_date()
    }

    fn first_settlement_date(&self) -> Date {
        self.base.first_settlement_date()
    }

    fn dated_date(&self) -> Date {
        self.base.dated_date()
    }

    fn face_value(&self) -> Decimal {
        self.original_face
    }

    fn cash_flows(&self, from: Date) -> Vec<BondCashFlow> {
        let maturity = self.base.maturity().unwrap();
        self.cash_flows_to_date(from, maturity)
    }

    fn next_coupon_date(&self, after: Date) -> Option<Date> {
        self.base.next_coupon_date(after)
    }

    fn previous_coupon_date(&self, before: Date) -> Option<Date> {
        self.base.previous_coupon_date(before)
    }

    fn accrued_interest(&self, settlement: Date) -> Decimal {
        // Accrued is based on remaining principal
        let factor = Decimal::try_from(self.current_factor(settlement)).unwrap_or(Decimal::ONE);
        self.base.accrued_interest(settlement) * factor
    }

    fn day_count_convention(&self) -> &str {
        self.base.day_count_convention()
    }

    fn calendar(&self) -> &CalendarId {
        self.base.calendar()
    }

    fn redemption_value(&self) -> Decimal {
        // Final redemption is remaining principal at maturity
        let maturity = self.base.maturity().unwrap();
        self.remaining_principal(maturity)
    }
}

// Implement FixedCouponBond
impl FixedCouponBond for SinkingFundBond {
    fn coupon_rate(&self) -> Decimal {
        self.base.coupon_rate()
    }

    fn coupon_frequency(&self) -> u32 {
        self.base.coupon_frequency()
    }

    fn first_coupon_date(&self) -> Option<Date> {
        self.base.first_coupon_date()
    }

    fn last_coupon_date(&self) -> Option<Date> {
        self.base.last_coupon_date()
    }

    fn is_ex_dividend(&self, settlement: Date) -> bool {
        self.base.is_ex_dividend(settlement)
    }
}

// Implement AmortizingBond
impl AmortizingBond for SinkingFundBond {
    fn amortization_schedule(&self) -> &AmortizationSchedule {
        &self.amortization
    }

    fn factor(&self, as_of: Date) -> f64 {
        self.current_factor(as_of)
    }

    fn outstanding_principal(&self, as_of: Date) -> Decimal {
        self.remaining_principal(as_of)
    }

    fn weighted_average_life(&self, from: Date) -> f64 {
        self.average_life(from)
    }
}

/// Builder for `SinkingFundBond`.
#[derive(Debug, Clone, Default)]
pub struct SinkingFundBondBuilder {
    base: Option<FixedRateBond>,
    schedule: Option<SinkingFundSchedule>,
}

impl SinkingFundBondBuilder {
    /// Sets the base fixed rate bond.
    #[must_use]
    pub fn base_bond(mut self, bond: FixedRateBond) -> Self {
        self.base = Some(bond);
        self
    }

    /// Sets the sinking fund schedule.
    #[must_use]
    pub fn sinking_schedule(mut self, schedule: SinkingFundSchedule) -> Self {
        self.schedule = Some(schedule);
        self
    }

    /// Builds the `SinkingFundBond`.
    ///
    /// # Errors
    ///
    /// Returns an error if base bond or schedule is missing.
    pub fn build(self) -> BondResult<SinkingFundBond> {
        let base = self
            .base
            .ok_or_else(|| BondError::missing_field("base_bond"))?;
        let schedule = self
            .schedule
            .ok_or_else(|| BondError::missing_field("sinking_schedule"))?;

        Ok(SinkingFundBond::new(base, schedule))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruments::FixedRateBond;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_base_bond() -> FixedRateBond {
        FixedRateBond::builder()
            .cusip_unchecked("123456789")
            .coupon_percent(6.0)
            .maturity(date(2029, 6, 15))
            .issue_date(date(2019, 6, 15))
            .us_corporate()
            .build()
            .unwrap()
    }

    fn create_sinking_schedule() -> SinkingFundSchedule {
        SinkingFundSchedule::new()
            .with_payment(SinkingFundPayment::new(date(2025, 6, 15), 20.0))
            .with_payment(SinkingFundPayment::new(date(2026, 6, 15), 20.0))
            .with_payment(SinkingFundPayment::new(date(2027, 6, 15), 20.0))
            .with_payment(SinkingFundPayment::new(date(2028, 6, 15), 20.0))
    }

    #[test]
    fn test_sinking_fund_payment() {
        let payment = SinkingFundPayment::new(date(2025, 6, 15), 25.0);
        assert!((payment.amount_decimal() - 0.25).abs() < 1e-10);
        assert!((payment.price - 100.0).abs() < 1e-10);

        let custom = SinkingFundPayment::with_price(date(2025, 6, 15), 25.0, 102.0);
        assert!((custom.price - 102.0).abs() < 1e-10);
    }

    #[test]
    fn test_acceleration_option() {
        let double_up = AccelerationOption::double_up();
        assert!((double_up.multiple - 2.0).abs() < 1e-10);
        assert!(double_up.max_amount_pct.is_none());

        let custom = AccelerationOption::custom(3.0, Some(50.0));
        assert!((custom.multiple - 3.0).abs() < 1e-10);
        assert_eq!(custom.max_amount_pct, Some(50.0));
    }

    #[test]
    fn test_sinking_fund_schedule_factor() {
        let schedule = create_sinking_schedule();

        // Before any payments
        let factor = schedule.factor(date(2024, 1, 1));
        assert!((factor - 1.0).abs() < 0.001);

        // After first payment (20% retired)
        let factor = schedule.factor(date(2025, 7, 1));
        assert!((factor - 0.80).abs() < 0.001);

        // After two payments (40% retired)
        let factor = schedule.factor(date(2026, 7, 1));
        assert!((factor - 0.60).abs() < 0.001);

        // After all payments (80% retired, 20% remains)
        let factor = schedule.factor(date(2028, 7, 1));
        assert!((factor - 0.20).abs() < 0.001);
    }

    #[test]
    fn test_sinking_fund_average_life() {
        let schedule = create_sinking_schedule();
        let settlement = date(2024, 1, 1);
        let maturity = date(2029, 6, 15);

        let avg_life = schedule.average_life(settlement, maturity);

        // With 20% at each of 5 years + remaining 20% at maturity
        // Average should be somewhere between settlement and maturity
        assert!(avg_life > 1.0);
        assert!(avg_life < 6.0);
    }

    #[test]
    fn test_sinking_fund_schedule_options() {
        let schedule = SinkingFundSchedule::new()
            .with_payment(SinkingFundPayment::new(date(2025, 6, 15), 50.0))
            .with_delivery_option()
            .with_double_up();

        assert!(schedule.has_delivery_option());
        assert!(schedule.acceleration().is_some());
        assert!((schedule.acceleration().unwrap().multiple - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_sinking_fund_to_amortization() {
        let schedule = create_sinking_schedule();
        let amort = schedule.to_amortization_schedule();

        assert_eq!(amort.amort_type, AmortizationType::SinkingFund);
        assert_eq!(amort.entries.len(), 4);
        assert!((amort.total_principal_pct() - 80.0).abs() < 0.001);
    }

    #[test]
    fn test_sinking_fund_bond_creation() {
        let base = create_base_bond();
        let schedule = create_sinking_schedule();
        let sf_bond = SinkingFundBond::new(base, schedule);

        assert_eq!(sf_bond.bond_type(), BondType::SinkingFund);
        assert_eq!(sf_bond.original_face(), dec!(100));
    }

    #[test]
    fn test_sinking_fund_bond_factor() {
        let base = create_base_bond();
        let schedule = create_sinking_schedule();
        let sf_bond = SinkingFundBond::new(base, schedule);

        let settlement = date(2025, 7, 1);
        let factor = sf_bond.current_factor(settlement);

        // After first 20% payment
        assert!((factor - 0.80).abs() < 0.001);

        let remaining = sf_bond.remaining_principal(settlement);
        assert_eq!(remaining, dec!(80));
    }

    #[test]
    fn test_sinking_fund_bond_average_life() {
        let base = create_base_bond();
        let schedule = create_sinking_schedule();
        let sf_bond = SinkingFundBond::new(base, schedule);

        let settlement = date(2024, 1, 1);
        let avg_life = sf_bond.average_life(settlement);

        // Should be between now and maturity
        assert!(avg_life > 1.0);
        assert!(avg_life < 6.0);
    }

    #[test]
    fn test_sinking_fund_accrued_interest() {
        let base = create_base_bond();
        let schedule = create_sinking_schedule();
        let sf_bond = SinkingFundBond::new(base, schedule);

        // Before any sinking fund payments
        let settlement1 = date(2024, 9, 15);
        let accrued1 = sf_bond.accrued_interest(settlement1);

        // After first 20% payment (factor = 0.80)
        let settlement2 = date(2025, 9, 15);
        let accrued2 = sf_bond.accrued_interest(settlement2);

        // Accrued should be proportionally less after sinking fund payment
        // (approximately 80% of original, but dates differ so not exact)
        assert!(accrued2 < accrued1 || accrued1 == Decimal::ZERO);
    }

    #[test]
    fn test_sinking_fund_cash_flows() {
        let base = create_base_bond();
        let schedule = create_sinking_schedule();
        let sf_bond = SinkingFundBond::new(base, schedule);

        let settlement = date(2024, 7, 1);
        let flows = sf_bond.cash_flows(settlement);

        // Should have multiple cash flows
        assert!(!flows.is_empty());

        // Should have sinking fund principal payments
        let principal_flows: Vec<_> = flows
            .iter()
            .filter(|cf| cf.flow_type == CashFlowType::Principal)
            .collect();
        assert!(!principal_flows.is_empty());
    }

    #[test]
    fn test_sinking_fund_yield_to_average_life() {
        let base = create_base_bond();
        let schedule = create_sinking_schedule();
        let sf_bond = SinkingFundBond::new(base, schedule);

        let settlement = date(2024, 1, 15);
        let ytal = sf_bond.yield_to_average_life(dec!(100), settlement);

        // Should calculate a yield
        assert!(ytal.is_ok());
        let yield_val = ytal.unwrap();
        // Yield should be reasonable (between 0 and 20%)
        assert!(yield_val > Decimal::ZERO);
        assert!(yield_val < dec!(0.20));
    }

    #[test]
    fn test_amortizing_bond_trait() {
        let base = create_base_bond();
        let schedule = create_sinking_schedule();
        let sf_bond = SinkingFundBond::new(base, schedule);

        let settlement = date(2025, 7, 1);

        // Test AmortizingBond trait methods
        let factor = sf_bond.factor(settlement);
        assert!((factor - 0.80).abs() < 0.001);

        let outstanding = sf_bond.outstanding_principal(settlement);
        assert_eq!(outstanding, dec!(80));

        let wal = sf_bond.weighted_average_life(settlement);
        assert!(wal > 0.0);

        let next_principal = sf_bond.next_principal_date(settlement);
        assert_eq!(next_principal, Some(date(2026, 6, 15)));
    }

    #[test]
    fn test_builder_validation() {
        // Missing base bond
        let result = SinkingFundBond::builder()
            .sinking_schedule(create_sinking_schedule())
            .build();
        assert!(result.is_err());

        // Missing schedule
        let result = SinkingFundBond::builder()
            .base_bond(create_base_bond())
            .build();
        assert!(result.is_err());

        // Valid build
        let result = SinkingFundBond::builder()
            .base_bond(create_base_bond())
            .sinking_schedule(create_sinking_schedule())
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_total_sinking_pct() {
        let schedule = create_sinking_schedule();
        assert!((schedule.total_sinking_pct() - 80.0).abs() < 0.001);

        // Full sinking
        let full = SinkingFundSchedule::new()
            .with_payment(SinkingFundPayment::new(date(2025, 6, 15), 50.0))
            .with_payment(SinkingFundPayment::new(date(2026, 6, 15), 50.0));
        assert!((full.total_sinking_pct() - 100.0).abs() < 0.001);
    }
}
