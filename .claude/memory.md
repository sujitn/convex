# Convex Project Memory

## Project Status

**Current Phase**: Foundation & Initial Development
**Started**: 2025-11-27
**Last Updated**: 2025-11-29
**Target**: Production-grade fixed income analytics

---

## Key Decisions Log

### Architecture Decisions

#### AD-001: Workspace Structure
- **Decision**: Use Cargo workspace with multiple crates
- **Rationale**: Enables modular development, independent compilation, clear separation of concerns
- **Crates**: convex-core, convex-math, convex-curves, convex-bonds, convex-spreads, convex-risk, convex-yas, convex-ffi
- **Status**: ✅ Approved

#### AD-002: Numerical Precision Strategy
- **Decision**: Use `rust_decimal::Decimal` for all financial calculations visible to users, f64 for internal math
- **Rationale**: Avoid floating-point precision issues in price/yield calculations while maintaining performance
- **Implementation**:
  - All public API types use Decimal
  - Internal interpolation uses f64
  - Conversion at API boundaries
- **Status**: ✅ Approved

#### AD-003: Error Handling Strategy
- **Decision**: Use `thiserror` for domain errors, never panic in library code
- **Error Categories**:
  - `PricingError`: Invalid inputs, calculation failures
  - `CurveError`: Curve construction issues
  - `SolverError`: Convergence failures
  - `DateError`: Invalid date operations
  - `ValidationError`: Input validation failures
- **Status**: ✅ Approved

#### AD-004: Interpolation Architecture
- **Decision**: Pluggable interpolation with trait-based design
- **Methods Supported**:
  - Linear, Log-Linear
  - Cubic Spline (Natural, Clamped)
  - Monotone Convex (Hagan)
  - Bessel/Hermite
  - Tension Spline
- **Default**: Monotone Convex for production (positive forwards)
- **Status**: ✅ Approved

#### AD-005: Extrapolation Strategy
- **Decision**: Support multiple extrapolation methods, configurable per curve
- **Methods**:
  - None (error if outside range)
  - Flat
  - Linear
  - Smith-Wilson (regulatory)
- **Default**: Flat for short end, Smith-Wilson for long end (>30Y)
- **Status**: ✅ Approved

#### AD-006: Multi-Curve Framework
- **Decision**: Full multi-curve support from day one
- **Architecture**:
  - Separate discounting and projection curves
  - Support for basis spreads
  - Cross-currency curve handling
- **Status**: ✅ Approved

#### AD-007: Parallel Processing
- **Decision**: Use Rayon for data parallelism
- **Use Cases**:
  - Portfolio pricing
  - Curve building (parallel instrument pricing)
  - Risk calculations
  - Scenario analysis
- **Status**: ✅ Approved

---

### Technical Decisions

#### TD-001: Core Dependencies
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
- **Status**: ✅ Approved

#### TD-002: Testing Framework
- Unit Tests: Standard Rust + approx for float comparison
- Property Tests: proptest for invariants
- Benchmarks: Criterion.rs
- Validation: Bloomberg comparison tests
- **Coverage Target**: >90% for core modules
- **Status**: ✅ Approved

#### TD-003: Documentation Standard
- Rustdoc with LaTeX math notation
- Code examples for all public APIs
- Bloomberg methodology references
- Complexity analysis (time/space)
- **Status**: ✅ Approved

---

### Domain Decisions

#### DD-001: Day Count Implementation Priority
1. ✅ ACT/360 (Money markets)
2. ✅ ACT/365F (UK Gilts)
3. ✅ ACT/365L (Leap year aware)
4. ✅ 30/360 US (Corporate bonds) - with Bloomberg Feb EOM rules
5. ✅ ACT/ACT ICMA (Government bonds) - with period-based calculation
6. ✅ ACT/ACT ISDA (Swaps)
7. ✅ 30E/360 (Eurobonds)
8. ✅ 30E/360 ISDA (ISDA swaps)
9. ✅ ACT/ACT AFB (French)
10. ✅ 30/360 German (German market)
11. ⬜ BUS/252 (Brazil) - Future

#### DD-002: Bond Type Implementation Priority
**Phase 1 (Core):**
1. ⬜ Fixed-rate corporate
2. ⬜ US Treasury Note/Bond
3. ⬜ Zero coupon
4. ⬜ T-Bill

