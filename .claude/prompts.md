# Claude Code Prompts for Convex Development

## Essential: Session Start Prompt

**Always begin each Claude Code session with this:**

```
Please read these project files in order:
1. .claude/context.md - Domain knowledge, accuracy requirements
2. .claude/architecture.md - System design, crate structure
3. .claude/conventions.md - Coding standards (if exists)
4. .claude/memory.md - Decisions and progress

Then run: tree -L 2 src/

Confirm you understand the Convex project before we proceed.
```

---

## Core Infrastructure Prompts

### 1. Core Types

```markdown
## Task: Implement Core Domain Types in convex-core

### Pre-Implementation
Read .claude/context.md sections: Accuracy Targets, Numerical Precision

### Requirements

Implement these newtypes in `convex-core/src/types/`:

**Price Types:**
- CleanPrice (always positive, percentage of par)
- DirtyPrice (includes accrued)

**Yield Types:**
- Yield (annual, as decimal e.g., 0.05 = 5%)

**Spread Types:**
- Spread (basis points, can be negative)

**Rate Types:**
- Rate (generic: coupon, discount, forward)

**Risk Types:**
- Duration, Convexity, DV01

### Implementation Pattern

```rust
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Yield(Decimal);

impl Yield {
    pub fn from_percent(pct: f64) -> Self;
    pub fn from_decimal(dec: Decimal) -> Self;
    pub fn as_decimal(&self) -> Decimal;
    pub fn as_percent(&self) -> f64;
}
```

### Validation
- All types must use Decimal for precision
- Implement Display with appropriate precision
- Include unit tests for all conversions
- Test edge cases (zero, negative where valid)
```

### 2. Day Count Conventions

```markdown
## Task: Implement Day Count Conventions in convex-core

### Pre-Implementation
Read .claude/context.md section: Day Count Conventions

### Requirements

Implement ALL day count conventions exactly matching Bloomberg:

**ACT Family:**
- ACT/360, ACT/365F, ACT/365L
- ACT/ACT ICMA (period-based)
- ACT/ACT ISDA (year-based)
- ACT/ACT AFB (French)

**30 Family:**
- 30/360 US (with exact month-end rules)
- 30E/360 (Eurobond)
- 30E/360 ISDA
- 30/360 German

### Critical: 30/360 US Month-End Rules

```rust
pub fn thirty_360_us(d1: Date, d2: Date) -> Decimal {
    // Rule 1: If D1 is last day of Feb → D1 = 30
    // Rule 2: If D1 = 31 → D1 = 30
    // Rule 3: If D2 is last day of Feb AND D1 was adjusted → D2 = 30
    // Rule 4: If D2 = 31 AND D1 >= 30 → D2 = 30
}
```

### Tests Required
- Bloomberg-verified test cases for each convention
- Leap year edge cases
- Month-end boundaries
- Same-day (zero fraction)
- Cross-year boundaries
```

### 3. Holiday Calendars

```markdown
## Task: Implement Holiday Calendar System in convex-core

### Pre-Implementation
Read .claude/context.md section: Settlement Conventions

### Requirements

Implement in `convex-core/src/calendar/`:

**Calendar Struct:**
- Bitmap storage for O(1) lookups
- Support 1970-2100 range
- Weekend rules (Sat/Sun, Fri/Sat, etc.)

**Calendars to Implement:**
- SIFMA (US bond market)
- US Government (Treasury)
- TARGET2 (Eurozone)
- UK Bank Holidays
- Japan

**Date Roll Conventions:**
- Following, ModifiedFollowing
- Preceding, ModifiedPreceding
- Unadjusted

**Methods:**
```rust
impl HolidayCalendar {
    fn is_business_day(&self, date: Date) -> bool;
    fn adjust(&self, date: Date, convention: DateRoll) -> Date;
    fn add_business_days(&self, date: Date, days: i32) -> Date;
    fn settlement_date(&self, trade: Date, days: u32) -> Date;
}
```

### Performance Target
- is_business_day: O(1), < 10ns
```

---

## Mathematical Engine Prompts

### 4. Solvers

