# Convex Coding Conventions & Best Practices

## Rust Style Guide

### General Principles

1. **Safety First**: Minimize unsafe code, document all safety invariants
2. **Zero-Cost Abstractions**: Use traits and generics without runtime overhead
3. **Explicit Over Implicit**: Make intentions clear in code
4. **Fail Fast**: Detect errors early, provide clear error messages
5. **Performance Matters**: Every operation counts in financial calculations

### Code Formatting

```rust
// Use rustfmt with default settings
// Run before every commit: cargo fmt

// Line length: 100 characters (rustfmt default)
// Indentation: 4 spaces
// Imports: Grouped and sorted by std, external, internal
```

### Naming Conventions

```rust
// Types: PascalCase
struct BondPricer;
enum YieldConvention;
trait DayCounter;

// Functions and methods: snake_case
fn calculate_price() -> Decimal;
fn get_discount_factor(date: Date) -> f64;

// Constants: SCREAMING_SNAKE_CASE
const BASIS_POINTS_PER_PERCENT: Decimal = dec!(100);
const DAYS_PER_YEAR_ACT_360: u32 = 360;

// Modules: snake_case
mod yield_curves;
mod day_count;

// Lifetimes: lowercase, descriptive
fn process<'curve, 'bond>(curve: &'curve Curve, bond: &'bond Bond) -> Result<Price>;
```

### Type Safety Patterns

```rust
// Use newtypes for domain concepts (prevent mixing incompatible values)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BondId(Uuid);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Yield {
    value: Decimal,
    convention: YieldConvention,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    value: Decimal,
    currency: Currency,
}

// Prevent invalid states at compile time
pub struct BondBuilder {
    // Required fields
    isin: Option<String>,
    maturity: Option<Date>,
    // Optional fields with defaults
    coupon_rate: Decimal,
    frequency: Frequency,
}

impl BondBuilder {
    // Builder pattern ensures required fields are set
    pub fn build(self) -> Result<Bond, BondBuildError> {
        let isin = self.isin.ok_or(BondBuildError::MissingIsin)?;
        let maturity = self.maturity.ok_or(BondBuildError::MissingMaturity)?;
        
        Ok(Bond {
            isin,
            maturity,
            coupon_rate: self.coupon_rate,
            frequency: self.frequency,
        })
    }
}
```

### Error Handling

```rust
// Use thiserror for error definitions
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PricingError {
    #[error("Failed to calculate yield: {reason}")]
    YieldCalculationFailed { reason: String },
    
    #[error("Invalid date: {0}")]
    InvalidDate(String),
    
    #[error("Curve not found for date {date} and currency {currency}")]
    CurveNotFound { date: Date, currency: Currency },
    
    #[error("Numerical convergence failed after {iterations} iterations")]
    ConvergenceFailed { iterations: u32 },
}

// Always use Result, never panic in library code
pub fn calculate_ytm(bond: &Bond, price: Price) -> Result<Yield, PricingError> {
    // Implementation
}

// Use expect() only for programmer errors (not user input)
let value = config.get("required_field")
    .expect("Configuration must have required_field - this is a bug");

// For recoverable errors, use ? operator
pub fn price_bond(bond: &Bond, curve: &YieldCurve) -> Result<Price, PricingError> {
    let cash_flows = generate_cash_flows(bond)?;  // Propagate errors
    let pv = discount_cash_flows(&cash_flows, curve)?;
    Ok(Price::new(pv, bond.currency))
}
```

### Documentation Standards

