//! Treasury Curve Construction Example
//!
//! This example demonstrates two approaches to building a Treasury yield curve:
//!
//! 1. **Simple Interpolated Curve**: Direct linear interpolation on market yields
//!    - Fast and simple, no bootstrapping required
//!    - Good for quick analysis and yield lookups
//!    - Does NOT exactly reprice the input instruments
//!
//! 2. **Bootstrapped Curve**: Global optimization to fit discount factors
//!    - Exactly reprices all input instruments
//!    - Required for accurate bond pricing and risk management
//!    - More computationally intensive
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
//!
//! Run with: cargo run --example treasury_curve

use convex_core::Date;
use convex_curves::bootstrap::{GlobalBootstrapConfig, GlobalBootstrapper};
use convex_curves::compounding::Compounding;
use convex_curves::curves::DiscountCurveBuilder;
use convex_curves::instruments::{TreasuryBill, TreasuryBond};
use convex_curves::interpolation::InterpolationMethod;
use convex_curves::traits::Curve;

/// Parse price in 32nds format (e.g., "99 5/32" -> 99.15625)
fn parse_price_32nds(whole: f64, thirty_seconds: f64) -> f64 {
    whole + thirty_seconds / 32.0
}

/// Convert T-Bill discount rate to price.
/// Price = 100 * (1 - rate * days/360)
fn discount_rate_to_price(rate: f64, days: i64) -> f64 {
    100.0 * (1.0 - rate * days as f64 / 360.0)
}

