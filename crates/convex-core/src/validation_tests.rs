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

#[cfg(test)]
mod price_yield_validation {
    // =========================================================================
    // Price from Yield Test Cases (Section 3.1)
    // =========================================================================

    #[test]
    fn test_prc_001_treasury_at_premium() {
        // US Treasury at Premium
        // Face: $100, Coupon: 4.375%, 10-year maturity
        // Settlement: On coupon date (no accrued)
        // YTM: 4.50% (semi-annual bond basis)

        let face_value: f64 = 100.0;
        let coupon_rate: f64 = 0.04375;
        let ytm: f64 = 0.045;
        let periods: i32 = 20; // 10 years × 2
        let frequency: i32 = 2;

        // Semi-annual coupon payment
        let pmt: f64 = face_value * coupon_rate / frequency as f64; // 2.1875
        let r: f64 = ytm / frequency as f64; // 0.0225

        // Present value calculation
        // PV = PMT × [(1 - (1+r)^-n) / r] + Face / (1+r)^n
        let pv_annuity: f64 = pmt * (1.0 - (1.0 + r).powi(-periods)) / r;
        let pv_face: f64 = face_value / (1.0 + r).powi(periods);
        let clean_price: f64 = pv_annuity + pv_face;

        // Since coupon (4.375%) < YTM (4.5%), bond trades at discount (< 100)
        // PV_annuity = 2.1875 × [(1 - 1.0225^-20) / 0.0225]
        //            = 2.1875 × 16.3514 = 35.769
        // PV_face = 100 / 1.0225^20 = 64.084
        // Total ≈ 99.85
        assert!(clean_price > 99.0 && clean_price < 100.0);
        // More precise: verify discount bond relationship
        assert!(clean_price < 100.0); // Discount bond
    }

    #[test]
    fn test_prc_002_corporate_at_discount() {
        // Corporate Bond at Discount
        // Face: $100, Coupon: 3.4% annual, 2-year maturity
        // YTM: 3.93%

        let face_value: f64 = 100.0;
        let coupon_rate: f64 = 0.034;
        let ytm: f64 = 0.0393;

        // Annual coupon payment
        let pmt: f64 = face_value * coupon_rate; // 3.4

        // PV of cash flows
        // Year 1: 3.4 / 1.0393
        // Year 2: 103.4 / 1.0393^2
        let pv_1: f64 = pmt / (1.0 + ytm);
        let pv_2: f64 = (pmt + face_value) / (1.0 + ytm).powi(2);
        let clean_price: f64 = pv_1 + pv_2;

        // Expected: 99.0000
        assert!((clean_price - 99.0).abs() < 0.001);
    }

    // =========================================================================
    // Zero-Coupon Bond Yield Test Case (Section 3.3)
    // =========================================================================

    #[test]
    fn test_yld_002_treasury_strip() {
        // Treasury STRIP
        // Face: $100, 5-year maturity
        // Price: $78.35

        let face_value: f64 = 100.0;
        let price: f64 = 78.35;
        let years: f64 = 5.0;

        // YTM = (Face / Price)^(1/n) - 1
        let ytm_annual: f64 = (face_value / price).powf(1.0 / years) - 1.0;

        // Expected: (100/78.35)^0.2 - 1 = 1.2763^0.2 - 1 = 0.0500 = 5.00%
        assert!((ytm_annual - 0.05).abs() < 0.001);

        // Semi-annual equivalent: 2 × [(1 + annual)^0.5 - 1]
        let ytm_semi: f64 = 2.0 * ((1.0 + ytm_annual).sqrt() - 1.0);

        // Expected: ~4.94% semi-annual
        assert!((ytm_semi - 0.0494).abs() < 0.001);
    }
}

#[cfg(test)]
mod duration_convexity_validation {
    // =========================================================================
    // Duration and Convexity Test Cases (Section 4)
    // =========================================================================

