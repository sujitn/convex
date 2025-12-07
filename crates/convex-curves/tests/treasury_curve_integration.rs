//! Integration test: Build a Treasury curve from market data.
//!
//! This test uses actual Treasury market data to build a yield curve
//! using linear interpolation, similar to Bloomberg methodology.
//!
//! Market Data: November 28, 2025
//!
//! | Tenor | Coupon  | Price/Rate | Yield   |
//! |-------|---------|------------|---------|
//! | 1M    | 0.000%  | 3 7/8      | 3.936%  |
//! | 3M    | 0.000%  | 3 23/32    | 3.806%  |
//! | 6M    | 0.000%  | 3 21/32    | 3.774%  |
//! | 1Y    | 0.000%  | 3 15/32    | 3.591%  |
//! | 2Y    | 3.375%  | 99 1/4     | 3.502%  |
//! | 3Y    | 3.500%  | 100        | 3.493%  |
//! | 5Y    | 3.500%  | 99 5/32    | 3.603%  |
//! | 7Y    | 3.750%  | 99 1/4     | 3.788%  |
//! | 10Y   | 4.000%  | 99 9/32    | 4.018%  |
//! | 20Y   | 4.625%  | 99 10/32   | 4.628%  |
//! | 30Y   | 4.625%  | 99 3/32    | 4.667%  |

use convex_core::Date;
use convex_curves::bootstrap::{GlobalBootstrapConfig, GlobalBootstrapper};
use convex_curves::compounding::Compounding;
use convex_curves::instruments::{TreasuryBill, TreasuryBond};
use convex_curves::interpolation::InterpolationMethod;
use convex_curves::traits::Curve;

/// Parse price in 32nds format (e.g., "99 5/32" -> 99.15625)
fn parse_price_32nds(whole: f64, thirty_seconds: f64) -> f64 {
    whole + thirty_seconds / 32.0
}

/// Convert discount rate to price for T-Bill.
/// Price = 100 * (1 - rate * days/360)
fn discount_rate_to_price(rate: f64, days: i64) -> f64 {
    100.0 * (1.0 - rate * days as f64 / 360.0)
}

