//! Sinking fund bond implementation.
//!
//! Provides sinking fund bonds with:
//! - Scheduled mandatory principal redemptions
//! - Delivery option (issuer can deliver bonds instead of cash)
//! - Acceleration/double-up provisions
//! - Average life
//! - Factor-adjusted cash flows

use rust_decimal::Decimal;

use convex_core::types::{Currency, Date, Frequency};

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
/// // Average life
/// let average_life = sf_bond.average_life(settlement)?;
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

    /// Complete checked deterministic sinking cash, including repayments between
    /// coupon dates. Delivery/acceleration/optional sinks require their own model.
    pub fn contract_terms(
        &self,
    ) -> Result<crate::contracts::BondTerms, crate::contracts::BondContractError> {
        use crate::contracts::PrincipalPrice;
        use convex_core::cashflows::{CashflowError, DatedValue, MAX_SCHEDULE_PERIODS};
        if !self.sinking_schedule.is_mandatory
            || self.sinking_schedule.delivery_option
            || self.sinking_schedule.acceleration.is_some()
        {
            return Err(crate::contracts::unsupported(
                "sinking option",
                "delivery, acceleration or optional sink cannot become mandatory cash",
            ));
        }
        let mut terms = self.base.contract_terms()?;
        let payments = &self.sinking_schedule.payments;
        if payments.len() > MAX_SCHEDULE_PERIODS
            || payments.windows(2).any(|p| p[0].date >= p[1].date)
        {
            return Err(crate::contracts::invalid(
                "sinking_schedule",
                "requires bounded sorted unique dates",
            ));
        }
        let mut start = terms.schedule.start_date;
        let mut remaining = terms.face_value;
        let mut profile = Vec::new();
        for payment in payments {
            if payment.date <= start
                || payment.date > terms.schedule.end_date
                || !payment.amount_pct.is_finite()
                || payment.amount_pct <= 0.0
                || payment.amount_pct > 100.0
                || !payment.price.is_finite()
                || payment.price < 0.0
                || payment.price > 1000.0
            {
                return Err(crate::contracts::invalid(
                    "sinking_schedule",
                    "invalid dates/amount/price",
                ));
            }
            let pct = Decimal::try_from(payment.amount_pct)
                .map_err(|_| crate::contracts::invalid("sink.amount", "not Decimal"))?;
            let multiplier = Decimal::try_from(payment.price)
                .map_err(|_| crate::contracts::invalid("sink.price", "not Decimal"))?
                .checked_div(Decimal::ONE_HUNDRED)
                .ok_or(CashflowError::Arithmetic("sink price"))?;
            let retired = terms
                .face_value
                .checked_mul(pct)
                .and_then(|x| x.checked_div(Decimal::ONE_HUNDRED))
                .ok_or(CashflowError::Arithmetic("sink amount"))?;
            if retired > remaining {
                return Err(crate::contracts::invalid(
                    "sinking_schedule",
                    "repayments exceed original principal",
                ));
            }
            profile.push(DatedValue {
                start,
                end: payment.date,
                value: remaining,
            });
            terms.principal_prices.push(PrincipalPrice {
                effective_date: payment.date,
                multiplier,
                amount: if payment.date == terms.schedule.end_date {
                    Some(retired)
                } else {
                    None
                },
            });
            remaining = remaining
                .checked_sub(retired)
                .ok_or(CashflowError::Arithmetic("sink remaining"))?;
            start = payment.date;
        }
        if start < terms.schedule.end_date {
            profile.push(DatedValue {
                start,
                end: terms.schedule.end_date,
                value: remaining,
            });
        }
        if !profile.is_empty() {
            terms.profiles.notional = profile;
        }
        Ok(terms)
    }

    /// Validate and compose actual dated sinking cash over the shared core.
    pub fn try_contract(
        &self,
    ) -> Result<crate::contracts::BondContract, crate::contracts::BondContractError> {
        self.contract_terms()?.build()
    }

    /// Returns the current factor (remaining principal / original).
    #[must_use]
    pub fn current_factor(&self, settlement: Date) -> f64 {
        self.sinking_schedule.factor(settlement)
    }

    /// The average life from `settlement` of its mandatory sinking cash, as
    /// [`BondContract::average_life`](crate::contracts::BondContract::average_life) reads
    /// it.
    ///
    /// # Errors
    ///
    /// As [`Self::try_contract`] and that average life.
    pub fn average_life(
        &self,
        settlement: Date,
    ) -> Result<crate::contracts::AverageLife, crate::contracts::BondContractError> {
        self.try_contract()?.average_life(settlement)
    }

    /// Returns the remaining principal at settlement.
    #[must_use]
    pub fn remaining_principal(&self, settlement: Date) -> Decimal {
        let factor = Decimal::try_from(self.current_factor(settlement)).unwrap_or(Decimal::ONE);
        self.original_face * factor
    }
}

// Implement Bond trait
impl Bond for SinkingFundBond {
    fn yield_cash_flows(
        &self,
        settlement: Date,
        frequency: Frequency,
    ) -> BondResult<crate::pricing::discount_time::PreparedDatedCash> {
        self.base
            .prepare_yield_cash(settlement, frequency, &self.try_cash_flows(settlement)?)
    }

