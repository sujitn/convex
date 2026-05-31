//! Validation Test Suite
//!
//! This module contains comprehensive tests based on the validation specification
//! with exact numerical test cases derived from Bloomberg methodologies.

#[cfg(test)]
mod day_count_validation {
    use crate::daycounts::{ActActIcma, ActActIsda, DayCount, Thirty360E, Thirty360US};
    use crate::types::Date;
    use rust_decimal_macros::dec;

    // =========================================================================
    // 30/360 US Test Cases (Section 1.2)
    // =========================================================================

    #[test]
    fn test_dc_001_jan31_to_feb28() {
        let dc = Thirty360US;
        let start = Date::from_ymd(2024, 1, 31).unwrap();
        let end = Date::from_ymd(2024, 2, 28).unwrap();

        // D1=31 → D1=30
        // D2=28 (Feb 28 in 2024 is NOT last day since Feb has 29 days)
        // Days = 30*(2-1) + (28-30) = 30 - 2 = 28
        assert_eq!(dc.day_count(start, end), 28);

        // Expected DCF = 28/360 = 0.077778
        let dcf = dc.year_fraction(start, end);
        let expected = dec!(28) / dec!(360);
        assert_eq!(dcf, expected);
        assert!((dcf - dec!(0.077778)).abs() < dec!(0.000001));
    }

    #[test]
    fn test_dc_002_feb28_to_mar31() {
        let dc = Thirty360US;
        let start = Date::from_ymd(2024, 2, 28).unwrap();
        let end = Date::from_ymd(2024, 3, 31).unwrap();

        // 2024 is leap year - Feb 28 is NOT the last day of Feb
        // D1=28 (no change), D2=31 (D1 < 30, so stays 31)
        // Days = 30*(3-2) + (31-28) = 30 + 3 = 33
        assert_eq!(dc.day_count(start, end), 33);

        let dcf = dc.year_fraction(start, end);
        assert!((dcf - dec!(0.091667)).abs() < dec!(0.000001));
    }

    #[test]
    fn test_dc_003_jan31_to_mar31() {
        let dc = Thirty360US;
        let start = Date::from_ymd(2024, 1, 31).unwrap();
        let end = Date::from_ymd(2024, 3, 31).unwrap();

        // D1=31 → D1=30, D2=31 → D2=30 (because D1>=30)
        // Days = 30*(3-1) + (30-30) = 60
        assert_eq!(dc.day_count(start, end), 60);

        let dcf = dc.year_fraction(start, end);
        assert!((dcf - dec!(0.166667)).abs() < dec!(0.000001));
    }

    #[test]
    fn test_dc_004_may31_to_aug31() {
        let dc = Thirty360US;
        let start = Date::from_ymd(2024, 5, 31).unwrap();
        let end = Date::from_ymd(2024, 8, 31).unwrap();

        // D1=31 → D1=30, D2=31 → D2=30 (because D1>=30)
        // Days = 30*(8-5) + (30-30) = 90
        assert_eq!(dc.day_count(start, end), 90);

        let dcf = dc.year_fraction(start, end);
        assert_eq!(dcf, dec!(0.25));
    }

    #[test]
    fn test_dc_005_jan15_to_jul15() {
        let dc = Thirty360US;
        let start = Date::from_ymd(2024, 1, 15).unwrap();
        let end = Date::from_ymd(2024, 7, 15).unwrap();

        // No adjustments needed (D1=15, D2=15)
        // Days = 30*(7-1) + (15-15) = 180
        assert_eq!(dc.day_count(start, end), 180);

        let dcf = dc.year_fraction(start, end);
        assert_eq!(dcf, dec!(0.5));
    }

    #[test]
    fn test_dc_006_aug01_to_jan13() {
        let dc = Thirty360US;
        let start = Date::from_ymd(2023, 8, 1).unwrap();
        let end = Date::from_ymd(2024, 1, 13).unwrap();

        // No adjustments needed (D1=1, D2=13)
        // Days = 360*(2024-2023) + 30*(1-8) + (13-1) = 360 - 210 + 12 = 162
        assert_eq!(dc.day_count(start, end), 162);

        let dcf = dc.year_fraction(start, end);
        assert!((dcf - dec!(0.45)).abs() < dec!(0.000001));
    }

    // =========================================================================
    // 30E/360 Test Cases (Section 1.3)
    // =========================================================================

    #[test]
    fn test_dc_007_thirty360e_vs_us_feb28_mar31() {
        let us = Thirty360US;
        let eu = Thirty360E;

        let start = Date::from_ymd(2024, 2, 28).unwrap();
        let end = Date::from_ymd(2024, 3, 31).unwrap();

        // US: D1=28, D2=31 (stays because D1 < 30)
        // Days = 30 + 3 = 33
        assert_eq!(us.day_count(start, end), 33);

        // 30E/360: D1=28, D2=31 → D2=30 (ALWAYS)
        // Days = 30 + 2 = 32
        assert_eq!(eu.day_count(start, end), 32);
    }

