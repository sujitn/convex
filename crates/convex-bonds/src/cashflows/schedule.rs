//! Schedule generation for bond cash flows.
//!
//! This module provides schedule generation with support for:
//! - Regular periodic schedules
//! - Stub periods (short/long first/last)
//! - End-of-month rules
//! - Business day adjustments
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_bonds::cashflows::{Schedule, ScheduleConfig, StubType};
//! use convex_core::types::{Date, Frequency};
//! use convex_core::calendars::BusinessDayConvention;
//!
//! let config = ScheduleConfig::new(
//!     Date::from_ymd(2020, 1, 15).unwrap(),
//!     Date::from_ymd(2025, 7, 15).unwrap(),
//!     Frequency::SemiAnnual,
//! );
//!
//! let schedule = Schedule::generate(config).unwrap();
//! for (start, end) in schedule.periods() {
//!     println!("{} to {}", start, end);
//! }
//! ```

use serde::{Deserialize, Serialize};

use convex_core::calendars::{BusinessDayConvention, Calendar, SIFMACalendar, Target2Calendar, WeekendCalendar};
use convex_core::types::{Date, Frequency};

use crate::error::{BondError, BondResult};
use crate::types::CalendarId;

/// Stub period type for irregular first or last coupon periods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StubType {
    /// No stub (schedule divides evenly)
    #[default]
    None,
    /// Short first period (front stub)
    ShortFirst,
    /// Long first period (front stub)
    LongFirst,
    /// Short last period (back stub)
    ShortLast,
    /// Long last period (back stub)
    LongLast,
}

impl StubType {
    /// Returns true if this is a front stub (affects first period).
    #[must_use]
    pub fn is_front_stub(&self) -> bool {
        matches!(self, StubType::ShortFirst | StubType::LongFirst)
    }

    /// Returns true if this is a back stub (affects last period).
    #[must_use]
    pub fn is_back_stub(&self) -> bool {
        matches!(self, StubType::ShortLast | StubType::LongLast)
    }
}

/// Configuration for schedule generation.
#[derive(Debug, Clone)]
pub struct ScheduleConfig {
    /// Start date (issue date or first accrual start)
    pub start_date: Date,
    /// End date (maturity)
    pub end_date: Date,
    /// Payment frequency
    pub frequency: Frequency,
    /// Calendar for business day adjustments
    pub calendar: CalendarId,
    /// Business day adjustment convention
    pub business_day_convention: BusinessDayConvention,
    /// End-of-month rule
    pub end_of_month: bool,
    /// First regular coupon date (for front stubs)
    pub first_regular_date: Option<Date>,
    /// Penultimate coupon date (for back stubs)
    pub penultimate_date: Option<Date>,
    /// Stub type
    pub stub_type: StubType,
}

impl ScheduleConfig {
    /// Creates a new schedule configuration with defaults.
    #[must_use]
    pub fn new(start_date: Date, end_date: Date, frequency: Frequency) -> Self {
        Self {
            start_date,
            end_date,
            frequency,
            calendar: CalendarId::weekend_only(),
            business_day_convention: BusinessDayConvention::ModifiedFollowing,
            end_of_month: true,
            first_regular_date: None,
            penultimate_date: None,
            stub_type: StubType::None,
        }
    }

    /// Sets the calendar for business day adjustments.
    #[must_use]
    pub fn with_calendar(mut self, calendar: CalendarId) -> Self {
        self.calendar = calendar;
        self
    }

    /// Sets the business day convention.
    #[must_use]
    pub fn with_business_day_convention(mut self, convention: BusinessDayConvention) -> Self {
        self.business_day_convention = convention;
        self
    }

    /// Sets the end-of-month rule.
    #[must_use]
    pub fn with_end_of_month(mut self, eom: bool) -> Self {
        self.end_of_month = eom;
        self
    }

    /// Sets the first regular coupon date (for front stubs).
    #[must_use]
    pub fn with_first_regular_date(mut self, date: Date) -> Self {
        self.first_regular_date = Some(date);
        self
    }

    /// Sets the penultimate coupon date (for back stubs).
    #[must_use]
    pub fn with_penultimate_date(mut self, date: Date) -> Self {
        self.penultimate_date = Some(date);
        self
    }

    /// Sets the stub type.
    #[must_use]
    pub fn with_stub_type(mut self, stub_type: StubType) -> Self {
        self.stub_type = stub_type;
        self
    }
}

/// A date schedule for coupon payments.
///
/// Contains unadjusted and adjusted dates for a bond's coupon schedule.
#[derive(Debug, Clone)]
pub struct Schedule {
    /// Unadjusted schedule dates
    unadjusted_dates: Vec<Date>,
    /// Adjusted schedule dates (for payment)
    adjusted_dates: Vec<Date>,
    /// Calendar used for adjustments
    calendar: CalendarId,
    /// Business day convention used
    convention: BusinessDayConvention,
}

