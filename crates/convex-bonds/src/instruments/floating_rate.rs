//! Floating Rate Notes (FRNs).
//!
//! This module provides floating rate note implementation with support for:
//! - SOFR compounding (in arrears, simple average, Term SOFR)
//! - EURIBOR, SONIA, €STR, and other reference rates
//! - Caps and floors (collars)
//! - Lookback and lockout conventions
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_bonds::instruments::FloatingRateNote;
//! use convex_bonds::types::{RateIndex, SOFRConvention};
//! use convex_core::types::Date;
//!
//! let frn = FloatingRateNote::builder()
//!     .cusip_unchecked("912828ZQ7")
//!     .index(RateIndex::SOFR)
//!     .sofr_convention(SOFRConvention::arrc_standard())
//!     .spread_bps(50)  // 50 basis points
//!     .maturity(Date::from_ymd(2026, 7, 31).unwrap())
//!     .issue_date(Date::from_ymd(2024, 7, 31).unwrap())
//!     .us_treasury_frn()
//!     .build()
//!     .unwrap();
//!
//! // Calculate accrued interest with current rate
//! let accrued = frn.accrued_interest_with_rate(settlement, dec!(0.053));
//! ```

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};

use crate::cashflows::{Schedule, ScheduleConfig};
use crate::error::{BondError, BondResult};
use crate::traits::{Bond, BondCashFlow, FloatingCouponBond};
use crate::types::{BondIdentifiers, BondType, CalendarId, Cusip, Isin, RateIndex, SOFRConvention};

/// A floating rate note (FRN).
///
/// FRNs pay coupons that reset periodically based on a reference rate
/// (such as SOFR, EURIBOR, or SONIA) plus a fixed spread.
///
/// # Features
///
/// - Support for all major reference rates (SOFR, €STR, SONIA, EURIBOR, etc.)
/// - SOFR compounding conventions (in arrears, simple average, Term SOFR)
/// - Caps and floors (rate collars)
/// - Lookback and lockout periods
/// - Full Bond trait implementation
#[derive(Debug, Clone)]
pub struct FloatingRateNote {
    /// Bond identifiers
    identifiers: BondIdentifiers,

    /// Reference rate index
    index: RateIndex,

    /// SOFR-specific compounding convention (if applicable)
    sofr_convention: Option<SOFRConvention>,

    /// Spread over reference rate in basis points
    spread_bps: Decimal,

    /// Maturity date
    maturity: Date,

    /// Issue date
    issue_date: Date,

    /// Payment frequency
    frequency: Frequency,

    /// Day count convention
    day_count: DayCountConvention,

    /// Reset lag in business days before period start
    reset_lag: i32,

    /// Payment delay in business days after period end
    payment_delay: u32,

    /// Rate cap (optional)
    cap: Option<Decimal>,

    /// Rate floor (optional)
    floor: Option<Decimal>,

    /// Settlement days
    settlement_days: u32,

    /// Calendar for business day adjustments
    calendar: CalendarId,

    /// Currency
    currency: Currency,

    /// Face value per unit
    face_value: Decimal,

    /// Current reference rate fixing (if known)
    current_rate: Option<Decimal>,
}

impl FloatingRateNote {
    /// Creates a new builder for `FloatingRateNote`.
    #[must_use]
    pub fn builder() -> FloatingRateNoteBuilder {
        FloatingRateNoteBuilder::default()
    }

    /// Returns the reference rate index.
    #[must_use]
    pub fn index(&self) -> &RateIndex {
        &self.index
    }

    /// Returns the SOFR convention if applicable.
    #[must_use]
    pub fn sofr_convention(&self) -> Option<&SOFRConvention> {
        self.sofr_convention.as_ref()
    }

    /// Returns the spread in basis points.
    #[must_use]
    pub fn spread_bps(&self) -> Decimal {
        self.spread_bps
    }

    /// Returns the spread as a decimal rate.
    #[must_use]
    pub fn spread_decimal(&self) -> Decimal {
        self.spread_bps / Decimal::from(10000)
    }

    /// Returns the maturity date.
    #[must_use]
    pub fn maturity_date(&self) -> Date {
        self.maturity
    }

    /// Returns the issue date.
    #[must_use]
    pub fn get_issue_date(&self) -> Date {
        self.issue_date
    }

    /// Returns the payment frequency.
    #[must_use]
    pub fn frequency(&self) -> Frequency {
        self.frequency
    }

    /// Returns the day count convention.
    #[must_use]
    pub fn day_count(&self) -> DayCountConvention {
        self.day_count
    }

    /// Returns the cap rate if any.
    #[must_use]
    pub fn cap(&self) -> Option<Decimal> {
        self.cap
    }

    /// Returns the floor rate if any.
    #[must_use]
    pub fn floor(&self) -> Option<Decimal> {
        self.floor
    }

    /// Returns the settlement days.
    #[must_use]
    pub fn settlement_days(&self) -> u32 {
        self.settlement_days
    }

    /// Returns the reset lag in business days.
    #[must_use]
    pub fn reset_lag(&self) -> i32 {
        self.reset_lag
    }

    /// Sets the current reference rate.
    pub fn set_current_rate(&mut self, rate: Decimal) {
        self.current_rate = Some(rate);
    }

