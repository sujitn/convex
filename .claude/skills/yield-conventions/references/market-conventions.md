# Market Conventions & Presets

## Design Principle

**MarketPreset** serves two purposes:
1. **Bond Creation** - Provides defaults when creating bonds for a market
2. **Validation** - Verifies a bond matches expected market conventions

The bond always owns its conventions. Presets don't override.

## MarketPreset Structure

```rust
pub struct MarketPreset {
    /// Human-readable name
    pub name: &'static str,
    
    /// Expected day count convention
    pub day_count: DayCountConvention,
    
    /// Expected coupon frequency
    pub frequency: Frequency,
    
    /// Settlement days (T+n)
    pub settlement_days: u32,
    
    /// Yield calculation method
    pub yield_method: YieldMethod,
    
    /// Money market threshold (days)
    pub money_market_threshold: Option<u32>,
    
    /// Has ex-dividend period
    pub ex_dividend_days: Option<u32>,
}

impl MarketPreset {
    /// Get yield calculator config
    pub fn yield_config(&self) -> YieldCalculatorConfig {
        YieldCalculatorConfig {
            method: self.yield_method,
            money_market_threshold: self.money_market_threshold,
            tolerance: 1e-10,
        }
    }
    
    /// Validate bond matches this market
    pub fn validate<B: Bond>(&self, bond: &B) -> Result<(), ConventionMismatch> {
        if bond.day_count().name() != self.day_count.name() {
            return Err(ConventionMismatch::DayCount {
                expected: self.day_count,
                actual: bond.day_count().name(),
            });
        }
        if bond.coupon_frequency() != self.frequency {
            return Err(ConventionMismatch::Frequency {
                expected: self.frequency,
                actual: bond.coupon_frequency(),
            });
        }
        Ok(())
    }
}
```

## Preset Definitions

### United States

```rust
pub const US_TREASURY: MarketPreset = MarketPreset {
    name: "US Treasury",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::SemiAnnual,
    settlement_days: 1,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: Some(182),
    ex_dividend_days: None,
};

pub const US_CORPORATE: MarketPreset = MarketPreset {
    name: "US Corporate",
    day_count: DayCountConvention::Thirty360Us,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: Some(182),
    ex_dividend_days: None,
};

pub const US_TBILL: MarketPreset = MarketPreset {
    name: "US T-Bill",
    day_count: DayCountConvention::Act360,
    frequency: Frequency::None,  // Zero coupon
    settlement_days: 1,
    yield_method: YieldMethod::Discount,
    money_market_threshold: None,
    ex_dividend_days: None,
};
```

### United Kingdom

```rust
pub const UK_GILT: MarketPreset = MarketPreset {
    name: "UK Gilt",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::SemiAnnual,
    settlement_days: 1,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: Some(7),  // 7 business days
};
```

### Eurozone

```rust
pub const GERMAN_BUND: MarketPreset = MarketPreset {
    name: "German Bund",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::Annual,  // KEY: Annual, not semi-annual
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};

pub const FRENCH_OAT: MarketPreset = MarketPreset {
    name: "French OAT",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::Annual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};

pub const ITALIAN_BTP: MarketPreset = MarketPreset {
    name: "Italian BTP",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::SemiAnnual,  // Like US, unlike DE/FR
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};
```

### Asia-Pacific

```rust
pub const JAPANESE_JGB: MarketPreset = MarketPreset {
    name: "Japanese JGB",
    day_count: DayCountConvention::Act365Fixed,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};

// Variant for simple yield quotation
pub const JAPANESE_JGB_SIMPLE: MarketPreset = MarketPreset {
    name: "Japanese JGB (Simple)",
    day_count: DayCountConvention::Act365Fixed,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Simple,
    money_market_threshold: None,
    ex_dividend_days: None,
};

pub const AUSTRALIAN_GOVT: MarketPreset = MarketPreset {
    name: "Australian Government",
    day_count: DayCountConvention::ActActIcma,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: None,
    ex_dividend_days: None,
};
```

### Canada

```rust
pub const CANADIAN_GOVT: MarketPreset = MarketPreset {
    name: "Canadian Government",
    day_count: DayCountConvention::Act365Fixed,
    frequency: Frequency::SemiAnnual,
    settlement_days: 2,
    yield_method: YieldMethod::Compounded,
    money_market_threshold: Some(365),  // Note: 365, not 182
    ex_dividend_days: None,
};
```