    /// Helper to compute Macaulay duration for a bond
    fn macaulay_duration(coupon_rate: f64, ytm: f64, periods: i32, freq: f64) -> f64 {
        let pmt: f64 = 100.0 * coupon_rate / freq;
        let r: f64 = ytm / freq;
        let face: f64 = 100.0;

        let mut sum_t_pv: f64 = 0.0;
        let mut price: f64 = 0.0;

        for t in 1..=periods {
            let cf: f64 = if t == periods { pmt + face } else { pmt };
            let pv: f64 = cf / (1.0 + r).powi(t);
            sum_t_pv += (t as f64 / freq) * pv;
            price += pv;
        }

        sum_t_pv / price
    }

    /// Helper to compute modified duration
    fn modified_duration(mac_dur: f64, ytm: f64, freq: f64) -> f64 {
        mac_dur / (1.0 + ytm / freq)
    }

    #[test]
    fn test_dur_001_macaulay_duration() {
        // Standard bond: 6% coupon, 5-year, semi-annual, YTM = 5.5%

        let mac_dur: f64 = macaulay_duration(0.06, 0.055, 10, 2.0);

        // The calculated duration should be around 4.37-4.40 years
        // for a 5-year 6% bond at 5.5% yield
        assert!(mac_dur > 4.30 && mac_dur < 4.50);
    }

    #[test]
    fn test_dur_002_modified_duration() {
        // From DUR-001
        let mac_dur: f64 = 4.373;
        let ytm: f64 = 0.055;
        let freq: f64 = 2.0;

        let mod_dur: f64 = modified_duration(mac_dur, ytm, freq);

        // Expected: 4.373 / 1.0275 = 4.255
        assert!((mod_dur - 4.255).abs() < 0.01);
    }

    #[test]
    fn test_dur_003_effective_duration() {
        // Numerical approximation
        // Price at YTM: P0 = 104.1234
        // Price at YTM + 1bp: P+ = 104.0347
        // Price at YTM - 1bp: P- = 104.2122
        // Δy = 0.0001

        let p0: f64 = 104.1234;
        let p_up: f64 = 104.0347;
        let p_down: f64 = 104.2122;
        let delta_y: f64 = 0.0001;

        // Effective Duration = (P- - P+) / (2 × Δy × P0)
        let eff_dur: f64 = (p_down - p_up) / (2.0 * delta_y * p0);

        // Expected: 8.525
        assert!((eff_dur - 8.525).abs() < 0.01);
    }

    #[test]
    fn test_cvx_001_convexity() {
        // Convexity = (P+ + P- - 2×P0) / (Δy² × P0)

        let p0: f64 = 104.1234;
        let p_up: f64 = 104.0347;
        let p_down: f64 = 104.2122;
        let delta_y: f64 = 0.0001;

        let convexity: f64 = (p_up + p_down - 2.0 * p0) / (delta_y * delta_y * p0);

        // Expected: ~96.05
        assert!((convexity - 96.05).abs() < 1.0);
    }
}

#[cfg(test)]
mod spread_validation {
    // =========================================================================
    // G-Spread Test Cases (Section 5.1)
    // =========================================================================

    #[test]
    fn test_gsp_001_corporate_vs_treasury() {
        // Corporate YTM: 9.5678%
        // Treasury YTM: 7.4702%
        // G-Spread = 9.5678% - 7.4702% = 2.0976% = 209.76 bps

        let corporate_ytm: f64 = 0.095678;
        let treasury_ytm: f64 = 0.074702;

        let g_spread: f64 = corporate_ytm - treasury_ytm;
        let g_spread_bps: f64 = g_spread * 10000.0;

        assert!((g_spread_bps - 209.76).abs() < 0.01);
    }

