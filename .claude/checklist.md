# Convex Development Checklist

## Phase 1: Foundation (Weeks 1-2)

### Core Infrastructure (convex-core)
- [x] Project Setup
  - [x] Create Cargo workspace
  - [x] Configure dependencies
  - [ ] Set up CI/CD pipeline
  - [x] Initialize git repository

- [x] Date & Calendar Types
  - [x] `Date` struct with year/month/day
  - [x] Date arithmetic (add days, months, years)
  - [x] Business day calendar trait
  - [x] US Federal Reserve calendar
  - [ ] UK Bank of England calendar
  - [ ] EUR TARGET2 calendar
  - [x] Business day adjustment conventions
  - [x] Tests for all calendars

- [x] Financial Types
  - [x] `Price` newtype with currency
  - [x] `Yield` type with convention
  - [x] `Spread` type in basis points
  - [x] `Currency` enum (USD, EUR, GBP, JPY, etc.)
  - [x] `Frequency` enum (Annual, SemiAnnual, Quarterly, Monthly)
  - [x] Type conversions and display

- [x] Day Count Conventions
  - [x] `DayCounter` trait definition
  - [x] ACT/360 implementation
  - [x] ACT/365 implementation
  - [x] 30/360 US implementation
  - [x] 30E/360 implementation
  - [x] ACT/ACT ICMA implementation
  - [x] ACT/ACT ISDA implementation
  - [x] Comprehensive tests for each
  - [ ] Validation against known reference values

### Documentation
- [ ] README with examples
- [ ] API documentation for all public types
- [ ] Architecture documentation
- [ ] Contributing guidelines

---

## Phase 2: Yield Curves (Weeks 3-4)

### Curve Infrastructure (convex-curves)
- [x] Core Abstractions
  - [x] `YieldCurve` trait
  - [x] `CurvePoint` struct
  - [x] `ZeroRate` representation
  - [x] `DiscountFactor` calculations
  - [x] `ForwardRate` calculations

- [x] Interpolation Methods
  - [x] Linear interpolation on zero rates
  - [ ] Flat forward interpolation
  - [x] Cubic spline interpolation
  - [ ] Log-linear on discount factors
  - [ ] Hermite cubic interpolation
  - [ ] Benchmarks for interpolation speed

- [x] Bootstrap Algorithms
  - [x] Bootstrap from deposit rates
  - [ ] Bootstrap from government bonds
  - [ ] Bootstrap from swaps
  - [ ] Handle overlapping tenors
  - [x] Error handling for invalid inputs

- [x] Curve Construction
  - [x] `CurveBuilder` pattern
  - [x] Validate input data consistency
  - [x] Support multiple reference dates
  - [ ] Cache interpolation coefficients

### Testing
- [x] Unit tests for all interpolation methods
- [ ] Integration tests with real market data
- [ ] Property tests for curve invariants
- [ ] Benchmark curve construction speed (target: <100μs for 50 points)

---

## Phase 3: Bond Pricing (Weeks 5-6)

### Bond Instruments (convex-bonds)
- [x] Fixed-Rate Bonds
  - [x] `FixedRateBond` struct
  - [x] `BondBuilder` pattern
  - [x] ISIN/CUSIP identifiers
  - [x] Coupon schedule generation
  - [ ] Handle stub periods
  - [ ] Handle irregular first/last coupon

- [x] Zero-Coupon Bonds
  - [x] `ZeroCouponBond` struct
  - [x] Simple pricing formula
  - [x] Discount factor calculation

- [x] Cash Flow Engine
  - [x] `CashFlow` struct (date, amount)
  - [x] Generate regular coupon schedule
  - [ ] Apply business day adjustments
  - [ ] Handle end-of-month rules
  - [x] Principal redemption

### Pricing Engine
- [x] Price from Yield
  - [x] Present value calculation
  - [x] Support all compounding frequencies
  - [x] Clean price calculation
  - [x] Dirty price (with accrued interest)
  - [x] Accrued interest calculation

- [x] Yield from Price (YTM)
  - [x] Newton-Raphson solver
  - [x] Brent's method as fallback
  - [x] Initial guess heuristic
  - [x] Convergence tolerance (1e-10)
  - [x] Maximum iterations (100)
  - [x] Clear error messages for non-convergence

- [ ] Money Market Yields
  - [ ] Discount Yield
  - [ ] Bond Equivalent Yield (BEY)
  - [ ] CD Equivalent Yield

