# convex-analytics

Fixed income analytics in Rust. Given a bond and a price, it computes yield,
spreads, duration, convexity and DV01. It also prices callable bonds, builds
curves from market quotes, values futures and credit default swaps, and proposes
hedges. It is meant for people building pricing, risk or portfolio tools.

## Where it sits

This is the top crate of the Convex workspace. It builds on `convex-math`,
`convex-core`, `convex-curves`, `convex-rates`, `convex-credit` and
`convex-bonds`. It does not re-export them, so you add the ones you use
directly. For the examples below you need:

```toml
[dependencies]
convex-analytics = "0.14"
convex-bonds = "0.14"
convex-core = "0.14"
convex-curves = "0.14"
rust_decimal_macros = "1.40"
```

## What's in it

Yields and prices:

- `functions`: free functions for yield to maturity, clean and dirty price from yield, duration, convexity and DV01.
- `yields`: the yield solver and engine behind those functions, plus current yield and simple yield.
- `pricing`: price a bond from a trader mark (price, yield or spread).
- `yas`: yield, spread and risk for a fixed-rate bond in one call (`YASCalculator`), and settlement invoices.
- `cashflows`: schedules, accrued interest and settlement dates.

Spreads:

- `spreads`: Z-spread, G-spread, I-spread, option-adjusted spread, asset swap spreads and discount margin for floaters.

Risk and hedging:

- `risk`: duration, convexity, DV01, key rate durations, value at risk, and the hedge advisor in `risk::hedging`.
- `credit_risk`: credit scenarios and CDS-based sensitivities for bonds.
- `quote_risk`: sensitivity of a result to the market quotes a curve was built from.

Callable and putable bonds:

- `options`: binomial tree and Hull-White short-rate model, re-exported from `convex-bonds`.
- `calibration`: calibrate a Hull-White one-factor model to swaption prices.
- `optional_bond`: callable and putable bond valuation and risk.

Curve calibration:

- `rate_calibration`: build a curve set from deposits, FRAs, futures, swaps, OIS and tenor basis quotes.
- `swap_calibration`, `cash_calibration`, `credit_calibration`: fits for swap curves, government bond curves and CDS credit curves.

Futures:

- `futures`: carry, pricing, risk and settlement for bond and rate futures.

Lifecycle:

- `lifecycle`: apply events such as coupons, fixings and exercises to a trade and get the next state and the proposed cash effects.

## Yield to maturity and duration

```rust
use convex_analytics::functions::{modified_duration, yield_to_maturity};
use convex_bonds::instruments::FixedRateBond;
use convex_core::types::{Date, Frequency};
use rust_decimal_macros::dec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A 5% semi-annual bond. The coupon is a decimal (0.05 is 5%). Day count
    // and calendar default to 30/360 US and SIFMA.
    let bond = FixedRateBond::builder()
        .cusip_unchecked("912828Z29")
        .coupon_rate(dec!(0.05))
        .issue_date(Date::from_ymd(2020, 5, 15)?)
        .maturity(Date::from_ymd(2030, 5, 15)?)
        .frequency(Frequency::SemiAnnual)
        .build()?;

    // Yield to maturity from a clean price of 98.50 per 100 face.
    let settlement = Date::from_ymd(2025, 5, 15)?;
    let ytm = yield_to_maturity(&bond, settlement, dec!(98.50), Frequency::SemiAnnual)?;
    println!("Yield to Maturity: {:.4}%", ytm.yield_percent()?);

    // Modified duration at that yield.
    let duration = modified_duration(&bond, settlement, ytm.yield_value, Frequency::SemiAnnual)?;
    println!("Modified duration: {:.4}", duration);

    Ok(())
}
```

`yield_to_maturity` returns a result you can read as a percent
(`yield_percent`) or as a decimal (`yield_value`). `dv01` and `convexity` in the
same module take similar inputs.

## Z-spread

The Z-spread is the constant amount added to every zero rate on a curve so that
the bond's discounted cash flows equal its dirty price.

```rust
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

    // A flat 4% continuously compounded discount curve to spread against.
    let settlement = Date::from_ymd(2025, 1, 15)?;
    let rate = 0.04_f64;
    let curve = DiscountCurveBuilder::new(settlement)
        .add_pillar(1.0, (-rate * 1.0).exp())
        .add_pillar(2.0, (-rate * 2.0).exp())
        .add_pillar(5.0, (-rate * 5.0).exp())
        .add_pillar(10.0, (-rate * 10.0).exp())
        .with_extrapolation()
        .build()?;

    // The Z-spread that reprices the bond to a dirty price of 97.50.
    let spread = z_spread(&bond, dec!(97.50), &curve, settlement)?;
    println!("Z-Spread: {:.2} bps", spread.as_bps());

    Ok(())
}
```

## Hedge advisor

`risk::hedging` takes a position's risk (DV01 and key rate durations), a set of
limits, and a discount curve. It sizes a hedge, reports what risk is left over,
and compares the candidates. The strategies are `duration_futures`,
`barbell_futures`, `key_rate_futures`, `cash_bond_pair` and
`interest_rate_swap`. `compare_hedges` ranks the proposals and `narrate`
turns the comparison into text.

Two limits to know about. Trading cost comes from `HeuristicCostFeed`, a fixed
table of rough round-trip costs by instrument type. The proposal records it as
`heuristic_v1`. If you have real quotes, implement the `CostFeed` trait. For
futures, a contract with no delivery basket of its own gets a single
synthetic deliverable, with an approximate conversion factor. Treat advisor
output as indicative, not as an order. Each proposal says so through its
execution status.

## Links

- API docs: <https://docs.rs/convex-analytics>
- Yield and price calculations: <https://github.com/sujitn/convex/blob/main/docs/YIELD-PRICING.md>
- Curve calibration: <https://github.com/sujitn/convex/blob/main/docs/RATE-CALIBRATION.md>
- Credit curve calibration: <https://github.com/sujitn/convex/blob/main/docs/CREDIT-CALIBRATION.md>
- Futures: <https://github.com/sujitn/convex/blob/main/docs/FUTURES-VALUATION.md>
- Lifecycle events: <https://github.com/sujitn/convex/blob/main/docs/LIFECYCLE.md>
- Source: <https://github.com/sujitn/convex>

## License

MIT. See LICENSE.

The CDS routes come from `convex-credit`, which is based on the ISDA CDS
Standard Model and carries the ISDA notice and license.
