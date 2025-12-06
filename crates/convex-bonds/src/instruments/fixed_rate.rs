//! Fixed rate bond implementation.
//!
//! Provides a complete fixed rate bond implementation with:
//! - Validated identifiers (CUSIP, ISIN)
//! - Market conventions (US Corporate, US Treasury, UK Gilt, etc.)
//! - Schedule caching
//! - Ex-dividend support
//! - Full Bond and FixedCouponBond trait implementation

use once_cell::sync::OnceCell;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};

use crate::cashflows::{AccruedInterestCalculator, Schedule, ScheduleConfig, StubType};
use crate::conventions::{self, BondConventions};
use crate::error::{BondError, BondResult, IdentifierError};
use crate::traits::{Bond, BondCashFlow, FixedCouponBond};
use crate::types::{BondIdentifiers, BondType, CalendarId, Cusip};

/// A fixed rate bond with full convention support.
///
/// This is a comprehensive fixed rate bond implementation that supports:
/// - Validated security identifiers (CUSIP, ISIN, etc.)
/// - Market-specific conventions (US Corporate, US Treasury, UK Gilt, etc.)
/// - Schedule generation with stub handling
/// - Ex-dividend accrued interest (for UK Gilts)
/// - Business day adjustments
///
/// # Performance
///
/// - Bond construction: < 500ns
/// - Cash flow generation: < 1μs (cached schedule)
/// - Accrued interest: < 100ns
///
/// # Example
///
/// ```rust,ignore
/// use convex_bonds::instruments::FixedRateBond;
/// use rust_decimal_macros::dec;
///
/// // Create a US corporate bond
/// let bond = FixedRateBond::builder()
///     .cusip("097023AH7")?
///     .coupon_percent(7.5)
///     .maturity(Date::from_ymd(2025, 6, 15).unwrap())
///     .issue_date(Date::from_ymd(2005, 5, 31).unwrap())
///     .us_corporate()
///     .build()?;
///
/// // Calculate accrued interest
/// let settlement = Date::from_ymd(2020, 4, 29).unwrap();
/// let accrued = bond.accrued_interest(settlement);
/// ```
#[derive(Debug, Clone)]
pub struct FixedRateBond {
    // Identification
    identifiers: BondIdentifiers,

    // Terms
    coupon_rate: Decimal,
    maturity: Date,
    issue_date: Date,
    dated_date: Date,
    first_coupon_date: Option<Date>,
    penultimate_coupon_date: Option<Date>,

    // Conventions
    frequency: Frequency,
    day_count: DayCountConvention,
    settlement_days: u32,
    calendar: CalendarId,
    business_day_convention: BusinessDayConvention,
    end_of_month: bool,

    // Currency
    currency: Currency,

    // Amounts
    face_value: Decimal,
    redemption_value: Decimal,

    // Ex-dividend
    ex_dividend_days: Option<u32>,

    // Classification
    bond_type: BondType,

    // Cached schedule (lazy initialization)
    #[allow(clippy::type_complexity)]
    schedule: OnceCell<Schedule>,
}

impl FixedRateBond {
    /// Creates a new builder for fixed rate bonds.
    #[must_use]
    pub fn builder() -> FixedRateBondBuilder {
        FixedRateBondBuilder::default()
    }

    /// Creates a fixed rate bond with explicit conventions.
    ///
    /// # Arguments
    ///
    /// * `identifiers` - Bond identifiers
    /// * `coupon_rate` - Annual coupon rate as decimal (0.05 for 5%)
    /// * `maturity` - Maturity date
    /// * `issue_date` - Issue date
    /// * `conventions` - Market conventions
    /// * `currency` - Bond currency
    #[must_use]
    pub fn with_conventions(
        identifiers: BondIdentifiers,
        coupon_rate: Decimal,
        maturity: Date,
        issue_date: Date,
        conventions: &BondConventions,
        currency: Currency,
    ) -> Self {
        Self {
            identifiers,
            coupon_rate,
            maturity,
            issue_date,
            dated_date: issue_date,
            first_coupon_date: None,
            penultimate_coupon_date: None,
            frequency: conventions.frequency(),
            day_count: conventions.day_count(),
            settlement_days: conventions.settlement_days(),
            calendar: conventions.calendar().clone(),
            business_day_convention: conventions.business_day_convention(),
            end_of_month: conventions.end_of_month(),
            currency,
            face_value: Decimal::ONE_HUNDRED,
            redemption_value: Decimal::ONE_HUNDRED,
            ex_dividend_days: conventions.ex_dividend_days(),
            bond_type: BondType::FixedRateCorporate,
            schedule: OnceCell::new(),
        }
    }

