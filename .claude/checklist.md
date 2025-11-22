# Convex Development Checklist

## Phase 1: Foundation (Weeks 1-2)

### Core Infrastructure (convex-core)
- [ ] Project Setup
  - [ ] Create Cargo workspace
  - [ ] Configure dependencies
  - [ ] Set up CI/CD pipeline
  - [ ] Initialize git repository

- [ ] Date & Calendar Types
  - [ ] `Date` struct with year/month/day
  - [ ] Date arithmetic (add days, months, years)
  - [ ] Business day calendar trait
  - [ ] US Federal Reserve calendar
  - [ ] UK Bank of England calendar
  - [ ] EUR TARGET2 calendar
  - [ ] Business day adjustment conventions
  - [ ] Tests for all calendars

- [ ] Financial Types
  - [ ] `Price` newtype with currency
  - [ ] `Yield` type with convention
  - [ ] `Spread` type in basis points
  - [ ] `Currency` enum (USD, EUR, GBP, JPY, etc.)
  - [ ] `Frequency` enum (Annual, SemiAnnual, Quarterly, Monthly)
  - [ ] Type conversions and display

- [ ] Day Count Conventions
  - [ ] `DayCounter` trait definition
  - [ ] ACT/360 implementation
  - [ ] ACT/365 implementation
  - [ ] 30/360 US implementation
  - [ ] 30E/360 implementation
  - [ ] ACT/ACT ICMA implementation
  - [ ] ACT/ACT ISDA implementation
  - [ ] Comprehensive tests for each
  - [ ] Validation against known reference values

### Documentation
- [ ] README with examples
- [ ] API documentation for all public types
- [ ] Architecture documentation
- [ ] Contributing guidelines

---

## Phase 2: Yield Curves (Weeks 3-4)

### Curve Infrastructure (convex-curves)
- [ ] Core Abstractions
  - [ ] `YieldCurve` trait
  - [ ] `CurvePoint` struct
  - [ ] `ZeroRate` representation
  - [ ] `DiscountFactor` calculations
  - [ ] `ForwardRate` calculations

- [ ] Interpolation Methods
  - [ ] Linear interpolation on zero rates
  - [ ] Flat forward interpolation
  - [ ] Cubic spline interpolation
  - [ ] Log-linear on discount factors
  - [ ] Hermite cubic interpolation
  - [ ] Benchmarks for interpolation speed

- [ ] Bootstrap Algorithms
  - [ ] Bootstrap from deposit rates
  - [ ] Bootstrap from government bonds
  - [ ] Bootstrap from swaps
  - [ ] Handle overlapping tenors
  - [ ] Error handling for invalid inputs

- [ ] Curve Construction
  - [ ] `CurveBuilder` pattern
  - [ ] Validate input data consistency
  - [ ] Support multiple reference dates
  - [ ] Cache interpolation coefficients

### Testing
- [ ] Unit tests for all interpolation methods
- [ ] Integration tests with real market data
- [ ] Property tests for curve invariants
- [ ] Benchmark curve construction speed (target: <100μs for 50 points)

---

## Phase 3: Bond Pricing (Weeks 5-6)

### Bond Instruments (convex-bonds)
- [ ] Fixed-Rate Bonds
  - [ ] `FixedRateBond` struct
  - [ ] `BondBuilder` pattern
  - [ ] ISIN/CUSIP identifiers
  - [ ] Coupon schedule generation
  - [ ] Handle stub periods
  - [ ] Handle irregular first/last coupon

- [ ] Zero-Coupon Bonds
  - [ ] `ZeroCouponBond` struct
  - [ ] Simple pricing formula
  - [ ] Discount factor calculation

- [ ] Cash Flow Engine
  - [ ] `CashFlow` struct (date, amount)
  - [ ] Generate regular coupon schedule
  - [ ] Apply business day adjustments
  - [ ] Handle end-of-month rules
  - [ ] Principal redemption

### Pricing Engine
- [ ] Price from Yield
  - [ ] Present value calculation
  - [ ] Support all compounding frequencies
  - [ ] Clean price calculation
  - [ ] Dirty price (with accrued interest)
  - [ ] Accrued interest calculation

- [ ] Yield from Price (YTM)
  - [ ] Newton-Raphson solver
  - [ ] Brent's method as fallback
  - [ ] Initial guess heuristic
  - [ ] Convergence tolerance (1e-10)
  - [ ] Maximum iterations (100)
  - [ ] Clear error messages for non-convergence

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
- [ ] G-Spread
  - [ ] Interpolate government yield at maturity
  - [ ] Calculate arithmetic difference
  - [ ] Handle interpolation edge cases

- [ ] I-Spread
  - [ ] Interpolate between two government bonds
  - [ ] Linear interpolation by default
  - [ ] Cubic spline option

- [ ] Z-Spread
  - [ ] Iterative solver to match price
  - [ ] Discount all cash flows with spread
  - [ ] Brent's method for root finding
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
- [ ] Macaulay Duration
  - [ ] Weighted average time to cash flows
  - [ ] Handle zero-coupon bonds correctly

- [ ] Modified Duration
  - [ ] Analytical formula
  - [ ] Relationship to price sensitivity

- [ ] Effective Duration
  - [ ] Finite difference method
  - [ ] Bump size (e.g., 10bps)
  - [ ] For bonds with embedded options

### Convexity
- [ ] Convexity calculation
  - [ ] Second derivative of price
  - [ ] Analytical formula
  - [ ] Finite difference method

### DV01 / PV01
- [ ] Dollar Value of 1bp
  - [ ] Calculate from duration
  - [ ] Direct bump calculation
  - [ ] Per $100 face value

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
- [ ] C-compatible structs
- [ ] Safe wrappers around Rust types
- [ ] Error handling via return codes
- [ ] Memory management (creation/destruction)
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