impl Schedule {
    /// Generates a schedule from the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the schedule configuration is invalid.
    pub fn generate(config: ScheduleConfig) -> BondResult<Self> {
        if config.end_date <= config.start_date {
            return Err(BondError::InvalidSchedule {
                message: "End date must be after start date".to_string(),
            });
        }

        if config.frequency.is_zero() {
            // Zero coupon: just start and end dates
            return Ok(Self {
                unadjusted_dates: vec![config.start_date, config.end_date],
                adjusted_dates: vec![config.start_date, config.end_date],
                calendar: config.calendar,
                convention: config.business_day_convention,
            });
        }

        let months_per_period = config.frequency.months_per_period() as i32;
        let mut unadjusted = Vec::new();

        // Determine generation direction based on stub type
        if config.stub_type.is_front_stub() || config.first_regular_date.is_some() {
            // Generate forward from first regular date or start
            Self::generate_forward(&config, months_per_period, &mut unadjusted)?;
        } else {
            // Generate backward from maturity (default)
            Self::generate_backward(&config, months_per_period, &mut unadjusted)?;
        }

        // Ensure start and end dates are included
        if unadjusted.first() != Some(&config.start_date) {
            unadjusted.insert(0, config.start_date);
        }
        if unadjusted.last() != Some(&config.end_date) {
            unadjusted.push(config.end_date);
        }

        // Remove duplicates and sort
        unadjusted.sort();
        unadjusted.dedup();

        // Adjust for business days
        let adjusted = Self::adjust_dates(&unadjusted, &config)?;

        Ok(Self {
            unadjusted_dates: unadjusted,
            adjusted_dates: adjusted,
            calendar: config.calendar,
            convention: config.business_day_convention,
        })
    }

    /// Generates dates backward from maturity.
    fn generate_backward(
        config: &ScheduleConfig,
        months_per_period: i32,
        dates: &mut Vec<Date>,
    ) -> BondResult<()> {
        let mut current = config.end_date;
        dates.push(current);

        while current > config.start_date {
            current = current.add_months(-months_per_period)?;

            // Handle end-of-month rule
            if config.end_of_month && config.end_date.is_end_of_month() {
                current = current.end_of_month();
            }

            // Handle penultimate date
            if let Some(penult) = config.penultimate_date {
                if current <= penult && current > config.start_date {
                    dates.push(penult);
                    current = penult;
                    continue;
                }
            }

            if current > config.start_date {
                dates.push(current);
            }
        }

        dates.push(config.start_date);
        dates.reverse();
        Ok(())
    }

    /// Generates dates forward from start date or first regular date.
    fn generate_forward(
        config: &ScheduleConfig,
        months_per_period: i32,
        dates: &mut Vec<Date>,
    ) -> BondResult<()> {
        dates.push(config.start_date);

        // If there's a first regular date, add it
        let start_point = config.first_regular_date.unwrap_or(config.start_date);
        if start_point != config.start_date {
            dates.push(start_point);
        }

        let mut current = start_point;

        while current < config.end_date {
            current = current.add_months(months_per_period)?;

            // Handle end-of-month rule
            if config.end_of_month && start_point.is_end_of_month() {
                current = current.end_of_month();
            }

            if current < config.end_date {
                dates.push(current);
            }
        }

        dates.push(config.end_date);
        Ok(())
    }

    /// Adjusts dates for business days using the configured calendar.
    fn adjust_dates(dates: &[Date], config: &ScheduleConfig) -> BondResult<Vec<Date>> {
        let calendar = config.calendar.to_calendar();

        dates
            .iter()
            .map(|&date| {
                calendar
                    .adjust(date, config.business_day_convention)
                    .map_err(|e| BondError::InvalidSchedule {
                        message: format!("Failed to adjust date {}: {}", date, e),
                    })
            })
            .collect()
    }

    /// Returns the unadjusted schedule dates.
    #[must_use]
    pub fn unadjusted_dates(&self) -> &[Date] {
        &self.unadjusted_dates
    }

    /// Returns the adjusted schedule dates (for payment).
    #[must_use]
    pub fn dates(&self) -> &[Date] {
        &self.adjusted_dates
    }