    /// Returns the current reference rate if set.
    #[must_use]
    pub fn current_rate(&self) -> Option<Decimal> {
        self.current_rate
    }

    /// Calculates the effective coupon rate after applying cap/floor.
    #[must_use]
    pub fn effective_rate(&self, index_rate: Decimal) -> Decimal {
        let mut rate = index_rate + self.spread_decimal();

        // Apply floor
        if let Some(floor) = self.floor {
            if rate < floor {
                rate = floor;
            }
        }

        // Apply cap
        if let Some(cap) = self.cap {
            if rate > cap {
                rate = cap;
            }
        }

        rate
    }

    /// Calculates the coupon amount for a period given the index rate.
    #[must_use]
    pub fn period_coupon(
        &self,
        period_start: Date,
        period_end: Date,
        index_rate: Decimal,
    ) -> Decimal {
        let dc = self.day_count.to_day_count();
        let year_frac = dc.year_fraction(period_start, period_end);
        let effective_rate = self.effective_rate(index_rate);

        self.face_value * effective_rate * Decimal::try_from(year_frac).unwrap_or(Decimal::ZERO)
    }

    /// Calculates accrued interest with a given reference rate.
    #[must_use]
    pub fn accrued_interest_with_rate(&self, settlement: Date, index_rate: Decimal) -> Decimal {
        if settlement <= self.issue_date {
            return Decimal::ZERO;
        }

        let Some(last_coupon) = self.previous_coupon_date(settlement) else {
            return Decimal::ZERO;
        };

        let Some(next_coupon) = self.next_coupon_date(settlement) else {
            return Decimal::ZERO;
        };

        if settlement >= next_coupon {
            return Decimal::ZERO;
        }

        let dc = self.day_count.to_day_count();
        let accrued_days = dc.day_count(last_coupon, settlement);
        let period_days = dc.day_count(last_coupon, next_coupon);

        if period_days == 0 {
            return Decimal::ZERO;
        }

        let effective_rate = self.effective_rate(index_rate);
        let periods_per_year = Decimal::from(self.frequency.periods_per_year());
        let period_coupon = self.face_value * effective_rate / periods_per_year;

        period_coupon * Decimal::from(accrued_days) / Decimal::from(period_days)
    }

    /// Calculates SOFR compounded in arrears for a period.
    ///
    /// This implements the ARRC standard methodology for calculating
    /// compounded SOFR over an interest period.
    ///
    /// # Arguments
    ///
    /// * `daily_rates` - Vector of (date, rate) tuples for daily SOFR fixings
    /// * `period_start` - Start of the interest period
    /// * `period_end` - End of the interest period
    ///
    /// # Returns
    ///
    /// The compounded rate for the period (annualized).
    #[must_use]
    pub fn sofr_compounded_in_arrears(
        &self,
        daily_rates: &[(Date, Decimal)],
        period_start: Date,
        period_end: Date,
    ) -> Decimal {
        let Some(SOFRConvention::CompoundedInArrears {
            lookback_days,
            observation_shift,
            lockout_days,
        }) = &self.sofr_convention
        else {
            return Decimal::ZERO;
        };

        let calendar = self.calendar.to_calendar();
        let mut compounded = 1.0_f64;
        let mut current = period_start;
        let mut days_count = 0_i64;

        while current < period_end {
            let next = calendar.add_business_days(current, 1);
            let weight_days = current.days_between(&next);

            // Determine observation date with lookback
            let observation_date = if *observation_shift {
                calendar.add_business_days(current, -(*lookback_days as i32))
            } else {
                current
            };

            // Apply lockout if applicable
            let rate_date = if let Some(lock) = lockout_days {
                let lock_start = calendar.add_business_days(period_end, -(*lock as i32));
                if current >= lock_start {
                    lock_start
                } else {
                    observation_date
                }
            } else {
                observation_date
            };

            // Look up the rate
            let rate = daily_rates
                .iter()
                .find(|(d, _)| *d == rate_date)
                .map_or(0.0, |(_, r)| r.to_string().parse::<f64>().unwrap_or(0.0));

            // Compound: (1 + rate * days/360)
            compounded *= 1.0 + rate * weight_days as f64 / 360.0;
            days_count += weight_days;
            current = next;
        }

        if days_count == 0 {
            return Decimal::ZERO;
        }

        // Annualize: ((compounded - 1) * 360 / days)
        let annualized = (compounded - 1.0) * 360.0 / days_count as f64;
        Decimal::try_from(annualized).unwrap_or(Decimal::ZERO)
    }

    /// Calculates simple average SOFR for a period.
    #[must_use]
    pub fn sofr_simple_average(
        &self,
        daily_rates: &[(Date, Decimal)],
        period_start: Date,
        period_end: Date,
    ) -> Decimal {
        let Some(SOFRConvention::SimpleAverage { lookback_days }) = &self.sofr_convention else {
            return Decimal::ZERO;
        };

        let calendar = self.calendar.to_calendar();
        let mut sum = 0.0_f64;
        let mut count = 0_i32;
        let mut current = period_start;

        while current < period_end {
            let observation_date = calendar.add_business_days(current, -(*lookback_days as i32));

            let rate = daily_rates
                .iter()
                .find(|(d, _)| *d == observation_date)
                .map_or(0.0, |(_, r)| r.to_string().parse::<f64>().unwrap_or(0.0));

            sum += rate;
            count += 1;
            current = calendar.add_business_days(current, 1);
        }

        if count == 0 {
            return Decimal::ZERO;
        }

        Decimal::try_from(sum / f64::from(count)).unwrap_or(Decimal::ZERO)
    }

