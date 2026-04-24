//! Tier 3.7 — BondPricer regression. Pre-8ae6574 the YTM body was hardcoded
//! to `(1+y/2)^(2·t)` with `t=days/365`; the refactor routes through
//! `YieldSolver` with the bond's own frequency + day count. Pricing at
//! coupon-rate-at-par gives true YTM = coupon; forcing the solver to
//! `(SemiAnnual, Act365F)` reproduces the old behaviour.
//!
//! Run: `cargo run -p reconcile_bench --example bondpricer_regression`

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use convex_bonds::instruments::{FixedBondBuilder, FixedRateBond};
use convex_bonds::pricing::{BondPricer, YieldSolver};
use convex_bonds::traits::Bond;
use convex_bonds::types::CalendarId;
use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};

struct Case {
    label: &'static str,
    frequency: Frequency,
    day_count: DayCountConvention,
    day_count_str: &'static str,
    coupon_pct: f64,
}

fn date(y: i32, m: u32, d: u32) -> Date {
    Date::from_ymd(y, m, d).unwrap()
}

fn main() {
    let settle = date(2025, 12, 31);
    let maturity = date(2035, 12, 31);
    let short_maturity = date(2027, 12, 31);
    let face = dec!(100);

    let cases = [
        Case {
            label: "ANNUAL_BUND_LIKE",
            frequency: Frequency::Annual,
            day_count: DayCountConvention::ActActIcma,
            day_count_str: "ACT/ACT",
            coupon_pct: 3.00,
        },
        Case {
            label: "QUARTERLY_FRN_LIKE",
            frequency: Frequency::Quarterly,
            day_count: DayCountConvention::Act360,
            day_count_str: "ACT/360",
            coupon_pct: 4.00,
        },
        Case {
            label: "SEMI_UST_LIKE",
            frequency: Frequency::SemiAnnual,
            day_count: DayCountConvention::ActActIcma,
            day_count_str: "ACT/ACT",
            coupon_pct: 4.00,
        },
    ];

    println!(
        "{:22} {:>10} {:>13} {:>13} {:>13} {:>13}",
        "bond", "coupon", "NEW ytm", "OLD ytm", "Δ new", "Δ old"
    );
    println!("{}", "-".repeat(88));

    for c in &cases {
        let maturity = if c.label.starts_with("QUARTERLY") {
            short_maturity
        } else {
            maturity
        };
        let coupon_dec = c.coupon_pct / 100.0;

        let fb = FixedBondBuilder::new()
            .isin(c.label)
            .coupon_rate(Decimal::try_from(coupon_dec).unwrap())
            .maturity(maturity)
            .frequency(c.frequency)
            .currency(Currency::USD)
            .face_value(face)
            .day_count(c.day_count_str)
            .issue_date(settle)
            .build()
            .expect("FixedBond");

        // Price at coupon-rate-at-par → clean price should be 100.
        let priced =
            BondPricer::price_from_yield(&fb, Decimal::try_from(coupon_dec).unwrap(), settle)
                .expect("price_from_yield");

        // NEW path: current BondPricer (delegates to YieldSolver with bond's freq + dc).
        let new_ytm = f64::try_from(
            BondPricer::yield_to_maturity(&fb, priced.clean_price, settle).expect("ytm"),
        )
        .unwrap_or(0.0);

        // OLD path: same YieldSolver forced to (SemiAnnual, Act365F) — the exact
        // math the pre-refactor body did. Build a matching FixedRateBond to pull
        // cashflows (same schedule as FixedBond for these straight bullets).
        let frb = FixedRateBond::builder()
            .cusip_unchecked(c.label)
            .coupon_rate(Decimal::try_from(coupon_dec).unwrap())
            .issue_date(settle)
            .maturity(maturity)
            .frequency(c.frequency)
            .day_count(c.day_count)
            .currency(Currency::USD)
            .face_value(face)
            .calendar(CalendarId::new(""))
            .business_day_convention(BusinessDayConvention::Unadjusted)
            .end_of_month(false)
            .build()
            .expect("FixedRateBond");
        let dirty = priced.clean_price.as_percentage() + priced.accrued_interest;
        let old_ytm = YieldSolver::new()
            .solve(
                &frb.cash_flows(settle),
                dirty,
                Decimal::ZERO,
                settle,
                DayCountConvention::Act365Fixed,
                Frequency::SemiAnnual,
            )
            .expect("old solve")
            .yield_value;

        println!(
            "{:22} {:>9.4}% {:>12.6}% {:>12.6}% {:>+12.2}bp {:>+12.2}bp",
            c.label,
            c.coupon_pct,
            new_ytm * 100.0,
            old_ytm * 100.0,
            (new_ytm - coupon_dec) * 10_000.0,
            (old_ytm - coupon_dec) * 10_000.0,
        );
    }
}