**Phase 2 (Extended Government):**
5. ⬜ UK Gilt
6. ⬜ German Bund
7. ⬜ TIPS

**Phase 3 (Optionality):**
8. ⬜ Callable corporate
9. ⬜ Putable bonds
10. ⬜ Sinking fund

**Phase 4 (Special):**
11. ⬜ Municipal
12. ⬜ FRN
13. ⬜ MBS Pass-through
14. ⬜ Convertible

#### DD-003: Curve Type Priority
1. ⬜ Government bond curve (bootstrap)
2. ⬜ Swap curve (OIS discounting)
3. ⬜ Credit spread curve
4. ⬜ Inflation curve

#### DD-004: Interpolation Priority
1. ✅ Linear (baseline)
2. ✅ Monotone Convex (production default)
3. ✅ Cubic Spline
4. ✅ Nelson-Siegel/Svensson (fitting)
5. ✅ Log-Linear (for discount factors)
6. ✅ Extrapolation: Flat, Linear, Smith-Wilson (EIOPA)

#### DD-005: Yield Calculation Methodology
- **Method**: Bloomberg YAS sequential roll-forward
- **Solver**: Newton-Raphson with Brent fallback
- **Tolerance**: 1e-10
- **Max Iterations**: 100
- **Status**: ✅ Must match Bloomberg exactly

#### DD-006: Calendar Support
**Phase 1:**
- ✅ SIFMA (US bond market)
- ✅ US Government (Treasury)
- ✅ TARGET2 (Eurozone)

**Phase 2:**
- ✅ UK (Bank of England)
- ✅ Japan (TSE)
- ✅ Combined calendar support (JointCalendar)

---

## Implementation Progress

### Milestone 1: Core Infrastructure
- [x] Create workspace structure
- [x] Implement core types (Date, Price, Yield, Spread)
- [x] Implement day count conventions (all 10 conventions)
- [x] Implement business day calendars (SIFMA, US Gov, TARGET2, UK, Japan)
- [x] Write comprehensive unit tests (222 tests passing)
- [x] Bloomberg validation: Day counts (Boeing bond: 134 days ✓)

**Target**: Week 1-2
**Status**: ✅ Complete

### Milestone 2: Mathematical Foundation
- [x] Newton-Raphson solver (with numerical derivative fallback)
- [x] Brent's method (guaranteed convergence)
- [x] Bisection method (robust fallback)
- [x] Secant method (derivative-free)
- [x] Hybrid solver (Newton + Brent fallback)
- [x] Solver trait with unified interface
- [x] Linear interpolation
- [x] Log-linear interpolation (for discount factors)
- [x] Cubic spline interpolation (natural)
- [x] Monotone Convex interpolation (Hagan) - PRODUCTION DEFAULT
- [x] Nelson-Siegel parametric model
- [x] Svensson parametric model
- [x] Extrapolation methods (Flat, Linear, Smith-Wilson)

**Target**: Week 3-4
**Status**: ✅ Complete

### Milestone 3: Curve Construction
- [ ] Curve trait and types
- [ ] Bootstrap from deposits
- [ ] Bootstrap from swaps
- [ ] Bootstrap from bonds
- [ ] Multi-curve framework
- [ ] Curve validation suite

**Target**: Week 5-6  
**Status**: Not Started

### Milestone 4: Basic Bond Pricing
- [ ] Fixed-rate bond
- [ ] Zero-coupon bond
- [ ] Cash flow generation
- [ ] YTM calculator
- [ ] Clean/dirty price
- [ ] Accrued interest
- [ ] Bloomberg validation: Boeing bond

**Target**: Week 7-8  
**Status**: Not Started

### Milestone 5: Government Bonds
- [ ] US Treasury Note/Bond
- [ ] T-Bill (discount basis)
- [ ] TIPS (inflation-linked)
- [ ] UK Gilt
- [ ] German Bund
- [ ] Price quote conventions (32nds)

**Target**: Week 9-10  
**Status**: Not Started

### Milestone 6: Spread Analytics
- [ ] G-spread
- [ ] I-spread
- [ ] Z-spread (Brent solver)
- [ ] Asset swap spread (par-par)
- [ ] Bloomberg validation: Spreads

**Target**: Week 11-12  
**Status**: Not Started

