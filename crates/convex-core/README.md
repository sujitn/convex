# convex-core

Core types, traits, and abstractions for the Convex fixed income analytics library.

## Overview

`convex-core` provides the foundational building blocks used throughout Convex:

- **Types**: Domain-specific types like `Date`, `Price`, `Yield`, `Currency`, `CashFlow`
- **Day Count Conventions**: Industry-standard day count fraction calculations
- **Business Day Calendars**: Holiday calendars for different markets with O(1) lookups
- **Dynamic Calendars**: Load calendars from JSON or build custom calendars at runtime
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

High-performance bitmap-based calendars with O(1) holiday lookups:

#### Built-in Calendars

| Calendar | Description | Holidays |
|----------|-------------|----------|
| `SIFMACalendar` | US fixed income | NY, MLK, Presidents', Good Friday, Memorial, Juneteenth, July 4, Labor, Columbus, Veterans, Thanksgiving, Christmas |
| `USGovernmentCalendar` | US Treasury | Same as SIFMA |
| `Target2Calendar` | Eurozone payments | New Year's, Good Friday, Easter Monday, May Day, Christmas, Boxing Day |
| `UKCalendar` | UK bank holidays | New Year's, Good Friday, Easter Monday, May holidays, Summer holiday, Christmas, Boxing Day + royal events |
| `JapanCalendar` | Japan holidays | New Year's (1-3), Coming of Age, Foundation Day, Emperor's Birthday, Equinoxes, Golden Week, Marine Day, Mountain Day, Respect for Aged, Culture Day, Labour Thanksgiving |

```rust
use convex_core::calendars::{Calendar, SIFMACalendar, BusinessDayConvention};
use convex_core::types::Date;

let cal = SIFMACalendar::new();
let date = Date::from_ymd(2025, 1, 1).unwrap(); // New Year's Day

assert!(!cal.is_business_day(date));

// Adjust to next business day
let adjusted = cal.adjust(date, BusinessDayConvention::Following).unwrap();

// Calculate settlement date (T+2)
let trade_date = Date::from_ymd(2025, 1, 15).unwrap();
let settle = cal.settlement_date(trade_date, 2);
```

#### Dynamic Calendars

Load calendars from JSON or build custom calendars at runtime:

```rust
use convex_core::calendars::{DynamicCalendar, WeekendType, Calendar};
use convex_core::types::Date;

// Create from JSON string
let json = r#"{
    "name": "My Calendar",
    "weekend": "SaturdaySunday",
    "holidays": ["2025-01-01", "2025-12-25"]
}"#;
let cal = DynamicCalendar::from_json(json).unwrap();

// Create from JSON file
let cal = DynamicCalendar::from_json_file("holidays.json").unwrap();

// Create from date list
let holidays = vec![
    Date::from_ymd(2025, 1, 1).unwrap(),
    Date::from_ymd(2025, 12, 25).unwrap(),
];
let cal = DynamicCalendar::from_dates("Custom", WeekendType::SaturdaySunday, holidays);

// Load from external source (database, API, etc.)
let cal = DynamicCalendar::from_loader(
    "Database Calendar",
    WeekendType::SaturdaySunday,
    2020,
    2030,
    |year| fetch_holidays_from_db(year),
);
```

#### Custom Calendar Builder

Build calendars with complex rules using the fluent builder API:

```rust
use convex_core::calendars::{CustomCalendarBuilder, WeekendType, Calendar};
use chrono::Weekday;

let cal = CustomCalendarBuilder::new("Trading Calendar")
    .weekend(WeekendType::SaturdaySunday)
    .year_range(2020, 2030)
    // Fixed holidays
    .add_fixed_holiday(1, 1)           // New Year's Day
    .add_fixed_holiday_observed(7, 4)  // July 4th with weekend observation
    .add_fixed_holiday_from(6, 19, 2021) // Juneteenth (since 2021)
    // Floating holidays
    .add_nth_weekday(1, Weekday::Mon, 3)  // MLK Day (3rd Monday in January)
    .add_nth_weekday(2, Weekday::Mon, 3)  // Presidents' Day
    .add_last_weekday(5, Weekday::Mon)    // Memorial Day
    .add_nth_weekday(9, Weekday::Mon, 1)  // Labor Day
    .add_nth_weekday(11, Weekday::Thu, 4) // Thanksgiving
    // Easter-based holidays
    .add_good_friday()
    .add_easter_monday()
    // Custom generator for special cases
    .add_custom(|year| {
        // Add company-specific holidays
        vec![chrono::NaiveDate::from_ymd_opt(year, 12, 24).unwrap()]
    })
    .build();
```

#### Dynamic Modification

Modify calendars at runtime:

```rust
use convex_core::calendars::{DynamicCalendar, SIFMACalendar, WeekendType, Calendar};

let mut cal = DynamicCalendar::new("Custom", WeekendType::SaturdaySunday);

// Add/remove individual holidays
cal.add_holiday(Date::from_ymd(2025, 3, 15).unwrap());
cal.remove_holiday(Date::from_ymd(2025, 3, 15).unwrap());

// Add multiple holidays
cal.add_holidays(vec![
    Date::from_ymd(2025, 1, 1).unwrap(),
    Date::from_ymd(2025, 12, 25).unwrap(),
]);

// Merge with another calendar
let sifma = SIFMACalendar::new();
cal.merge_from(&sifma);
```

#### JSON Import/Export

```rust
use convex_core::calendars::{DynamicCalendar, CalendarData, WeekendType};

// Build CalendarData programmatically
let data = CalendarData::new("My Calendar")
    .with_weekend(WeekendType::SaturdaySunday)
    .with_holiday("2025-01-01")
    .with_holidays(vec!["2025-12-25", "2025-12-26"]);

let cal = data.build().unwrap();

// Export to JSON
let json = cal.to_json().unwrap();
cal.to_json_file("output.json").unwrap();
```

#### Joint Calendars

Combine multiple calendars for cross-border transactions:

```rust
use convex_core::calendars::{JointCalendar, SIFMACalendar, Target2Calendar, Calendar};

let us_eur = JointCalendar::new(vec![
    Box::new(SIFMACalendar::new()),
    Box::new(Target2Calendar::new()),
]);

// A date is a business day only if it's a business day in ALL calendars
let date = Date::from_ymd(2025, 5, 1).unwrap(); // May Day (TARGET2 holiday)
assert!(!us_eur.is_business_day(date));
```

### Performance

All calendars use bitmap storage for O(1) holiday lookups:

| Operation | Time | Memory |
|-----------|------|--------|
| `is_business_day()` | < 10 ns | - |
| `is_holiday()` | < 10 ns | - |
| Calendar initialization | ~1 ms | ~12 KB |

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
convex-core = { path = "../convex-core" }
```

## License

MIT OR Apache-2.0
