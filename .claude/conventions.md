# Convex Coding Conventions

## Type System

### Use Newtypes - Never Raw f64 in Public APIs

```rust
// ✅ CORRECT - Type-safe
pub fn calculate_yield(price: CleanPrice, settlement: Date) -> Result<Yield, Error>

// ❌ WRONG - Unsafe
pub fn calculate_yield(price: f64, settlement: Date) -> Result<f64, Error>
```

### Newtype Pattern

```rust
use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Yield(Decimal);

impl Yield {
    pub fn from_percent(pct: f64) -> Self {
        Self(Decimal::from_f64(pct / 100.0).unwrap_or(Decimal::ZERO))
    }
    
    pub fn from_decimal(dec: Decimal) -> Self {
        Self(dec)
    }
    
    #[inline]
    pub fn as_decimal(&self) -> Decimal {
        self.0
    }
    
    #[inline]
    pub fn as_percent(&self) -> f64 {
        (self.0 * dec!(100)).to_f64().unwrap_or(0.0)
    }
}

impl std::fmt::Display for Yield {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.6}%", self.as_percent())
    }
}
```

### Decimal for Financial Calculations

```rust
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

// ✅ CORRECT
let price = dec!(99.875);
let coupon = dec!(5.0) / dec!(100);

// ❌ WRONG - Floating point errors
let price = 99.875_f64;
```

---

## Error Handling

### Module-Specific Error Enums

```rust
#[derive(Debug, thiserror::Error)]
pub enum YieldSolverError {
    #[error("solver did not converge after {iterations} iterations (last: {last_yield:.6}%, error: {error:.2e})")]
    NoConvergence {
        iterations: u32,
        last_yield: f64,
        error: f64,
    },
    
    #[error("invalid price {price}: must be positive")]
    InvalidPrice { price: Decimal },
    
    #[error("no cash flows after settlement {settlement}")]
    NoCashFlows { settlement: Date },
    
    #[error("day count error: {0}")]
    DayCount(#[from] DayCountError),
}
```

### Never Panic in Library Code

```rust
// ✅ CORRECT
pub fn calculate(input: Input) -> Result<Output, Error> {
    if input.is_invalid() {
        return Err(Error::InvalidInput { ... });
    }
    Ok(compute(input))
}

// ❌ WRONG
pub fn calculate(input: Input) -> Output {
    assert!(input.is_valid());
    input.value.unwrap()
}
```

---

## Documentation

### Every Public Item Must Have Docs

```rust
/// Calculates the yield-to-maturity for a bond given its clean price.
///
/// Uses Newton-Raphson iteration with Bloomberg's sequential roll-forward
/// methodology for money market instruments.
///
/// # Arguments
///
/// * `bond` - The bond instrument
/// * `settlement` - Settlement date
/// * `clean_price` - Market clean price
///
/// # Returns
///
/// * `Ok(Yield)` - The calculated yield-to-maturity
/// * `Err(YieldSolverError)` - If calculation fails
///
/// # Example
///
/// ```
/// use convex::prelude::*;
/// use rust_decimal_macros::dec;
///
/// let bond = FixedRateBond::builder()
///     .coupon_rate(Rate::from_percent(5.0))
///     .maturity(Date::from_ymd(2030, 6, 15))
///     .build()?;
///
/// let ytm = calculate_yield(&bond, Date::today(), CleanPrice::new(dec!(98.5))?)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Bloomberg Validation
///
/// Validated against Bloomberg YAS to ±0.00001%.
pub fn calculate_yield(
    bond: &impl Bond,
    settlement: Date,
    clean_price: CleanPrice,
) -> Result<Yield, YieldSolverError> {
    // implementation
}
```

---

## Testing

### Unit Tests in Same File

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    
    #[test]
    fn test_yield_par_bond() {
        let bond = create_test_bond(dec!(5.0), date!(2030-06-15));
        let ytm = calculate_yield(&bond, date!(2025-06-15), CleanPrice::par())
            .expect("should succeed");
        
        assert!((ytm.as_percent() - 5.0).abs() < 0.001);
    }
    
    #[test]
    fn test_yield_negative_price_error() {
        let bond = create_test_bond(dec!(5.0), date!(2030-06-15));
        let result = CleanPrice::new(dec!(-5.0));
        assert!(result.is_err());
    }
}
```

