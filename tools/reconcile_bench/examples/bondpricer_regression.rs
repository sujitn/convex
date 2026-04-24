//! Tier 3.7 — YieldSolver regression. Pre-8ae6574 the YTM body was hardcoded
//! to `(1+y/2)^(2·t)` with `t=days/365`; the refactor routes through
//! `YieldSolver` with the bond's own frequency + day count. Pricing at
//! coupon-rate-at-par gives true YTM = coupon; forcing the solver to
//! `(SemiAnnual, Act365F)` reproduces the old behaviour.
//!
//! Run: `cargo run -p reconcile_bench --example bondpricer_regression`

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use convex_bonds::instruments::FixedRateBond;
use convex_bonds::pricing::YieldSolver;
use convex_bonds::traits::{Bond, BondAnalytics};
use convex_bonds::types::CalendarId;
use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};

struct Case {
    label: &'static str,
    frequency: Frequency,
    day_count: DayCountConvention,
    coupon_pct: f64,
}

fn date(y: i32, m: u32, d: u32) -> Date {
    Date::from_ymd(y, m, d).unwrap()
}

fn main() {
    let settle = date(2025, 12, 31);
    let long_mat = date(2035, 12, 31);
    let short_mat = date(2027, 12, 31);

    let cases = [
        Case {
            label: "ANNUAL_BUND_LIKE",
            frequency: Frequency::Annual,
            day_count: DayCountConvention::ActActIcma,
            coupon_pct: 3.00,
        },
        Case {
            label: "QUARTERLY_FRN_LIKE",
            frequency: Frequency::Quarterly,
            day_count: DayCountConvention::Act360,
            coupon_pct: 4.00,
        },
        Case {
            label: "SEMI_UST_LIKE",
            frequency: Frequency::SemiAnnual,
            day_count: DayCountConvention::ActActIcma,
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
            short_mat
        } else {
            long_mat
        };
        let coupon_dec = c.coupon_pct / 100.0;

        let bond = FixedRateBond::builder()
            .cusip_unchecked(c.label)
            .coupon_rate(Decimal::try_from(coupon_dec).unwrap())
            .issue_date(settle)
            .maturity(maturity)
            .frequency(c.frequency)
            .day_count(c.day_count)
            .currency(Currency::USD)
            .face_value(dec!(100))
            .calendar(CalendarId::new(""))
            .business_day_convention(BusinessDayConvention::Unadjusted)
            .end_of_month(false)
            .build()
            .expect("FixedRateBond");

        // Price at coupon-rate-at-par → clean should be 100 (no accrued at issue).
        let clean = bond
            .clean_price_from_yield(settle, coupon_dec, c.frequency)
            .expect("clean_price_from_yield");
        let clean_dec = Decimal::try_from(clean).unwrap();

        // NEW path: BondAnalytics::yield_to_maturity, which routes through
        // YieldSolver with the bond's actual frequency + day count.
        let new_ytm = bond
            .yield_to_maturity(settle, clean_dec, c.frequency)
            .expect("ytm")
            .yield_value;

        // OLD path: force YieldSolver to (SemiAnnual, Act365F) — the math the
        // pre-8ae6574 body did. Cashflows come from the same bond.
        let cash_flows = bond.cash_flows(settle);
        let accrued = bond.accrued_interest(settle);
        let old_ytm = YieldSolver::new()
            .solve(
                &cash_flows,
                clean_dec,
                accrued,
                settle,
                DayCountConvention::Act365Fixed,
                Frequency::SemiAnnual,
            )
            .expect("old solve")
            .yield_value;

        let new_f = new_ytm.to_f64().unwrap_or(0.0);
        println!(
            "{:22} {:>9.4}% {:>12.6}% {:>12.6}% {:>+12.2}bp {:>+12.2}bp",
            c.label,
            c.coupon_pct,
            new_f * 100.0,
            old_ytm * 100.0,
            (new_f - coupon_dec) * 10_000.0,
            (old_ytm - coupon_dec) * 10_000.0,
        );
    }
}