    #[test]
    fn test_dc_008_thirty360e_vs_us_jan30_mar31() {
        let us = Thirty360US;
        let eu = Thirty360E;

        let start = Date::from_ymd(2024, 1, 30).unwrap();
        let end = Date::from_ymd(2024, 3, 31).unwrap();

        // US: D1=30 (stays 30), D2=31 → D2=30 (because D1>=30)
        // Days = 60 + 0 = 60
        assert_eq!(us.day_count(start, end), 60);

        // 30E/360: D1=30, D2=31 → D2=30
        // Days = 60 + 0 = 60
        assert_eq!(eu.day_count(start, end), 60);
    }

    #[test]
    fn test_dc_009_thirty360e_vs_us_jan15_mar31() {
        let us = Thirty360US;
        let eu = Thirty360E;

        let start = Date::from_ymd(2024, 1, 15).unwrap();
        let end = Date::from_ymd(2024, 3, 31).unwrap();

        // US: D1=15, D2=31 (stays because D1 < 30)
        // Days = 60 + 16 = 76
        assert_eq!(us.day_count(start, end), 76);

        // 30E/360: D1=15, D2=31 → D2=30
        // Days = 60 + 15 = 75
        assert_eq!(eu.day_count(start, end), 75);
    }

    // =========================================================================
    // ACT/ACT ISDA Test Cases (Section 1.4)
    // =========================================================================

    #[test]
    fn test_dc_010_actact_isda_spanning_years() {
        let dc = ActActIsda;
        let start = Date::from_ymd(2023, 12, 15).unwrap();
        let end = Date::from_ymd(2024, 3, 15).unwrap();

        // Days in 2023 (non-leap): Dec 15 to Dec 31 = 17 days (incl Dec 31) / 365
        // Days in 2024 (leap): Jan 1 to Mar 15 = 74 days / 366
        // DCF = 17/365 + 74/366

        let dcf = dc.year_fraction(start, end);

        // The actual implementation uses:
        // - days_in_start_year: Dec 15 to Dec 31 + 1 = 17 days
        // - days_in_end_year: Jan 1 to Mar 15 = 74 days
        let expected = dec!(17) / dec!(365) + dec!(74) / dec!(366);

        assert!((dcf - expected).abs() < dec!(0.000001));
        // Total ~0.248877
        assert!((dcf - dec!(0.2488)).abs() < dec!(0.001));
    }

    #[test]
    fn test_dc_011_actact_isda_full_year_spanning() {
        let dc = ActActIsda;
        let start = Date::from_ymd(2023, 6, 1).unwrap();
        let end = Date::from_ymd(2024, 6, 1).unwrap();

        // 2023 (non-leap): Jun 1 to Dec 31 = 214 days / 365
        // 2024 (leap): Jan 1 to Jun 1 = 152 days / 366
        // DCF = 214/365 + 152/366 = 0.586301 + 0.415301 = 1.001602

        let dcf = dc.year_fraction(start, end);
        let expected = dec!(214) / dec!(365) + dec!(152) / dec!(366);

        assert!((dcf - expected).abs() < dec!(0.000001));
    }

    // =========================================================================
    // ACT/ACT ICMA Test Cases (Section 1.5)
    // =========================================================================

    #[test]
    fn test_dc_013_actact_icma_treasury() {
        let dc = ActActIcma::semi_annual();

        // Settlement: Aug 20, 2024
        // Last coupon: May 15, 2024
        // Next coupon: Nov 15, 2024
        let period_start = Date::from_ymd(2024, 5, 15).unwrap();
        let settlement = Date::from_ymd(2024, 8, 20).unwrap();
        let period_end = Date::from_ymd(2024, 11, 15).unwrap();

        let actual_days = period_start.days_between(&settlement);
        let period_days = period_start.days_between(&period_end);

        assert_eq!(actual_days, 97);
        assert_eq!(period_days, 184);

        // DCF = Actual Days / (Freq × Period Days)
        // DCF = 97 / (2 × 184) = 97/368 = 0.263587
        let dcf = dc.year_fraction_with_period(period_start, settlement, period_start, period_end);
        let expected = dec!(97) / dec!(368);

        assert!((dcf - expected).abs() < dec!(0.000001));
        assert!((dcf - dec!(0.263587)).abs() < dec!(0.000001));
    }