```rust
/// Calculates the yield-to-maturity of a bond using Newton-Raphson iteration.
///
/// The yield-to-maturity is the internal rate of return of the bond's cash flows,
/// calculated using the formula:
///
/// $$P = \sum_{i=1}^{n} \frac{CF_i}{(1 + y/f)^{t_i}}$$
///
/// where:
/// - $P$ is the bond price
/// - $CF_i$ is the cash flow at time $i$
/// - $y$ is the yield-to-maturity
/// - $f$ is the compounding frequency
/// - $t_i$ is the time to cash flow $i$
///
/// # Arguments
///
/// * `bond` - The bond specification
/// * `price` - The market price of the bond
/// * `settlement_date` - The settlement date for the calculation
///
/// # Returns
///
/// The yield-to-maturity as a `Yield` struct with appropriate convention.
///
/// # Errors
///
/// Returns `PricingError::ConvergenceFailed` if the Newton-Raphson iteration
/// does not converge within 100 iterations.
///
/// # Examples
///
/// ```
/// use convex_bonds::{Bond, Price, calculate_ytm};
/// use rust_decimal_macros::dec;
///
/// let bond = Bond::builder()
///     .isin("US912828Z229")
///     .coupon_rate(dec!(2.5))
///     .maturity(Date::from_ymd(2030, 5, 15))
///     .build()?;
///
/// let price = Price::new(dec!(98.50), Currency::USD);
/// let ytm = calculate_ytm(&bond, price, Date::today())?;
/// assert!(ytm.value() > dec!(2.5)); // Price below par => yield above coupon
/// ```
///
/// # Algorithm Complexity
///
/// - Time: O(n * k) where n is number of cash flows, k is iterations (typically < 10)
/// - Space: O(n) for cash flow storage
///
/// # References
///
/// - Tuckman, B. (2011). *Fixed Income Securities*, Chapter 2
/// - Bloomberg YAS function documentation
pub fn calculate_ytm(
    bond: &Bond,
    price: Price,
    settlement_date: Date,
) -> Result<Yield, PricingError> {
    // Implementation
}
```

### Performance Best Practices

```rust
// 1. Use const for compile-time constants
const BASIS_POINTS: Decimal = dec!(10000);

// 2. Use inline for small, frequently called functions
#[inline]
pub fn basis_points_to_decimal(bps: i32) -> Decimal {
    Decimal::from(bps) / BASIS_POINTS
}

// 3. Avoid allocations in hot paths
pub fn discount_cash_flows(
    cash_flows: &[CashFlow],  // Borrow, don't take ownership
    curve: &YieldCurve,
) -> Decimal {
    // Use iterator, avoid collecting into Vec
    cash_flows.iter()
        .map(|cf| cf.amount * curve.discount_factor(cf.date))
        .sum()
}

// 4. Use iterators instead of loops
// Good
let total: Decimal = cash_flows.iter()
    .map(|cf| cf.amount)
    .sum();

// Avoid
let mut total = Decimal::ZERO;
for cf in cash_flows {
    total += cf.amount;
}

// 5. Pre-allocate when size is known
let mut results = Vec::with_capacity(bonds.len());

// 6. Use Cow for conditional cloning
use std::borrow::Cow;

pub fn normalize_isin<'a>(isin: &'a str) -> Cow<'a, str> {
    if isin.starts_with("US") {
        Cow::Borrowed(isin)
    } else {
        Cow::Owned(format!("US{}", isin))
    }
}

// 7. Use SIMD for vectorizable operations (when stable)
// For now, rely on auto-vectorization and consider packed_simd for critical paths
```

### Testing Conventions

```rust
// Unit tests in same file
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use approx::assert_relative_eq;

    #[test]
    fn test_ytm_calculation_par_bond() {
        // Arrange
        let bond = create_test_bond(dec!(5.0), Date::from_ymd(2030, 1, 1));
        let price = Price::new(dec!(100.0), Currency::USD);
        let settlement = Date::from_ymd(2025, 1, 1);

        // Act
        let ytm = calculate_ytm(&bond, price, settlement).unwrap();

        // Assert
        assert_relative_eq!(
            ytm.value().to_f64().unwrap(),
            0.05,
            epsilon = 1e-6
        );
    }

    #[test]
    fn test_ytm_negative_price_error() {
        let bond = create_test_bond(dec!(5.0), Date::from_ymd(2030, 1, 1));
        let price = Price::new(dec!(-1.0), Currency::USD);
        let settlement = Date::from_ymd(2025, 1, 1);

        let result = calculate_ytm(&bond, price, settlement);
        assert!(matches!(result, Err(PricingError::InvalidPrice { .. })));
    }
}