### Naming Convention

```rust
#[test]
fn test_{function}_{scenario}() { }

// Examples:
fn test_yield_calculation_par_bond() { }
fn test_yield_calculation_negative_price_error() { }
fn test_z_spread_boeing_bloomberg_match() { }
```

### Bloomberg Validation

```rust
#[test]
fn test_boeing_ytm_bloomberg() {
    let bond = create_boeing_bond();
    let settlement = date!(2020-04-29);
    let price = CleanPrice::new(dec!(110.503)).unwrap();
    
    let ytm = calculate_yield(&bond, settlement, price).unwrap();
    
    assert_bloomberg_match!(ytm.as_percent(), 4.905895, 0.00001);
}
```

---

## Performance

### Use #[inline] Appropriately

```rust
// Small, hot functions
#[inline]
pub fn discount_factor(rate: Decimal, time: Decimal) -> Decimal {
    Decimal::ONE / (Decimal::ONE + rate).powd(time)
}

// Only when benchmarks prove benefit
#[inline(always)]
fn critical_hot_path() { }
```

### Avoid Allocations in Hot Paths

```rust
// ✅ Good
let sum: Decimal = items.iter().map(|x| x.value).sum();

// ❌ Bad
let values: Vec<Decimal> = items.iter().map(|x| x.value).collect();
let sum: Decimal = values.iter().sum();
```

---

## Naming

### Functions: verb_noun, snake_case

```rust
calculate_yield()
price_from_yield()
bootstrap_curve()
```

### Types: PascalCase, descriptive

```rust
FixedRateBond
YieldSolver
HolidayCalendar
```

### Constants: SCREAMING_SNAKE_CASE

```rust
pub const YIELD_TOLERANCE: Decimal = dec!(0.0000000001);
pub const MAX_ITERATIONS: u32 = 100;
```

---

## Module Organization

### mod.rs Contains Only Re-exports

```rust
//! Day count convention implementations.

mod traits;
mod actual;
mod thirty;
mod errors;

pub use traits::DayCount;
pub use actual::{Act360, Act365Fixed, ActActIcma};
pub use thirty::{Thirty360Us, Thirty360Eu};
pub use errors::DayCountError;
```

### One Concept Per File

```
daycounts/
├── mod.rs          # Re-exports only
├── traits.rs       # DayCount trait
├── actual.rs       # ACT/* implementations
├── thirty.rs       # 30/* implementations
└── errors.rs       # DayCountError
```

---

## Dependencies

### Approved Core Dependencies

```toml
[dependencies]
rust_decimal = "1.34"
rust_decimal_macros = "1.34"
chrono = { version = "0.4", default-features = false }
thiserror = "1.0"
serde = { version = "1.0", features = ["derive"] }
rayon = "1.10"
arrayvec = "0.7"

[dev-dependencies]
criterion = "0.5"
proptest = "1.4"
approx = "0.5"
```

---

## Constants

```rust
pub mod constants {
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    
    /// Newton-Raphson convergence tolerance (Bloomberg standard)
    pub const YIELD_TOLERANCE: Decimal = dec!(0.0000000001);
    
    /// Maximum solver iterations
    pub const MAX_ITERATIONS: u32 = 100;
    
    /// Bump size for numerical derivatives (1 basis point)
    pub const BUMP_SIZE: Decimal = dec!(0.0001);
    
    /// Z-spread solver tolerance
    pub const SPREAD_TOLERANCE: Decimal = dec!(0.00000001);
    
    /// Discount factor tolerance
    pub const DF_TOLERANCE: f64 = 1e-12;
}
```
