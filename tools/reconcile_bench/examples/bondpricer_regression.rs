//! BondPricer numerical regression probe for Tier 3.7.
//!
//! Before commit 8ae6574, `BondPricer::yield_to_maturity` / `::price_from_yield`
//! hardcoded semi-annual compounding `(1 + y/2)^(2·t)` with `t = days/365`.
//! That's the correct discount for a semi-annual UST in ACT/365F land, but
//! wrong for every annual / quarterly / non-ACT-365 bond that came through
//! the API.
//!
//! The new path delegates to `YieldSolver` using the bond's actual frequency
//! and day count. This probe shows:
//!
//!   * NEW path: given a clean price at coupon-rate-at-par, YTM round-trips
//!     to the coupon rate (within 1e-10). This is the contract the user of
//!     `BondPricer::yield_to_maturity` expects.
//!   * OLD path (simulated): forcing `YieldSolver` to `(SemiAnnual, Act365F)`
//!     — the same math the pre-refactor body did — drifts by a well-defined
//!     amount on any non-semi-annual bond.
//!
//! Run:
//!     cargo run -p reconcile_bench --example bondpricer_regression

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use convex_bonds::instruments::{FixedBond, FixedBondBuilder, FixedRateBond};
use convex_bonds::pricing::BondPricer;
use convex_bonds::pricing::{YieldResult, YieldSolver};
use convex_bonds::traits::{Bond, BondCashFlow};
use convex_bonds::types::CalendarId;
use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};

fn date(y: i32, m: u32, d: u32) -> Date {
    Date::from_ymd(y, m, d).unwrap()
}

struct Scenario {
    label: &'static str,
    frequency: Frequency,
    day_count: DayCountConvention,
    day_count_name: &'static str,
    coupon_pct: f64,
    issue: Date,
    maturity: Date,
    settlement: Date,
    face: Decimal,
}

fn build_fixed_rate(s: &Scenario) -> FixedRateBond {
    FixedRateBond::builder()
        .cusip_unchecked(s.label)
        .coupon_rate(Decimal::try_from(s.coupon_pct / 100.0).unwrap())
        .issue_date(s.issue)
        .maturity(s.maturity)
        .frequency(s.frequency)
        .day_count(s.day_count)
        .currency(Currency::USD)
        .face_value(s.face)
        .calendar(CalendarId::new(""))
        .business_day_convention(BusinessDayConvention::Unadjusted)
        .end_of_month(false)
        .build()
        .expect("build frb")
}

fn build_fixed(s: &Scenario) -> FixedBond {
    // BondPricer uses the legacy FixedBond type. Build one mirroring the FRB.
    FixedBondBuilder::new()
        .isin(s.label)
        .coupon_rate(Decimal::try_from(s.coupon_pct / 100.0).unwrap())
        .maturity(s.maturity)
        .frequency(s.frequency)
        .currency(Currency::USD)
        .face_value(s.face)
        .day_count(s.day_count_name)
        .issue_date(s.issue)
        .build()
        .expect("build fb")
}

fn solve_old_style(cash_flows: &[BondCashFlow], dirty: Decimal, settle: Date) -> YieldResult {
    // Simulate the pre-8ae6574 formula by forcing (SemiAnnual, Act365Fixed).
    // `project_discount_fractions` short-circuits ACT/365F to raw days/365,
    // and `SemiAnnual` drives (1+y/2)^(2t) — bit-for-bit the old body.
    YieldSolver::new()
        .solve(
            cash_flows,
            dirty, // treat dirty as clean + zero accrued
            Decimal::ZERO,
            settle,
            DayCountConvention::Act365Fixed,
            Frequency::SemiAnnual,
        )
        .expect("YieldSolver (SemiAnnual, Act365F)")
}

fn main() {
    let scenarios = [
        Scenario {
            label: "ANNUAL_BUND_LIKE",
            frequency: Frequency::Annual,
            day_count: DayCountConvention::ActActIcma,
            day_count_name: "ACT/ACT",
            coupon_pct: 3.00,
            issue: date(2025, 12, 31),
            maturity: date(2035, 12, 31),
            settlement: date(2025, 12, 31),
            face: dec!(100),
        },
        Scenario {
            label: "QUARTERLY_FRN_LIKE",
            frequency: Frequency::Quarterly,
            day_count: DayCountConvention::Act360,
            day_count_name: "ACT/360",
            coupon_pct: 4.00,
            issue: date(2025, 12, 31),
            maturity: date(2027, 12, 31),
            settlement: date(2025, 12, 31),
            face: dec!(100),
        },
        Scenario {
            label: "SEMI_UST_LIKE",
            frequency: Frequency::SemiAnnual,
            day_count: DayCountConvention::ActActIcma,
            day_count_name: "ACT/ACT",
            coupon_pct: 4.00,
            issue: date(2025, 12, 31),
            maturity: date(2035, 12, 31),
            settlement: date(2025, 12, 31),
            face: dec!(100),
        },
    ];

    println!(
        "{:22} {:>10} {:>13} {:>13} {:>13} {:>13}",
        "bond", "coupon", "NEW ytm", "OLD ytm", "Δ new", "Δ old"
    );
    println!("{}", "-".repeat(88));

    for s in &scenarios {
        let coupon_dec = s.coupon_pct / 100.0;

        // Issue → settle on same day, price at par via the NEW BondPricer path.
        let fb = build_fixed(s);
        let priced =
            BondPricer::price_from_yield(&fb, Decimal::try_from(coupon_dec).unwrap(), s.settlement)
                .expect("price_from_yield");

        // NEW YTM: current BondPricer path.
        let new_ytm = BondPricer::yield_to_maturity(&fb, priced.clean_price, s.settlement)
            .expect("yield_to_maturity");
        let new_f = f64::try_from(new_ytm).unwrap_or(0.0);

        // OLD YTM: pull cash flows from the FRB (same schedule as FixedBond
        // under these straight-bullet scenarios) and re-solve with the
        // pre-refactor (SemiAnnual, Act365F) substitution.
        let frb = build_fixed_rate(s);
        let cash_flows = frb.cash_flows(s.settlement);
        let dirty_dec: Decimal = Decimal::try_from(
            f64::try_from(priced.clean_price.as_percentage()).unwrap_or(100.0)
                + f64::try_from(priced.accrued_interest).unwrap_or(0.0),
        )
        .unwrap();
        let old = solve_old_style(&cash_flows, dirty_dec, s.settlement);

        println!(
            "{:22} {:>9.4}% {:>12.6}% {:>12.6}% {:>+12.2}bp {:>+12.2}bp",
            s.label,
            s.coupon_pct,
            new_f * 100.0,
            old.yield_value * 100.0,
            (new_f - coupon_dec) * 10_000.0,
            (old.yield_value - coupon_dec) * 10_000.0,
        );
    }
}
