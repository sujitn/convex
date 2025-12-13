# Market Conventions Reference

Currency-specific day counts, settlement, compounding, and instrument conventions for post-LIBOR markets.

## Day Count Conventions

### ACT/360

Used: USD, EUR money markets and swaps

```rust
fn act_360(start: Date, end: Date) -> f64 {
    (end - start).days() as f64 / 360.0
}
```

### ACT/365 Fixed

Used: GBP (all instruments)

```rust
fn act_365_fixed(start: Date, end: Date) -> f64 {
    (end - start).days() as f64 / 365.0
}
```

### ACT/ACT ICMA

Used: Bond yield calculations (most markets)

```rust
fn act_act_icma(
    start: Date,
    end: Date,
    coupon_freq: u32,
    ref_start: Date,
    ref_end: Date,
) -> f64 {
    let actual_days = (end - start).days() as f64;
    let ref_period_days = (ref_end - ref_start).days() as f64;
    actual_days / (ref_period_days * coupon_freq as f64)
}
```

### ACT/ACT ISDA

Used: Swap fixed legs (some markets)

```rust
fn act_act_isda(start: Date, end: Date) -> f64 {
    let mut dcf = 0.0;
    let mut current = start;
    
    while current.year() < end.year() {
        let year_end = Date::new(current.year(), 12, 31);
        let days_in_year = if is_leap_year(current.year()) { 366.0 } else { 365.0 };
        dcf += (year_end - current).days() as f64 / days_in_year;
        current = Date::new(current.year() + 1, 1, 1);
    }
    
    let days_in_final_year = if is_leap_year(end.year()) { 366.0 } else { 365.0 };
    dcf += (end - current).days() as f64 / days_in_final_year;
    dcf
}
```

### 30/360 (Bond Basis)

Used: USD corporate bonds, EUR bonds

```rust
fn thirty_360(start: Date, end: Date) -> f64 {
    let (y1, m1, mut d1) = (start.year(), start.month(), start.day());
    let (y2, m2, mut d2) = (end.year(), end.month(), end.day());
    
    // US convention adjustments
    if d1 == 31 { d1 = 30; }
    if d2 == 31 && d1 >= 30 { d2 = 30; }
    
    let days = 360 * (y2 - y1) + 30 * (m2 - m1) + (d2 - d1);
    days as f64 / 360.0
}
```

### 30E/360 (Eurobond)

```rust
fn thirty_e_360(start: Date, end: Date) -> f64 {
    let (y1, m1, mut d1) = (start.year(), start.month(), start.day());
    let (y2, m2, mut d2) = (end.year(), end.month(), end.day());
    
    if d1 == 31 { d1 = 30; }
    if d2 == 31 { d2 = 30; }
    
    let days = 360 * (y2 - y1) + 30 * (m2 - m1) + (d2 - d1);
    days as f64 / 360.0
}
```

---

## USD Conventions

### SOFR OIS Swaps

| Parameter | Value |
|-----------|-------|
| Reference Rate | SOFR (Secured Overnight Financing Rate) |
| Day Count (Both Legs) | ACT/360 |
| Payment Frequency | Annual |
| Compounding | Daily compounding in arrears |
| Settlement | T+2 |
| Business Days | New York |
| Holiday Calendar | USNY (Federal Reserve) |

### USD Treasury Bonds

| Parameter | Value |
|-----------|-------|
| Day Count (Yield) | ACT/ACT ICMA |
| Day Count (Accrued) | ACT/ACT |
| Coupon Frequency | Semi-annual |
| Settlement | T+1 |
| Compounding | Semi-annual |
| Business Days | New York |

### USD Corporate Bonds

| Parameter | Value |
|-----------|-------|
| Day Count (Yield) | 30/360 |
| Day Count (Accrued) | 30/360 |
| Coupon Frequency | Semi-annual |
| Settlement | T+2 |
| Compounding | Semi-annual |

---

## EUR Conventions

### €STR OIS Swaps

| Parameter | Value |
|-----------|-------|
| Reference Rate | €STR (Euro Short-Term Rate) |
| Day Count (Both Legs) | ACT/360 |
| Payment Frequency | Annual (>1Y), single (≤1Y) |
| Compounding | Daily compounding in arrears |
| Settlement | T+2 |
| Business Days | TARGET |
| Holiday Calendar | TARGET2 |

### EURIBOR Swaps (Projection)

| Parameter | Value |
|-----------|-------|
| Reference Rate | EURIBOR (3M/6M) |
| Fixed Leg Day Count | 30/360 |
| Float Leg Day Count | ACT/360 |
| Fixed Payment | Annual |
| Float Payment | 3M or 6M (matching tenor) |
| Settlement | T+2 |