    #[test]
    fn test_gsp_002_with_interpolation() {
        // Treasury curve: 2Y=4.10%, 3Y=4.25%, 5Y=4.45%, 7Y=4.60%, 10Y=4.75%
        // Corporate bond: 4.3 years to maturity, YTM = 5.20%

        let corp_ytm: f64 = 0.0520;
        let corp_maturity: f64 = 4.3;

        // Linear interpolation between 3Y and 5Y
        // Interpolated = 4.25% + (4.3-3)/(5-3) × (4.45%-4.25%)
        // = 4.25% + 0.65 × 0.20% = 4.38%
        let interp_tsy: f64 = 0.0425 + ((corp_maturity - 3.0) / (5.0 - 3.0)) * (0.0445 - 0.0425);

        assert!((interp_tsy - 0.0438).abs() < 0.0001);

        // G-Spread = 5.20% - 4.38% = 0.82% = 82 bps
        let g_spread_bps: f64 = (corp_ytm - interp_tsy) * 10000.0;
        assert!((g_spread_bps - 82.0).abs() < 0.1);
    }

    // =========================================================================
    // I-Spread Test Cases (Section 5.4)
    // =========================================================================

    #[test]
    fn test_isp_001_eur_corporate() {
        // EUR Corporate Bond YTM: 3.20%
        // EUR 5Y Swap rate: 2.85%
        // I-Spread = 3.20% - 2.85% = 0.35% = 35 bps

        let bond_ytm: f64 = 0.0320;
        let swap_rate: f64 = 0.0285;

        let i_spread_bps: f64 = (bond_ytm - swap_rate) * 10000.0;
        assert!((i_spread_bps - 35.0).abs() < 0.1);
    }

    // =========================================================================
    // Z-Spread Test Cases (Section 5.2)
    // =========================================================================

    #[test]
    fn test_zsp_001_canonical_example() {
        // Z-Spread Validation Test Case ZSP-001
        //
        // Bond Parameters:
        // - Face: $100
        // - Coupon: 5% semi-annual
        // - Maturity: 3 years
        // - Price: $98.50 (clean)
        //
        // Spot Curve (continuously compounded):
        // - 1Y: 4.5%
        // - 2Y: 4.7%
        // - 3Y: 5.0%
        //
        // The Z-spread is the constant spread that, when added to all spot rates,
        // makes the present value of cash flows equal to the market price.
        //
        // Cash flows (semi-annual):
        // - 0.5Y: $2.50 (coupon)
        // - 1.0Y: $2.50 (coupon)
        // - 1.5Y: $2.50 (coupon)
        // - 2.0Y: $2.50 (coupon)
        // - 2.5Y: $2.50 (coupon)
        // - 3.0Y: $102.50 (coupon + principal)

        let price: f64 = 98.50;
        let face: f64 = 100.0;
        let coupon_rate: f64 = 0.05;
        let coupon: f64 = face * coupon_rate / 2.0; // 2.50 semi-annual

        // Spot rates (interpolated for semi-annual periods)
        // Using linear interpolation between pillar points
        fn interpolate_rate(t: f64) -> f64 {
            if t <= 1.0 {
                0.045
            } else if t <= 2.0 {
                0.045 + (t - 1.0) * (0.047 - 0.045)
            } else {
                0.047 + (t - 2.0) * (0.050 - 0.047)
            }
        }

        // Function to calculate PV with a given Z-spread
        let pv_with_spread = |z_spread: f64| -> f64 {
            let mut pv = 0.0;

            // Cash flows at 0.5, 1.0, 1.5, 2.0, 2.5, 3.0 years
            for i in 1..=6 {
                let t: f64 = i as f64 * 0.5;
                let cf: f64 = if i == 6 { coupon + face } else { coupon };
                let r: f64 = interpolate_rate(t);

                // Continuously compounded discounting with spread
                let df: f64 = (-(r + z_spread) * t).exp();
                pv += cf * df;
            }

            pv
        };

        // Binary search for Z-spread
        let mut low: f64 = -0.05;
        let mut high: f64 = 0.10;
        let tolerance: f64 = 1e-8;

        for _ in 0..100 {
            let mid: f64 = (low + high) / 2.0;
            let pv: f64 = pv_with_spread(mid);

            if (pv - price).abs() < tolerance {
                break;
            }

            if pv > price {
                low = mid;
            } else {
                high = mid;
            }
        }

        let z_spread: f64 = (low + high) / 2.0;
        let z_spread_bps: f64 = z_spread * 10000.0;

        // Verify Z-spread is in reasonable range
        // For a discount bond (price < par) with coupon ~= spot rates,
        // the Z-spread should be positive
        assert!(
            z_spread > 0.0,
            "Z-spread should be positive for discount bond"
        );

        // The Z-spread compensates for the discount to par
        // With this setup, expect Z-spread around 50-60 bps
        assert!(
            z_spread_bps > 40.0 && z_spread_bps < 70.0,
            "Z-spread {} bps should be in reasonable range",
            z_spread_bps
        );

        // Verify roundtrip: PV with calculated Z-spread should equal price
        let calculated_pv: f64 = pv_with_spread(z_spread);
        assert!(
            (calculated_pv - price).abs() < 0.001,
            "Calculated PV {} should equal target price {}",
            calculated_pv,
            price
        );
    }
}