    /// Returns the annual coupon rate as a decimal.
    #[must_use]
    pub fn coupon_rate_decimal(&self) -> Decimal {
        self.coupon_rate
    }

    /// Returns the annual coupon amount per unit of face value.
    #[must_use]
    pub fn annual_coupon(&self) -> Decimal {
        self.face_value * self.coupon_rate
    }

    /// Returns the coupon amount per period per unit of face value.
    #[must_use]
    pub fn coupon_per_period(&self) -> Decimal {
        let periods = self.frequency.periods_per_year();
        if periods == 0 {
            Decimal::ZERO
        } else {
            self.annual_coupon() / Decimal::from(periods)
        }
    }

    /// Returns the day count convention.
    #[must_use]
    pub fn day_count(&self) -> DayCountConvention {
        self.day_count
    }

    /// Returns the payment frequency.
    #[must_use]
    pub fn frequency(&self) -> Frequency {
        self.frequency
    }

    /// Returns the settlement days (T+n).
    #[must_use]
    pub fn settlement_days(&self) -> u32 {
        self.settlement_days
    }

    /// Returns the ex-dividend days if applicable.
    #[must_use]
    pub fn ex_dividend_days(&self) -> Option<u32> {
        self.ex_dividend_days
    }

    /// Gets or generates the payment schedule.
    ///
    /// The schedule is lazily computed and cached for performance.
    ///
    /// Uses backward generation from maturity to ensure correct regular coupon dates
    /// regardless of the dated_date.
    fn schedule(&self) -> &Schedule {
        self.schedule.get_or_init(|| {
            // Determine stub type based on whether we have explicit first/penultimate dates
            let stub_type = if self.first_coupon_date.is_some() {
                StubType::ShortFirst
            } else if self.penultimate_coupon_date.is_some() {
                StubType::ShortLast
            } else {
                // Default: generate backward from maturity (no explicit stub)
                StubType::None
            };

            let config = ScheduleConfig::new(self.dated_date, self.maturity, self.frequency)
                .with_calendar(self.calendar.clone())
                .with_business_day_convention(self.business_day_convention)
                .with_end_of_month(self.end_of_month)
                .with_stub_type(stub_type);

            let config = if let Some(first) = self.first_coupon_date {
                config.with_first_regular_date(first)
            } else {
                config
            };

            let config = if let Some(penult) = self.penultimate_coupon_date {
                config.with_penultimate_date(penult)
            } else {
                config
            };

            Schedule::generate(config).expect("Schedule generation failed")
        })
    }

    /// Finds the previous and next coupon dates for a given settlement date.
    fn coupon_dates_for_settlement(&self, settlement: Date) -> (Date, Date) {
        let schedule = self.schedule();
        let dates = schedule.unadjusted_dates();

        for window in dates.windows(2) {
            if settlement >= window[0] && settlement < window[1] {
                return (window[0], window[1]);
            }
        }

        // Settlement after last coupon before maturity
        let n = dates.len();
        if n >= 2 {
            (dates[n - 2], dates[n - 1])
        } else {
            (self.dated_date, self.maturity)
        }
    }

    /// Calculates accrued interest at settlement.
    ///
    /// Handles both standard accrued and ex-dividend accrued (UK Gilts).
    fn calculate_accrued(&self, settlement: Date) -> Decimal {
        if self.frequency.is_zero() {
            return Decimal::ZERO;
        }

        let (last_coupon, next_coupon) = self.coupon_dates_for_settlement(settlement);

        match self.ex_dividend_days {
            Some(ex_div_days) => AccruedInterestCalculator::ex_dividend(
                settlement,
                last_coupon,
                next_coupon,
                self.coupon_rate,
                self.face_value,
                self.day_count,
                self.frequency,
                ex_div_days,
                &self.calendar,
            ),
            None => AccruedInterestCalculator::standard(
                settlement,
                last_coupon,
                next_coupon,
                self.coupon_rate,
                self.face_value,
                self.day_count,
                self.frequency,
            ),
        }
    }
}

// Implement the Bond trait from traits/bond.rs
impl Bond for FixedRateBond {
    fn identifiers(&self) -> &BondIdentifiers {
        &self.identifiers
    }

    fn bond_type(&self) -> BondType {
        self.bond_type
    }

    fn currency(&self) -> Currency {
        self.currency
    }