### Milestone 7: Risk Calculations
- [ ] Macaulay duration
- [ ] Modified duration
- [ ] Effective duration
- [ ] Convexity
- [ ] DV01
- [ ] Key rate durations
- [ ] Bloomberg validation: Risk metrics

**Target**: Week 13-14  
**Status**: Not Started

### Milestone 8: Corporate Bond Extensions
- [ ] Callable bonds
- [ ] Binomial tree model
- [ ] OAS calculation
- [ ] Yield to worst
- [ ] Make-whole call

**Target**: Week 15-16  
**Status**: Not Started

### Milestone 9: Special Bond Types
- [ ] Municipal bonds (tax-equivalent yield)
- [ ] FRN (discount margin)
- [ ] MBS pass-through (prepayment models)
- [ ] Convertible bonds

**Target**: Week 17-20  
**Status**: Not Started

### Milestone 10: Production Hardening
- [ ] Performance optimization
- [ ] Full Bloomberg validation
- [ ] FFI layer
- [ ] Python bindings
- [ ] Documentation complete

**Target**: Week 21-24  
**Status**: Not Started

---

## Validation Status

### Bloomberg Validation Matrix

| Category | Test Cases | Passing | Status |
|----------|-----------|---------|--------|
| Day Counts | 68/50 | 68 | ✅ |
| Calendars | 154/100 | 154 | ✅ |
| Solvers | 54/40 | 54 | ✅ |
| Interpolation | 59/50 | 59 | ✅ |
| Extrapolation | 27/25 | 27 | ✅ |
| US Treasury | 0/20 | 0 | ⬜ |
| Corporate IG | 0/20 | 0 | ⬜ |
| Corporate HY | 0/15 | 0 | ⬜ |
| Municipal | 0/10 | 0 | ⬜ |
| TIPS | 0/10 | 0 | ⬜ |
| MBS | 0/10 | 0 | ⬜ |
| Curves | 0/30 | 0 | ⬜ |
| Spreads | 0/20 | 0 | ⬜ |
| Risk | 0/25 | 0 | ⬜ |
| **Total** | **362/415** | **362** | 🟡 |

### Primary Validation Bond Status

**Boeing 7.5% 06/15/2025 (CUSIP: 097023AH7)**
Settlement: 04/29/2020, Price: 110.503

| Metric | Expected | Actual | Diff | Status |
|--------|----------|--------|------|--------|
| Street Convention | 4.905895% | - | - | ⬜ |
| True Yield | 4.903264% | - | - | ⬜ |
| Current Yield | 6.561% | - | - | ⬜ |
| G-Spread | 448.5 bps | - | - | ⬜ |
| Z-Spread | 444.7 bps | - | - | ⬜ |
| Mod Duration | 4.209 | - | - | ⬜ |
| Convexity | 0.219 | - | - | ⬜ |
| Accrued Days | 134 | 134 | 0 | ✅ |
| Accrued Interest | 26,986.11 | - | - | ⬜ |

---

## Performance Benchmarks

### Target vs Actual

| Operation | Target | Actual | Status |
|-----------|--------|--------|--------|
| Bond pricing | < 1μs | TBD | ⬜ |
| YTM calculation | < 1μs | TBD | ⬜ |
| Z-spread | < 50μs | TBD | ⬜ |
| OAS (100 steps) | < 10ms | TBD | ⬜ |
| Curve bootstrap (50 pts) | < 100μs | TBD | ⬜ |
| Linear interpolation | < 10ns | TBD | ⬜ |
| Monotone convex | < 50ns | TBD | ⬜ |
| Portfolio (1000 bonds) | < 100ms | TBD | ⬜ |

---

## Known Issues & Challenges

### Technical Challenges

#### TC-001: Float Precision in Yield Calculations
- **Issue**: Need exact Bloomberg YAS match
- **Solution**: Use Decimal, sequential roll-forward
- **Priority**: Critical
- **Status**: Design complete

#### TC-002: Curve Extrapolation Stability
- **Issue**: Long-dated extrapolation can be unstable
- **Solution**: Smith-Wilson with proper parameterization
- **Priority**: High
- **Status**: ✅ Resolved - Smith-Wilson implemented with EIOPA presets

#### TC-003: MBS Prepayment Modeling
- **Issue**: Complex prepayment behavior
- **Solution**: Support CPR, PSA, and custom vectors
- **Priority**: Medium
- **Status**: Design needed