    /// Returns an identifier string for display.
    #[must_use]
    pub fn identifier(&self) -> String {
        if let Some(cusip) = self.identifiers.cusip() {
            return cusip.to_string();
        }
        if let Some(isin) = self.identifiers.isin() {
            return isin.to_string();
        }
        if let Some(ticker) = self.identifiers.ticker() {
            return ticker.to_string();
        }
        "UNKNOWN".to_string()
    }

    /// Generates cash flows with projected rates from a forward curve.
    ///
    /// This method projects coupon amounts using forward rates from the
    /// provided curve, applying the spread and any caps/floors.
    ///
    /// # Arguments
    ///
    /// * `from` - Settlement date
    /// * `forward_curve` - Curve for projecting forward rates
    ///
    /// # Returns
    ///
    /// Vector of cash flows with projected coupon amounts.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let flows = frn.cash_flows_projected(settlement, &forward_curve);
    /// for flow in flows {
    ///     println!("Date: {}, Amount: {}", flow.date(), flow.amount());
    /// }
    /// ```
    pub fn cash_flows_projected<C>(&self, from: Date, forward_curve: &C) -> Vec<BondCashFlow>
    where
        C: convex_curves::traits::Curve,
    {
        if from >= self.maturity {
            return Vec::new();
        }

        let Ok(schedule) = self.schedule() else {
            return Vec::new();
        };

        let mut flows = Vec::new();
        let ref_date = forward_curve.reference_date();

        for (start, end) in schedule.unadjusted_periods() {
            if end <= from {
                continue;
            }

            // Calculate time fractions for forward rate lookup
            let t1 = start.days_between(&ref_date) as f64 / 365.0;
            let t2 = end.days_between(&ref_date) as f64 / 365.0;

            // Get forward rate from curve
            let fwd_rate = if t1 < 0.0 && t2 > 0.0 {
                // Period spans reference date - use rate to end
                forward_curve.forward_rate(0.0, t2.abs()).unwrap_or(0.0)
            } else if t1 >= 0.0 && t2 > 0.0 {
                forward_curve.forward_rate(t1, t2).unwrap_or(0.0)
            } else {
                // Historical period - use spot rate
                forward_curve
                    .zero_rate(t2.abs(), convex_curves::Compounding::Simple)
                    .unwrap_or(0.0)
            };

            // Apply spread, cap, and floor
            let projected_rate = Decimal::try_from(fwd_rate).unwrap_or(Decimal::ZERO);
            let effective_rate = self.effective_rate(projected_rate);

            // Calculate coupon using day count
            let dc = self.day_count.to_day_count();
            let year_frac = dc.year_fraction(start, end);
            let coupon_amount = self.face_value
                * effective_rate
                * Decimal::try_from(year_frac).unwrap_or(Decimal::ZERO);

            if end == self.maturity {
                flows.push(
                    BondCashFlow::coupon_and_principal(end, coupon_amount, self.face_value)
                        .with_accrual(start, end)
                        .with_reference_rate(projected_rate),
                );
            } else {
                flows.push(
                    BondCashFlow::coupon(end, coupon_amount)
                        .with_accrual(start, end)
                        .with_reference_rate(projected_rate),
                );
            }
        }

        flows
    }

    /// Returns all fixing dates required for coupon calculations.
    ///
    /// For overnight compounded rates (SOFR, SONIA), this returns all business
    /// days in each coupon period. For term rates (EURIBOR, Term SOFR), this
    /// returns only the fixing dates based on reset lag.
    ///
    /// # Arguments
    ///
    /// * `from` - Settlement date (only returns dates for future periods)
    ///
    /// # Returns
    ///
    /// Vector of dates when index fixings are needed.
    #[must_use]
    pub fn required_fixing_dates(&self, from: Date) -> Vec<Date> {
        let Ok(schedule) = self.schedule() else {
            return Vec::new();
        };

        let calendar = self.calendar.to_calendar();
        let mut dates = Vec::new();

        // Determine if this is an overnight compounding index
        let is_overnight = matches!(
            self.index,
            RateIndex::SOFR | RateIndex::SONIA | RateIndex::ESTR
        );

        for (start, end) in schedule.unadjusted_periods() {
            if end <= from {
                continue;
            }

            if is_overnight {
                // For overnight rates - need all business days in period
                if let Some(conv) = &self.sofr_convention {
                    let lookback = conv.lookback_days().unwrap_or(0);
                    let obs_shift = conv.is_in_arrears();

                    let mut current = start;
                    while current < end {
                        let obs_date = if obs_shift {
                            calendar.add_business_days(current, -(lookback as i32))
                        } else {
                            current
                        };
                        dates.push(obs_date);
                        current = calendar.add_business_days(current, 1);
                    }
                }
            } else {
                // For term rates - just the fixing date
                let fixing_date = calendar.add_business_days(start, self.reset_lag);
                dates.push(fixing_date);
            }
        }

        // Remove duplicates and sort
        dates.sort();
        dates.dedup();
        dates
    }

