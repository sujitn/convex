# Convex - Fixed Income Analytics Library

## Project Overview

Convex is a high-performance, production-grade fixed income analytics library written in Rust. It provides comprehensive bond pricing, yield curve construction, and risk analytics capabilities comparable to Bloomberg YAS (Yield Analysis System).

## Domain Context

### Fixed Income Fundamentals

**Bond Types Supported:**
- **Government Bonds**: Treasury securities, sovereign debt, inflation-linked bonds (TIPS/Linkers)
- **Corporate Bonds**: Investment grade and high yield, callable/putable bonds, convertible bonds
- **Money Market Instruments**: T-Bills, Commercial Paper, Certificates of Deposit
- **Agency Bonds**: GSE securities (Fannie Mae, Freddie Mac, etc.)
- **Structured Products**: Asset-backed securities (ABS), Mortgage-backed securities (MBS)

**Key Analytics Requirements:**

1. **Yield Curve Construction**
   - Bootstrap methods for zero-coupon curve extraction
   - Interpolation methods: Linear, Cubic Spline, Nelson-Siegel, Svensson
   - Multiple curve frameworks for different currencies
   - Day count conventions: ACT/360, ACT/365, 30/360, ACT/ACT

2. **Pricing Methodologies**
   - Present Value (PV) calculations with proper discounting
   - Clean Price vs Dirty Price (with accrued interest)
   - Yield-to-Maturity (YTM) calculation using Newton-Raphson
   - Money Market Yields (Discount Yield, Bond Equivalent Yield)
   - Bloomberg YAS-compatible yield calculations

3. **Spread Analytics**
   - **Z-Spread**: Zero-volatility spread over benchmark curve
   - **G-Spread**: Spread over government curve at specific tenor
   - **I-Spread**: Interpolated spread between two benchmark points
   - **Asset Swap Spread**: Par-par and proceeds asset swap spreads
   - **OAS**: Option-adjusted spread (for callable/putable bonds)
   - **Credit Spread**: Over interpolated sovereign/swap curves

4. **Risk Metrics**
   - **Duration**: Macaulay, Modified, Effective
   - **Convexity**: Price sensitivity to yield changes
   - **DV01**: Dollar value of 1bp change (PV01/PVBP)
   - **Key Rate Durations**: Sensitivity to specific curve points
   - **Partial DV01s**: Risk by maturity bucket
   - **Greeks**: For embedded options (callable/putable bonds)

5. **Cash Flow Analytics**
   - Coupon schedule generation with business day adjustments
   - Accrued interest calculations
   - Settlement date conventions (T+1, T+2, T+3)
   - Principal redemption schedules (amortizing bonds)
   - Sinking fund schedules

## Technical Architecture Principles

### Performance Optimization

1. **SIMD Vectorization**: Use packed_simd for parallel calculations
2. **Zero-Copy Operations**: Minimize allocations in hot paths
3. **Cache Optimization**: Structure data for cache-friendly access patterns
4. **Parallel Processing**: Use rayon for multi-threaded curve building
5. **LTO and PGO**: Enable link-time optimization and profile-guided optimization

### Rust Best Practices

1. **Type Safety**: Use newtypes for domain concepts (Price, Yield, Spread)
2. **Error Handling**: Comprehensive Result types, never panic in library code
3. **Generic Programming**: Support different numerical types (f64, Decimal)
4. **Zero-Cost Abstractions**: Trait-based design with no runtime overhead
5. **Memory Safety**: No unsafe code unless absolutely necessary and documented

### API Design

1. **Builder Pattern**: For complex configurations (CurveBuilder, BondBuilder)
2. **Fluent Interfaces**: Chainable methods for common workflows
3. **Immutability**: Immutable data structures by default
4. **Ergonomic APIs**: Sensible defaults, optional parameters
5. **Type-Driven Design**: Leverage type system to prevent invalid states

## Project Structure

```
convex/
├── Cargo.toml                 # Workspace definition
├── crates/
│   ├── convex-core/          # Core abstractions and traits
│   │   ├── src/
│   │   │   ├── types/        # Domain types (Date, Price, Yield, etc.)
│   │   │   ├── daycounts/    # Day count conventions
│   │   │   ├── calendars/    # Business day calendars
│   │   │   └── traits/       # Core traits
│   │   └── Cargo.toml
│   │
│   ├── convex-curves/        # Yield curve construction
│   │   ├── src/
│   │   │   ├── bootstrap/    # Bootstrap algorithms
│   │   │   ├── interpolation/ # Interpolation methods
│   │   │   └── multi_curve/   # Multi-curve frameworks
│   │   └── Cargo.toml
│   │
│   ├── convex-bonds/         # Bond pricing and analytics
│   │   ├── src/
│   │   │   ├── instruments/  # Bond types
│   │   │   ├── pricing/      # Pricing engines
│   │   │   ├── cashflows/    # Cash flow generation
│   │   │   └── risk/         # Risk calculations
│   │   └── Cargo.toml
│   │
│   ├── convex-spreads/       # Spread calculations
│   │   ├── src/
│   │   │   ├── zspread/
│   │   │   ├── asset_swap/
│   │   │   └── oas/
│   │   └── Cargo.toml
│   │
│   ├── convex-math/          # Mathematical utilities
│   │   ├── src/
│   │   │   ├── solvers/      # Root finding (Newton-Raphson, Brent)
│   │   │   ├── optimization/ # Optimization algorithms
│   │   │   └── linear_algebra/ # Matrix operations
│   │   └── Cargo.toml
│   │
│   ├── convex-ffi/           # FFI layer for language bindings
│   │   ├── src/
│   │   │   ├── c_api/        # C API
│   │   │   └── safety/       # FFI safety wrappers
│   │   └── Cargo.toml
│   │
│   └── convex-python/        # Python bindings (PyO3)
│       └── Cargo.toml
│
├── bindings/
│   ├── java/                 # JNI bindings
│   ├── csharp/               # C# P/Invoke
│   └── excel/                # Excel RTD/XLL plugin
│
├── benchmarks/               # Performance benchmarks
├── examples/                 # Usage examples
└── tests/                    # Integration tests
```