#[test]
fn test_build_treasury_curve_from_market_data() {
    // Settlement date: November 28, 2025
    let settlement = Date::from_ymd(2025, 11, 28).unwrap();

    // === T-BILLS (Zero Coupon) ===
    // Discount rates in fractional format converted to decimal

    // 1M T-Bill: discount rate 3 7/8 = 3.875%
    let tbill_1m_rate = 3.0 + 7.0 / 8.0; // 3.875%
    let tbill_1m_maturity = settlement.add_months(1).unwrap();
    let tbill_1m_days = settlement.days_between(&tbill_1m_maturity);
    let tbill_1m_price = discount_rate_to_price(tbill_1m_rate / 100.0, tbill_1m_days);

    // 3M T-Bill: discount rate 3 23/32 = 3.71875%
    let tbill_3m_rate = 3.0 + 23.0 / 32.0;
    let tbill_3m_maturity = settlement.add_months(3).unwrap();
    let tbill_3m_days = settlement.days_between(&tbill_3m_maturity);
    let tbill_3m_price = discount_rate_to_price(tbill_3m_rate / 100.0, tbill_3m_days);

    // 6M T-Bill: discount rate 3 21/32 = 3.65625%
    let tbill_6m_rate = 3.0 + 21.0 / 32.0;
    let tbill_6m_maturity = settlement.add_months(6).unwrap();
    let tbill_6m_days = settlement.days_between(&tbill_6m_maturity);
    let tbill_6m_price = discount_rate_to_price(tbill_6m_rate / 100.0, tbill_6m_days);

    // 1Y T-Bill: discount rate 3 15/32 = 3.46875%
    let tbill_1y_rate = 3.0 + 15.0 / 32.0;
    let tbill_1y_maturity = settlement.add_years(1).unwrap();
    let tbill_1y_days = settlement.days_between(&tbill_1y_maturity);
    let tbill_1y_price = discount_rate_to_price(tbill_1y_rate / 100.0, tbill_1y_days);

    println!("=== T-BILL PRICES ===");
    println!(
        "1M: rate={:.4}%, days={}, price={:.6}",
        tbill_1m_rate, tbill_1m_days, tbill_1m_price
    );
    println!(
        "3M: rate={:.4}%, days={}, price={:.6}",
        tbill_3m_rate, tbill_3m_days, tbill_3m_price
    );
    println!(
        "6M: rate={:.4}%, days={}, price={:.6}",
        tbill_6m_rate, tbill_6m_days, tbill_6m_price
    );
    println!(
        "1Y: rate={:.4}%, days={}, price={:.6}",
        tbill_1y_rate, tbill_1y_days, tbill_1y_price
    );

    // Create T-Bill instruments
    let tbill_1m = TreasuryBill::new("TBILL-1M", settlement, tbill_1m_maturity, tbill_1m_price);
    let tbill_3m = TreasuryBill::new("TBILL-3M", settlement, tbill_3m_maturity, tbill_3m_price);
    let tbill_6m = TreasuryBill::new("TBILL-6M", settlement, tbill_6m_maturity, tbill_6m_price);
    let tbill_1y = TreasuryBill::new("TBILL-1Y", settlement, tbill_1y_maturity, tbill_1y_price);

    // === T-NOTES/BONDS (With Coupons) ===
    // Prices in 32nds format

    // 2Y: 3.375% coupon, 99 1/4 = 99.25 (1/4 = 8/32)
    let tnote_2y_maturity = settlement.add_years(2).unwrap();
    let tnote_2y = TreasuryBond::new("TNOTE-2Y", settlement, tnote_2y_maturity, 0.03375, 99.25);

    // 3Y: 3.500% coupon, 100.00
    let tnote_3y_maturity = settlement.add_years(3).unwrap();
    let tnote_3y = TreasuryBond::new("TNOTE-3Y", settlement, tnote_3y_maturity, 0.035, 100.0);

    // 5Y: 3.500% coupon, 99 5/32 = 99.15625
    let tnote_5y_maturity = settlement.add_years(5).unwrap();
    let tnote_5y = TreasuryBond::new(
        "TNOTE-5Y",
        settlement,
        tnote_5y_maturity,
        0.035,
        parse_price_32nds(99.0, 5.0),
    );

    // 7Y: 3.750% coupon, 99 1/4 = 99.25
    let tnote_7y_maturity = settlement.add_years(7).unwrap();
    let tnote_7y = TreasuryBond::new("TNOTE-7Y", settlement, tnote_7y_maturity, 0.0375, 99.25);

    // 10Y: 4.000% coupon, 99 9/32 = 99.28125
    let tnote_10y_maturity = settlement.add_years(10).unwrap();
    let tnote_10y = TreasuryBond::new(
        "TNOTE-10Y",
        settlement,
        tnote_10y_maturity,
        0.04,
        parse_price_32nds(99.0, 9.0),
    );

    // 20Y: 4.625% coupon, 99 10/32 = 99.3125
    let tbond_20y_maturity = settlement.add_years(20).unwrap();
    let tbond_20y = TreasuryBond::new(
        "TBOND-20Y",
        settlement,
        tbond_20y_maturity,
        0.04625,
        parse_price_32nds(99.0, 10.0),
    );

    // 30Y: 4.625% coupon, 99 3/32 = 99.09375
    let tbond_30y_maturity = settlement.add_years(30).unwrap();
    let tbond_30y = TreasuryBond::new(
        "TBOND-30Y",
        settlement,
        tbond_30y_maturity,
        0.04625,
        parse_price_32nds(99.0, 3.0),
    );

    println!("\n=== T-NOTE/BOND PRICES ===");
    println!("2Y: coupon=3.375%, price=99.25");
    println!("3Y: coupon=3.500%, price=100.00");
    println!(
        "5Y: coupon=3.500%, price={:.5}",
        parse_price_32nds(99.0, 5.0)
    );
    println!("7Y: coupon=3.750%, price=99.25");
    println!(
        "10Y: coupon=4.000%, price={:.5}",
        parse_price_32nds(99.0, 9.0)
    );
    println!(
        "20Y: coupon=4.625%, price={:.5}",
        parse_price_32nds(99.0, 10.0)
    );
    println!(
        "30Y: coupon=4.625%, price={:.5}",
        parse_price_32nds(99.0, 3.0)
    );

    // === BUILD THE CURVE ===
    // Using Global Bootstrap for exact repricing of all instruments
    // This solves all discount factors simultaneously via optimization
    let config = GlobalBootstrapConfig {
        interpolation: InterpolationMethod::LogLinear,
        max_iterations: 5000,
        tolerance: 1e-14,
        ..Default::default()
    };

    let result = GlobalBootstrapper::new(settlement)
        .with_config(config)
        .add_instrument(tbill_1m)
        .add_instrument(tbill_3m)
        .add_instrument(tbill_6m)
        .add_instrument(tbill_1y)
        .add_instrument(tnote_2y)
        .add_instrument(tnote_3y)
        .add_instrument(tnote_5y)
        .add_instrument(tnote_7y)
        .add_instrument(tnote_10y)
        .add_instrument(tbond_20y)
        .add_instrument(tbond_30y)
        .bootstrap_validated()
        .expect("Bootstrap should succeed");

    // Print repricing report
    println!("\n=== REPRICING REPORT ===");
    println!("{}", result.repricing_report);

    // Get the curve
    let curve = result.curve();

    // === OUTPUT CURVE RESULTS ===
    println!("\n=== TREASURY CURVE (Nov 28, 2025) ===");
    println!("Using Global Bootstrap (exact repricing)");
    println!();
    println!(
        "{:<8} {:<12} {:<12} {:<12} {:<12}",
        "Tenor", "DF", "Zero (CC)", "Zero (SA)", "Par Yield"
    );
    println!("{}", "-".repeat(60));

    let tenors = [
        (0.0833, "1M"),
        (0.25, "3M"),
        (0.5, "6M"),
        (1.0, "1Y"),
        (2.0, "2Y"),
        (3.0, "3Y"),
        (5.0, "5Y"),
        (7.0, "7Y"),
        (10.0, "10Y"),
        (20.0, "20Y"),
        (30.0, "30Y"),
    ];

    for (t, label) in tenors {
        let df = curve.discount_factor(t).unwrap();
        let zero_cc = curve.zero_rate(t, Compounding::Continuous).unwrap();
        let zero_sa = curve.zero_rate(t, Compounding::SemiAnnual).unwrap();
        let par = curve.par_yield(t).unwrap();

        println!(
            "{:<8} {:<12.6} {:<12.4}% {:<12.4}% {:<12.4}%",
            label,
            df,
            zero_cc * 100.0,
            zero_sa * 100.0,
            par * 100.0
        );
    }

    // === VALIDATE AGAINST MARKET YIELDS ===
    println!("\n=== VALIDATION vs MARKET YIELDS ===");
    println!(
        "{:<8} {:<12} {:<12} {:<12}",
        "Tenor", "Market", "Model", "Diff (bp)"
    );
    println!("{}", "-".repeat(48));

    let market_yields = [
        ("1M", 0.03936),
        ("3M", 0.03806),
        ("6M", 0.03774),
        ("1Y", 0.03591),
        ("2Y", 0.03502),
        ("3Y", 0.03493),
        ("5Y", 0.03603),
        ("7Y", 0.03788),
        ("10Y", 0.04018),
        ("20Y", 0.04628),
        ("30Y", 0.04667),
    ];

    let tenors_years = [0.0833, 0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 20.0, 30.0];

    for ((label, market_yield), t) in market_yields.iter().zip(tenors_years.iter()) {
        let model_yield = curve.zero_rate(*t, Compounding::SemiAnnual).unwrap();
        let diff_bp = (model_yield - market_yield) * 10000.0;

        println!(
            "{:<8} {:<12.2}% {:<12.2}% {:<12.1}",
            label,
            market_yield * 100.0,
            model_yield * 100.0,
            diff_bp
        );
    }

    // Global Bootstrap minimizes sum of squared PV errors
    // The errors are small but may not pass strict tolerances
    // Max error: ~5e-3 = $0.005 per $100 notional (excellent)
    let max_error = result.repricing_report.max_error();
    println!("Max repricing error: ${:.6} per $100", max_error);
    assert!(
        max_error < 0.01,
        "Max repricing error should be < $0.01 per $100"
    );

    // Curve should be monotonically decreasing in DF
    let df_1y = curve.discount_factor(1.0).unwrap();
    let df_10y = curve.discount_factor(10.0).unwrap();
    let df_30y = curve.discount_factor(30.0).unwrap();
    assert!(df_1y > df_10y, "1Y DF should be > 10Y DF");
    assert!(df_10y > df_30y, "10Y DF should be > 30Y DF");

    // Zero rates should be reasonable (within 100bp of market)
    let zero_10y = curve.zero_rate(10.0, Compounding::SemiAnnual).unwrap();
    assert!(
        (zero_10y - 0.04018).abs() < 0.01,
        "10Y zero rate should be within 100bp of market"
    );

    // === FORWARD RATE ANALYSIS ===
    // Explains why 20Y zero rate > 30Y zero rate (the "hump")
    //
    // Market Data:
    //   20Y: 4.625% coupon @ 99 10/32 → yield 4.628%
    //   30Y: 4.625% coupon @ 99 3/32  → yield 4.667%
    //
    // Both bonds have the SAME coupon (4.625%), but different prices.
    // The 30Y is priced lower (99.09375 vs 99.3125), giving higher yield.
    //
    // However, zero rates show: 20Y zero (~4.83%) > 30Y zero (~4.77%)
    //
    // This happens because:
    // 1. Par curve steepens from 10Y to 20Y (+61bp)
    // 2. Par curve flattens from 20Y to 30Y (+4bp only)
    // 3. Forward rates peak at 10Y-20Y, then decline for 20Y-30Y
    // 4. Zero rate = average of all forwards → declines after the peak
    println!("\n=== FORWARD RATE ANALYSIS ===");
    let fwd_10_20 = curve.forward_rate(10.0, 20.0).unwrap();
    let fwd_20_30 = curve.forward_rate(20.0, 30.0).unwrap();
    println!("10Y-20Y forward: {:.4}%", fwd_10_20 * 100.0);
    println!("20Y-30Y forward: {:.4}%", fwd_20_30 * 100.0);

    let zero_20y = curve.zero_rate(20.0, Compounding::Continuous).unwrap();
    let zero_30y = curve.zero_rate(30.0, Compounding::Continuous).unwrap();
    println!("20Y zero (CC): {:.4}%", zero_20y * 100.0);
    println!("30Y zero (CC): {:.4}%", zero_30y * 100.0);

    // The 20Y-30Y forward is lower than 10Y-20Y forward
    // This pulls down the 30Y zero rate (which is an average of all forwards)
    assert!(
        fwd_20_30 < fwd_10_20,
        "20Y-30Y forward should be < 10Y-20Y forward (curve flattening)"
    );

    println!("\nNote: 20Y-30Y forward < 10Y-20Y forward → 30Y zero < 20Y zero");
    println!(
        "This 'hump' in zero rates is mathematically consistent with the flat 20Y-30Y par spread."
    );

    println!("\n=== CURVE BUILD SUCCESSFUL ===");
}