    fn maturity(&self) -> Option<Date> {
        Some(self.maturity)
    }

    fn issue_date(&self) -> Date {
        self.issue_date
    }

    fn first_settlement_date(&self) -> Date {
        // Typically the issue date adjusted for settlement days
        self.issue_date
    }

    fn dated_date(&self) -> Date {
        self.dated_date
    }

    fn face_value(&self) -> Decimal {
        self.face_value
    }

    fn cash_flows(&self, from: Date) -> Vec<BondCashFlow> {
        let schedule = self.schedule();
        let dates = schedule.dates();
        let unadjusted = schedule.unadjusted_dates();

        let mut flows = Vec::new();

        for (i, window) in unadjusted.windows(2).enumerate() {
            let accrual_start = window[0];
            let accrual_end = window[1];
            let payment_date = dates.get(i + 1).copied().unwrap_or(accrual_end);

            if payment_date <= from {
                continue;
            }

            let coupon = self.coupon_per_period();
            let is_final = i == unadjusted.len() - 2;

            if is_final {
                // Final payment: coupon + principal
                flows.push(
                    BondCashFlow::coupon_and_principal(payment_date, coupon, self.redemption_value)
                        .with_accrual(accrual_start, accrual_end),
                );
            } else {
                flows.push(
                    BondCashFlow::coupon(payment_date, coupon)
                        .with_accrual(accrual_start, accrual_end),
                );
            }
        }

        flows
    }

    fn next_coupon_date(&self, after: Date) -> Option<Date> {
        let schedule = self.schedule();
        schedule.dates().iter().find(|&&d| d > after).copied()
    }

    fn previous_coupon_date(&self, before: Date) -> Option<Date> {
        let schedule = self.schedule();
        schedule
            .dates()
            .iter()
            .rev()
            .find(|&&d| d < before)
            .copied()
    }

    fn accrued_interest(&self, settlement: Date) -> Decimal {
        self.calculate_accrued(settlement)
    }

    fn day_count_convention(&self) -> &str {
        match self.day_count {
            DayCountConvention::Act360 => "ACT/360",
            DayCountConvention::Act365Fixed => "ACT/365F",
            DayCountConvention::Act365Leap => "ACT/365L",
            DayCountConvention::ActActIsda => "ACT/ACT ISDA",
            DayCountConvention::ActActIcma => "ACT/ACT ICMA",
            DayCountConvention::ActActAfb => "ACT/ACT AFB",
            DayCountConvention::Thirty360US => "30/360 US",
            DayCountConvention::Thirty360E => "30E/360",
            DayCountConvention::Thirty360EIsda => "30E/360 ISDA",
            DayCountConvention::Thirty360German => "30/360 German",
        }
    }

    fn calendar(&self) -> &CalendarId {
        &self.calendar
    }

    fn redemption_value(&self) -> Decimal {
        self.redemption_value
    }
}

// Implement FixedCouponBond trait
impl FixedCouponBond for FixedRateBond {
    fn coupon_rate(&self) -> Decimal {
        self.coupon_rate
    }

    fn coupon_frequency(&self) -> u32 {
        self.frequency.periods_per_year()
    }

    fn first_coupon_date(&self) -> Option<Date> {
        self.first_coupon_date.or_else(|| {
            let schedule = self.schedule();
            schedule.dates().get(1).copied()
        })
    }

    fn last_coupon_date(&self) -> Option<Date> {
        let schedule = self.schedule();
        let dates = schedule.dates();
        if dates.len() >= 2 {
            dates.get(dates.len() - 2).copied()
        } else {
            None
        }
    }

    fn is_ex_dividend(&self, settlement: Date) -> bool {
        if let Some(ex_div_days) = self.ex_dividend_days {
            let (_, next_coupon) = self.coupon_dates_for_settlement(settlement);
            let calendar = self.calendar.to_calendar();
            let ex_div_date = calendar.add_business_days(next_coupon, -(ex_div_days as i32));
            settlement >= ex_div_date
        } else {
            false
        }
    }
}

/// Builder for `FixedRateBond`.
///
/// Provides a fluent API for constructing fixed rate bonds with proper
/// validation and convention support.
#[derive(Debug, Clone, Default)]
pub struct FixedRateBondBuilder {
    identifiers: Option<BondIdentifiers>,
    coupon_rate: Option<Decimal>,
    maturity: Option<Date>,
    issue_date: Option<Date>,
    dated_date: Option<Date>,
    first_coupon_date: Option<Date>,
    penultimate_coupon_date: Option<Date>,
    frequency: Option<Frequency>,
    day_count: Option<DayCountConvention>,
    settlement_days: Option<u32>,
    calendar: Option<CalendarId>,
    business_day_convention: Option<BusinessDayConvention>,
    end_of_month: Option<bool>,
    currency: Option<Currency>,
    face_value: Option<Decimal>,
    redemption_value: Option<Decimal>,
    ex_dividend_days: Option<u32>,
    bond_type: Option<BondType>,
}