```markdown
## Task: Implement Root-Finding Solvers in convex-math

### Requirements

Implement in `convex-math/src/solvers/`:

**Newton-Raphson:**
- Tolerance: 1e-10 (configurable)
- Max iterations: 100
- Analytical derivative when available

**Brent's Method:**
- For Z-spread and OAS
- Guaranteed convergence
- Tolerance: 1e-10

**Hybrid Solver:**
- Start with Newton
- Fall back to Brent if diverging

```rust
pub trait Solver {
    fn solve<F, D>(
        &self,
        f: F,
        derivative: Option<D>,
        initial_guess: f64,
        bounds: Option<(f64, f64)>,
    ) -> Result<f64, SolverError>
    where
        F: Fn(f64) -> f64,
        D: Fn(f64) -> f64;
}
```

### Performance Targets
- YTM solve: < 1μs
- Z-spread solve: < 50μs
```

### 5. Interpolation Methods

```markdown
## Task: Implement Interpolation Methods in convex-math

### Pre-Implementation
Read .claude/context.md section: Interpolation Methods

### Requirements

Implement ALL interpolation methods:

**On Zero Rates:**
- Linear
- Log-Linear
- Cubic Spline (Natural)
- Monotone Convex (Hagan) ← PRODUCTION DEFAULT

**Parametric:**
- Nelson-Siegel
- Svensson

```rust
pub trait Interpolator: Send + Sync {
    fn new(x: &[f64], y: &[f64]) -> Result<Self, InterpolationError>;
    fn interpolate(&self, x: f64) -> f64;
    fn derivative(&self, x: f64) -> f64;
}
```

### Monotone Convex Implementation
Critical for production - must ensure:
- Positive forward rates
- No oscillation
- C1 continuity

### Tests Required
- Accuracy vs known analytical solutions
- Edge cases (at/near pillars)
- Derivative accuracy
- Positive forward rate validation
```

### 6. Extrapolation Methods

```markdown
## Task: Implement Extrapolation Methods in convex-math

### Requirements

**Methods:**
- Flat (constant from last point)
- Linear (slope continuation)
- Smith-Wilson (regulatory standard)

**Smith-Wilson Implementation:**
```rust
pub struct SmithWilson {
    pub ultimate_forward_rate: f64,  // e.g., 4.2% for EUR
    pub convergence_speed: f64,      // Alpha
    pub last_liquid_point: f64,      // e.g., 20Y
}
```

Must match EIOPA specification exactly for regulatory curves.

### Tests
- Convergence to UFR
- Smoothness at transition point
- Match regulatory test cases
```

---

## Curve Construction Prompts

### 7. Curve Bootstrap

```markdown
## Task: Implement Curve Bootstrapping in convex-curves

### Pre-Implementation
Read .claude/context.md section: Yield Curve Construction

### Requirements

**Curve Types:**
- Discount curve (discount factors)
- Zero curve (zero rates)
- Forward curve (instantaneous forwards)

**Bootstrap from Instruments:**
- Deposits (O/N to 12M)
- FRAs
- Interest Rate Swaps (2Y-50Y)
- OIS Swaps

**Algorithm:**
```rust
pub struct CurveBootstrapper {
    interpolation: InterpolationMethod,
    extrapolation: Extrapolation,
    solver: BootstrapSolver,
    tolerance: f64,  // 1e-12
}

impl CurveBootstrapper {
    pub fn bootstrap(
        &self,
        instruments: &[CurveInstrument],
        valuation_date: Date,
    ) -> Result<BootstrappedCurve, CurveError>;
}
```

### Validation Requirements
- All input instruments must reprice to tolerance
- Forward rates must be positive (with monotone convex)
- No arbitrage violations

### Tests
- Reprice all input instruments to 1e-8
- Compare to Bloomberg curves
- Test with different interpolation methods
```

### 8. Multi-Curve Framework

```markdown
## Task: Implement Multi-Curve Framework in convex-curves

### Requirements

**Multi-Curve Environment:**
```rust
pub struct MultiCurveEnvironment {
    discount_curve: Curve,                          // OIS
    projection_curves: HashMap<RateIndex, Curve>,   // SOFR, EURIBOR, etc.
    basis_curves: HashMap<BasisKey, Curve>,         // Basis spreads
}
```

**Rate Indices:**
- SOFR, ESTR, SONIA (overnight)
- Term SOFR, EURIBOR (term)
- Legacy LIBOR (for existing trades)

**Dual Curve Bootstrap:**
- Discount with OIS
- Project with term rate
- Handle basis consistently

### Tests
- FRA pricing under dual curve
- Swap pricing with OIS discounting
- Basis swap pricing
```