### German Bunds

| Parameter | Value |
|-----------|-------|
| Day Count (Yield) | ACT/ACT ICMA |
| Day Count (Accrued) | ACT/ACT |
| Coupon Frequency | Annual |
| Settlement | T+2 |
| Compounding | Annual |
| Business Days | Frankfurt |

### EUR Corporate Bonds

| Parameter | Value |
|-----------|-------|
| Day Count | ACT/ACT or 30E/360 |
| Coupon Frequency | Annual |
| Settlement | T+2 |

---

## GBP Conventions

### SONIA OIS Swaps

| Parameter | Value |
|-----------|-------|
| Reference Rate | SONIA (Sterling Overnight Index Average) |
| Day Count (Both Legs) | ACT/365 Fixed |
| Payment Frequency | Annual |
| Compounding | Daily compounding in arrears |
| Settlement | T+0 (same day) |
| Payment Lag | 1 business day |
| Lookback | 5 banking days (shift without weight) |
| Business Days | London |
| Holiday Calendar | GBLO |

### UK Gilts

| Parameter | Value |
|-----------|-------|
| Day Count (Yield) | ACT/ACT ICMA |
| Day Count (Accrued) | ACT/ACT |
| Coupon Frequency | Semi-annual |
| Settlement | T+1 |
| Compounding | Semi-annual |
| Ex-Dividend Period | 7 business days before coupon |

**Ex-Dividend Handling** (Post July 31, 1998):
```rust
fn gilt_accrued(
    settlement: Date,
    prev_coupon: Date,
    next_coupon: Date,
    coupon_rate: f64,
    face_value: f64,
) -> f64 {
    let ex_div_date = next_coupon.subtract_business_days(7, Calendar::GBLO);
    
    if settlement >= ex_div_date {
        // Ex-dividend: negative accrued
        let days_to_coupon = (next_coupon - settlement).days() as f64;
        let period_days = (next_coupon - prev_coupon).days() as f64;
        -coupon_rate / 2.0 * face_value * days_to_coupon / period_days
    } else {
        // Cum-dividend: positive accrued
        let days_from_coupon = (settlement - prev_coupon).days() as f64;
        let period_days = (next_coupon - prev_coupon).days() as f64;
        coupon_rate / 2.0 * face_value * days_from_coupon / period_days
    }
}
```

### GBP Corporate Bonds

| Parameter | Value |
|-----------|-------|
| Day Count | ACT/365 Fixed |
| Coupon Frequency | Annual or Semi-annual |
| Settlement | T+2 |

---

## Settlement Date Calculations

### Business Day Conventions

```rust
pub enum BusinessDayConvention {
    Following,           // Next business day
    ModifiedFollowing,   // Next unless different month, then previous
    Preceding,           // Previous business day
    ModifiedPreceding,   // Previous unless different month, then next
    Unadjusted,          // No adjustment
}

fn adjust_date(date: Date, conv: BusinessDayConvention, cal: &Calendar) -> Date {
    if cal.is_business_day(date) {
        return date;
    }
    
    match conv {
        BusinessDayConvention::Following => cal.next_business_day(date),
        BusinessDayConvention::ModifiedFollowing => {
            let next = cal.next_business_day(date);
            if next.month() != date.month() {
                cal.prev_business_day(date)
            } else {
                next
            }
        }
        BusinessDayConvention::Preceding => cal.prev_business_day(date),
        BusinessDayConvention::ModifiedPreceding => {
            let prev = cal.prev_business_day(date);
            if prev.month() != date.month() {
                cal.next_business_day(date)
            } else {
                prev
            }
        }
        BusinessDayConvention::Unadjusted => date,
    }
}
```

### Spot Date Calculation

```rust
fn spot_date(trade_date: Date, currency: Currency, calendar: &Calendar) -> Date {
    let spot_lag = match currency {
        Currency::GBP => 0,  // T+0
        Currency::USD | Currency::EUR => 2,  // T+2
    };
    
    let mut date = trade_date;
    let mut business_days = 0;
    
    while business_days < spot_lag {
        date = date.add_days(1);
        if calendar.is_business_day(date) {
            business_days += 1;
        }
    }
    date
}
```

---

## Compounding Conventions

### Daily Compounding (RFR)

```rust
fn compound_rfr(
    rates: &[(Date, f64)],  // Daily fixing rates
    start: Date,
    end: Date,
    day_count: DayCount,
) -> f64 {
    let mut compounded = 1.0;
    
    for (date, rate) in rates {
        if *date >= start && *date < end {
            let dcf = day_count.year_fraction(*date, date.add_days(1));
            compounded *= 1.0 + rate * dcf;
        }
    }
    
    let total_dcf = day_count.year_fraction(start, end);
    (compounded - 1.0) / total_dcf
}
```