    fn try_cash_flows(&self, from: Date) -> BondResult<Vec<BondCashFlow>> {
        self.try_contract()
            .and_then(|bond| {
                bond.fixed_projection(
                    from,
                    convex_core::cashflows::PaymentDateEntitlement::Exclude,
                )
            })
            .and_then(|cash| cash.cashflows())
            .map_err(|e| BondError::invalid_spec(e.to_string()))
    }

    fn try_accrued_interest(&self, settlement: Date) -> BondResult<Decimal> {
        let cash = self
            .try_contract()
            .and_then(|bond| {
                bond.fixed_projection(
                    settlement,
                    convex_core::cashflows::PaymentDateEntitlement::Exclude,
                )
            })
            .map_err(|e| BondError::invalid_spec(e.to_string()))?;
        cash.accrued
            .checked_div(self.original_face)
            .and_then(|x| x.checked_mul(Decimal::ONE_HUNDRED))
            .ok_or_else(|| BondError::invalid_spec("accrued normalization overflow"))
    }
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

    fn dated_date(&self) -> Date {
        self.base.dated_date()
    }

    fn face_value(&self) -> Decimal {
        self.original_face
    }

    fn frequency(&self) -> Frequency {
        self.base.frequency()
    }

    /// Factor-adjusted cash flows. Coupon at sink date D accrues on the
    /// pre-paydown factor; see test_coupon_at_sink_date_uses_pre_paydown_factor.
    fn cash_flows(&self, settlement: Date) -> Vec<BondCashFlow> {
        let mut flows = Vec::new();
        let maturity = self.base.maturity().unwrap();
        let coupon_rate = self.base.coupon_rate();
        let freq = self.base.coupon_frequency();

        let base_flows = self.base.cash_flows(settlement);
        let mut remaining_factor = self.current_factor(settlement);

        for cf in base_flows {
            if cf.date > maturity {
                break;
            }

            let pre_paydown_factor = remaining_factor;

            if let Some(sf_payment) = self.sinking_schedule.payment_on(cf.date) {
                let sf_amount = self.original_face
                    * Decimal::try_from(sf_payment.amount_pct / 100.0).unwrap_or(Decimal::ZERO)
                    * Decimal::try_from(sf_payment.price / 100.0).unwrap_or(Decimal::ONE);

                flows.push(BondCashFlow {
                    date: cf.date,
                    amount: sf_amount,
                    flow_type: CashFlowType::Principal,
                    accrual_start: cf.accrual_start,
                    accrual_end: cf.accrual_end,
                    factor: Decimal::try_from(pre_paydown_factor).unwrap_or(Decimal::ONE),
                    reference_rate: None,
                });

                remaining_factor -= sf_payment.amount_pct / 100.0;
                remaining_factor = remaining_factor.max(0.0);
            }

            if cf.is_coupon() || cf.is_principal() {
                let coupon_factor = Decimal::try_from(pre_paydown_factor).unwrap_or(Decimal::ONE);
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

        // Principal not sunk by maturity is repaid on it.
        if remaining_factor > 0.0 {
            let final_amount =
                self.original_face * Decimal::try_from(remaining_factor).unwrap_or(Decimal::ZERO);
            flows.push(BondCashFlow::principal(maturity, final_amount));
        }

        flows
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

    /// Regression: sink-date coupon uses pre-paydown factor.
    #[test]
    fn test_sinker_coupon_uses_pre_paydown_factor() {
        use convex_core::calendars::BusinessDayConvention;
        use convex_core::daycounts::DayCountConvention;
        use convex_core::{Currency, Date};
        use rust_decimal::Decimal;
        use rust_decimal_macros::dec;

        let issue = Date::from_ymd(2020, 12, 15).unwrap();
        let maturity = Date::from_ymd(2030, 12, 15).unwrap();
        let base = FixedRateBond::builder()
            .cusip_unchecked("SINK_TEST")
            .coupon_rate(dec!(0.05))
            .issue_date(issue)
            .maturity(maturity)
            .frequency(Frequency::SemiAnnual)
            .day_count(DayCountConvention::Thirty360US)
            .currency(Currency::USD)
            .face_value(dec!(100))
            .calendar(crate::types::CalendarId::new(""))
            .business_day_convention(BusinessDayConvention::Unadjusted)
            .build()
            .unwrap();
        let schedule = SinkingFundSchedule::new()
            .with_payment(SinkingFundPayment::new(date(2026, 12, 15), 20.0))
            .with_payment(SinkingFundPayment::new(date(2027, 12, 15), 20.0))
            .with_payment(SinkingFundPayment::new(date(2028, 12, 15), 20.0))
            .with_payment(SinkingFundPayment::new(date(2029, 12, 15), 20.0))
            .with_payment(SinkingFundPayment::new(date(2030, 12, 15), 20.0));
        let bond = SinkingFundBond::new(base, schedule);

        let flows = bond.cash_flows(date(2025, 12, 31));

        let mat_coupon = flows
            .iter()
            .find(|cf| cf.date == maturity && cf.is_coupon())
            .expect("maturity coupon missing");
        assert!((mat_coupon.amount - dec!(0.5)).abs() < dec!(0.0001));

        let coupon_total: Decimal = flows
            .iter()
            .filter(|cf| cf.is_coupon())
            .map(|cf| cf.amount)
            .sum();
        assert!((coupon_total - dec!(15.0)).abs() < dec!(0.001));
    }
}