    /// Returns an iterator over the coupon periods (start, end) using adjusted dates.
    pub fn periods(&self) -> impl Iterator<Item = (Date, Date)> + '_ {
        self.adjusted_dates.windows(2).map(|w| (w[0], w[1]))
    }

    /// Returns an iterator over the coupon periods using unadjusted dates.
    ///
    /// Useful for accrual calculations which typically use unadjusted dates.
    pub fn unadjusted_periods(&self) -> impl Iterator<Item = (Date, Date)> + '_ {
        self.unadjusted_dates.windows(2).map(|w| (w[0], w[1]))
    }

    /// Returns the number of periods in the schedule.
    #[must_use]
    pub fn num_periods(&self) -> usize {
        self.adjusted_dates.len().saturating_sub(1)
    }

    /// Returns true if the schedule is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.adjusted_dates.len() < 2
    }

    /// Returns the calendar used for this schedule.
    #[must_use]
    pub fn calendar(&self) -> &CalendarId {
        &self.calendar
    }

    /// Returns the business day convention used.
    #[must_use]
    pub fn convention(&self) -> BusinessDayConvention {
        self.convention
    }
}

impl CalendarId {
    /// Converts the calendar ID to a boxed Calendar trait object.
    #[must_use]
    pub fn to_calendar(&self) -> Box<dyn Calendar> {
        match self.as_str() {
            "US_GOVERNMENT" | "US-GOV" => Box::new(SIFMACalendar::new()),
            "SIFMA" | "US" => Box::new(SIFMACalendar::new()),
            "TARGET2" | "EUR" => Box::new(Target2Calendar::new()),
            "WEEKEND" => Box::new(WeekendCalendar),
            _ => Box::new(WeekendCalendar),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schedule_generation_semiannual() {
        let config = ScheduleConfig::new(
            Date::from_ymd(2020, 1, 15).unwrap(),
            Date::from_ymd(2025, 1, 15).unwrap(),
            Frequency::SemiAnnual,
        );

        let schedule = Schedule::generate(config).unwrap();

        // 5 years, semi-annual = 10 periods + 1 for start = 11 dates
        assert_eq!(schedule.dates().len(), 11);
        assert_eq!(schedule.num_periods(), 10);
    }

    #[test]
    fn test_schedule_generation_annual() {
        let config = ScheduleConfig::new(
            Date::from_ymd(2020, 6, 15).unwrap(),
            Date::from_ymd(2025, 6, 15).unwrap(),
            Frequency::Annual,
        );

        let schedule = Schedule::generate(config).unwrap();

        // 5 years, annual = 5 periods + 1 for start = 6 dates
        assert_eq!(schedule.dates().len(), 6);
        assert_eq!(schedule.num_periods(), 5);
    }

    #[test]
    fn test_schedule_generation_zero_coupon() {
        let config = ScheduleConfig::new(
            Date::from_ymd(2020, 1, 15).unwrap(),
            Date::from_ymd(2025, 1, 15).unwrap(),
            Frequency::Zero,
        );

        let schedule = Schedule::generate(config).unwrap();

        assert_eq!(schedule.dates().len(), 2);
        assert_eq!(schedule.num_periods(), 1);
    }

    #[test]
    fn test_schedule_with_front_stub() {
        let config = ScheduleConfig::new(
            Date::from_ymd(2020, 3, 15).unwrap(),  // Odd start
            Date::from_ymd(2025, 6, 15).unwrap(),
            Frequency::SemiAnnual,
        )
        .with_first_regular_date(Date::from_ymd(2020, 6, 15).unwrap())
        .with_stub_type(StubType::ShortFirst);

        let schedule = Schedule::generate(config).unwrap();

        // First period should be short (Mar 15 to Jun 15)
        let periods: Vec<_> = schedule.unadjusted_periods().collect();
        assert_eq!(periods[0].0, Date::from_ymd(2020, 3, 15).unwrap());
        assert_eq!(periods[0].1, Date::from_ymd(2020, 6, 15).unwrap());
    }

    #[test]
    fn test_schedule_periods_iterator() {
        let config = ScheduleConfig::new(
            Date::from_ymd(2024, 1, 15).unwrap(),
            Date::from_ymd(2025, 1, 15).unwrap(),
            Frequency::Quarterly,
        );

        let schedule = Schedule::generate(config).unwrap();
        let periods: Vec<_> = schedule.periods().collect();

        assert_eq!(periods.len(), 4);
    }

    #[test]
    fn test_invalid_schedule() {
        let config = ScheduleConfig::new(
            Date::from_ymd(2025, 1, 15).unwrap(),
            Date::from_ymd(2020, 1, 15).unwrap(),  // End before start
            Frequency::SemiAnnual,
        );

        let result = Schedule::generate(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_stub_type_predicates() {
        assert!(StubType::ShortFirst.is_front_stub());
        assert!(StubType::LongFirst.is_front_stub());
        assert!(!StubType::ShortFirst.is_back_stub());

        assert!(StubType::ShortLast.is_back_stub());
        assert!(StubType::LongLast.is_back_stub());
        assert!(!StubType::ShortLast.is_front_stub());

        assert!(!StubType::None.is_front_stub());
        assert!(!StubType::None.is_back_stub());
    }
}
