# convex-yas

Bloomberg YAS (Yield Analysis System) replication for the Convex fixed income analytics library.

## Overview

`convex-yas` provides Bloomberg YAS-compatible analytics, designed to match Bloomberg's yield and spread calculations to production accuracy:

### Yield Calculations
- **Street Convention**: Standard market yield calculation
- **True Yield**: Yield adjusted for settlement timing
- **Current Yield**: Annual coupon / Clean price
- **Simple Yield**: Non-compounded yield measure
- **Money Market Yields**: Discount yield, bank discount yield, bond equivalent yield

### Complete Analytics
- Full YAS-style analysis with all metrics
- Settlement invoice calculations
- YAS screen-style formatting

### Accuracy Targets
| Metric | Tolerance |
|--------|-----------|
| Street Yield | +/- 0.00001% |
| True Yield | +/- 0.00001% |
| G-Spread | +/- 0.1 bps |
| Z-Spread | +/- 0.1 bps |
| Duration | +/- 0.001 |
| Convexity | +/- 0.001 |

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
convex-yas = "0.1"
```

## Usage

### Full YAS Analysis

```rust
use convex_yas::{YasAnalysis, YasResult};
use convex_bonds::FixedRateBond;
use convex_curves::DiscountCurve;

let bond = FixedRateBond::builder()
    .cusip("097023AH7")
    .coupon_rate(dec!(0.075))
    .maturity(Date::from_ymd(2025, 6, 15))
    .frequency(Frequency::SemiAnnual)
    .day_count(DayCount::Thirty360US)
    .build()?;

let settlement = Date::from_ymd(2024, 4, 29);
let price = dec!(110.503);

let yas = YasAnalysis::new(&bond, price, settlement)
    .with_treasury_curve(&treasury_curve)
    .with_swap_curve(&swap_curve)
    .analyze()?;

println!("Street Convention: {:.6}%", yas.street_yield.as_percent());
println!("True Yield:        {:.6}%", yas.true_yield.as_percent());
println!("Current Yield:     {:.3}%", yas.current_yield.as_percent());
println!("G-Spread:          {:.1} bps", yas.g_spread.as_bps());
println!("Z-Spread:          {:.1} bps", yas.z_spread.as_bps());
println!("Modified Duration: {:.3}", yas.modified_duration);
println!("Convexity:         {:.3}", yas.convexity);
println!("Accrued Interest:  ${:.2}", yas.accrued_interest);
```

### Yield Types

```rust
use convex_yas::yields::{StreetYield, TrueYield, CurrentYield};

// Street convention yield
let street = StreetYield::calculate(&bond, price, settlement)?;

// True yield (adjusted for settlement)
let true_yield = TrueYield::calculate(&bond, price, settlement)?;

// Current yield
let current = CurrentYield::calculate(&bond, price)?;
```

### Settlement Invoice

```rust
use convex_yas::invoice::SettlementInvoice;

let invoice = SettlementInvoice::generate(
    &bond,
    price,
    settlement,
    notional: dec!(1_000_000),
)?;

println!("Principal:   ${:.2}", invoice.principal);
println!("Accrued:     ${:.2}", invoice.accrued);
println!("Total Due:   ${:.2}", invoice.total);
```

### Money Market Yields

```rust
use convex_yas::yields::MoneyMarketYield;
use convex_bonds::government::TreasuryBill;

let mm = MoneyMarketYield::calculate(&tbill, settlement)?;

println!("Discount Yield: {:.3}%", mm.discount_yield.as_percent());
println!("BEY:            {:.3}%", mm.bond_equivalent_yield.as_percent());
```

## Bloomberg Validation

This crate is validated against Bloomberg YAS for the following reference bonds:

- **Boeing 7.5% 06/15/2025** (Corporate IG) - Primary validation bond
- **US Treasury 10Y** - ACT/ACT ICMA validation
- **US T-Bills** - Money market yield validation
- **Callable corporates** - YTW and OAS validation
- **Municipal bonds** - Tax-equivalent yield validation

## License

This project is licensed under the MIT License - see the [LICENSE](../../LICENSE) file for details.