---

## Bond Pricing Prompts

### 9. Fixed Rate Bond

```markdown
## Task: Implement Fixed Rate Bond Pricing in convex-bonds

### Pre-Implementation
Read .claude/context.md section: Pricing Methodologies

### Requirements

**Bond Struct:**
```rust
pub struct FixedRateBond {
    identifiers: BondIdentifiers,
    coupon_rate: Rate,
    maturity: Date,
    issue_date: Date,
    frequency: Frequency,
    day_count: DayCountConvention,
    settlement_days: u32,
    calendar: CalendarId,
}
```

**Implement:**
- Cash flow generation
- Accrued interest calculation
- Price from yield (closed form)
- Yield from price (Newton-Raphson)
- Clean/dirty price conversion

**Bloomberg Validation:**
Use Boeing 7.5% 06/15/2025 as primary test case:
- Settlement: 04/29/2020
- Price: 110.503
- Expected YTM: 4.905895%

### Tolerance
- Yield: ±0.00001%
- Price: ±0.0001
```

### 10. US Treasury Securities

```markdown
## Task: Implement US Treasury Securities in convex-bonds

### Requirements

**Treasury Note/Bond:**
- ACT/ACT ICMA day count
- Semi-annual frequency
- T+1 settlement
- 32nds price quote parsing

**Treasury Bill:**
```rust
pub fn tbill_price_from_discount(
    discount_rate: Rate,
    settlement: Date,
    maturity: Date,
) -> CleanPrice;

pub fn tbill_bond_equivalent_yield(
    price: CleanPrice,
    settlement: Date,
    maturity: Date,
) -> Yield;
```

**Price Quote Parsing:**
```rust
// "99-16+" = 99 + 16.5/32 = 99.515625
pub fn parse_treasury_price(quote: &str) -> Result<CleanPrice, ParseError>;
```

### Validation
- Compare to Treasury Direct
- Bloomberg UST pricing
```

### 11. TIPS (Inflation-Linked)

```markdown
## Task: Implement TIPS in convex-bonds

### Requirements

**TIPS Struct:**
```rust
pub struct Tips {
    cusip: String,
    real_coupon: Rate,           // Real (not nominal) coupon
    maturity: Date,
    base_cpi: Decimal,           // Reference CPI at issue
    deflation_floor: bool,       // Usually true
}
```

**Calculations:**
- Index ratio = Reference CPI / Base CPI
- Inflation-adjusted principal
- Real yield calculation
- Breakeven inflation

### CPI Indexation
- 3-month lag
- Linear interpolation between monthly values

### Validation
- Bloomberg TIPS pricing
- Treasury inflation calculations
```

### 12. Callable Bonds

```markdown
## Task: Implement Callable Bond Pricing in convex-bonds

### Requirements

**Call Schedule:**
```rust
pub struct CallSchedule {
    features: Vec<CallFeature>,
}

pub struct CallFeature {
    first_call_date: Date,
    call_price: Decimal,
    call_frequency: Option<Frequency>,
}
```

**Yield Calculations:**
- Yield to call (each call date)
- Yield to worst (minimum of all)

**OAS Calculation:**
```rust
pub struct OASCalculator {
    rate_model: RateModel,      // Hull-White or BDT
    tree_steps: usize,          // 100+ for accuracy
    volatility: f64,            // ATM swaption vol
}
```

**Binomial Tree:**
- Build interest rate tree
- Backward induction with exercise
- Solve for OAS matching market price

### Performance Target
- OAS: < 10ms (100 step tree)
```

### 13. Municipal Bonds

