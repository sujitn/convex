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

use convex_core::calendars::{
    BusinessDayConvention, Calendar, JapanCalendar, JointCalendar, SIFMACalendar, Target2Calendar,
    UKCalendar, WeekendCalendar,
};
use convex_core::cashflows::{CouponPeriod, CouponSchedule, DateRules, ScheduleSpec};
use convex_core::types::{Date, Frequency};

use crate::error::{BondError, BondResult};
use crate::types::CalendarId;

// Keep the established bond/analytics path while sharing the actual type.
pub use convex_core::cashflows::StubType;

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
    /// Shared validated contractual dates, references and coupon periods.
    coupon_schedule: CouponSchedule,
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
        let calendar = config.calendar.try_to_calendar()?;
        Self::generate_with_calendar(config, calendar.as_ref())
    }

    /// Generate using an immutable supplied calendar, including custom holidays.
    /// Existing `None` retains the historical automatic short-front-stub default;
    /// the shared ScheduleSpec uses `None` to require an exact regular grid.
    pub fn generate_with_calendar(
        config: ScheduleConfig,
        calendar: &dyn Calendar,
    ) -> BondResult<Self> {
        let stub_type = if config.stub_type == StubType::None && !config.frequency.is_zero() {
            if config.first_regular_date.is_some() {
                StubType::ShortLast
            } else {
                StubType::ShortFirst
            }
        } else {
            config.stub_type
        };
        let spec = ScheduleSpec {
            start_date: config.start_date,
            end_date: config.end_date,
            frequency: config.frequency,
            stub_type,
            end_of_month: config.end_of_month,
            first_regular_date: config.first_regular_date,
            penultimate_date: config.penultimate_date,
        };
        let rules = DateRules {
            payment_adjustment: if config.frequency.is_zero() {
                BusinessDayConvention::Unadjusted
            } else {
                config.business_day_convention
            },
            ..DateRules::default()
        };
        let coupon_schedule =
            CouponSchedule::generate(&spec, &rules, calendar).map_err(|error| {
                BondError::InvalidSchedule {
                    message: error.to_string(),
                }
            })?;
        let first = if config.frequency.is_zero() {
            config.start_date
        } else {
            calendar
                .adjust(config.start_date, config.business_day_convention)
                .map_err(|error| BondError::InvalidSchedule {
                    message: error.to_string(),
                })?
        };
        let mut adjusted_dates = Vec::with_capacity(coupon_schedule.contractual_dates().len());
        adjusted_dates.push(first);
        adjusted_dates.extend(
            coupon_schedule
                .periods()
                .iter()
                .map(CouponPeriod::payment_date),
        );
        Ok(Self {
            coupon_schedule,
            adjusted_dates,
            calendar: config.calendar,
            convention: config.business_day_convention,
        })
    }

    /// Shared period details, including unadjusted accrual and ICMA references.
    #[must_use]
    pub fn coupon_periods(&self) -> &[CouponPeriod] {
        self.coupon_schedule.periods()
    }

    /// Returns the unadjusted schedule dates.
    #[must_use]
    pub fn unadjusted_dates(&self) -> &[Date] {
        self.coupon_schedule.contractual_dates()
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
        self.coupon_schedule
            .contractual_dates()
            .windows(2)
            .map(|w| (w[0], w[1]))
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
    /// Resolve supported identifiers and joint holiday calendars. Unknown names
    /// fail instead of silently using a weekend-only production schedule.
    pub fn try_to_calendar(&self) -> BondResult<Box<dyn Calendar>> {
        // Preserve the existing single empty weekend spelling, but reject empty
        // components in a joint calendar rather than silently changing its terms.
        if self.as_str().is_empty() {
            return Ok(Box::new(WeekendCalendar));
        }
        if self.as_str().len() > 256 {
            return Err(BondError::InvalidSchedule {
                message: "Calendar identifier exceeds 256 bytes".into(),
            });
        }
        let components: Vec<_> = self.components().collect();
        if components.is_empty() || components.len() > 16 {
            return Err(BondError::InvalidSchedule {
                message: "Calendar requires 1..16 components".into(),
            });
        }
        let mut calendars: Vec<Box<dyn Calendar>> = Vec::with_capacity(components.len());
        for component in components {
            let calendar: Box<dyn Calendar> = match component {
                // Preserve the established bond calendar aliases.
                "US_GOVERNMENT" | "US-GOV" | "USGov" | "SIFMA" | "US" => Box::new(SIFMACalendar::new()),
                "TARGET2" | "EUR" => Box::new(Target2Calendar::new()),
                "UK" | "GBP" => Box::new(UKCalendar::new()),
                "Japan" | "JPY" => Box::new(JapanCalendar::new()),
                "WEEKEND" => Box::new(WeekendCalendar),
                _ => return Err(BondError::InvalidSchedule { message: format!("Unsupported calendar {component}; supply a calendar with generate_with_calendar") }),
            };
            calendars.push(calendar);
        }
        if calendars.len() == 1 {
            calendars.pop().ok_or_else(|| BondError::InvalidSchedule {
                message: "Empty calendar".into(),
            })
        } else {
            Ok(Box::new(JointCalendar::new(calendars)))
        }
    }

    /// Compatible identifier convenience. Prefer try_to_calendar or a supplied
    /// calendar at checked ingress; this older infallible API retains its fallback.
    #[must_use]
    pub fn to_calendar(&self) -> Box<dyn Calendar> {
        match self.as_str() {
            "US_GOVERNMENT" | "US-GOV" | "USGov" | "SIFMA" | "US" => Box::new(SIFMACalendar::new()),
            "TARGET2" | "EUR" => Box::new(Target2Calendar::new()),
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
            Date::from_ymd(2020, 3, 15).unwrap(), // Odd start
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
            Date::from_ymd(2020, 1, 15).unwrap(), // End before start
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