// Property-based tests
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn ytm_increases_when_price_decreases(
            coupon in 0.01f64..0.10f64,
            price1 in 95.0f64..100.0f64,
            price2 in 90.0f64..94.99f64
        ) {
            let bond = create_test_bond(Decimal::from_f64(coupon).unwrap(), future_date());
            let settlement = Date::today();
            
            let ytm1 = calculate_ytm(&bond, Price::from_f64(price1), settlement).unwrap();
            let ytm2 = calculate_ytm(&bond, Price::from_f64(price2), settlement).unwrap();
            
            prop_assert!(ytm2.value() > ytm1.value());
        }
    }
}

// Integration tests in tests/ directory
// tests/bond_pricing.rs
use convex_bonds::*;

#[test]
fn test_us_treasury_pricing() {
    // Full end-to-end test with real-world data
}
```

### Concurrency Patterns

```rust
// Use Rayon for data parallelism
use rayon::prelude::*;

pub fn price_portfolio(
    bonds: &[Bond],
    curve: &YieldCurve,
) -> Vec<Result<Price, PricingError>> {
    bonds.par_iter()
        .map(|bond| price_bond(bond, curve))
        .collect()
}

// Use Arc for shared ownership in multi-threaded context
use std::sync::Arc;

pub struct PricingService {
    curves: Arc<CurveCache>,  // Shared, read-only access
}

// Use RwLock for rarely-written, frequently-read data
use std::sync::RwLock;

pub struct CurveCache {
    curves: RwLock<HashMap<CurveKey, YieldCurve>>,
}

impl CurveCache {
    pub fn get(&self, key: &CurveKey) -> Option<YieldCurve> {
        self.curves.read().unwrap().get(key).cloned()
    }
    
    pub fn insert(&self, key: CurveKey, curve: YieldCurve) {
        self.curves.write().unwrap().insert(key, curve);
    }
}
```

### Module Organization

```rust
// Flat, not deeply nested
// Good: convex_bonds::instruments::fixed_rate
// Avoid: convex_bonds::instruments::bonds::government::fixed_rate

// Public API in lib.rs or mod.rs
// convex_bonds/src/lib.rs
pub mod instruments;
pub mod pricing;
pub mod risk;

pub use instruments::{Bond, BondType};
pub use pricing::{price_bond, calculate_ytm};
pub use risk::{calculate_duration, calculate_convexity};

// Re-export commonly used types
pub use convex_core::{Date, Price, Yield, Currency};

// Internal modules are private
mod internal_helpers;
```

### Trait Design

```rust
// Traits should be focused and composable
pub trait PricingEngine {
    fn price(&self, bond: &Bond, curve: &YieldCurve) -> Result<Price, PricingError>;
}

pub trait RiskCalculator {
    fn duration(&self, bond: &Bond, curve: &YieldCurve) -> Result<Decimal, PricingError>;
    fn convexity(&self, bond: &Bond, curve: &YieldCurve) -> Result<Decimal, PricingError>;
}

// Prefer associated types over generic parameters when there's one natural type
pub trait YieldCurve {
    type Point;
    
    fn discount_factor(&self, date: Date) -> Self::Point;
    fn zero_rate(&self, date: Date) -> Self::Point;
}

// Use trait bounds judiciously
pub fn price_and_risk<E, R>(
    bond: &Bond,
    curve: &YieldCurve,
    pricing_engine: &E,
    risk_calculator: &R,
) -> Result<(Price, Decimal, Decimal), PricingError>
where
    E: PricingEngine,
    R: RiskCalculator,
{
    let price = pricing_engine.price(bond, curve)?;
    let duration = risk_calculator.duration(bond, curve)?;
    let convexity = risk_calculator.convexity(bond, curve)?;
    Ok((price, duration, convexity))
}
```

### Async Patterns (Future Consideration)

```rust
// If async support is needed, use tokio
// Keep sync API as primary, provide async wrappers