    /// Calculates accrued interest using rates from a fixing store.
    ///
    /// For overnight compounded rates, this compounds the daily rates
    /// from period start to settlement. For term rates, uses the fixed
    /// rate for the period.
    ///
    /// # Arguments
    ///
    /// * `settlement` - Settlement date
    /// * `store` - Index fixing store with historical rates
    ///
    /// # Returns
    ///
    /// Accrued interest amount, or zero if fixings are unavailable.
    #[must_use]
    pub fn accrued_interest_from_store(
        &self,
        settlement: Date,
        store: &crate::indices::IndexFixingStore,
    ) -> Decimal {
        if settlement <= self.issue_date {
            return Decimal::ZERO;
        }

        let Some(last_coupon) = self.previous_coupon_date(settlement) else {
            return Decimal::ZERO;
        };

        // For overnight compounded rates, we need to compound from period start to settlement
        if let Some(conv) = &self.sofr_convention {
            if conv.is_in_arrears() {
                let calendar = self.calendar.to_calendar();
                let rate = crate::indices::OvernightCompounding::compounded_rate(
                    store,
                    &self.index,
                    last_coupon,
                    settlement,
                    conv,
                    calendar.as_ref(),
                );

                if let Some(r) = rate {
                    let dc = self.day_count.to_day_count();
                    let year_frac = dc.year_fraction(last_coupon, settlement);
                    let effective = self.effective_rate(r);
                    return self.face_value
                        * effective
                        * Decimal::try_from(year_frac).unwrap_or(Decimal::ZERO);
                }
            }
        }

        // Fall back to term rate lookup
        let calendar = self.calendar.to_calendar();
        let fixing_date = calendar.add_business_days(last_coupon, self.reset_lag);

        if let Some(rate) = store.get_fixing(&self.index, fixing_date) {
            self.accrued_interest_with_rate(settlement, rate)
        } else {
            Decimal::ZERO
        }
    }

    /// Generates the payment schedule.
    fn schedule(&self) -> BondResult<Schedule> {
        let config = ScheduleConfig::new(self.issue_date, self.maturity, self.frequency)
            .with_calendar(self.calendar.clone());
        Schedule::generate(config)
    }
}

// ==================== Bond Trait Implementation ====================

impl Bond for FloatingRateNote {
    fn identifiers(&self) -> &BondIdentifiers {
        &self.identifiers
    }

    fn bond_type(&self) -> BondType {
        match (&self.cap, &self.floor) {
            (Some(_), Some(_)) => BondType::CollaredFRN,
            (Some(_), None) => BondType::CappedFRN,
            (None, Some(_)) => BondType::FlooredFRN,
            (None, None) => BondType::FloatingRateNote,
        }
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
        let calendar = self.calendar.to_calendar();
        calendar.add_business_days(self.issue_date, self.settlement_days as i32)
    }

    fn dated_date(&self) -> Date {
        self.issue_date
    }

    fn face_value(&self) -> Decimal {
        self.face_value
    }

    fn cash_flows(&self, from: Date) -> Vec<BondCashFlow> {
        if from >= self.maturity {
            return Vec::new();
        }

        let Ok(schedule) = self.schedule() else {
            return Vec::new();
        };

        let mut flows = Vec::new();

        for (start, end) in schedule.unadjusted_periods() {
            if end <= from {
                continue;
            }

            // For FRNs, we generate cash flows with estimated rate
            // The actual amount depends on the reference rate at fixing
            let rate = self.current_rate.unwrap_or(Decimal::ZERO);
            let coupon_amount = self.period_coupon(start, end, rate);

            if end == self.maturity {
                // Final payment includes principal
                flows.push(
                    BondCashFlow::coupon_and_principal(end, coupon_amount, self.face_value)
                        .with_accrual(start, end),
                );
            } else {
                flows.push(BondCashFlow::coupon(end, coupon_amount).with_accrual(start, end));
            }
        }

        flows
    }

    fn next_coupon_date(&self, after: Date) -> Option<Date> {
        let schedule = self.schedule().ok()?;
        schedule.dates().iter().find(|&&d| d > after).copied()
    }

    fn previous_coupon_date(&self, before: Date) -> Option<Date> {
        let schedule = self.schedule().ok()?;
        schedule
            .dates()
            .iter()
            .filter(|&&d| d < before)
            .next_back()
            .copied()
    }

    fn accrued_interest(&self, settlement: Date) -> Decimal {
        // Use current rate if set, otherwise assume zero
        let rate = self.current_rate.unwrap_or(Decimal::ZERO);
        self.accrued_interest_with_rate(settlement, rate)
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
        self.face_value
    }
}

// ==================== FloatingCouponBond Trait Implementation ====================

impl FloatingCouponBond for FloatingRateNote {
    fn rate_index(&self) -> &RateIndex {
        &self.index
    }