fn main() {
    println!("===========================================");
    println!("  Treasury Curve Construction Example");
    println!("  Market Data: November 28, 2025");
    println!("===========================================\n");

    // Settlement date
    let settlement = Date::from_ymd(2025, 11, 28).unwrap();

    // Market yields (CMT rates)
    let market_yields = [
        (1.0 / 12.0, 0.03936, "1M"),
        (0.25, 0.03806, "3M"),
        (0.5, 0.03774, "6M"),
        (1.0, 0.03591, "1Y"),
        (2.0, 0.03502, "2Y"),
        (3.0, 0.03493, "3Y"),
        (5.0, 0.03603, "5Y"),
        (7.0, 0.03788, "7Y"),
        (10.0, 0.04018, "10Y"),
        (20.0, 0.04628, "20Y"),
        (30.0, 0.04667, "30Y"),
    ];

    // =========================================================================
    // APPROACH 1: Simple Interpolated Yield Curve (No Bootstrapping)
    // =========================================================================
    println!("=========================================");
    println!("  APPROACH 1: Simple Interpolated Curve");
    println!("=========================================");
    println!("Method: Linear interpolation on market yields");
    println!("Use case: Quick analysis, yield lookups\n");

    // Build curve directly from yields - no bootstrapping required!
    // Just take the market yields and convert to discount factors
    let simple_curve = DiscountCurveBuilder::new(settlement)
        .add_zero_rate(1.0 / 12.0, 0.03936)
        .add_zero_rate(0.25, 0.03806)
        .add_zero_rate(0.5, 0.03774)
        .add_zero_rate(1.0, 0.03591)
        .add_zero_rate(2.0, 0.03502)
        .add_zero_rate(3.0, 0.03493)
        .add_zero_rate(5.0, 0.03603)
        .add_zero_rate(7.0, 0.03788)
        .add_zero_rate(10.0, 0.04018)
        .add_zero_rate(20.0, 0.04628)
        .add_zero_rate(30.0, 0.04667)
        .with_interpolation(InterpolationMethod::Linear)
        .with_extrapolation()
        .build()
        .expect("Simple curve should build");

    println!("Simple Curve Results:");
    println!("{:<8} {:<12} {:<12} {:<12}", "Tenor", "Yield (%)", "DF", "Zero (CC)");
    println!("{}", "-".repeat(48));

    for (tenor, market_yield, label) in &market_yields {
        let df = simple_curve.discount_factor(*tenor).unwrap();
        let zero_cc = simple_curve.zero_rate(*tenor, Compounding::Continuous).unwrap();
        println!(
            "{:<8} {:<12.3} {:<12.6} {:<12.4}",
            label,
            market_yield * 100.0,
            df,
            zero_cc * 100.0
        );
    }

    // Interpolated yields
    println!("\nInterpolated Points (not at pillar tenors):");
    println!("{:<8} {:<12} {:<12}", "Tenor", "Zero (CC)", "DF");
    println!("{}", "-".repeat(36));

    for (tenor, label) in [(4.0, "4Y"), (8.0, "8Y"), (15.0, "15Y"), (25.0, "25Y")] {
        let df = simple_curve.discount_factor(tenor).unwrap();
        let zero_cc = simple_curve.zero_rate(tenor, Compounding::Continuous).unwrap();
        println!("{:<8} {:<12.4}% {:<12.6}", label, zero_cc * 100.0, df);
    }

    // =========================================================================
    // APPROACH 2: Bootstrapped Curve (Global Optimization)
    // =========================================================================
    println!("\n=========================================");
    println!("  APPROACH 2: Bootstrapped Curve");
    println!("=========================================");
    println!("Method: Global optimization on instruments");
    println!("Use case: Accurate pricing, risk management\n");

    // Create T-Bill instruments from discount rates
    // 1M T-Bill
    let tbill_1m_rate = 3.0 + 7.0 / 8.0; // 3.875% (3 7/8)
    let tbill_1m_maturity = settlement.add_months(1).unwrap();
    let tbill_1m_days = settlement.days_between(&tbill_1m_maturity);
    let tbill_1m_price = discount_rate_to_price(tbill_1m_rate / 100.0, tbill_1m_days);
    let tbill_1m = TreasuryBill::new("TBILL-1M", settlement, tbill_1m_maturity, tbill_1m_price);

    // 3M T-Bill
    let tbill_3m_rate = 3.0 + 23.0 / 32.0; // 3.71875% (3 23/32)
    let tbill_3m_maturity = settlement.add_months(3).unwrap();
    let tbill_3m_days = settlement.days_between(&tbill_3m_maturity);
    let tbill_3m_price = discount_rate_to_price(tbill_3m_rate / 100.0, tbill_3m_days);
    let tbill_3m = TreasuryBill::new("TBILL-3M", settlement, tbill_3m_maturity, tbill_3m_price);

    // 6M T-Bill
    let tbill_6m_rate = 3.0 + 21.0 / 32.0; // 3.65625% (3 21/32)
    let tbill_6m_maturity = settlement.add_months(6).unwrap();
    let tbill_6m_days = settlement.days_between(&tbill_6m_maturity);
    let tbill_6m_price = discount_rate_to_price(tbill_6m_rate / 100.0, tbill_6m_days);
    let tbill_6m = TreasuryBill::new("TBILL-6M", settlement, tbill_6m_maturity, tbill_6m_price);

    // 1Y T-Bill
    let tbill_1y_rate = 3.0 + 15.0 / 32.0; // 3.46875% (3 15/32)
    let tbill_1y_maturity = settlement.add_years(1).unwrap();
    let tbill_1y_days = settlement.days_between(&tbill_1y_maturity);
    let tbill_1y_price = discount_rate_to_price(tbill_1y_rate / 100.0, tbill_1y_days);
    let tbill_1y = TreasuryBill::new("TBILL-1Y", settlement, tbill_1y_maturity, tbill_1y_price);

    // Create T-Note/Bond instruments from prices
    let tnote_2y = TreasuryBond::new(
        "TNOTE-2Y",
        settlement,
        settlement.add_years(2).unwrap(),
        0.03375, // 3.375% coupon
        99.25,   // 99 1/4
    );

    let tnote_3y = TreasuryBond::new(
        "TNOTE-3Y",
        settlement,
        settlement.add_years(3).unwrap(),
        0.035, // 3.500% coupon
        100.0,
    );

    let tnote_5y = TreasuryBond::new(
        "TNOTE-5Y",
        settlement,
        settlement.add_years(5).unwrap(),
        0.035, // 3.500% coupon
        parse_price_32nds(99.0, 5.0),
    );

    let tnote_7y = TreasuryBond::new(
        "TNOTE-7Y",
        settlement,
        settlement.add_years(7).unwrap(),
        0.0375, // 3.750% coupon
        99.25,
    );

    let tnote_10y = TreasuryBond::new(
        "TNOTE-10Y",
        settlement,
        settlement.add_years(10).unwrap(),
        0.04, // 4.000% coupon
        parse_price_32nds(99.0, 9.0),
    );

    let tbond_20y = TreasuryBond::new(
        "TBOND-20Y",
        settlement,
        settlement.add_years(20).unwrap(),
        0.04625, // 4.625% coupon
        parse_price_32nds(99.0, 10.0),
    );

    let tbond_30y = TreasuryBond::new(
        "TBOND-30Y",
        settlement,
        settlement.add_years(30).unwrap(),
        0.04625, // 4.625% coupon
        parse_price_32nds(99.0, 3.0),
    );

    // Bootstrap the curve
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

    let boot_curve = result.curve();

    println!("Bootstrapped Curve Results:");
    println!(
        "{:<8} {:<12} {:<12} {:<12} {:<12}",
        "Tenor", "DF", "Zero (CC)", "Zero (SA)", "Par Yield"
    );
    println!("{}", "-".repeat(60));

    let tenors = [
        (1.0 / 12.0, "1M"),
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
        let df = boot_curve.discount_factor(t).unwrap();
        let zero_cc = boot_curve.zero_rate(t, Compounding::Continuous).unwrap();
        let zero_sa = boot_curve.zero_rate(t, Compounding::SemiAnnual).unwrap();
        let par = boot_curve.par_yield(t).unwrap();

        println!(
            "{:<8} {:<12.6} {:<12.4}% {:<12.4}% {:<12.4}%",
            label,
            df,
            zero_cc * 100.0,
            zero_sa * 100.0,
            par * 100.0
        );
    }

    // Repricing report
    println!("\nRepricing Quality:");
    println!("Max error: ${:.6} per $100 notional", result.repricing_report.max_error());

    // Forward rates - this explains the zero rate "hump"
    println!("\nForward Rates (explains zero rate behavior):");
    println!("{:<12} {:<12}", "Period", "Forward (CC)");
    println!("{}", "-".repeat(28));

    let fwd_10_20 = boot_curve.forward_rate(10.0, 20.0).unwrap();
    let fwd_20_30 = boot_curve.forward_rate(20.0, 30.0).unwrap();
    println!("{:<12} {:<12.4}%", "10Y-20Y", fwd_10_20 * 100.0);
    println!("{:<12} {:<12.4}%", "20Y-30Y", fwd_20_30 * 100.0);

    println!("\nNote: 20Y-30Y forward < 10Y-20Y forward explains why 30Y zero < 20Y zero");

    // =========================================================================
    // COMPARISON
    // =========================================================================
    println!("\n=========================================");
    println!("  COMPARISON: Simple vs Bootstrapped");
    println!("=========================================\n");

    println!(
        "{:<8} {:<14} {:<14} {:<12}",
        "Tenor", "Simple DF", "Boot DF", "Diff (bp)"
    );
    println!("{}", "-".repeat(52));

    for (t, label) in [(1.0, "1Y"), (2.0, "2Y"), (5.0, "5Y"), (10.0, "10Y"), (30.0, "30Y")] {
        let simple_df = simple_curve.discount_factor(t).unwrap();
        let boot_df = boot_curve.discount_factor(t).unwrap();

        // Calculate difference in implied zero rate (basis points)
        let simple_zero = -simple_df.ln() / t;
        let boot_zero = -boot_df.ln() / t;
        let diff_bp = (simple_zero - boot_zero) * 10000.0;

        println!(
            "{:<8} {:<14.8} {:<14.8} {:<12.2}",
            label, simple_df, boot_df, diff_bp
        );
    }

    println!("\n=========================================");
    println!("  KEY TAKEAWAYS");
    println!("=========================================");
    println!("
Simple Interpolated Curve:
  - Uses market yields directly with linear interpolation
  - Fast to construct, good for quick yield lookups
  - Does NOT exactly reprice the input instruments
  - Suitable for: approximations, educational purposes

Bootstrapped Curve:
  - Calibrates discount factors to exactly reprice instruments
  - Uses actual T-Bill prices and bond cash flows
  - More accurate for pricing and risk management
  - Suitable for: production pricing, hedging, trading
");
}
