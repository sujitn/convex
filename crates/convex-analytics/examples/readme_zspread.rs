//! Mirrors the Z-Spread snippet in the repository README so it stays
//! compile-checked. Run with: `cargo run -p convex-analytics --example readme_zspread`.

use convex_analytics::spreads::z_spread;
use convex_bonds::instruments::FixedRateBond;
use convex_core::types::{Date, Frequency};
use convex_curves::curves::DiscountCurveBuilder;
use rust_decimal_macros::dec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 3.75% semi-annual corporate bond.
    let bond = FixedRateBond::builder()
        .cusip_unchecked("459200KJ1")
        .coupon_rate(dec!(0.0375))
        .issue_date(Date::from_ymd(2018, 11, 15)?)
        .maturity(Date::from_ymd(2028, 11, 15)?)
        .frequency(Frequency::SemiAnnual)
        .build()?;

    // A flat 4% continuously-compounded discount curve to spread against.
    let settlement = Date::from_ymd(2025, 1, 15)?;
    let rate = 0.04_f64;
    let curve = DiscountCurveBuilder::new(settlement)
        .add_pillar(1.0, (-rate * 1.0).exp())
        .add_pillar(2.0, (-rate * 2.0).exp())
        .add_pillar(5.0, (-rate * 5.0).exp())
        .add_pillar(10.0, (-rate * 10.0).exp())
        .with_extrapolation()
        .build()?;

    // Z-spread that reprices the bond to a dirty price of 102.50.
    let spread = z_spread(&bond, dec!(102.50), &curve, settlement)?;
    println!("Z-Spread: {:.2} bps", spread.as_bps());

    Ok(())
}
