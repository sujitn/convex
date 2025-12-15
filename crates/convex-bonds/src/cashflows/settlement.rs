//! Settlement date calculation utilities.
//!
//! This module provides utilities for calculating settlement dates from
//! trade dates, determining ex-dividend status, and handling market-specific
//! settlement conventions.
//!
//! # Reference
//!
//! - T2S Settlement Cycles
//! - DTCC Settlement Conventions

use convex_core::types::Date;

use crate::types::{CalendarId, DayType, ExDividendRules, SettlementRules};

/// Settlement date calculator.
///
/// Provides utilities for calculating settlement dates and determining
/// ex-dividend status based on market conventions.
///
/// # Example
///
/// ```rust
/// use convex_bonds::cashflows::settlement::SettlementCalculator;
/// use convex_bonds::types::SettlementRules;
/// use convex_core::types::Date;
///
/// let rules = SettlementRules::us_treasury();
/// let trade_date = Date::from_ymd(2025, 3, 10).unwrap();
///
/// // For simple case without calendar lookup
/// let settlement = SettlementCalculator::settlement_date_simple(trade_date, &rules);
/// ```
pub struct SettlementCalculator;

impl SettlementCalculator {
    /// Calculates settlement date from trade date using simple calendar day offset.
    ///
    /// This is a simplified calculation that doesn't account for holidays.
    /// For production use, use `settlement_date` with a proper calendar.
    ///
    /// # Arguments
    ///
    /// * `trade_date` - The trade execution date
    /// * `rules` - Settlement rules for the market/instrument
    #[must_use]
    pub fn settlement_date_simple(trade_date: Date, rules: &SettlementRules) -> Date {
        let days = rules.days as i64;

        if rules.use_business_days {
            // Approximate: add extra days for weekends
            // Proper implementation should use calendar
            let calendar_days = days + (days / 5) * 2 + 2;
            trade_date.add_days(calendar_days)
        } else {
            trade_date.add_days(days)
        }
    }

    /// Calculates the ex-dividend date for a coupon payment.
    ///
    /// Returns the first date on which the bond trades ex-dividend,
    /// meaning the buyer will not receive the next coupon.
    ///
    /// # Arguments
    ///
    /// * `coupon_date` - The coupon payment date
    /// * `rules` - Ex-dividend rules for the market/instrument
    #[must_use]
    pub fn ex_dividend_date(coupon_date: Date, rules: &ExDividendRules) -> Date {
        let days = rules.days as i64;

        match rules.day_type {
            DayType::CalendarDays => coupon_date.add_days(-days),
            DayType::BusinessDays => {
                // Approximate: add extra days for weekends
                let calendar_days = days + (days / 5) * 2;
                coupon_date.add_days(-calendar_days)
            }
        }
    }

    /// Determines if settlement is in the ex-dividend period.
    ///
    /// During the ex-dividend period, the buyer does not receive the
    /// upcoming coupon payment.
    ///
    /// # Arguments
    ///
    /// * `settlement` - Settlement date
    /// * `next_coupon` - Next coupon payment date
    /// * `rules` - Ex-dividend rules
    #[must_use]
    pub fn is_ex_dividend(settlement: Date, next_coupon: Date, rules: &ExDividendRules) -> bool {
        let ex_date = Self::ex_dividend_date(next_coupon, rules);
        settlement >= ex_date && settlement < next_coupon
    }

    /// Calculates the record date for Italian BTPs and similar bonds.
    ///
    /// Record date is when ownership is recorded for coupon payment purposes.
    ///
    /// # Arguments
    ///
    /// * `coupon_date` - The coupon payment date
    /// * `days_before` - Number of business days before coupon
    #[must_use]
    pub fn record_date(coupon_date: Date, days_before: u32) -> Date {
        // Approximate business days
        let calendar_days = days_before as i64 + (days_before as i64 / 5) * 2;
        coupon_date.add_days(-calendar_days)
    }