```markdown
## Task: Implement Municipal Bond Pricing in convex-bonds

### Requirements

**Tax-Equivalent Yield:**
```rust
pub fn taxable_equivalent_yield(
    tax_exempt_yield: Yield,
    federal_tax_rate: Decimal,
    state_tax_rate: Option<Decimal>,
    is_amt_subject: bool,
) -> Yield;
```

**De Minimis Rule:**
- Threshold: 0.25% × years to maturity
- Affects tax treatment of discount

**Bond Types:**
- General Obligation (GO)
- Revenue
- Pre-refunded

### Validation
- EMMA data
- Bloomberg MUNI pricing
```

### 14. MBS Pass-Through

```markdown
## Task: Implement MBS Pass-Through Pricing in convex-bonds

### Requirements

**MBS Structure:**
```rust
pub struct MBSPassThrough {
    pool_number: String,
    issuer: AgencyIssuer,       // GNMA, FNMA, FHLMC
    original_balance: Decimal,
    current_factor: Decimal,
    pass_through_rate: Rate,
    wam: u32,                   // Weighted avg maturity (months)
    warm: u32,                  // Weighted avg remaining maturity
}
```

**Prepayment Models:**
```rust
pub enum PrepaymentModel {
    CPR(f64),                   // Constant prepayment rate
    SMM(f64),                   // Single monthly mortality
    PSA(f64),                   // PSA speed (100 = standard)
    Vector(Vec<f64>),           // Custom vector
}
```

**Cash Flow Projection:**
- Monthly scheduled principal
- Prepayment calculation
- Interest calculation
- Factor adjustment

**Yield Table:**
- Yield at various PSA speeds
- Price sensitivity to prepayment

### Performance Target
- Full cash flow projection: < 100ms
```

---

## Spread Calculation Prompts

### 15. Z-Spread

```markdown
## Task: Implement Z-Spread Calculation in convex-spreads

### Requirements

**Definition:**
Z-spread is the constant spread over the spot curve that reprices the bond.

```rust
pub struct ZSpreadCalculator {
    tolerance: Decimal,      // 1e-8 bps
    max_iterations: u32,     // 50
}

impl ZSpreadCalculator {
    pub fn calculate(
        &self,
        bond: &impl Bond,
        settlement: Date,
        dirty_price: Decimal,
        spot_curve: &impl SpotCurve,
    ) -> Result<Spread, SpreadError>;
}
```

**Algorithm:**
- Use Brent's method (more robust than Newton)
- Bracket between -100bps and 2000bps
- Continuous discounting

### Bloomberg Validation
Boeing bond Z-spread: 444.7 bps (±0.1 bps)
```

### 16. OAS Calculation

```markdown
## Task: Implement OAS Calculation in convex-spreads

### Requirements

OAS = Spread over tree that prices callable bond correctly

**Algorithm:**
1. Build interest rate tree
2. Calibrate to ATM swaption vols
3. Backward induction with optimal call exercise
4. Solve for OAS matching market price

**Tree Implementation:**
```rust
pub struct BinomialTree {
    steps: usize,
    dt: f64,
    rates: Vec<Vec<f64>>,        // Rate at each node
    probabilities: Vec<Vec<f64>>, // Transition probs
}
```

**OAS vs Z-Spread:**
- OAS < Z-spread for callable bonds (option value)
- Difference = option cost in bps

### Performance Target
- 100 step tree: < 10ms
```

---

## Risk Calculation Prompts

### 17. Duration & Convexity

```markdown
## Task: Implement Risk Metrics in convex-risk

### Requirements

**Duration Types:**
```rust
// Macaulay: weighted average time
pub fn macaulay_duration(
    cash_flows: &[CashFlow],
    settlement: Date,
    yield_val: Yield,
    day_count: &impl DayCount,
) -> Duration;

// Modified: price sensitivity
pub fn modified_duration(macaulay: Duration, yield_val: Yield, freq: Frequency) -> Duration;

// Effective: for bonds with optionality
pub fn effective_duration(
    bond: &impl Bond,
    settlement: Date,
    curve: &impl Curve,
    bump_size: Decimal,
) -> Duration;
```

**Convexity:**
```rust
pub fn analytical_convexity(...) -> Convexity;
pub fn effective_convexity(...) -> Convexity;
```

**DV01:**
```rust
pub fn dv01(
    modified_duration: Duration,
    dirty_price: Decimal,
    face_value: Decimal,
) -> DV01;
```

### Bloomberg Validation
Boeing bond:
- Mod Duration: 4.209 (±0.001)
- Convexity: 0.219 (±0.001)
```