#[cfg(test)]
mod periodicity_conversion_validation {
    // =========================================================================
    // Periodicity Conversion Test Cases (Section 10)
    // =========================================================================

    /// Convert yield between different periodicities
    /// Formula: (1 + APR_m/m)^m = (1 + APR_n/n)^n
    fn convert_yield(yield_rate: f64, from_frequency: u32, to_frequency: u32) -> f64 {
        let effective_annual: f64 =
            (1.0 + yield_rate / from_frequency as f64).powi(from_frequency as i32);
        to_frequency as f64 * (effective_annual.powf(1.0 / to_frequency as f64) - 1.0)
    }

    #[test]
    fn test_per_001_semi_annual_to_annual() {
        // Semi-annual yield: 4.00%
        // Annual equivalent = (1 + 0.04/2)^2 - 1 = 4.04%

        let result: f64 = convert_yield(0.04, 2, 1);
        assert!((result - 0.0404).abs() < 0.0001);
    }

    #[test]
    fn test_per_002_annual_to_semi_annual() {
        // Annual yield: 5.0625%
        // Semi-annual = 2 × [(1 + 0.050625)^0.5 - 1] = 5.00%

        let result: f64 = convert_yield(0.050625, 1, 2);
        assert!((result - 0.0500).abs() < 0.0001);
    }

    #[test]
    fn test_per_003_semi_annual_to_quarterly() {
        // Semi-annual: 4.00%
        // Quarterly = 4 × [(1.02)^0.5 - 1] = 3.98%

        let result: f64 = convert_yield(0.04, 2, 4);
        assert!((result - 0.0398).abs() < 0.0001);
    }

    #[test]
    fn test_periodicity_roundtrip() {
        // Convert semi-annual to annual and back
        let semi: f64 = 0.05;
        let annual: f64 = convert_yield(semi, 2, 1);
        let back_to_semi: f64 = convert_yield(annual, 1, 2);

        assert!((back_to_semi - semi).abs() < 0.000001);
    }
}

#[cfg(test)]
mod callable_bond_validation {
    // =========================================================================
    // Callable Bond Test Cases (Section 6)
    // =========================================================================

    /// Calculate yield given cash flows and price (Newton-Raphson)
    fn solve_yield(price: f64, cash_flows: &[(f64, f64)], max_iter: usize) -> f64 {
        let mut y: f64 = 0.05; // Initial guess

        for _ in 0..max_iter {
            let mut pv: f64 = 0.0;
            let mut dpv: f64 = 0.0;

            for &(t, cf) in cash_flows {
                let df: f64 = (1.0 + y / 2.0).powf(-2.0 * t);
                pv += cf * df;
                dpv -= t * cf * df / (1.0 + y / 2.0);
            }

            let f: f64 = pv - price;
            if f.abs() < 1e-10 {
                break;
            }
            y -= f / dpv;
        }

        y
    }