## Key Dependencies

**Core:**
- `chrono` or `time`: Date/time handling with timezone support
- `rust_decimal`: High-precision decimal arithmetic (avoid floating point issues)
- `thiserror`: Error handling
- `serde`: Serialization/deserialization

**Performance:**
- `rayon`: Data parallelism
- `packed_simd`: SIMD operations
- `ndarray`: Multi-dimensional arrays for matrix operations
- `approx`: Floating point comparisons in tests

**Mathematical:**
- `nalgebra`: Linear algebra
- `argmin`: Optimization framework
- `statrs`: Statistical distributions

**FFI:**
- `cbindgen`: Generate C headers
- `pyo3`: Python bindings
- `jni`: Java bindings

## Critical Domain Requirements

### Day Count Conventions (Must be exact)
- **ACT/360**: Actual days / 360 (Money market)
- **ACT/365**: Actual days / 365 (UK Gilts)
- **30/360 US**: 30-day months, 360-day year (Corporate bonds)
- **30E/360**: European convention
- **ACT/ACT ICMA**: Actual/Actual per period (Government bonds)
- **ACT/ACT ISDA**: Actual/Actual with year convention

### Business Day Conventions
- **Following**: Move to next business day
- **Modified Following**: Unless crosses month boundary
- **Preceding**: Move to previous business day
- **Modified Preceding**: Unless crosses month boundary
- **Unadjusted**: No adjustment

### Settlement Conventions by Market
- US Treasuries: T+1
- US Corporate: T+2
- European Bonds: T+2
- UK Gilts: T+1
- Emerging Markets: T+0 to T+3

### Bloomberg YAS Methodology
Must replicate Bloomberg's sequential roll-forward approach:
1. Start with settlement date
2. Roll forward to next coupon period
3. Calculate days in period using actual calendar
4. Apply day count convention
5. Compound yield sequentially through all periods
6. Iterate using Newton-Raphson to solve for yield

## Code Quality Standards

### Testing Requirements
- **Unit Tests**: 90%+ code coverage
- **Property-Based Tests**: Use `proptest` for invariant checking
- **Integration Tests**: Real-world bond pricing scenarios
- **Benchmark Tests**: Criterion.rs for performance tracking
- **Validation Tests**: Compare against Bloomberg/Reuters reference prices

### Documentation Standards
- All public APIs must have rustdoc comments
- Include mathematical formulas in LaTeX notation
- Provide code examples for common use cases
- Document algorithm complexity (time/space)
- Reference academic papers or Bloomberg docs where applicable

### Performance Targets
- Single bond price: < 1 microsecond
- YTM calculation: < 10 microseconds
- Bootstrap 50-point curve: < 100 microseconds
- Z-spread calculation: < 50 microseconds
- Thread-safe and lock-free where possible

## Future Roadmap

### Phase 1: Core Foundation (Current)
- Basic bond types (fixed coupon, zero coupon)
- Yield curve construction (linear, cubic spline)
- YTM and price calculations
- Basic spread calculations (G-spread, I-spread)

### Phase 2: Advanced Analytics
- Callable/putable bonds with OAS
- Floating rate notes with spread over LIBOR/SOFR
- Inflation-linked bonds
- Advanced interpolation (Nelson-Siegel, Svensson)
- Key rate durations

### Phase 3: Exotic Products
- Convertible bonds
- Asset-backed securities
- CDS pricing integration
- Multi-curve frameworks (OIS discounting)

### Phase 4: Integration & Bindings
- Python bindings (PyO3)
- Java bindings (JNI)
- C# bindings (P/Invoke)
- Excel plugin (XLL/RTD)
- REST API service

### Phase 5: Production Features
- Real-time market data integration
- Distributed computing support
- GPU acceleration for Monte Carlo
- Time series analytics
- Portfolio analytics

## References

**Academic:**
- *Fixed Income Securities* by Bruce Tuckman
- *The Handbook of Fixed Income Securities* by Frank Fabozzi
- *Interest Rate Models* by Damiano Brigo and Fabio Mercurio

**Industry Standards:**
- ISDA definitions for swap conventions
- Bloomberg Financial Analysis Function Reference
- ICE Benchmark Administration for LIBOR conventions
- ARRC recommendations for SOFR transition

**Technical:**
- Rust Performance Book
- Rust API Guidelines
- QuantLib design patterns (for reference, not copying)

## Contact & Collaboration

This is a production-grade library intended for use in trading systems, risk management, and portfolio analytics. Code contributions should prioritize correctness, performance, and maintainability in that order.
