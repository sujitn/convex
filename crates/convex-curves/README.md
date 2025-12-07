# convex-curves

Yield curve construction and interpolation for the Convex fixed income analytics library.

## Overview

`convex-curves` provides comprehensive yield curve functionality:

- **Curve Types**: Zero curves, discount curves, forward curves
- **Bootstrap**: Construct curves from market instruments
- **Interpolation**: Linear, cubic spline, and parametric methods
- **Multi-Curve**: Support for multi-curve frameworks

## Features

### Building Curves

```rust
use convex_curves::prelude::*;
use convex_core::Date;
use rust_decimal_macros::dec;

let curve = ZeroCurveBuilder::new()
    .reference_date(Date::from_ymd(2025, 1, 15).unwrap())
    .add_rate(Date::from_ymd(2025, 4, 15).unwrap(), dec!(0.045))
    .add_rate(Date::from_ymd(2025, 7, 15).unwrap(), dec!(0.048))
    .add_rate(Date::from_ymd(2026, 1, 15).unwrap(), dec!(0.050))
    .interpolation(InterpolationMethod::Linear)
    .build()
    .unwrap();
```

### Getting Rates and Discount Factors

```rust
// Get zero rate at a date
let rate = curve.zero_rate_at(Date::from_ymd(2025, 6, 15).unwrap()).unwrap();

// Get discount factor
let df = curve.discount_factor_at(Date::from_ymd(2025, 6, 15).unwrap()).unwrap();
```

### Bootstrapping from Market Instruments

```rust
use convex_curves::bootstrap::{bootstrap_curve, BootstrapInstrument};

let instruments = vec![
    BootstrapInstrument::Deposit {
        maturity: Date::from_ymd(2025, 4, 1).unwrap(),
        rate: dec!(0.04),
    },
    BootstrapInstrument::Swap {
        maturity: Date::from_ymd(2027, 1, 1).unwrap(),
        rate: dec!(0.045),
        frequency: 2,
    },
];

let curve = bootstrap_curve(
    Date::from_ymd(2025, 1, 1).unwrap(),
    &instruments,
    InterpolationMethod::CubicSpline,
).unwrap();
```

### Interpolation Methods

- **Linear**: Simple linear interpolation on zero rates
- **LogLinear**: Linear interpolation on log discount factors
- **CubicSpline**: Smooth cubic spline interpolation
- **NelsonSiegel**: Parametric Nelson-Siegel model
- **Svensson**: Extended Svensson model

### Calendar Integration

Use business day calendars for curve construction and date adjustments:

```rust
use convex_core::calendars::{SIFMACalendar, Calendar, BusinessDayConvention};

let cal = SIFMACalendar::new();

// Adjust curve dates to business days
let adjusted_date = cal.adjust(maturity, BusinessDayConvention::ModifiedFollowing).unwrap();

// Calculate forward dates
let spot_date = cal.settlement_date(trade_date, 2);
```

## Curve Types

### ZeroCurve

Zero-coupon yield curve representing continuously compounded rates.

### DiscountCurve

Discount factor curve for present value calculations.

### ForwardCurve

Forward rate curve derived from zero curve.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
convex-curves = "0.1"
```

## License

This project is licensed under the MIT License - see the [LICENSE](../../LICENSE) file for details.