    fn spread_bps(&self) -> Decimal {
        self.spread_bps
    }

    fn reset_frequency(&self) -> u32 {
        self.frequency.periods_per_year()
    }

    fn lookback_days(&self) -> u32 {
        self.sofr_convention
            .as_ref()
            .and_then(crate::types::SOFRConvention::lookback_days)
            .unwrap_or(0)
    }

    fn floor(&self) -> Option<Decimal> {
        self.floor
    }

    fn cap(&self) -> Option<Decimal> {
        self.cap
    }

    fn next_reset_date(&self, after: Date) -> Option<Date> {
        self.next_coupon_date(after)
    }

    fn fixing_date(&self, reset_date: Date) -> Date {
        let calendar = self.calendar.to_calendar();
        calendar.add_business_days(reset_date, self.reset_lag)
    }
}

// ==================== Builder ====================

/// Builder for `FloatingRateNote`.
#[derive(Debug, Clone, Default)]
pub struct FloatingRateNoteBuilder {
    identifiers: Option<BondIdentifiers>,
    index: Option<RateIndex>,
    sofr_convention: Option<SOFRConvention>,
    spread_bps: Option<Decimal>,
    maturity: Option<Date>,
    issue_date: Option<Date>,
    frequency: Option<Frequency>,
    day_count: Option<DayCountConvention>,
    reset_lag: Option<i32>,
    payment_delay: Option<u32>,
    cap: Option<Decimal>,
    floor: Option<Decimal>,
    settlement_days: Option<u32>,
    calendar: Option<CalendarId>,
    currency: Option<Currency>,
    face_value: Option<Decimal>,
}

impl FloatingRateNoteBuilder {
    /// Creates a new builder.
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

    /// Sets the CUSIP identifier with validation.
    pub fn cusip(mut self, cusip: &str) -> Result<Self, crate::error::IdentifierError> {
        let cusip = Cusip::new(cusip)?;
        self.identifiers = Some(BondIdentifiers::new().with_cusip(cusip));
        Ok(self)
    }

    /// Sets the CUSIP identifier without validation.
    #[must_use]
    pub fn cusip_unchecked(mut self, cusip: &str) -> Self {
        self.identifiers = Some(BondIdentifiers::new().with_cusip(Cusip::new_unchecked(cusip)));
        self
    }

    /// Sets the ISIN identifier.
    #[must_use]
    pub fn isin_unchecked(mut self, isin: &str) -> Self {
        self.identifiers = Some(BondIdentifiers::new().with_isin(Isin::new_unchecked(isin)));
        self
    }

    /// Sets the reference rate index.
    #[must_use]
    pub fn index(mut self, index: RateIndex) -> Self {
        self.index = Some(index);
        self
    }

    /// Sets the SOFR compounding convention.
    #[must_use]
    pub fn sofr_convention(mut self, convention: SOFRConvention) -> Self {
        self.sofr_convention = Some(convention);
        self
    }

    /// Sets the spread in basis points.
    #[must_use]
    pub fn spread_bps(mut self, bps: i32) -> Self {
        self.spread_bps = Some(Decimal::from(bps));
        self
    }