### Validation
- [ ] Compare to Bloomberg YAS
  - [ ] US Treasury bonds
  - [ ] UK Gilts
  - [ ] German Bunds
  - [ ] Corporate bonds
- [ ] Test with negative yields
- [ ] Test with very long maturities (50+ years)
- [ ] Test with very short maturities (<1 month)

---

## Phase 4: Spread Analytics (Weeks 7-8)

### Basic Spreads (convex-spreads)
- [x] G-Spread
  - [x] Interpolate government yield at maturity
  - [x] Calculate arithmetic difference
  - [ ] Handle interpolation edge cases

- [x] I-Spread
  - [x] Interpolate between two government bonds
  - [x] Linear interpolation by default
  - [ ] Cubic spline option

- [x] Z-Spread
  - [x] Iterative solver to match price
  - [x] Discount all cash flows with spread
  - [x] Brent's method for root finding
  - [ ] Target speed: <50μs

### Asset Swap Spreads
- [ ] Par-Par Asset Swap
  - [ ] Calculate swap rate
  - [ ] Compute spread to swap curve
  - [ ] Handle currency conventions

- [ ] Proceeds Asset Swap
  - [ ] Adjust for bond price ≠ par
  - [ ] Calculate equivalent spread

### Testing
- [ ] Validate against Bloomberg ISPR, ASW functions
- [ ] Test with inverted curves
- [ ] Test with large spreads (distressed bonds)
- [ ] Performance benchmarks

---

## Phase 5: Risk Calculations (Weeks 9-10)

### Duration Metrics
- [x] Macaulay Duration
  - [x] Weighted average time to cash flows
  - [x] Handle zero-coupon bonds correctly

- [x] Modified Duration
  - [x] Analytical formula
  - [x] Relationship to price sensitivity

- [ ] Effective Duration
  - [ ] Finite difference method
  - [ ] Bump size (e.g., 10bps)
  - [ ] For bonds with embedded options

### Convexity
- [x] Convexity calculation
  - [x] Second derivative of price
  - [ ] Analytical formula
  - [x] Finite difference method

### DV01 / PV01
- [x] Dollar Value of 1bp
  - [x] Calculate from duration
  - [x] Direct bump calculation
  - [x] Per $100 face value

### Key Rate Durations
- [ ] Partial DV01s by maturity bucket
  - [ ] Define standard buckets (3M, 6M, 1Y, 2Y, 5Y, 10Y, 30Y)
  - [ ] Bump individual curve points
  - [ ] Reprice bond
  - [ ] Calculate sensitivity

### Parallel Execution
- [ ] Use Rayon for batch calculations
- [ ] Parallel risk for portfolio
- [ ] Thread-safe curve access

---

## Phase 6: Advanced Features (Weeks 11-14)

### Callable/Putable Bonds
- [ ] Call schedule definition
- [ ] Binomial tree for interest rate model
- [ ] Trinomial tree option
- [ ] Backward induction with exercise
- [ ] OAS calculation
- [ ] Effective duration/convexity

### Floating Rate Notes
- [ ] Reference rate (LIBOR, SOFR)
- [ ] Spread over reference
- [ ] Reset dates and frequencies
- [ ] Forward projection
- [ ] Price on reset date
- [ ] Between reset dates

### Advanced Interpolation
- [ ] Nelson-Siegel model
  - [ ] Parameter fitting
  - [ ] Bootstrap integration
  
- [ ] Svensson extension
  - [ ] Additional parameters
  - [ ] Better long-end fit

### Multi-Curve Framework
- [ ] OIS discounting curve
- [ ] LIBOR projection curves
- [ ] Tenor basis spreads
- [ ] Cross-currency basis
- [ ] Dual curve bootstrap

---

## Phase 7: Language Bindings (Weeks 15-18)

### C API (convex-ffi)
- [x] C-compatible structs
- [x] Safe wrappers around Rust types
- [x] Error handling via return codes
- [x] Memory management (creation/destruction)
- [ ] Generate header with cbindgen
- [ ] Example C program

### Python Bindings (PyO3)
- [ ] Wrap core types (Bond, Curve, Price)
- [ ] Pythonic API design
- [ ] Type hints for IDE support
- [ ] Rich `__repr__` and `__str__`
- [ ] NumPy array support for batch operations
- [ ] Comprehensive docstrings
- [ ] Setup.py for pip installation
- [ ] Python test suite
- [ ] Example Jupyter notebooks