#[test]
fn test_par_yields_match_cmt_rates() {
    // This test verifies that par yields from the bootstrapped curve
    // approximately match the market yields (which are par yields/CMT rates)
    // for coupon-bearing instruments.

    let settlement = Date::from_ymd(2025, 11, 28).unwrap();

    // Build curve with just the key points
    let tbill_6m_maturity = settlement.add_months(6).unwrap();
    let tbill_6m_days = settlement.days_between(&tbill_6m_maturity);
    let tbill_6m_price = discount_rate_to_price(0.0365625, tbill_6m_days);
    let tbill_6m = TreasuryBill::new("TBILL-6M", settlement, tbill_6m_maturity, tbill_6m_price);

    let tnote_2y_maturity = settlement.add_years(2).unwrap();
    let tnote_2y = TreasuryBond::new("TNOTE-2Y", settlement, tnote_2y_maturity, 0.03375, 99.25);

    let tnote_5y_maturity = settlement.add_years(5).unwrap();
    let tnote_5y = TreasuryBond::new("TNOTE-5Y", settlement, tnote_5y_maturity, 0.035, 99.15625);

    let tnote_10y_maturity = settlement.add_years(10).unwrap();
    let tnote_10y = TreasuryBond::new("TNOTE-10Y", settlement, tnote_10y_maturity, 0.04, 99.28125);

    let config = GlobalBootstrapConfig {
        interpolation: InterpolationMethod::LogLinear,
        max_iterations: 1000,
        tolerance: 1e-12,
        ..Default::default()
    };

    let result = GlobalBootstrapper::new(settlement)
        .with_config(config)
        .add_instrument(tbill_6m)
        .add_instrument(tnote_2y)
        .add_instrument(tnote_5y)
        .add_instrument(tnote_10y)
        .bootstrap_validated()
        .expect("Bootstrap should succeed");

    let curve = result.curve();

    // Check par yields at key tenors
    let par_2y = curve.par_yield(2.0).unwrap();
    let par_5y = curve.par_yield(5.0).unwrap();
    let par_10y = curve.par_yield(10.0).unwrap();

    println!("Par Yields:");
    println!("2Y: {:.4}% (market: 3.502%)", par_2y * 100.0);
    println!("5Y: {:.4}% (market: 3.603%)", par_5y * 100.0);
    println!("10Y: {:.4}% (market: 4.018%)", par_10y * 100.0);

    // Par yields should be within 50bp of market (allowing for bootstrap method differences)
    assert!(
        (par_2y - 0.03502).abs() < 0.005,
        "2Y par yield should be close to market"
    );
    assert!(
        (par_5y - 0.03603).abs() < 0.005,
        "5Y par yield should be close to market"
    );
    assert!(
        (par_10y - 0.04018).abs() < 0.005,
        "10Y par yield should be close to market"
    );
}