impl FixedRateBondBuilder {
    /// Creates a new builder with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the bond identifiers.
    #[must_use]
    pub fn identifiers(mut self, ids: BondIdentifiers) -> Self {
        self.identifiers = Some(ids);
        self
    }

    /// Sets the CUSIP identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the CUSIP is invalid.
    pub fn cusip(mut self, cusip: &str) -> Result<Self, IdentifierError> {
        let cusip = Cusip::new(cusip)?;
        self.identifiers = Some(BondIdentifiers::new().with_cusip(cusip));
        Ok(self)
    }

    /// Sets the CUSIP without validation.
    #[must_use]
    pub fn cusip_unchecked(mut self, cusip: &str) -> Self {
        let cusip = Cusip::new_unchecked(cusip);
        self.identifiers = Some(BondIdentifiers::new().with_cusip(cusip));
        self
    }

    /// Sets the coupon rate as a decimal (0.05 for 5%).
    #[must_use]
    pub fn coupon_rate(mut self, rate: Decimal) -> Self {
        self.coupon_rate = Some(rate);
        self
    }

    /// Sets the coupon rate as a percentage (5.0 for 5%).
    #[must_use]
    pub fn coupon_percent(mut self, percent: f64) -> Self {
        self.coupon_rate = Some(Decimal::try_from(percent / 100.0).unwrap_or(Decimal::ZERO));
        self
    }

    /// Sets the maturity date.
    #[must_use]
    pub fn maturity(mut self, date: Date) -> Self {
        self.maturity = Some(date);
        self
    }

    /// Sets the issue date.
    #[must_use]
    pub fn issue_date(mut self, date: Date) -> Self {
        self.issue_date = Some(date);
        self
    }

    /// Sets the dated date (interest accrual start).
    #[must_use]
    pub fn dated_date(mut self, date: Date) -> Self {
        self.dated_date = Some(date);
        self
    }

    /// Sets the first coupon date (for odd first coupon).
    #[must_use]
    pub fn first_coupon_date(mut self, date: Date) -> Self {
        self.first_coupon_date = Some(date);
        self
    }

    /// Sets the penultimate coupon date (for odd last coupon).
    #[must_use]
    pub fn penultimate_coupon_date(mut self, date: Date) -> Self {
        self.penultimate_coupon_date = Some(date);
        self
    }

    /// Sets the payment frequency.
    #[must_use]
    pub fn frequency(mut self, freq: Frequency) -> Self {
        self.frequency = Some(freq);
        self
    }

    /// Sets the day count convention.
    #[must_use]
    pub fn day_count(mut self, dc: DayCountConvention) -> Self {
        self.day_count = Some(dc);
        self
    }

    /// Sets the settlement days (T+n).
    #[must_use]
    pub fn settlement_days(mut self, days: u32) -> Self {
        self.settlement_days = Some(days);
        self
    }

    /// Sets the calendar for business day adjustments.
    #[must_use]
    pub fn calendar(mut self, cal: CalendarId) -> Self {
        self.calendar = Some(cal);
        self
    }

    /// Sets the business day convention.
    #[must_use]
    pub fn business_day_convention(mut self, convention: BusinessDayConvention) -> Self {
        self.business_day_convention = Some(convention);
        self
    }

    /// Sets the end-of-month rule.
    #[must_use]
    pub fn end_of_month(mut self, eom: bool) -> Self {
        self.end_of_month = Some(eom);
        self
    }

    /// Sets the currency.
    #[must_use]
    pub fn currency(mut self, currency: Currency) -> Self {
        self.currency = Some(currency);
        self
    }

    /// Sets the face value (default: 100).
    #[must_use]
    pub fn face_value(mut self, value: Decimal) -> Self {
        self.face_value = Some(value);
        self
    }

    /// Sets the redemption value (default: 100).
    #[must_use]
    pub fn redemption_value(mut self, value: Decimal) -> Self {
        self.redemption_value = Some(value);
        self
    }

