# convex-core

The base types for Convex fixed income work: dates, prices, yields, currencies,
day count conventions, business day calendars, and the shared cash flow
schedule code.

Use it directly if you need those pieces on their own, or if you are writing a
crate that plugs into the Convex curve and bond crates.

## Where it sits

`convex-core` depends on `convex-math` for its numerical routines. `convex-curves`
builds on it, then `convex-rates`, `convex-credit` and `convex-bonds`, and finally
`convex-analytics`.

## Install

```toml
[dependencies]
convex-core = "0.14"
rust_decimal_macros = "1.40"
```

## What's in it

- `types`: `Date`, `Price`, `Yield`, `Spread`, `Currency`, `Frequency`,
  `Compounding`, `CashFlow` and `CashFlowSchedule`, and the `Mark` quote type.
- `daycounts`: the `DayCount` trait with Act/360, Act/365 Fixed and Leap,
  Act/Act ISDA, ICMA and AFB, and the 30/360 US, E, E ISDA and German
  conventions. `DayCountConvention` selects one at runtime.
- `calendars`: the `Calendar` trait, `BusinessDayConvention`, calendars for SIFMA,
  US Government, TARGET2, UK, Japan and the US, and `DynamicCalendar` for holiday
  lists you supply yourself.
- `cashflows`: coupon schedule generation (`ScheduleSpec`, `CouponSchedule`),
  coupon and principal amounts, and overnight-rate compounding.
- `fixings`: a fixed history of published index rates, used for floating coupons.
- `contracts`: request and result types with explicit units, such as
  `PricePer100`, `DecimalRate` and `BasisPoints`, so a percent cannot be passed
  where a basis point amount is expected.
- `futures`: reference terms for exchange-traded futures.
- `traits`: `YieldCurve`, `PricingEngine`, `RiskCalculator` and `Discountable`.
- `ids` and `error`: identifier types, and `ConvexError` with the
  `ConvexResult` alias.

Several types have a "checked" form, meaning a constructor or function that
returns a `Result` and rejects bad input instead of panicking or rounding it
away. For example, `Date::from_ymd` returns an error for 30 February.

The optional `schemars` feature adds JSON Schema derives for the public types.

## Examples

Year fractions and business days:

```rust
use convex_core::calendars::{BusinessDayConvention, Calendar, SIFMACalendar};
use convex_core::daycounts::{DayCount, Thirty360US};
use convex_core::types::Date;
use rust_decimal_macros::dec;

let issue = Date::from_ymd(2025, 1, 15).unwrap();
let next_coupon = Date::from_ymd(2025, 7, 15).unwrap();

// A semi-annual 5% coupon on 30/360 US accrues half a year of interest.
let fraction = Thirty360US.year_fraction(issue, next_coupon);
assert_eq!(fraction, dec!(0.5));
assert_eq!(dec!(100) * dec!(0.05) * fraction, dec!(2.5));

// 4 July 2025 is a holiday, so the following business day is Monday the 7th.
let calendar = SIFMACalendar::new();
let july_4 = Date::from_ymd(2025, 7, 4).unwrap();
let paid_on = calendar.adjust(july_4, BusinessDayConvention::Following).unwrap();
assert_eq!(paid_on, Date::from_ymd(2025, 7, 7).unwrap());
```

Prices and yields:

```rust
use convex_core::prelude::*;
use rust_decimal_macros::dec;

// A price is a percentage of par. Accrued interest turns clean into dirty.
let clean = Price::new(dec!(98.50), Currency::USD);
let dirty = clean.to_dirty(dec!(1.25)).unwrap();
assert_eq!(dirty.as_percentage(), dec!(99.75));

// A yield carries its compounding convention.
let y = Yield::new(dec!(0.05), Compounding::SemiAnnual);
assert_eq!(y.as_bps().unwrap(), 500);
let annual = y.convert_to(Compounding::Annual);
assert!(annual.value() > y.value());
```

## Links

- API documentation: <https://docs.rs/convex-core>
- Shared cash flow code: <https://github.com/sujitn/convex/blob/main/docs/CASHFLOW-PRIMITIVES.md>

## License

MIT. See LICENSE.