#[cfg(feature = "async")]
pub mod async_api {
    use super::*;
    use tokio::task;

    pub async fn price_bond_async(
        bond: Bond,
        curve: YieldCurve,
    ) -> Result<Price, PricingError> {
        task::spawn_blocking(move || {
            price_bond(&bond, &curve)
        }).await
        .map_err(|e| PricingError::TaskFailed(e.to_string()))?
    }
}
```

### Optimization Guidelines

```rust
// 1. Profile before optimizing
// Use cargo flamegraph or perf

// 2. Use criterion for benchmarks
use criterion::{criterion_group, criterion_main, Criterion};

fn benchmark_ytm(c: &mut Criterion) {
    let bond = create_benchmark_bond();
    let price = Price::new(dec!(98.5), Currency::USD);
    
    c.bench_function("ytm_calculation", |b| {
        b.iter(|| calculate_ytm(&bond, price, Date::today()))
    });
}

criterion_group!(benches, benchmark_ytm);
criterion_main!(benches);

// 3. Use release mode for realistic benchmarks
// cargo bench

// 4. Consider CPU features
// [profile.release]
// codegen-units = 1
// lto = "fat"
// opt-level = 3

// 5. Use target-cpu=native for maximum performance
// RUSTFLAGS="-C target-cpu=native" cargo build --release
```

### FFI Safety

```rust
// For C API bindings
#[no_mangle]
pub extern "C" fn convex_calculate_ytm(
    bond_ptr: *const Bond,
    price: f64,
    out_ytm: *mut f64,
) -> i32 {
    // Validate pointers
    if bond_ptr.is_null() || out_ytm.is_null() {
        return -1; // Error code
    }
    
    // SAFETY: Caller guarantees valid pointer and lifetime
    let bond = unsafe { &*bond_ptr };
    
    match calculate_ytm(bond, Price::from_f64(price), Date::today()) {
        Ok(ytm) => {
            // SAFETY: out_ytm is valid for write
            unsafe { *out_ytm = ytm.value().to_f64().unwrap_or(0.0) };
            0 // Success
        }
        Err(_) => -2, // Calculation error
    }
}
```

### Logging and Observability

```rust
// Use log facade for flexibility
use log::{debug, info, warn, error};

pub fn price_bond(bond: &Bond, curve: &YieldCurve) -> Result<Price, PricingError> {
    debug!("Pricing bond ISIN: {}", bond.isin);
    
    let cash_flows = generate_cash_flows(bond)?;
    debug!("Generated {} cash flows", cash_flows.len());
    
    let price = discount_cash_flows(&cash_flows, curve)?;
    info!("Bond {} priced at {}", bond.isin, price.value);
    
    Ok(price)
}

// For production, consider structured logging
#[cfg(feature = "tracing")]
use tracing::{instrument, event, Level};

#[instrument(skip(curve))]
pub fn price_bond_traced(bond: &Bond, curve: &YieldCurve) -> Result<Price, PricingError> {
    event!(Level::DEBUG, isin = %bond.isin, "Starting pricing");
    // Implementation
}
```

### Code Review Checklist

Before submitting code, ensure:

- [ ] Code compiles without warnings (`cargo clippy`)
- [ ] Formatted with rustfmt (`cargo fmt`)
- [ ] All tests pass (`cargo test`)
- [ ] Added tests for new functionality
- [ ] Documentation updated (rustdoc comments)
- [ ] No panics in library code
- [ ] Error handling is comprehensive
- [ ] Performance considerations addressed
- [ ] Public API is ergonomic
- [ ] Backwards compatibility maintained (if applicable)

---

*These conventions should be followed consistently across the entire codebase.*
