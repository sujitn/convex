//! Mirrors the Quick Start snippet in the repository README so it stays
//! compile-checked. Run with: `cargo run -p convex-analytics --example readme_quickstart`.

use convex_analytics::functions::yield_to_maturity;
use convex_bonds::instruments::FixedRateBond;
use convex_core::types::{Date, Frequency};
use rust_decimal_macros::dec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A 5% semi-annual bond. Coupon is a decimal (0.05 == 5%); day count and
    // calendar default to 30/360 US and SIFMA.
    let bond = FixedRateBond::builder()
        .cusip_unchecked("912828Z29")
        .coupon_rate(dec!(0.05))
        .issue_date(Date::from_ymd(2020, 5, 15)?)
        .maturity(Date::from_ymd(2030, 5, 15)?)
        .frequency(Frequency::SemiAnnual)
        .build()?;

    // Yield to maturity from a clean price of 98.50 (per 100 face).
    let settlement = Date::from_ymd(2025, 5, 15)?;
    let ytm = yield_to_maturity(&bond, settlement, dec!(98.50), Frequency::SemiAnnual)?;

    println!("Yield to Maturity: {:.4}%", ytm.yield_percent());

    Ok(())
}
