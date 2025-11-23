# convex-bonds

Bond pricing and analytics for the Convex fixed income analytics library.

## Overview

`convex-bonds` provides comprehensive bond analysis:

- **Instruments**: Fixed coupon bonds, zero coupon bonds
- **Pricing**: Present value, clean/dirty price, yield-to-maturity
- **Cash Flows**: Schedule generation with business day adjustments
- **Risk**: Duration, convexity, DV01, key rate durations

## Features

### Creating Bonds

```rust
use convex_bonds::prelude::*;
use convex_core::types::{Date, Currency, Frequency};
use rust_decimal_macros::dec;

let bond = FixedBondBuilder::new()
    .isin("US912828Z229")
    .coupon_rate(dec!(0.025))  // 2.5%
    .maturity(Date::from_ymd(2030, 5, 15).unwrap())
    .frequency(Frequency::SemiAnnual)
    .currency(Currency::USD)
    .day_count("ACT/ACT")
    .build()
    .unwrap();
```

### Pricing

```rust
use convex_bonds::pricing::BondPricer;

let settlement = Date::from_ymd(2025, 1, 15).unwrap();

// Price from yield
let result = BondPricer::price_from_yield(&bond, dec!(0.03), settlement).unwrap();
println!("Clean Price: {}", result.clean_price);
println!("Dirty Price: {}", result.dirty_price);
println!("Accrued: {}", result.accrued_interest);

// Calculate YTM from price
let ytm = BondPricer::yield_to_maturity(&bond, result.clean_price, settlement).unwrap();
```

### Cash Flow Generation

```rust
use convex_bonds::cashflows::CashFlowGenerator;

let schedule = CashFlowGenerator::generate(&bond, settlement).unwrap();

for cf in schedule.iter() {
    println!("{}: {} ({})", cf.date(), cf.amount(), cf.cf_type());
}

let accrued = CashFlowGenerator::accrued_interest(&bond, settlement).unwrap();
```

### Risk Analytics

```rust
use convex_bonds::risk::RiskCalculator;

let metrics = RiskCalculator::calculate(&bond, dec!(0.03), settlement).unwrap();

println!("Macaulay Duration: {}", metrics.duration.macaulay);
println!("Modified Duration: {}", metrics.duration.modified);
println!("Convexity: {}", metrics.convexity);
println!("DV01: {}", metrics.dv01);

// Estimate price change for 50bp yield increase
let price_change = RiskCalculator::estimate_price_change(
    &metrics,
    result.clean_price,
    dec!(0.005),
);
```

## Bond Types

### FixedBond

Standard fixed coupon bond with:
- Periodic coupon payments
- Principal at maturity
- Configurable day count conventions

### ZeroCouponBond

Discount bond with:
- No periodic coupons
- Single payment at maturity
- Issued below par

## License

MIT OR Apache-2.0