## Summary Table

| Preset | Day Count | Freq | Settle | MM Thresh | Ex-Div |
|--------|-----------|------|--------|-----------|--------|
| US_TREASURY | ACT/ACT ICMA | Semi | T+1 | 182 | - |
| US_CORPORATE | 30/360 US | Semi | T+2 | 182 | - |
| US_TBILL | ACT/360 | - | T+1 | - | - |
| UK_GILT | ACT/ACT ICMA | Semi | T+1 | - | 7 days |
| GERMAN_BUND | ACT/ACT ICMA | Annual | T+2 | - | - |
| FRENCH_OAT | ACT/ACT ICMA | Annual | T+2 | - | - |
| ITALIAN_BTP | ACT/ACT ICMA | Semi | T+2 | - | - |
| JAPANESE_JGB | ACT/365F | Semi | T+2 | - | - |
| AUSTRALIAN_GOVT | ACT/ACT ICMA | Semi | T+2 | - | - |
| CANADIAN_GOVT | ACT/365F | Semi | T+2 | 365 | - |

## Usage Examples

### Creating a Bond with Preset Defaults

```rust
let bond = FixedRateBond::builder()
    .with_preset(&US_CORPORATE)  // Sets day_count, frequency
    .coupon_rate(dec!(5.0))
    .maturity(date!(2030-06-15))
    .build()?;
```

### Validating a Bond

```rust
let bond = load_bond_from_somewhere();

// Ensure it matches US Corporate conventions
US_CORPORATE.validate(&bond)?;

// Get appropriate yield calculator
let calc = YieldCalculator::new(US_CORPORATE.yield_config());
let ytm = calc.yield_from_price(&bond, settlement, price)?;
```

### Comparing Yields Across Markets

```rust
// Same bond priced in different yield conventions
let bond = create_test_bond();

// US convention (semi-annual compounding)
let us_calc = YieldCalculator::new(US_TREASURY.yield_config());
let us_ytm = us_calc.yield_from_price(&bond, settlement, price)?;

// German convention (annual compounding)  
let de_calc = YieldCalculator::new(GERMAN_BUND.yield_config());
let de_ytm = de_calc.yield_from_price(&bond, settlement, price)?;

// Convert for comparison
let us_annual = convert_rate(us_ytm, SemiAnnual, Annual)?;
assert_close!(us_annual, de_ytm);  // Should match
```

## Day Count Convention Details

### 30/360 US Month-End Rules

```rust
fn thirty_360_us(d1: Date, d2: Date) -> i32 {
    let (mut day1, mut day2) = (d1.day(), d2.day());
    
    // Rule 1: If D1 is last day of Feb → D1 = 30
    if is_last_day_of_feb(d1) { day1 = 30; }
    // Rule 2: If D1 = 31 → D1 = 30
    else if day1 == 31 { day1 = 30; }
    
    // Rule 3: If D2 is last day of Feb AND D1 was Feb → D2 = 30
    if is_last_day_of_feb(d2) && is_last_day_of_feb(d1) { day2 = 30; }
    // Rule 4: If D2 = 31 AND D1 ≥ 30 → D2 = 30
    else if day2 == 31 && day1 >= 30 { day2 = 30; }
    
    360 * (d2.year() - d1.year()) 
        + 30 * (d2.month() as i32 - d1.month() as i32) 
        + (day2 as i32 - day1 as i32)
}
```

### ACT/ACT ICMA (Bond Basis)

```rust
fn act_act_icma(
    start: Date,
    end: Date,
    period_start: Date,
    period_end: Date,
    frequency: Frequency,
) -> Decimal {
    let actual_days = (end - start).num_days();
    let period_days = (period_end - period_start).num_days();
    let freq = frequency.periods_per_year();
    
    Decimal::from(actual_days) / (Decimal::from(freq) * Decimal::from(period_days))
}
```

### Year Basis by Day Count

| Day Count | Year Basis | Notes |
|-----------|------------|-------|
| ACT/360 | 360 | Money market |
| ACT/365F | 365 | UK, Japan, Canada |
| ACT/ACT | Actual | Leap year aware |
| 30/360 | 360 | US Corporate |
| 30E/360 | 360 | Eurobond |
