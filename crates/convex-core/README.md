# convex-core

Core types, traits, and abstractions for the Convex fixed income analytics library.

## Overview

`convex-core` provides the foundational building blocks used throughout Convex:

- **Types**: Domain-specific types like `Date`, `Price`, `Yield`, `Currency`, `CashFlow`
- **Day Count Conventions**: Industry-standard day count fraction calculations
- **Business Day Calendars**: Holiday calendars for different markets
- **Traits**: Core abstractions for curves, pricing engines, and risk calculators

## Features

### Type Safety

All domain concepts are represented as distinct types to prevent mixing incompatible values:

```rust
use convex_core::prelude::*;
use rust_decimal_macros::dec;

// These are different types - can't accidentally mix them
let price = Price::new(dec!(98.50), Currency::USD);
let yield_val = Yield::new(dec!(0.05), Compounding::SemiAnnual);
let spread = Spread::new(dec!(125), SpreadType::ZSpread);
```

### Day Count Conventions

Full support for industry-standard day count conventions:

- **ACT/360**: Money market instruments
- **ACT/365**: UK Gilts, AUD/NZD markets
- **30/360**: US corporate bonds
- **30E/360**: European convention
- **ACT/ACT ISDA**: Government bonds
- **ACT/ACT ICMA**: ISMA convention

```rust
use convex_core::daycounts::{DayCount, Act360, Thirty360};
use convex_core::types::Date;

let dc = Act360;
let start = Date::from_ymd(2025, 1, 1).unwrap();
let end = Date::from_ymd(2025, 7, 1).unwrap();

let year_fraction = dc.year_fraction(start, end);
```

### Business Day Calendars

Calendars for different markets with holiday detection and date adjustment:

```rust
use convex_core::calendars::{Calendar, USCalendar, BusinessDayConvention};
use convex_core::types::Date;

let cal = USCalendar;
let date = Date::from_ymd(2025, 1, 1).unwrap(); // New Year's Day

assert!(!cal.is_business_day(date));

let adjusted = cal.adjust(date, BusinessDayConvention::Following).unwrap();
```

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
convex-core = { path = "../convex-core" }
```

## License

MIT OR Apache-2.0