    /// Validates that settlement date is not before trade date.
    #[must_use]
    pub fn is_valid_settlement(
        trade_date: Date,
        settlement: Date,
        rules: &SettlementRules,
    ) -> bool {
        if settlement < trade_date {
            return false;
        }

        if !rules.allow_same_day && settlement == trade_date {
            return false;
        }

        true
    }

    /// Returns the minimum settlement date for a trade date.
    #[must_use]
    pub fn minimum_settlement(trade_date: Date, rules: &SettlementRules) -> Date {
        if rules.allow_same_day {
            trade_date
        } else {
            trade_date.add_days(1)
        }
    }

    /// Calculates the number of days between trade and settlement.
    #[must_use]
    pub fn settlement_lag(trade_date: Date, settlement: Date) -> i64 {
        trade_date.days_between(&settlement)
    }

    /// Returns the standard settlement days for common markets.
    #[must_use]
    pub fn standard_settlement_days(calendar: &CalendarId) -> u32 {
        // Most markets are T+2, with some exceptions
        match calendar.as_str() {
            "USGov" | "NYC" | "SIFMA" => 1, // US Treasuries are T+1
            "UK" => 1,                      // UK Gilts are T+1
            "Japan" => 2,                   // JGBs are T+2
            _ => 2,                         // Default T+2
        }
    }
}

/// Settlement status for a bond trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettlementStatus {
    /// Regular settlement (cum-dividend)
    Regular,
    /// Settlement in ex-dividend period
    ExDividend,
    /// Settlement on coupon date (special handling)
    CouponDate,
    /// Settlement after maturity (invalid for most bonds)
    PostMaturity,
}

impl SettlementStatus {
    /// Determines the settlement status for a given settlement date.
    #[must_use]
    pub fn for_settlement(
        settlement: Date,
        next_coupon: Option<Date>,
        maturity: Date,
        ex_div_rules: Option<&ExDividendRules>,
    ) -> Self {
        // Check if past maturity
        if settlement > maturity {
            return Self::PostMaturity;
        }

        // Check if on maturity (final coupon)
        if settlement == maturity {
            return Self::CouponDate;
        }

        // Check ex-dividend if applicable
        if let (Some(coupon), Some(rules)) = (next_coupon, ex_div_rules) {
            if settlement == coupon {
                return Self::CouponDate;
            }

            if SettlementCalculator::is_ex_dividend(settlement, coupon, rules) {
                return Self::ExDividend;
            }
        }

        Self::Regular
    }

    /// Returns true if settlement receives the next coupon.
    #[must_use]
    pub const fn receives_coupon(self) -> bool {
        matches!(self, Self::Regular)
    }

