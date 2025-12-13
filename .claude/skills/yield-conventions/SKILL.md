---
name: convex-yield-conventions
description: |
  Bond yield convention implementation for the Convex pricing library. Use this skill when:
  (1) Implementing yield calculation methods (compounded, simple, discount, add-on)
  (2) Adding money market yield calculations with sequential roll-forward
  (3) Implementing Bond Equivalent Yield (BEY) with 182-day threshold
  (4) Creating market presets for bond creation and validation
  (5) Implementing compounding frequency conversions
  (6) Extending bond pricing to support global market conventions
---

# Convex Yield Conventions Skill

Extends the Convex bond pricing library with generic yield conventions.

## Core Principle: Separation of Concerns

```
Bond           → Owns: day_count, frequency, settlement rules
YieldMethod    → Controls: how to calculate yield from price
MarketPreset   → Provides: defaults for bond creation, validation
```

**The bond is the source of truth for its conventions.**
YieldConvention does NOT override the bond's day count or frequency.

## Architecture

```
convex-core/src/
├── types/
│   └── yield_method.rs      # YieldMethod enum only
└── compounding/
    ├── frequency.rs         # CompoundingFrequency enum
    └── converter.rs         # Rate conversion utilities

convex-bonds/src/
├── traits.rs                # Bond trait (owns day_count, frequency)
├── instruments/
│   └── fixed.rs             # FixedRateBond with conventions
└── pricing/
    └── yield_calculator.rs  # Uses bond's conventions

convex-yas/src/
└── presets.rs               # MarketPreset for validation/defaults
```

## Core Types

### YieldMethod (calculation approach only)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YieldMethod {
    /// Newton-Raphson: Price = Σ CF/(1+y/f)^(f×t)
    Compounded,
    /// y = (Coupon + (Redemption-Price)/Years) / Price
    Simple,
    /// y = (Face-Price)/Face × (Basis/Days) - T-Bills
    Discount,
    /// y = (Face-Price)/Price × (Basis/Days) - Money market
    AddOn,
}
```

### YieldCalculatorConfig (minimal)

```rust
pub struct YieldCalculatorConfig {
    /// Primary calculation method
    pub method: YieldMethod,
    /// Days threshold for money market switch (typically 182)
    pub money_market_threshold: Option<u32>,
    /// Solver tolerance (default: 1e-10)
    pub tolerance: f64,
}
```

### Bond Trait (owns conventions)

```rust
pub trait Bond {
    fn day_count(&self) -> &dyn DayCount;
    fn coupon_frequency(&self) -> Frequency;
    fn coupon_rate(&self) -> Decimal;
    fn maturity(&self) -> Date;
    fn face_value(&self) -> Decimal;
    fn cash_flows_from(&self, settlement: Date) -> Vec<CashFlow>;
    fn accrued_interest(&self, settlement: Date) -> Decimal;
}
```

### MarketPreset (for creation & validation)

```rust
pub struct MarketPreset {
    pub name: &'static str,
    pub day_count: DayCountConvention,
    pub frequency: Frequency,
    pub settlement_days: u32,
    pub yield_method: YieldMethod,
    pub money_market_threshold: Option<u32>,
}

impl MarketPreset {
    /// Validate bond matches this market's conventions
    pub fn validate<B: Bond>(&self, bond: &B) -> Result<(), ConventionMismatch>;
    
    /// Get yield calculator config for this market
    pub fn yield_config(&self) -> YieldCalculatorConfig;
}
```

## Market Presets

| Preset | Day Count | Frequency | Settle | MM Threshold |
|--------|-----------|-----------|--------|--------------|
| `US_TREASURY` | ACT/ACT ICMA | Semi | T+1 | 182 |
| `US_CORPORATE` | 30/360 US | Semi | T+2 | 182 |
| `US_TBILL` | ACT/360 | - | T+1 | - |
| `UK_GILT` | ACT/ACT ICMA | Semi | T+1 | - |
| `GERMAN_BUND` | ACT/ACT ICMA | Annual | T+2 | - |
| `JAPANESE_JGB` | ACT/365F | Semi | T+2 | - |

## Yield Calculator Flow

```rust
impl YieldCalculator {
    pub fn yield_from_price<B: Bond>(
        &self,
        bond: &B,
        settlement: Date,
        price: CleanPrice,
    ) -> Result<Yield, YieldError> {
        // 1. Get conventions FROM THE BOND
        let day_count = bond.day_count();
        let frequency = bond.coupon_frequency();
        
        // 2. Determine method (config only controls this)
        let days_to_mat = (bond.maturity() - settlement).num_days() as u32;
        let method = self.effective_method(days_to_mat);
        
        // 3. Calculate using bond's conventions
        match method {
            YieldMethod::Compounded => {
                self.solve_compounded(bond, settlement, price, day_count, frequency)
            }
            YieldMethod::Simple => {
                self.calc_simple(bond, settlement, price, day_count)
            }
            YieldMethod::AddOn => {
                self.calc_money_market(bond, settlement, price, day_count)
            }
            // ...
        }
    }
}
```

## Key Thresholds

**182 days** - Money market method switch for US markets
**365 days** - Money market threshold for Canadian markets

## Sequential Roll-Forward (for short-dated coupon bonds)

```
C₁ = C × (1 + y × D₁/basis)
Cᵢ = (C + Cᵢ₋₁) × (1 + y × Dᵢ/basis)
```

Where `basis` comes from the **bond's day count convention**.

## Reference Files

- `references/formulas.md` - Mathematical formulas
- `references/market-conventions.md` - Market specifications  
- `references/test-cases.md` - Bloomberg validation tests