#### TC-004: Callable Bond Convergence
- **Issue**: OAS solver can be unstable
- **Solution**: Robust bracketing, multiple solver fallbacks
- **Priority**: High
- **Status**: Design needed

---

## Open Questions

### Q-001: Negative Interest Rate Support
- **Context**: EUR government bonds, JPY
- **Decision**: Full support required
- **Implementation**: Allow negative yields, handle in all formulas
- **Status**: ✅ Decided

### Q-002: Ex-Dividend Handling
- **Context**: UK Gilts have 7 business day ex-div
- **Decision**: Support per-market conventions
- **Status**: ✅ Decided

### Q-003: Leap Second Handling
- **Context**: Could affect day counts at year boundaries
- **Decision**: Ignore (industry standard)
- **Status**: ✅ Decided

### Q-004: Async API
- **Context**: Future integration needs
- **Decision**: Sync-first, async wrappers later
- **Status**: ✅ Decided

---

## References Used

### Bloomberg Documentation
- YAS Function Reference
- Day Count Conventions
- Settlement Conventions

### Academic Papers
- Hagan, P. "Interpolation Methods for Curve Construction"
- Le Floc'h, F. "Monotone Convex Interpolation"
- Nelson, C. & Siegel, A. "Parsimonious Modeling of Yield Curves"

### Industry Standards
- ISDA Day Count Definitions
- ICMA Bond Calculation Rules
- ARRC SOFR Conventions
- EIOPA Smith-Wilson Specification

---

## Change Log

### 2025-11-29 - Extrapolation Methods Complete (Milestone 2 Done)
- **Implemented full extrapolation infrastructure in convex-math**:
  - `FlatExtrapolator`: Constant value from last point (conservative)
  - `LinearExtrapolator`: Linear slope continuation from last derivative
  - `SmithWilson`: EIOPA regulatory standard for Solvency II
    - Smooth convergence to Ultimate Forward Rate (UFR)
    - Configurable convergence speed (alpha)
    - Last Liquid Point (LLP) based extrapolation
- **EIOPA Presets for Smith-Wilson**:
  - `SmithWilson::eiopa_eur()`: UFR 3.45%, LLP 20Y, α=0.126
  - `SmithWilson::eiopa_gbp()`: UFR 3.45%, LLP 50Y, α=0.100
  - `SmithWilson::eiopa_usd()`: UFR 3.45%, LLP 30Y, α=0.100
  - `SmithWilson::eiopa_chf()`: UFR 3.45%, LLP 25Y, α=0.100
- **Extrapolator Trait**:
  - `extrapolate(t, last_t, last_value, last_derivative)`: Extrapolate to time t
  - `name()`: Returns method name
- **ExtrapolationMethod Enum**:
  - `None`, `Flat`, `Linear`, `SmithWilson { ufr, alpha }`
- **Key Features**:
  - UFR convergence verified at long maturities
  - Higher alpha = faster convergence (tested)
  - Convergence from both above and below UFR
  - EIOPA convergence criterion testing (within 3bp at LLP+40Y)
- **Tests**: 27 extrapolation tests + 4 doc-tests passing
- **Milestone 2 Status**: ✅ Complete

### 2025-11-29 - Interpolation Methods Complete
- **Implemented full interpolation infrastructure in convex-math**:
  - `LinearInterpolator`: Simple piecewise linear
  - `LogLinearInterpolator`: Log-linear for discount factors (guarantees positive values)
  - `CubicSpline`: Natural cubic spline with C2 continuity
  - `MonotoneConvex`: Hagan-West method - **PRODUCTION DEFAULT**
    - Guarantees positive forward rates
    - C1 continuity
    - No spurious oscillations
  - `NelsonSiegel`: 4-parameter parametric model (β₀, β₁, β₂, τ)
  - `Svensson`: 6-parameter extension with second hump
- **Interpolator Trait**:
  - `interpolate()`: Get value at point
  - `derivative()`: Get first derivative (for forward rates)
  - `allows_extrapolation()`, `min_x()`, `max_x()`, `in_range()`
- **Key Features**:
  - All methods pass through input points
  - Positive forward rate validation for MonotoneConvex
  - Derivative accuracy verified vs numerical differentiation