    /// Returns true if accrued interest should be negative.
    #[must_use]
    pub const fn negative_accrued(self) -> bool {
        matches!(self, Self::ExDividend)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ExDivAccruedMethod;

    #[test]
    fn test_settlement_date_simple_t1() {
        let rules = SettlementRules::us_treasury();
        let trade = Date::from_ymd(2025, 3, 10).unwrap();

        let settlement = SettlementCalculator::settlement_date_simple(trade, &rules);

        // T+1 with business days should be approximately 1-3 calendar days later
        let days = trade.days_between(&settlement);
        assert!(days >= 1 && days <= 5);
    }

    #[test]
    fn test_settlement_date_simple_calendar() {
        let rules = SettlementRules {
            days: 2,
            use_business_days: false,
            ..Default::default()
        };
        let trade = Date::from_ymd(2025, 3, 10).unwrap();

        let settlement = SettlementCalculator::settlement_date_simple(trade, &rules);

        // T+2 calendar days
        assert_eq!(settlement, Date::from_ymd(2025, 3, 12).unwrap());
    }

    #[test]
    fn test_ex_dividend_date() {
        let rules = ExDividendRules::uk_gilt();
        let coupon = Date::from_ymd(2025, 6, 15).unwrap();

        let ex_date = SettlementCalculator::ex_dividend_date(coupon, &rules);

        // UK Gilt: 7 business days ~ 9-11 calendar days before
        let days = ex_date.days_between(&coupon);
        assert!(days >= 7 && days <= 14);
    }

    #[test]
    fn test_is_ex_dividend() {
        let rules = ExDividendRules {
            days: 7,
            day_type: DayType::CalendarDays,
            accrued_method: ExDivAccruedMethod::NegativeAccrued,
        };

        let coupon = Date::from_ymd(2025, 6, 15).unwrap();
        let ex_date = Date::from_ymd(2025, 6, 8).unwrap(); // 7 days before

        // In ex-div period
        let settlement_in = Date::from_ymd(2025, 6, 10).unwrap();
        assert!(SettlementCalculator::is_ex_dividend(
            settlement_in,
            coupon,
            &rules
        ));

        // Before ex-div period
        let settlement_before = Date::from_ymd(2025, 6, 1).unwrap();
        assert!(!SettlementCalculator::is_ex_dividend(
            settlement_before,
            coupon,
            &rules
        ));
    }

    #[test]
    fn test_settlement_status_regular() {
        let settlement = Date::from_ymd(2025, 3, 15).unwrap();
        let coupon = Date::from_ymd(2025, 6, 15).unwrap();
        let maturity = Date::from_ymd(2030, 6, 15).unwrap();

        let status = SettlementStatus::for_settlement(settlement, Some(coupon), maturity, None);

        assert_eq!(status, SettlementStatus::Regular);
        assert!(status.receives_coupon());
    }

    #[test]
    fn test_settlement_status_ex_dividend() {
        let rules = ExDividendRules {
            days: 7,
            day_type: DayType::CalendarDays,
            accrued_method: ExDivAccruedMethod::NegativeAccrued,
        };

        let settlement = Date::from_ymd(2025, 6, 10).unwrap();
        let coupon = Date::from_ymd(2025, 6, 15).unwrap();
        let maturity = Date::from_ymd(2030, 6, 15).unwrap();

        let status =
            SettlementStatus::for_settlement(settlement, Some(coupon), maturity, Some(&rules));

        assert_eq!(status, SettlementStatus::ExDividend);
        assert!(!status.receives_coupon());
        assert!(status.negative_accrued());
    }

    #[test]
    fn test_settlement_status_post_maturity() {
        let settlement = Date::from_ymd(2030, 7, 1).unwrap();
        let maturity = Date::from_ymd(2030, 6, 15).unwrap();

        let status = SettlementStatus::for_settlement(settlement, None, maturity, None);

        assert_eq!(status, SettlementStatus::PostMaturity);
    }

    #[test]
    fn test_settlement_validation() {
        let rules = SettlementRules::us_treasury();
        let trade = Date::from_ymd(2025, 3, 10).unwrap();

        // Valid: settlement after trade
        let valid = Date::from_ymd(2025, 3, 11).unwrap();
        assert!(SettlementCalculator::is_valid_settlement(
            trade, valid, &rules
        ));

        // Invalid: settlement before trade
        let invalid = Date::from_ymd(2025, 3, 9).unwrap();
        assert!(!SettlementCalculator::is_valid_settlement(
            trade, invalid, &rules
        ));
    }

    #[test]
    fn test_standard_settlement_days() {
        let us_gov = CalendarId::us_government();
        let sifma = CalendarId::sifma();
        let uk = CalendarId::uk();
        let target = CalendarId::target2();

        assert_eq!(SettlementCalculator::standard_settlement_days(&us_gov), 1);
        assert_eq!(SettlementCalculator::standard_settlement_days(&sifma), 1);
        assert_eq!(SettlementCalculator::standard_settlement_days(&uk), 1);
        assert_eq!(SettlementCalculator::standard_settlement_days(&target), 2);
    }
}