    /// Sets the spread as a decimal.
    #[must_use]
    pub fn spread_decimal(mut self, spread: Decimal) -> Self {
        self.spread_bps = Some(spread * Decimal::from(10000));
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

    /// Sets the reset lag in business days.
    #[must_use]
    pub fn reset_lag(mut self, days: i32) -> Self {
        self.reset_lag = Some(days);
        self
    }

    /// Sets the payment delay in business days.
    #[must_use]
    pub fn payment_delay(mut self, days: u32) -> Self {
        self.payment_delay = Some(days);
        self
    }

    /// Sets the rate cap.
    #[must_use]
    pub fn cap(mut self, rate: Decimal) -> Self {
        self.cap = Some(rate);
        self
    }

    /// Sets the rate floor.
    #[must_use]
    pub fn floor(mut self, rate: Decimal) -> Self {
        self.floor = Some(rate);
        self
    }

    /// Sets the settlement days.
    #[must_use]
    pub fn settlement_days(mut self, days: u32) -> Self {
        self.settlement_days = Some(days);
        self
    }

    /// Sets the calendar.
    #[must_use]
    pub fn calendar(mut self, calendar: CalendarId) -> Self {
        self.calendar = Some(calendar);
        self
    }

    /// Sets the currency.
    #[must_use]
    pub fn currency(mut self, currency: Currency) -> Self {
        self.currency = Some(currency);
        self
    }

    /// Sets the face value.
    #[must_use]
    pub fn face_value(mut self, value: Decimal) -> Self {
        self.face_value = Some(value);
        self
    }

    // ==================== Market Convention Presets ====================

    /// Applies US Treasury FRN conventions.
    ///
    /// - Index: SOFR
    /// - Convention: Simple average with 2-day lookback
    /// - Day count: ACT/360
    /// - Frequency: Quarterly
    /// - Settlement: T+1
    #[must_use]
    pub fn us_treasury_frn(mut self) -> Self {
        self.index = Some(RateIndex::SOFR);
        self.sofr_convention = Some(SOFRConvention::SimpleAverage { lookback_days: 2 });
        self.day_count = Some(DayCountConvention::Act360);
        self.frequency = Some(Frequency::Quarterly);
        self.settlement_days = Some(1);
        self.calendar = Some(CalendarId::us_government());
        self.currency = Some(Currency::USD);
        self.reset_lag = Some(-2);
        self
    }

    /// Applies corporate SOFR FRN conventions.
    ///
    /// - Index: SOFR
    /// - Convention: Compounded in arrears with 5-day lookback
    /// - Day count: ACT/360
    /// - Frequency: Quarterly
    /// - Settlement: T+2
    #[must_use]
    pub fn corporate_sofr(mut self) -> Self {
        self.index = Some(RateIndex::SOFR);
        self.sofr_convention = Some(SOFRConvention::arrc_standard());
        self.day_count = Some(DayCountConvention::Act360);
        self.frequency = Some(Frequency::Quarterly);
        self.settlement_days = Some(2);
        self.calendar = Some(CalendarId::us_government());
        self.currency = Some(Currency::USD);
        self.reset_lag = Some(-2);
        self
    }

    /// Applies UK SONIA FRN conventions.
    ///
    /// - Index: SONIA
    /// - Day count: ACT/365F
    /// - Frequency: Quarterly
    /// - Settlement: T+1
    #[must_use]
    pub fn uk_sonia_frn(mut self) -> Self {
        self.index = Some(RateIndex::SONIA);
        self.day_count = Some(DayCountConvention::Act365Fixed);
        self.frequency = Some(Frequency::Quarterly);
        self.settlement_days = Some(1);
        self.calendar = Some(CalendarId::uk());
        self.currency = Some(Currency::GBP);
        self.reset_lag = Some(-5);
        self
    }

    /// Applies €STR FRN conventions.
    ///
    /// - Index: €STR
    /// - Day count: ACT/360
    /// - Frequency: Quarterly
    /// - Settlement: T+2
    #[must_use]
    pub fn estr_frn(mut self) -> Self {
        self.index = Some(RateIndex::ESTR);
        self.day_count = Some(DayCountConvention::Act360);
        self.frequency = Some(Frequency::Quarterly);
        self.settlement_days = Some(2);
        self.calendar = Some(CalendarId::target2());
        self.currency = Some(Currency::EUR);
        self.reset_lag = Some(-2);
        self
    }

    /// Applies EURIBOR FRN conventions.
    ///
    /// - Day count: ACT/360
    /// - Frequency: Quarterly (3M EURIBOR)
    /// - Settlement: T+2
    #[must_use]
    pub fn euribor_frn(mut self, tenor: crate::types::Tenor) -> Self {
        self.index = Some(RateIndex::EURIBOR { tenor });
        self.day_count = Some(DayCountConvention::Act360);
        self.frequency = Some(Frequency::Quarterly);
        self.settlement_days = Some(2);
        self.calendar = Some(CalendarId::target2());
        self.currency = Some(Currency::EUR);
        self.reset_lag = Some(-2);
        self
    }

    /// Builds the `FloatingRateNote`.
    pub fn build(self) -> BondResult<FloatingRateNote> {
        let identifiers = self.identifiers.unwrap_or_default();
        let index = self.index.ok_or(BondError::MissingField {
            field: "index".to_string(),
        })?;
        let maturity = self.maturity.ok_or(BondError::MissingField {
            field: "maturity".to_string(),
        })?;
        let issue_date = self.issue_date.ok_or(BondError::MissingField {
            field: "issue_date".to_string(),
        })?;

        if maturity <= issue_date {
            return Err(BondError::InvalidSpec {
                reason: "Maturity must be after issue date".to_string(),
            });
        }

        Ok(FloatingRateNote {
            identifiers,
            index,
            sofr_convention: self.sofr_convention,
            spread_bps: self.spread_bps.unwrap_or(Decimal::ZERO),
            maturity,
            issue_date,
            frequency: self.frequency.unwrap_or(Frequency::Quarterly),
            day_count: self.day_count.unwrap_or(DayCountConvention::Act360),
            reset_lag: self.reset_lag.unwrap_or(-2),
            payment_delay: self.payment_delay.unwrap_or(0),
            cap: self.cap,
            floor: self.floor,
            settlement_days: self.settlement_days.unwrap_or(2),
            calendar: self.calendar.unwrap_or_else(CalendarId::weekend_only),
            currency: self.currency.unwrap_or(Currency::USD),
            face_value: self.face_value.unwrap_or(Decimal::ONE_HUNDRED),
            current_rate: None,
        })
    }
}

// ==================== Serde Support ====================

// Helper functions for DayCountConvention serialization
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
        _ => DayCountConvention::Act360, // Default
    }
}