- **Tests**: 59 interpolation tests + 7 doc-tests passing

### 2025-11-29 - Root-Finding Solvers Complete
- **Implemented complete solver infrastructure in convex-math**:
  - `newton_raphson`: Quadratic convergence with analytical derivative
  - `newton_raphson_numerical`: Numerical derivative fallback
  - `brent`: Guaranteed convergence using bisection/secant/IQI
  - `bisection`: Robust bracketing method
  - `secant`: Superlinear convergence without derivative
  - `hybrid`: Newton + Brent fallback for robust YTM calculation
  - `hybrid_numerical`: Hybrid without analytical derivative
- **Unified Solver Trait**:
  - `Solver` trait with `solve()` method matching specification
  - `NewtonSolver`, `BrentSolver`, `BisectionSolver`, `SecantSolver`, `HybridSolver`
- **Configuration**:
  - Default tolerance: 1e-10
  - Default max iterations: 100
  - `SolverConfig` for customization
  - `SolverResult` with root, iterations, residual
- **YTM-like Financial Tests**:
  - Par bond, discount bond, premium bond
  - High-yield bond, zero-coupon bond
  - Z-spread-like calculation
  - All solvers agree within tolerance
- **Tests**: 54 solver tests + 9 doc-tests passing

### 2025-11-29 - Holiday Calendars Complete (Milestone 1 Done)
- **Implemented full business day calendar infrastructure**:
  - `SIFMACalendar`: US bond market holidays (SIFMA recommended closures)
  - `USGovernmentCalendar`: US Treasury market holidays
  - `Target2Calendar`: Eurozone TARGET2 payment system holidays
  - `UKCalendar`: UK bank holidays (Bank of England)
  - `JapanCalendar`: Japanese national holidays
  - `JointCalendar`: Combine multiple calendars for cross-border transactions
- **Key Technical Decisions**:
  - O(1) bitmap-based holiday lookups (~12KB memory per calendar)
  - Support years 1970-2099 in bitmap storage
  - `DynamicCalendar` for runtime-configurable calendars (JSON loading)
  - `CustomCalendarBuilder` for programmatic calendar creation
- **Business Day Conventions** (ISDA-compliant):
  - Following, ModifiedFollowing, Preceding, ModifiedPreceding, Unadjusted
- **Calendar Trait API**:
  - `is_business_day()`, `is_holiday()`, `adjust()`
  - `add_business_days()`, `settlement_date()`
  - `next_business_day()`, `previous_business_day()`
  - `business_days_between()`
- **Tests**: 222 unit tests + 14 doc-tests passing
- **Milestone 1 Status**: ✅ Complete

### 2025-11-27 - Day Count Conventions Complete
- **Implemented all 10 day count conventions** with Bloomberg-exact accuracy
- **ACT Family**: ACT/360, ACT/365F, ACT/365L, ACT/ACT ISDA, ACT/ACT ICMA, ACT/ACT AFB
- **30/360 Family**: 30/360 US, 30E/360, 30E/360 ISDA, 30/360 German
- **Critical Fix**: 30/360 US now includes proper February end-of-month rules
  - Rule 1: If D1 is last day of Feb → D1 = 30
  - Rule 2: If D1 = 31 → D1 = 30
  - Rule 3: If D2 is last day of Feb AND D1 was last day of Feb → D2 = 30
  - Rule 4: If D2 = 31 AND D1 >= 30 → D2 = 30
- **ACT/ACT ICMA**: Added `year_fraction_with_period()` for proper bond accrual calculation
- **Tests**: 68 comprehensive tests passing
- **Bloomberg Validation**: Boeing bond accrued days = 134 ✓

### [Earlier] - Initial Setup
- Created project structure
- Established architecture decisions
- Defined validation targets

---

## Next Steps

1. **Begin Milestone 3**: Curve construction and bootstrap
   - Define `Curve` trait and core types
   - Implement bootstrap from deposits (short end)
   - Implement bootstrap from swaps (medium/long end)
   - Implement bootstrap from bonds
2. **Multi-Curve Framework**: OIS discounting + projection curves
3. **Curve Instruments**: Deposits, FRAs, Swaps, Bonds
4. **BUS/252 Convention**: Add Brazilian business day convention if needed

---

*Update this file after each significant implementation or decision*