    /// Sets the ex-dividend days.
    #[must_use]
    pub fn ex_dividend_days(mut self, days: u32) -> Self {
        self.ex_dividend_days = Some(days);
        self
    }

    /// Applies US Corporate bond conventions.
    ///
    /// - Day count: 30/360 US
    /// - Frequency: Semi-annual
    /// - Settlement: T+2
    /// - Calendar: SIFMA
    #[must_use]
    pub fn us_corporate(mut self) -> Self {
        let conv = conventions::us_corporate::investment_grade();
        self.frequency = Some(conv.frequency());
        self.day_count = Some(conv.day_count());
        self.settlement_days = Some(conv.settlement_days());
        self.calendar = Some(conv.calendar().clone());
        self.business_day_convention = Some(conv.business_day_convention());
        self.end_of_month = Some(conv.end_of_month());
        self.currency = Some(Currency::USD);
        self.bond_type = Some(BondType::FixedRateCorporate);
        self
    }

    /// Applies US Treasury note/bond conventions.
    ///
    /// - Day count: ACT/ACT ICMA
    /// - Frequency: Semi-annual
    /// - Settlement: T+1
    /// - Calendar: US Government
    #[must_use]
    pub fn us_treasury(mut self) -> Self {
        let conv = conventions::us_treasury::note_bond();
        self.frequency = Some(conv.frequency());
        self.day_count = Some(conv.day_count());
        self.settlement_days = Some(conv.settlement_days());
        self.calendar = Some(conv.calendar().clone());
        self.business_day_convention = Some(conv.business_day_convention());
        self.end_of_month = Some(conv.end_of_month());
        self.currency = Some(Currency::USD);
        self.bond_type = Some(BondType::TreasuryNote);
        self
    }

    /// Applies UK Gilt conventions.
    ///
    /// - Day count: ACT/ACT ICMA
    /// - Frequency: Semi-annual
    /// - Settlement: T+1
    /// - Ex-dividend: 7 business days
    #[must_use]
    pub fn uk_gilt(mut self) -> Self {
        let conv = conventions::uk_gilt::conventional();
        self.frequency = Some(conv.frequency());
        self.day_count = Some(conv.day_count());
        self.settlement_days = Some(conv.settlement_days());
        self.calendar = Some(conv.calendar().clone());
        self.business_day_convention = Some(conv.business_day_convention());
        self.end_of_month = Some(conv.end_of_month());
        self.ex_dividend_days = conv.ex_dividend_days();
        self.currency = Some(Currency::GBP);
        self.bond_type = Some(BondType::Gilt);
        self
    }

    /// Applies German Bund conventions.
    ///
    /// - Day count: ACT/ACT ICMA
    /// - Frequency: Annual
    /// - Settlement: T+2
    /// - Calendar: TARGET2
    #[must_use]
    pub fn german_bund(mut self) -> Self {
        let conv = conventions::german_bund::bund();
        self.frequency = Some(conv.frequency());
        self.day_count = Some(conv.day_count());
        self.settlement_days = Some(conv.settlement_days());
        self.calendar = Some(conv.calendar().clone());
        self.business_day_convention = Some(conv.business_day_convention());
        self.end_of_month = Some(conv.end_of_month());
        self.currency = Some(Currency::EUR);
        self.bond_type = Some(BondType::Bund);
        self
    }

    /// Applies conventions from a `BondConventions` object.
    #[must_use]
    pub fn with_conventions(mut self, conv: &BondConventions) -> Self {
        self.frequency = Some(conv.frequency());
        self.day_count = Some(conv.day_count());
        self.settlement_days = Some(conv.settlement_days());
        self.calendar = Some(conv.calendar().clone());
        self.business_day_convention = Some(conv.business_day_convention());
        self.end_of_month = Some(conv.end_of_month());
        self.ex_dividend_days = conv.ex_dividend_days();
        self
    }