    #[test]
    fn test_dc_014_actact_icma_november_coupon() {
        let dc = ActActIcma::semi_annual();

        // Settlement: Dec 10, 2024
        // Last coupon: Nov 15, 2024
        // Next coupon: May 15, 2025
        let period_start = Date::from_ymd(2024, 11, 15).unwrap();
        let settlement = Date::from_ymd(2024, 12, 10).unwrap();
        let period_end = Date::from_ymd(2025, 5, 15).unwrap();

        let actual_days = period_start.days_between(&settlement);
        let period_days = period_start.days_between(&period_end);

        assert_eq!(actual_days, 25);
        assert_eq!(period_days, 181);

        // DCF = 25 / (2 × 181) = 25/362 = 0.069061
        let dcf = dc.year_fraction_with_period(period_start, settlement, period_start, period_end);
        let expected = dec!(25) / dec!(362);

        assert!((dcf - expected).abs() < dec!(0.000001));
        assert!((dcf - dec!(0.069061)).abs() < dec!(0.000001));
    }

    #[test]
    fn test_dc_015_actact_icma_march_coupon() {
        let dc = ActActIcma::semi_annual();

        // Settlement: Jun 15, 2024
        // Period: Mar 1 to Sep 1, 2024
        let period_start = Date::from_ymd(2024, 3, 1).unwrap();
        let settlement = Date::from_ymd(2024, 6, 15).unwrap();
        let period_end = Date::from_ymd(2024, 9, 1).unwrap();

        let actual_days = period_start.days_between(&settlement);
        let period_days = period_start.days_between(&period_end);

        assert_eq!(actual_days, 106);
        assert_eq!(period_days, 184);

        // DCF = 106 / (2 × 184) = 106/368 = 0.288043
        let dcf = dc.year_fraction_with_period(period_start, settlement, period_start, period_end);
        let expected = dec!(106) / dec!(368);

        assert!((dcf - expected).abs() < dec!(0.000001));
        assert!((dcf - dec!(0.288043)).abs() < dec!(0.000001));
    }
}

#[cfg(test)]
mod accrued_interest_validation {
    use crate::daycounts::{ActActIcma, DayCount, Thirty360US};
    use crate::types::Date;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    // =========================================================================
    // Accrued Interest Test Cases (Section 2)
    // =========================================================================

    #[test]
    fn test_accr_001_treasury_accrued() {
        // 10-Year Treasury Note
        // Face: $100, Coupon: 4.375%, Semi-annual, ACT/ACT ICMA
        // Settlement: Dec 10, 2024
        // Last coupon: Nov 15, 2024
        // Next coupon: May 15, 2025

        let _dc = ActActIcma::semi_annual();
        let face_value = dec!(100);
        let coupon_rate = dec!(0.04375);
        let frequency = dec!(2);

        let period_start = Date::from_ymd(2024, 11, 15).unwrap();
        let settlement = Date::from_ymd(2024, 12, 10).unwrap();
        let period_end = Date::from_ymd(2025, 5, 15).unwrap();

        // Days accrued: Nov 15 to Dec 10 = 25 days
        // Days in period: Nov 15 to May 15 = 181 days
        let days_accrued = period_start.days_between(&settlement);
        let days_in_period = period_start.days_between(&period_end);

        assert_eq!(days_accrued, 25);
        assert_eq!(days_in_period, 181);

        // Semi-annual coupon: 4.375% / 2 = 2.1875%
        // Accrued = Face × Coupon/Freq × (Days/Period_Days)
        // Accrued = 100 × (0.04375/2) × (25/181) = 100 × 0.021875 × 0.138122
        // Accrued = 0.302144

        let accrued = face_value
            * (coupon_rate / frequency)
            * (Decimal::from(days_accrued) / Decimal::from(days_in_period));

        // Exact: 100 × 0.021875 × (25/181) = 2.1875 × 0.138121... = 0.302145...
        assert!((accrued - dec!(0.302145)).abs() < dec!(0.00001));
    }

    #[test]
    fn test_accr_002_corporate_accrued() {
        // US Corporate Bond
        // Face: $1000, Coupon: 5.625%, Semi-annual, 30/360
        // Settlement: Jan 13, 2021
        // Last coupon: Aug 1, 2020
        // Next coupon: Feb 1, 2021

        let dc = Thirty360US;
        let face_value = dec!(1000);
        let coupon_rate = dec!(0.05625);
        let _frequency = dec!(2);

        let last_coupon = Date::from_ymd(2020, 8, 1).unwrap();
        let settlement = Date::from_ymd(2021, 1, 13).unwrap();

        // 30/360 calculation:
        // Aug: 29 days (30-1), Sep: 30, Oct: 30, Nov: 30, Dec: 30, Jan: 13
        // Days = 360*(2021-2020) + 30*(1-8) + (13-1) = 360 - 210 + 12 = 162
        let days = dc.day_count(last_coupon, settlement);
        assert_eq!(days, 162);

        // Accrued = Face × (Coupon/360) × Days
        // Accrued = 1000 × (0.05625/360) × 162 = 25.3125
        let accrued = face_value * (coupon_rate / dec!(360)) * Decimal::from(days);

        assert!((accrued - dec!(25.3125)).abs() < dec!(0.0001));
    }
}