impl Serialize for FloatingRateNote {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("FloatingRateNote", 16)?;
        state.serialize_field("identifiers", &self.identifiers)?;
        state.serialize_field("index", &self.index)?;
        state.serialize_field("sofr_convention", &self.sofr_convention)?;
        state.serialize_field("spread_bps", &self.spread_bps)?;
        state.serialize_field("maturity", &self.maturity)?;
        state.serialize_field("issue_date", &self.issue_date)?;
        state.serialize_field("frequency", &self.frequency)?;
        state.serialize_field("day_count", &day_count_to_string(&self.day_count))?;
        state.serialize_field("reset_lag", &self.reset_lag)?;
        state.serialize_field("payment_delay", &self.payment_delay)?;
        state.serialize_field("cap", &self.cap)?;
        state.serialize_field("floor", &self.floor)?;
        state.serialize_field("settlement_days", &self.settlement_days)?;
        state.serialize_field("calendar", &self.calendar)?;
        state.serialize_field("currency", &self.currency)?;
        state.serialize_field("face_value", &self.face_value)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for FloatingRateNote {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct FloatingRateNoteData {
            identifiers: BondIdentifiers,
            index: RateIndex,
            sofr_convention: Option<SOFRConvention>,
            spread_bps: Decimal,
            maturity: Date,
            issue_date: Date,
            frequency: Frequency,
            day_count: String,
            reset_lag: i32,
            payment_delay: u32,
            cap: Option<Decimal>,
            floor: Option<Decimal>,
            settlement_days: u32,
            calendar: CalendarId,
            currency: Currency,
            face_value: Decimal,
        }

        let data = FloatingRateNoteData::deserialize(deserializer)?;
        Ok(FloatingRateNote {
            identifiers: data.identifiers,
            index: data.index,
            sofr_convention: data.sofr_convention,
            spread_bps: data.spread_bps,
            maturity: data.maturity,
            issue_date: data.issue_date,
            frequency: data.frequency,
            day_count: string_to_day_count(&data.day_count),
            reset_lag: data.reset_lag,
            payment_delay: data.payment_delay,
            cap: data.cap,
            floor: data.floor,
            settlement_days: data.settlement_days,
            calendar: data.calendar,
            currency: data.currency,
            face_value: data.face_value,
            current_rate: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    #[test]
    fn test_frn_builder() {
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("912828ZQ7")
            .index(RateIndex::SOFR)
            .sofr_convention(SOFRConvention::arrc_standard())
            .spread_bps(50)
            .maturity(date(2026, 7, 31))
            .issue_date(date(2024, 7, 31))
            .build()
            .unwrap();

        assert_eq!(frn.spread_bps(), dec!(50));
        assert_eq!(frn.spread_decimal(), dec!(0.0050));
        assert!(frn.sofr_convention().is_some());
    }

    #[test]
    fn test_us_treasury_frn() {
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("912828ZQ7")
            .spread_bps(15)
            .maturity(date(2026, 7, 31))
            .issue_date(date(2024, 7, 31))
            .us_treasury_frn()
            .build()
            .unwrap();

        assert_eq!(*frn.index(), RateIndex::SOFR);
        assert_eq!(frn.day_count(), DayCountConvention::Act360);
        assert_eq!(frn.frequency(), Frequency::Quarterly);
        assert_eq!(frn.settlement_days(), 1);
    }

    #[test]
    fn test_corporate_sofr_frn() {
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .spread_bps(150)
            .maturity(date(2027, 6, 15))
            .issue_date(date(2024, 6, 15))
            .corporate_sofr()
            .build()
            .unwrap();

        assert_eq!(*frn.index(), RateIndex::SOFR);
        assert!(frn.sofr_convention().unwrap().is_in_arrears());
        assert_eq!(frn.settlement_days(), 2);
    }

    #[test]
    fn test_effective_rate_with_floor() {
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .index(RateIndex::SOFR)
            .spread_bps(50) // 50 bps spread
            .floor(dec!(0.01)) // 1% floor
            .maturity(date(2026, 6, 15))
            .issue_date(date(2024, 6, 15))
            .build()
            .unwrap();

        // Index at 0.3%, spread 0.5% = 0.8% < floor 1%
        let effective = frn.effective_rate(dec!(0.003));
        assert_eq!(effective, dec!(0.01)); // Floor applied

        // Index at 0.6%, spread 0.5% = 1.1% > floor 1%
        let effective = frn.effective_rate(dec!(0.006));
        assert_eq!(effective, dec!(0.011)); // No floor
    }

    #[test]
    fn test_effective_rate_with_cap() {
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .index(RateIndex::SOFR)
            .spread_bps(50)
            .cap(dec!(0.08)) // 8% cap
            .maturity(date(2026, 6, 15))
            .issue_date(date(2024, 6, 15))
            .build()
            .unwrap();

        // Index at 8%, spread 0.5% = 8.5% > cap 8%
        let effective = frn.effective_rate(dec!(0.08));
        assert_eq!(effective, dec!(0.08)); // Cap applied

        // Index at 5%, spread 0.5% = 5.5% < cap 8%
        let effective = frn.effective_rate(dec!(0.05));
        assert_eq!(effective, dec!(0.055)); // No cap
    }

    #[test]
    fn test_effective_rate_collar() {
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .index(RateIndex::SOFR)
            .spread_bps(50)
            .floor(dec!(0.02)) // 2% floor
            .cap(dec!(0.06)) // 6% cap
            .maturity(date(2026, 6, 15))
            .issue_date(date(2024, 6, 15))
            .build()
            .unwrap();

        // Below floor
        assert_eq!(frn.effective_rate(dec!(0.01)), dec!(0.02));

        // In range
        assert_eq!(frn.effective_rate(dec!(0.04)), dec!(0.045));

        // Above cap
        assert_eq!(frn.effective_rate(dec!(0.06)), dec!(0.06));
    }

    #[test]
    fn test_period_coupon() {
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .index(RateIndex::SOFR)
            .spread_bps(50)
            .face_value(dec!(100))
            .maturity(date(2026, 6, 15))
            .issue_date(date(2024, 6, 15))
            .day_count(DayCountConvention::Act360)
            .build()
            .unwrap();

        // 90-day period at 5% (with 0.5% spread = 5.5%)
        let coupon = frn.period_coupon(date(2025, 1, 15), date(2025, 4, 15), dec!(0.05));

        // 100 * 0.055 * (90/360) = 1.375
        assert!(coupon > dec!(1.37) && coupon < dec!(1.38));
    }

    #[test]
    fn test_bond_type_classification() {
        // Plain FRN
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .index(RateIndex::SOFR)
            .maturity(date(2026, 6, 15))
            .issue_date(date(2024, 6, 15))
            .build()
            .unwrap();
        assert_eq!(frn.bond_type(), BondType::FloatingRateNote);

        // Capped FRN
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .index(RateIndex::SOFR)
            .cap(dec!(0.08))
            .maturity(date(2026, 6, 15))
            .issue_date(date(2024, 6, 15))
            .build()
            .unwrap();
        assert_eq!(frn.bond_type(), BondType::CappedFRN);

        // Floored FRN
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .index(RateIndex::SOFR)
            .floor(dec!(0.02))
            .maturity(date(2026, 6, 15))
            .issue_date(date(2024, 6, 15))
            .build()
            .unwrap();
        assert_eq!(frn.bond_type(), BondType::FlooredFRN);

        // Collared FRN
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .index(RateIndex::SOFR)
            .cap(dec!(0.08))
            .floor(dec!(0.02))
            .maturity(date(2026, 6, 15))
            .issue_date(date(2024, 6, 15))
            .build()
            .unwrap();
        assert_eq!(frn.bond_type(), BondType::CollaredFRN);
    }

    #[test]
    fn test_accrued_interest() {
        let mut frn = FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .index(RateIndex::SOFR)
            .spread_bps(50)
            .face_value(dec!(100))
            .frequency(Frequency::Quarterly)
            .day_count(DayCountConvention::Act360)
            .maturity(date(2026, 6, 15))
            .issue_date(date(2024, 6, 15))
            .build()
            .unwrap();

        // Set current rate
        frn.set_current_rate(dec!(0.05)); // 5% SOFR

        // Settlement mid-period
        let settlement = date(2025, 2, 15);
        let accrued = frn.accrued_interest(settlement);

        // Should have some accrued interest
        assert!(accrued > Decimal::ZERO);
    }

    #[test]
    fn test_cash_flows() {
        let mut frn = FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .index(RateIndex::SOFR)
            .spread_bps(50)
            .frequency(Frequency::Quarterly)
            .maturity(date(2025, 6, 15))
            .issue_date(date(2024, 6, 15))
            .build()
            .unwrap();

        frn.set_current_rate(dec!(0.05));

        let flows = frn.cash_flows(date(2024, 6, 15));

        // 1 year quarterly = 4 payments
        assert_eq!(flows.len(), 4);

        // Last payment includes principal
        assert!(flows.last().unwrap().is_principal());
    }

    #[test]
    fn test_sonia_frn() {
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("GBTEST001")
            .spread_bps(25)
            .maturity(date(2026, 9, 30))
            .issue_date(date(2024, 9, 30))
            .uk_sonia_frn()
            .build()
            .unwrap();

        assert_eq!(*frn.index(), RateIndex::SONIA);
        assert_eq!(frn.day_count(), DayCountConvention::Act365Fixed);
        assert_eq!(frn.currency(), Currency::GBP);
    }

    #[test]
    fn test_estr_frn() {
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("EUTEST001")
            .spread_bps(30)
            .maturity(date(2026, 12, 15))
            .issue_date(date(2024, 12, 15))
            .estr_frn()
            .build()
            .unwrap();

        assert_eq!(*frn.index(), RateIndex::ESTR);
        assert_eq!(frn.day_count(), DayCountConvention::Act360);
        assert_eq!(frn.currency(), Currency::EUR);
    }

    #[test]
    fn test_sofr_convention_display() {
        let conv = SOFRConvention::arrc_standard();
        let display = format!("{}", conv);
        assert!(display.contains("5D lookback"));
        assert!(display.contains("observation shift"));
    }

    #[test]
    fn test_missing_required_fields() {
        // Missing index
        let result = FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .maturity(date(2026, 6, 15))
            .issue_date(date(2024, 6, 15))
            .build();
        assert!(result.is_err());

        // Missing maturity
        let result = FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .index(RateIndex::SOFR)
            .issue_date(date(2024, 6, 15))
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_dates() {
        let result = FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .index(RateIndex::SOFR)
            .maturity(date(2024, 6, 15))
            .issue_date(date(2026, 6, 15)) // Issue after maturity
            .build();
        assert!(result.is_err());
    }
}