    /// Builds the `FixedRateBond`.
    ///
    /// # Errors
    ///
    /// Returns an error if required fields are missing or values are invalid.
    pub fn build(self) -> BondResult<FixedRateBond> {
        let identifiers = self
            .identifiers
            .ok_or_else(|| BondError::missing_field("identifiers"))?;
        let coupon_rate = self
            .coupon_rate
            .ok_or_else(|| BondError::missing_field("coupon_rate"))?;
        let maturity = self
            .maturity
            .ok_or_else(|| BondError::missing_field("maturity"))?;
        let issue_date = self
            .issue_date
            .ok_or_else(|| BondError::missing_field("issue_date"))?;

        // Validate
        if maturity <= issue_date {
            return Err(BondError::invalid_spec(
                "maturity must be after issue_date",
            ));
        }
        if coupon_rate < Decimal::ZERO {
            return Err(BondError::invalid_spec("coupon_rate cannot be negative"));
        }

        let dated_date = self.dated_date.unwrap_or(issue_date);

        Ok(FixedRateBond {
            identifiers,
            coupon_rate,
            maturity,
            issue_date,
            dated_date,
            first_coupon_date: self.first_coupon_date,
            penultimate_coupon_date: self.penultimate_coupon_date,
            frequency: self.frequency.unwrap_or(Frequency::SemiAnnual),
            day_count: self.day_count.unwrap_or(DayCountConvention::Thirty360US),
            settlement_days: self.settlement_days.unwrap_or(2),
            calendar: self.calendar.unwrap_or_else(CalendarId::sifma),
            business_day_convention: self
                .business_day_convention
                .unwrap_or(BusinessDayConvention::Following),
            end_of_month: self.end_of_month.unwrap_or(true),
            currency: self.currency.unwrap_or(Currency::USD),
            face_value: self.face_value.unwrap_or(Decimal::ONE_HUNDRED),
            redemption_value: self.redemption_value.unwrap_or(Decimal::ONE_HUNDRED),
            ex_dividend_days: self.ex_dividend_days,
            bond_type: self.bond_type.unwrap_or(BondType::FixedRateCorporate),
            schedule: OnceCell::new(),
        })
    }
}

/// Helper function to convert DayCountConvention to string for serialization.
fn day_count_to_string(dc: &DayCountConvention) -> &'static str {
    match dc {
        DayCountConvention::Act360 => "Act360",
        DayCountConvention::Act365Fixed => "Act365Fixed",
        DayCountConvention::Act365Leap => "Act365Leap",
        DayCountConvention::ActActIsda => "ActActIsda",
        DayCountConvention::ActActIcma => "ActActIcma",
        DayCountConvention::ActActAfb => "ActActAfb",
        DayCountConvention::Thirty360US => "Thirty360US",
        DayCountConvention::Thirty360E => "Thirty360E",
        DayCountConvention::Thirty360EIsda => "Thirty360EIsda",
        DayCountConvention::Thirty360German => "Thirty360German",
    }
}

/// Helper function to convert string to DayCountConvention for deserialization.
fn string_to_day_count(s: &str) -> DayCountConvention {
    match s {
        "Act360" => DayCountConvention::Act360,
        "Act365Fixed" => DayCountConvention::Act365Fixed,
        "Act365Leap" => DayCountConvention::Act365Leap,
        "ActActIsda" => DayCountConvention::ActActIsda,
        "ActActIcma" => DayCountConvention::ActActIcma,
        "ActActAfb" => DayCountConvention::ActActAfb,
        "Thirty360US" => DayCountConvention::Thirty360US,
        "Thirty360E" => DayCountConvention::Thirty360E,
        "Thirty360EIsda" => DayCountConvention::Thirty360EIsda,
        "Thirty360German" => DayCountConvention::Thirty360German,
        _ => DayCountConvention::Thirty360US, // Default fallback
    }
}

// Implement Serialize manually to skip the OnceCell and handle DayCountConvention
impl Serialize for FixedRateBond {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("FixedRateBond", 18)?;
        state.serialize_field("identifiers", &self.identifiers)?;
        state.serialize_field("coupon_rate", &self.coupon_rate)?;
        state.serialize_field("maturity", &self.maturity)?;
        state.serialize_field("issue_date", &self.issue_date)?;
        state.serialize_field("dated_date", &self.dated_date)?;
        state.serialize_field("first_coupon_date", &self.first_coupon_date)?;
        state.serialize_field("penultimate_coupon_date", &self.penultimate_coupon_date)?;
        state.serialize_field("frequency", &self.frequency)?;
        state.serialize_field("day_count", &day_count_to_string(&self.day_count))?;
        state.serialize_field("settlement_days", &self.settlement_days)?;
        state.serialize_field("calendar", &self.calendar)?;
        state.serialize_field("business_day_convention", &self.business_day_convention)?;
        state.serialize_field("end_of_month", &self.end_of_month)?;
        state.serialize_field("currency", &self.currency)?;
        state.serialize_field("face_value", &self.face_value)?;
        state.serialize_field("redemption_value", &self.redemption_value)?;
        state.serialize_field("ex_dividend_days", &self.ex_dividend_days)?;
        state.serialize_field("bond_type", &self.bond_type)?;
        state.end()
    }
}