---

## YAS Replication Prompts

### 18. Full YAS Implementation

```markdown
## Task: Implement Bloomberg YAS Replication in convex-yas

### Requirements

**YAS Analysis Result:**
```rust
pub struct YasAnalysis {
    // Yields
    pub street_convention: Yield,
    pub true_yield: Yield,
    pub current_yield: Yield,
    
    // Spreads
    pub g_spread: Spread,
    pub i_spread: Spread,
    pub z_spread: Spread,
    pub asw_spread: Spread,
    
    // Risk
    pub modified_duration: Duration,
    pub convexity: Convexity,
    pub dv01: DV01,
    
    // Invoice
    pub invoice: SettlementInvoice,
}
```

**Sequential Roll-Forward:**
For bonds < 1 year, must use Bloomberg's exact methodology

### Bloomberg Validation
ALL fields must match within tolerance for Boeing bond:
- Street Convention: 4.905895% (±0.00001%)
- G-Spread: 448.5 bps (±0.1 bps)
- Z-Spread: 444.7 bps (±0.1 bps)
- Mod Duration: 4.209 (±0.001)
```

---

## Testing & Validation Prompts

### 19. Bloomberg Validation Suite

```markdown
## Task: Create Bloomberg Validation Test Suite

### Requirements

Create comprehensive validation tests in `tests/bloomberg_validation/`:

**Test Structure:**
```rust
#[test]
fn test_boeing_full_yas() {
    let bond = create_boeing_bond();
    let settlement = date!(2020-04-29);
    let price = CleanPrice::new(dec!(110.503)).unwrap();
    
    let yas = YasAnalysis::calculate(&bond, settlement, price, &curve).unwrap();
    
    assert_bloomberg_match!(yas.street_convention.as_percent(), 4.905895, 0.00001);
    assert_bloomberg_match!(yas.g_spread.as_bps(), 448.5, 0.1);
    // ... all fields
}
```

**Validation Macro:**
```rust
#[macro_export]
macro_rules! assert_bloomberg_match {
    ($actual:expr, $expected:expr, $tolerance:expr) => {
        let diff = ($actual - $expected).abs();
        assert!(diff <= $tolerance, 
            "Bloomberg mismatch: expected {}, got {}, diff {}", 
            $expected, $actual, diff);
    };
}
```

### Coverage Required
- All bond types
- All day count conventions
- All spread types
- All risk metrics
- All curve operations
```

### 20. Performance Benchmarks

```markdown
## Task: Create Performance Benchmark Suite

### Requirements

Create benchmarks in `benches/`:

```rust
use criterion::{criterion_group, criterion_main, Criterion, black_box};

fn bench_yield_calculation(c: &mut Criterion) {
    let bond = create_benchmark_bond();
    let settlement = date!(2024-06-15);
    let price = dec!(99.5);
    
    c.bench_function("yield_from_price", |b| {
        b.iter(|| calculate_yield(
            black_box(&bond),
            black_box(settlement),
            black_box(price),
        ))
    });
}

criterion_group!(benches, 
    bench_yield_calculation,
    bench_z_spread,
    bench_curve_bootstrap,
    bench_interpolation,
);
criterion_main!(benches);
```

### Targets
- Yield calculation: < 1μs
- Z-spread: < 50μs
- Curve bootstrap: < 100μs
- Portfolio (1000): < 100ms
```

---

## Quick Reference

### File Locations
- Domain knowledge: `.claude/context.md`
- Architecture: `.claude/architecture.md`
- Progress tracking: `.claude/memory.md`

### Commands
```bash
cargo test --workspace              # All tests
cargo test -p convex-bonds          # Crate tests
cargo test --test bloomberg         # Validation tests
cargo bench                         # Benchmarks
cargo clippy -- -D warnings         # Linting
cargo fmt                           # Formatting
cargo doc --open                    # Documentation
```

### Session End
```
Please update .claude/memory.md with:
- What was implemented
- Any decisions made
- Validation status
- Open issues
```