### Lookback with Observation Shift (SONIA)

```rust
fn sonia_compounded_rate(
    fixings: &BTreeMap<Date, f64>,
    accrual_start: Date,
    accrual_end: Date,
    lookback_days: u32,
    calendar: &Calendar,
) -> f64 {
    let obs_start = calendar.add_business_days(accrual_start, -(lookback_days as i32));
    let obs_end = calendar.add_business_days(accrual_end, -(lookback_days as i32));
    
    let mut compounded = 1.0;
    let mut current = obs_start;
    
    while current < obs_end {
        let rate = fixings.get(&current).expect("Missing fixing");
        let dcf = 1.0 / 365.0;  // ACT/365 Fixed, single day
        compounded *= 1.0 + rate * dcf;
        current = calendar.next_business_day(current.add_days(1));
    }
    
    let total_dcf = (accrual_end - accrual_start).days() as f64 / 365.0;
    (compounded - 1.0) / total_dcf
}
```

---

## Bloomberg-Specific Quirks

### Street Convention Yields

Hybrid compounding: semi-annual except final coupon period uses simple interest.

```rust
fn street_convention_yield(
    price: f64,
    cash_flows: &[(Date, f64)],
    settlement: Date,
) -> f64 {
    // Newton-Raphson with Street convention discounting
    let mut y = initial_guess(price, cash_flows);
    
    for _ in 0..100 {
        let (pv, duration) = street_pv_and_duration(y, cash_flows, settlement);
        let error = pv - price;
        if error.abs() < 1e-10 { break; }
        y += error / duration;
    }
    y
}

fn street_discount_factor(
    y: f64,
    t: f64,
    settlement: Date,
    next_coupon: Date,
) -> f64 {
    let days_to_next = (next_coupon - settlement).days() as f64;
    let period_days = 182.5;  // Approximate semi-annual
    
    if days_to_next < period_days {
        // Final period: simple interest
        1.0 / (1.0 + y * t)
    } else {
        // Normal: semi-annual compounding
        1.0 / (1.0 + y / 2.0).powf(2.0 * t)
    }
}
```

### Curve Source Codes

| Code | Curve |
|------|-------|
| S23 | USD SOFR |
| S45 | EUR €STR |
| S42 | GBP SONIA |
| 8 | Custom user curve |

---

## Data Provider Abstraction

```rust
pub trait MarketDataProvider {
    fn settlement_date(&self) -> Date;
    fn spot_curve(&self, ccy: Currency) -> Result<YieldCurve, Error>;
    fn swap_curve(&self, ccy: Currency) -> Result<YieldCurve, Error>;
    fn government_curve(&self, ccy: Currency) -> Result<YieldCurve, Error>;
    fn volatility_surface(&self, ccy: Currency) -> Result<VolSurface, Error>;
    
    // Individual rate queries
    fn ois_rate(&self, ccy: Currency, tenor: Tenor) -> Result<f64, Error>;
    fn government_yield(&self, ccy: Currency, tenor: Tenor) -> Result<f64, Error>;
    fn swaption_vol(&self, ccy: Currency, expiry: Tenor, tenor: Tenor) -> Result<f64, Error>;
}

// Implementations for different providers
pub struct BloombergProvider { /* B-PIPE connection */ }
pub struct RefinitivProvider { /* TREP connection */ }
pub struct FileProvider { /* CSV/JSON files */ }
pub struct MockProvider { /* Test data */ }
```

---

## Quick Reference Tables

### Day Count by Instrument Type

| Instrument | USD | EUR | GBP |
|------------|-----|-----|-----|
| OIS Swap | ACT/360 | ACT/360 | ACT/365F |
| Government Bond | ACT/ACT | ACT/ACT | ACT/ACT |
| Corporate Bond | 30/360 | 30E/360 | ACT/365F |
| Money Market | ACT/360 | ACT/360 | ACT/365F |

### Settlement Days

| Instrument | USD | EUR | GBP |
|------------|-----|-----|-----|
| Government Bond | T+1 | T+2 | T+1 |
| Corporate Bond | T+2 | T+2 | T+2 |
| OIS Swap | T+2 | T+2 | T+0 |

### Coupon Frequency

| Instrument | USD | EUR | GBP |
|------------|-----|-----|-----|
| Government Bond | Semi-annual | Annual | Semi-annual |
| Corporate Bond | Semi-annual | Annual | Varies |