// Implement Deserialize manually to initialize the OnceCell and handle DayCountConvention
impl<'de> Deserialize<'de> for FixedRateBond {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct FixedRateBondData {
            identifiers: BondIdentifiers,
            coupon_rate: Decimal,
            maturity: Date,
            issue_date: Date,
            dated_date: Date,
            first_coupon_date: Option<Date>,
            penultimate_coupon_date: Option<Date>,
            frequency: Frequency,
            day_count: String,
            settlement_days: u32,
            calendar: CalendarId,
            business_day_convention: BusinessDayConvention,
            end_of_month: bool,
            currency: Currency,
            face_value: Decimal,
            redemption_value: Decimal,
            ex_dividend_days: Option<u32>,
            bond_type: BondType,
        }

        let data = FixedRateBondData::deserialize(deserializer)?;
        Ok(FixedRateBond {
            identifiers: data.identifiers,
            coupon_rate: data.coupon_rate,
            maturity: data.maturity,
            issue_date: data.issue_date,
            dated_date: data.dated_date,
            first_coupon_date: data.first_coupon_date,
            penultimate_coupon_date: data.penultimate_coupon_date,
            frequency: data.frequency,
            day_count: string_to_day_count(&data.day_count),
            settlement_days: data.settlement_days,
            calendar: data.calendar,
            business_day_convention: data.business_day_convention,
            end_of_month: data.end_of_month,
            currency: data.currency,
            face_value: data.face_value,
            redemption_value: data.redemption_value,
            ex_dividend_days: data.ex_dividend_days,
            bond_type: data.bond_type,
            schedule: OnceCell::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    /// Helper to create a date.
    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    #[test]
    fn test_fixed_rate_bond_builder() {
        let bond = FixedRateBond::builder()
            .cusip_unchecked("097023AH7")
            .coupon_percent(7.5)
            .maturity(date(2025, 6, 15))
            .issue_date(date(2005, 5, 31))
            .us_corporate()
            .build()
            .unwrap();

        assert_eq!(bond.coupon_rate(), dec!(0.075));
        assert_eq!(bond.coupon_frequency(), 2);
        assert_eq!(bond.frequency(), Frequency::SemiAnnual);
        assert_eq!(bond.day_count(), DayCountConvention::Thirty360US);
        assert_eq!(bond.settlement_days(), 2);
    }

    #[test]
    fn test_coupon_per_period() {
        let bond = FixedRateBond::builder()
            .cusip_unchecked("TEST12345")
            .coupon_percent(5.0)
            .maturity(date(2030, 6, 15))
            .issue_date(date(2020, 6, 15))
            .frequency(Frequency::SemiAnnual)
            .face_value(dec!(100))
            .build()
            .unwrap();

        // 5% annual coupon, semi-annual = 2.50 per period
        assert_eq!(bond.coupon_per_period(), dec!(2.5));
    }

    #[test]
    fn test_cash_flows() {
        let bond = FixedRateBond::builder()
            .cusip_unchecked("TEST12345")
            .coupon_percent(5.0)
            .maturity(date(2025, 6, 15))
            .issue_date(date(2020, 6, 15))
            .frequency(Frequency::SemiAnnual)
            .build()
            .unwrap();

        let settlement = date(2024, 1, 1);
        let flows = bond.cash_flows(settlement);

        // Should have remaining coupon payments plus final
        assert!(!flows.is_empty());

        // Last flow should include principal
        let last = flows.last().unwrap();
        assert!(last.is_principal());

        // All flows should have accrual periods
        for flow in &flows {
            assert!(flow.accrual_start.is_some());
            assert!(flow.accrual_end.is_some());
        }
    }

    /// Boeing 7.5% 06/15/2025 - Bloomberg YAS validation
    ///
    /// Settlement: 04/29/2020
    /// Last coupon: 12/15/2019
    /// Next coupon: 06/15/2020
    ///
    /// 30/360 US calculation:
    /// Dec 15 to Apr 29:
    /// - Dec: 15 days (15 to 30)
    /// - Jan: 30 days
    /// - Feb: 30 days
    /// - Mar: 30 days
    /// - Apr: 29 days
    /// Total: 134 days
    ///
    /// Period: Dec 15 to Jun 15 = 180 days (30/360)
    /// Coupon per period = 100 * 0.075 / 2 = 3.75
    /// Accrued = 3.75 * 134/180 = 2.791667 per $100 face
    /// Accrued = 27,916.67 per $1M face
    #[test]
    fn test_boeing_bond_accrued() {
        let bond = FixedRateBond::builder()
            .cusip_unchecked("097023AH7")
            .coupon_percent(7.5)
            .maturity(date(2025, 6, 15))
            .issue_date(date(2005, 5, 31))
            .us_corporate()
            .build()
            .unwrap();

        let settlement = date(2020, 4, 29);
        let accrued_per_100 = bond.accrued_interest(settlement);

        // Expected: 3.75 * 134/180 = 2.7916666...
        // With some tolerance for rounding
        assert!(
            accrued_per_100 > dec!(2.79) && accrued_per_100 < dec!(2.80),
            "Accrued per $100 = {} (expected ~2.79166)",
            accrued_per_100
        );

        // Per $1M face value (10,000 units of $100)
        let accrued_per_1m = accrued_per_100 * Decimal::from(10_000);
        assert!(
            accrued_per_1m > dec!(27900) && accrued_per_1m < dec!(28000),
            "Accrued per $1M = {} (expected ~27916.67)",
            accrued_per_1m
        );
    }

    #[test]
    fn test_us_treasury_conventions() {
        let bond = FixedRateBond::builder()
            .cusip_unchecked("912828Z229")
            .coupon_percent(2.5)
            .maturity(date(2030, 5, 15))
            .issue_date(date(2020, 5, 15))
            .us_treasury()
            .build()
            .unwrap();

        assert_eq!(bond.day_count(), DayCountConvention::ActActIcma);
        assert_eq!(bond.settlement_days(), 1);
        assert_eq!(bond.bond_type(), BondType::TreasuryNote);
    }

    #[test]
    fn test_uk_gilt_conventions() {
        let bond = FixedRateBond::builder()
            .cusip_unchecked("GILT00001")
            .coupon_percent(4.0)
            .maturity(date(2030, 1, 15))
            .issue_date(date(2020, 1, 15))
            .uk_gilt()
            .build()
            .unwrap();

        assert_eq!(bond.day_count(), DayCountConvention::ActActIcma);
        assert_eq!(bond.ex_dividend_days(), Some(7));
        assert_eq!(bond.currency(), Currency::GBP);
    }

    #[test]
    fn test_missing_fields() {
        let result = FixedRateBond::builder().build();
        assert!(result.is_err());

        let result = FixedRateBond::builder()
            .cusip_unchecked("TEST")
            .coupon_percent(5.0)
            .build();
        assert!(result.is_err()); // Missing maturity
    }

    #[test]
    fn test_invalid_coupon_rate() {
        let result = FixedRateBond::builder()
            .cusip_unchecked("TEST12345")
            .coupon_rate(dec!(-0.05))
            .maturity(date(2030, 1, 1))
            .issue_date(date(2020, 1, 1))
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_dates() {
        let result = FixedRateBond::builder()
            .cusip_unchecked("TEST12345")
            .coupon_percent(5.0)
            .maturity(date(2020, 1, 1))
            .issue_date(date(2025, 1, 1)) // Issue after maturity
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_bond_trait_methods() {
        let bond = FixedRateBond::builder()
            .cusip_unchecked("TEST12345")
            .coupon_percent(5.0)
            .maturity(date(2030, 6, 15))
            .issue_date(date(2020, 6, 15))
            .build()
            .unwrap();

        assert_eq!(bond.maturity(), Some(date(2030, 6, 15)));
        assert_eq!(bond.issue_date(), date(2020, 6, 15));
        assert_eq!(bond.face_value(), Decimal::ONE_HUNDRED);
        assert_eq!(bond.redemption_value(), Decimal::ONE_HUNDRED);
        assert!(!bond.has_matured(date(2025, 1, 1)));
        assert!(bond.has_matured(date(2031, 1, 1)));
    }

    #[test]
    fn test_next_previous_coupon_dates() {
        let bond = FixedRateBond::builder()
            .cusip_unchecked("TEST12345")
            .coupon_percent(5.0)
            .maturity(date(2025, 6, 15))
            .issue_date(date(2020, 6, 15))
            .frequency(Frequency::SemiAnnual)
            .build()
            .unwrap();

        let settlement = date(2024, 3, 1);

        // Next coupon should be Jun 15, 2024
        let next = bond.next_coupon_date(settlement);
        assert!(next.is_some());

        // Previous coupon should be Dec 15, 2023
        let prev = bond.previous_coupon_date(settlement);
        assert!(prev.is_some());
    }
}