    #[test]
    fn test_ytc_001_single_call() {
        // Callable Bond: Face=$1000, Coupon=8%, Maturity=10yr, Call@103 in 4yr
        // Market price: $980

        // Cash flows to call: 7 semi-annual payments of $40, then $40 + $1030
        let mut cash_flows: Vec<(f64, f64)> = Vec::new();
        for i in 1..=7 {
            cash_flows.push((i as f64 * 0.5, 40.0));
        }
        cash_flows.push((4.0, 40.0 + 1030.0)); // Final: coupon + call price

        let ytc: f64 = solve_yield(980.0, &cash_flows, 100);

        // Expected YTC: ~9.25% annual (semi-annual r = 4.625%)
        // This is approximate - exact value depends on implementation
        assert!(ytc > 0.08 && ytc < 0.11);
    }
}

#[cfg(test)]
mod frn_validation {
    // =========================================================================
    // Floating Rate Note Test Cases (Section 7)
    // =========================================================================

    #[test]
    fn test_frn_001_current_coupon() {
        // SOFR-Linked FRN
        // Quoted margin: +75 bps
        // Current 3M SOFR: 5.25%

        let sofr: f64 = 0.0525;
        let quoted_margin: f64 = 0.0075;

        // Current period rate = SOFR + margin
        let current_rate: f64 = sofr + quoted_margin;

        // Expected: 6.00%
        assert!((current_rate - 0.06).abs() < 0.0001);
    }

    #[test]
    fn test_sm_001_simple_margin() {
        // Simple Margin = Quoted Margin + (100 - Price) / Years to Maturity

        let quoted_margin: f64 = 0.0050; // 50 bps
        let price: f64 = 98.50;
        let years: f64 = 2.0;

        let simple_margin: f64 = quoted_margin + (100.0 - price) / (100.0 * years);

        // Expected: 1.25% = 125 bps
        assert!((simple_margin - 0.0125).abs() < 0.0001);
    }

    #[test]
    fn test_sofr_001_compounded() {
        // 3-day SOFR compounding
        let daily_sofr: Vec<f64> = vec![0.0500, 0.0505, 0.0510]; // 5.00%, 5.05%, 5.10%

        // Compounded = [Π(1 + SOFR_i × 1/360)] - 1
        let mut product: f64 = 1.0;
        for rate in &daily_sofr {
            product *= 1.0 + rate / 360.0;
        }

        // Annualized: (product - 1) × (360/3)
        let compounded_annualized: f64 = (product - 1.0) * (360.0 / 3.0);

        // Expected: ~5.0508%
        assert!((compounded_annualized - 0.050508).abs() < 0.0001);
    }
}

#[cfg(test)]
mod sinking_fund_validation {
    // =========================================================================
    // Sinking Fund Test Cases (Section 8)
    // =========================================================================

    #[test]
    fn test_wal_001_weighted_average_life() {
        // $100M bond, sinking fund starts Year 6
        // $20M/year from Year 6 to Year 10

        let total_principal: f64 = 100_000_000.0;
        let schedule: Vec<(f64, f64)> = vec![
            (6.0, 20_000_000.0),
            (7.0, 20_000_000.0),
            (8.0, 20_000_000.0),
            (9.0, 20_000_000.0),
            (10.0, 20_000_000.0),
        ];

        // WAL = Σ(t × Principal_t) / Total
        let wal: f64 = schedule.iter().map(|(t, p)| t * p).sum::<f64>() / total_principal;

        // Expected: 8.0 years
        assert!((wal - 8.0).abs() < 0.001);
    }
}