### Java Bindings (JNI)
- [ ] JNI wrapper layer
- [ ] Java classes mirroring Rust types
- [ ] Maven/Gradle build integration
- [ ] Exception handling
- [ ] Java examples and tests
- [ ] Documentation (Javadoc)

### C# Bindings (P/Invoke)
- [ ] C# wrapper classes
- [ ] NuGet package
- [ ] .NET Standard support
- [ ] Exception marshaling
- [ ] C# examples and tests
- [ ] XML documentation

### Excel Plugin
- [ ] XLL add-in (C++)
- [ ] RTD server for real-time data
- [ ] UDF functions for pricing
- [ ] UDF functions for risk
- [ ] Installation wizard
- [ ] User documentation

---

## Phase 8: Production Features (Weeks 19-24)

### Performance Optimization
- [ ] Profile hot paths
- [ ] SIMD vectorization for discounting
- [ ] Cache-friendly data layouts
- [ ] Reduce allocations in hot loops
- [ ] LTO and PGO builds
- [ ] Target-specific optimizations

### Caching Layer
- [ ] Cache yield curves
- [ ] Cache bond prices
- [ ] TTL-based invalidation
- [ ] LRU eviction policy
- [ ] Thread-safe access

### Real-Time Market Data
- [ ] Market data provider interface
- [ ] Bloomberg integration
- [ ] Reuters integration
- [ ] FIX protocol support
- [ ] Curve updates
- [ ] Automatic re-pricing

### Distributed Computing
- [ ] Redis-based state sharing
- [ ] Distributed curve storage
- [ ] Horizontal scaling support
- [ ] Load balancing

### REST API Service
- [ ] HTTP endpoints for pricing
- [ ] WebSocket for real-time updates
- [ ] Authentication/authorization
- [ ] Rate limiting
- [ ] OpenAPI/Swagger documentation

### Portfolio Analytics
- [ ] Portfolio-level risk
- [ ] Aggregated DV01 by currency
- [ ] VaR calculation
- [ ] Stress testing framework
- [ ] Scenario analysis

---

## Ongoing Tasks

### Documentation
- [ ] Keep API docs up-to-date
- [ ] Add examples for new features
- [ ] User guides for each module
- [ ] Performance tuning guide
- [ ] Migration guides

### Testing
- [ ] Maintain >90% code coverage
- [ ] Add regression tests for bugs
- [ ] Property-based tests for invariants
- [ ] Fuzz testing for parsers
- [ ] Load testing for API service

### Code Quality
- [ ] Regular `cargo clippy` runs
- [ ] Format with `cargo fmt`
- [ ] Dependency audits (`cargo audit`)
- [ ] Security reviews
- [ ] Performance regression tracking

### Community
- [ ] Respond to GitHub issues
- [ ] Review pull requests
- [ ] Update CHANGELOG
- [ ] Write blog posts
- [ ] Conference talks/presentations

---

## Current Test Summary (178 tests passing)

| Crate | Unit Tests | Doc Tests |
|-------|------------|-----------|
| convex-core | 102 | 6 |
| convex-math | 32 | 5 |
| convex-curves | 10 | 0 |
| convex-bonds | 14 | 0 |
| convex-spreads | 5 | 0 |
| convex-ffi | 4 | 0 |

---

## Success Criteria

### Performance
- ✅ Bond pricing: <1 microsecond
- ✅ YTM calculation: <10 microseconds
- ✅ Bootstrap 50-point curve: <100 microseconds
- ✅ Z-spread: <50 microseconds
- ✅ Portfolio (1000 bonds): <10 milliseconds

### Accuracy
- ✅ Match Bloomberg YAS within 1e-6 for prices
- ✅ Match Bloomberg YAS within 1e-10 for yields
- ✅ All day count conventions exact
- ✅ Business day calendars accurate

### Code Quality
- ✅ >90% test coverage
- ✅ Zero clippy warnings
- ✅ Comprehensive documentation
- ✅ All examples compile and run

### Production Readiness
- ✅ No panics in library code
- ✅ Thread-safe
- ✅ Robust error handling
- ✅ Battle-tested with real market data

---

*This checklist should be updated as the project progresses. Mark items as complete with `[x]` when done.*
