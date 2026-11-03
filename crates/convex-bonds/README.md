# convex-bonds

Bond instruments for Rust: fixed-rate, zero-coupon, floating-rate, callable and
sinking-fund bonds, with coupon schedules, accrued interest, cash flows and a
yield solver. You describe the bond, ask for the cash flows after a settlement
date, and price or solve a yield from them.

Use it directly if you want the bond definitions and their dated cash without
the wider analytics layer. Spreads, duration and other risk measures are in
`convex-analytics`, which wraps these types.

## Where it fits

`convex-bonds` builds on `convex-core` (dates, day counts, calendars, cash flow
primitives), `convex-math` (solvers) and `convex-curves` (curves). The
`convex-analytics` crate builds on it.

## Install

```toml
[dependencies]
convex-bonds = "0.14"
convex-core = "0.14"
rust_decimal_macros = "1"
```

## What's in it

- `instruments`: `FixedRateBond`, `ZeroCouponBond`, `FloatingRateNote`,
  `CallableBond`, `SinkingFundBond`, each with a builder.
- `traits`: the `Bond` trait (cash flows, accrued interest, dates, identifiers)
  that all instruments implement, plus `BondCashFlow`.
- `cashflows`: `Schedule` and `ScheduleConfig` for coupon dates and stubs,
  `AccruedInterestCalculator`, and the shared cash flow types from `convex-core`.
- `contracts`: `BondTerms`, a complete serializable description of a bond that
  can be rebuilt into an instrument, and projection of its cash flows.
- `pricing`: `YieldSolver` and `YieldResult`, current yield, and helpers for
  discounting dated cash.
- `conventions`: day count, frequency and settlement presets for common
  markets (US Treasury, US corporate, gilts, Bunds, JGBs, Eurobonds).
- `options`: a binomial tree and a Hull-White short rate model for bonds with
  embedded options.
- `types`: identifiers (`Cusip`, `Isin`, `Figi`, `Sedol`), call and put
  schedules, amortization schedules and price quote types.
- `arrc` and `fixings`: compounded overnight rate coupons (SOFR style) and
  fixing history.

Several constructors return a `Result` (`build()`, `try_cash_flows`,
`try_accrued_interest`). They refuse inconsistent input, such as a maturity
before the issue date, instead of choosing a fallback.

## Cash flows and accrued interest

A 5% semi-annual bond maturing 15 May 2030, looked at on 20 June 2025:

```rust
use convex_bonds::instruments::FixedRateBond;
use convex_core::types::{Date, Frequency};
use rust_decimal_macros::dec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The coupon is a decimal, so 0.05 means 5%. Day count and calendar
    // default to 30/360 US and SIFMA.
    let bond = FixedRateBond::builder()
        .cusip_unchecked("912828Z29")
        .coupon_rate(dec!(0.05))
        .issue_date(Date::from_ymd(2020, 5, 15)?)
        .maturity(Date::from_ymd(2030, 5, 15)?)
        .frequency(Frequency::SemiAnnual)
        .build()?;

    let settlement = Date::from_ymd(2025, 6, 20)?;

    // 15 May to 20 June is 35 days on a 30/360 basis: 2.5 * 35 / 180.
    let accrued = bond.try_accrued_interest(settlement)?;
    println!("accrued per 100: {accrued}");
    assert!(accrued > dec!(0.486) && accrued < dec!(0.487));

    let flows = bond.try_cash_flows(settlement)?;
    for cf in &flows {
        println!("{}  {:>8}  {:?}", cf.date, cf.amount, cf.flow_type);
    }
    // Ten payments from November 2025 to May 2030; the last includes principal.
    assert_eq!(flows.len(), 10);
    Ok(())
}
```

## Solving a yield

`YieldSolver` takes the cash flows, a clean price per 100 and the accrued
interest, and returns the yield that discounts the cash to the dirty price.

```rust
use convex_bonds::instruments::FixedRateBond;
use convex_bonds::pricing::YieldSolver;
use convex_core::types::{Date, Frequency};
use rust_decimal_macros::dec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bond = FixedRateBond::builder()
        .cusip_unchecked("912828Z29")
        .coupon_rate(dec!(0.05))
        .issue_date(Date::from_ymd(2020, 5, 15)?)
        .maturity(Date::from_ymd(2030, 5, 15)?)
        .frequency(Frequency::SemiAnnual)
        .build()?;
    let settlement = Date::from_ymd(2025, 5, 15)?;

    let flows = bond.try_cash_flows(settlement)?;
    let accrued = bond.try_accrued_interest(settlement)?;
    let result = YieldSolver::new().solve(
        &flows,
        dec!(98.50),
        accrued,
        settlement,
        bond.day_count(),
        bond.frequency(),
    )?;

    println!("yield: {:.4}%", result.yield_percent()?);
    Ok(())
}
```

## Links

- API documentation: <https://docs.rs/convex-bonds>
- Bond terms and contracts: <https://github.com/sujitn/convex/blob/main/docs/BOND-CONTRACTS.md>
- Cash flow primitives: <https://github.com/sujitn/convex/blob/main/docs/CASHFLOW-PRIMITIVES.md>
- Accrued interest methods: <https://github.com/sujitn/convex/blob/main/docs/ACCRUED-METHODS.md>
- Yield pricing: <https://github.com/sujitn/convex/blob/main/docs/YIELD-PRICING.md>
- Repository: <https://github.com/sujitn/convex>

## License

MIT. See LICENSE.
