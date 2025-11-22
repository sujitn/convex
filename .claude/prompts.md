# Claude Code Prompts for Convex Development

## Getting Started Prompts

### Initial Project Setup

```
Create the initial Cargo workspace for the Convex fixed income analytics library with the following structure:
- convex-core (core types and traits)
- convex-curves (yield curve construction)
- convex-bonds (bond pricing)
- convex-spreads (spread calculations)
- convex-math (mathematical utilities)
- convex-ffi (FFI layer)

Set up each crate with appropriate dependencies and basic module structure. Include proper workspace configuration for shared dependencies.
```

### Core Types Development

```
Implement the core domain types in convex-core:

1. Date type with business day arithmetic
2. Price newtype with currency support (USD, EUR, GBP, JPY)
3. Yield type with convention support (ACT/360, ACT/365, 30/360)
4. Spread type in basis points
5. Currency enum

Each type should:
- Use rust_decimal for precision
- Implement Display, Debug, PartialEq
- Include serde support
- Have comprehensive unit tests
- Include documentation with examples

Ensure types prevent invalid states at compile time.
```

## Domain-Specific Prompts

### Day Count Conventions

```
Implement all major day count conventions in convex-core:

Required conventions:
1. ACT/360 - Actual days / 360 (Money market)
2. ACT/365 - Actual days / 365 (UK Gilts)
3. 30/360 US - 30-day months, 360-day year
4. 30E/360 - European convention
5. ACT/ACT ICMA - Actual/Actual per period
6. ACT/ACT ISDA - Actual/Actual with year convention

Each implementation must:
- Handle month-end rules correctly
- Match Bloomberg's implementation exactly
- Include edge case tests (leap years, month boundaries)
- Provide clear documentation with examples
- Pass validation tests against known reference values

Create a DayCounter trait and implement it for each convention.
```

### Yield Curve Construction

```
Implement yield curve bootstrap functionality in convex-curves:

Requirements:
1. Bootstrap from deposit rates (overnight to 1 year)
2. Bootstrap from government bond prices
3. Support multiple interpolation methods:
   - Linear on zero rates
   - Cubic spline
   - Log-linear on discount factors

The implementation should:
- Use rayon for parallel processing when beneficial
- Cache interpolation coefficients
- Handle negative rates correctly
- Provide clear error messages for invalid inputs
- Include comprehensive tests with real market data examples

Target performance: Bootstrap 50-point curve in < 100 microseconds.
```

### Bond Pricing Engine

```
Create a bond pricing engine in convex-bonds that calculates:

1. Clean price from yield
2. Dirty price (including accrued interest)
3. Yield-to-maturity from price using Newton-Raphson
4. Money market yields (Discount Yield, Bond Equivalent Yield)

The implementation must:
- Exactly replicate Bloomberg YAS methodology
- Use sequential roll-forward for yield calculations
- Handle all coupon frequencies (annual, semi-annual, quarterly, monthly)
- Calculate accrued interest correctly for all day count conventions
- Converge within 100 iterations for YTM
- Include tolerance of 1e-10 for convergence

Provide extensive tests comparing outputs to Bloomberg reference values.
```

### Spread Analytics

```
Implement spread calculation modules in convex-spreads:

1. Z-Spread Calculator:
   - Iteratively solve for spread that matches market price
   - Discount all cash flows with spread over zero curve
   - Use Brent method for root finding
   - Target < 50 microseconds per calculation

2. G-Spread Calculator:
   - Interpolate government yield at bond's maturity
   - Simple arithmetic difference
   - Handle interpolation edge cases

3. Asset Swap Spread:
   - Par-par asset swap spread
   - Proceeds asset swap spread
   - Require swap curve as input

Each calculator should have comprehensive tests and match industry standards.
```

## Implementation Quality Prompts

### Testing Requirements

```
For the [MODULE_NAME] module, create a comprehensive test suite including:

1. Unit Tests:
   - Test each public function with valid inputs
   - Test boundary conditions (zero, negative, very large values)
   - Test error cases and error message clarity
   
2. Property-Based Tests:
   - Invariants that should always hold
   - Relationships between functions (e.g., price increases when yield decreases)
   - Use proptest with appropriate strategies

3. Integration Tests:
   - End-to-end scenarios with real bond data
   - Multi-step workflows (curve building -> bond pricing -> risk calculation)

4. Validation Tests:
   - Compare outputs to Bloomberg/Reuters reference values
   - Known test cases from academic papers
   - Edge cases that have caused issues historically

Ensure test coverage exceeds 90% for the module.
```

### Performance Optimization

```
Profile and optimize the [FUNCTION_NAME] function:

1. First, create a benchmark using criterion:
   - Realistic input data
   - Multiple scenarios (best/average/worst case)
   - Compare against performance target

2. Analyze the profile:
   - Identify hot paths
   - Find allocation bottlenecks
   - Check for unnecessary cloning

3. Apply optimizations:
   - Use inline for small functions
   - Eliminate allocations in hot paths
   - Consider SIMD for vectorizable operations
   - Use iterators instead of loops
   - Pre-allocate with capacity

4. Validate:
   - Ensure correctness is maintained
   - Measure improvement
   - Document optimization decisions

Target: Achieve [X microseconds] per operation.
```

### Documentation Enhancement

```
Enhance documentation for the [MODULE/TYPE/FUNCTION]:

1. Add comprehensive rustdoc comments including:
   - Clear description of purpose and behavior
   - Mathematical formulas in LaTeX notation
   - All parameters with types and constraints
   - Return value and type
   - Possible errors and when they occur
   - Complexity analysis (time and space)
   
2. Include code examples:
   - Basic usage
   - Common patterns
   - Edge cases and how to handle them
   
3. Add cross-references:
   - Related functions/types
   - Academic papers
   - Industry standards (Bloomberg, ISDA)

4. Ensure examples compile and run:
   - Use ```rust,no_run or ```rust for examples
   - Test examples with cargo test --doc
```

## Advanced Feature Prompts

### Callable Bond Support

```
Extend the bond pricing engine to support callable bonds:

1. Define CallSchedule struct:
   - Call dates and call prices
   - Call types (American, European, Bermudan)
   
2. Implement OAS calculation:
   - Build binomial/trinomial interest rate tree
   - Backward induction with optimal exercise
   - Calibrate tree to volatility surface
   
3. Calculate option-adjusted metrics:
   - Option-adjusted spread
   - Effective duration and convexity
   - Option cost (OAS - Z-spread)

The implementation should handle:
- Multiple call dates
- Make-whole call provisions
- Par call with 30-day notice

Include tests with known callable bond examples.
```

### Multi-Curve Framework

```
Implement multi-curve framework for post-crisis discounting:

1. Separate discounting and projection curves:
   - OIS curve for discounting
   - LIBOR/SOFR curve for projections
   
2. Support basis spreads:
   - Tenor basis (3M vs 6M LIBOR)
   - Cross-currency basis
   
3. Implement dual curve bootstrap:
   - Bootstrap projection curve using OIS discounting
   - Handle FRA, futures, swaps consistently

The framework should:
- Be backwards compatible with single curve
- Support transition to SOFR
- Include comprehensive basis documentation

Reference ISDA definitions for all calculations.
```

### FFI and Language Bindings

```
Create C API and Python bindings for the bond pricing functionality:

1. C API (convex-ffi):
   - Define C-compatible structs for Bond, Price, Yield
   - Implement C functions that wrap Rust functions
   - Ensure proper error handling and null safety
   - Generate header file with cbindgen
   
2. Python Bindings (PyO3):
   - Create Python classes wrapping Rust types
   - Implement __repr__, __str__ for user-friendly display
   - Support numpy arrays for batch operations
   - Include type hints for IDE support
   
3. Documentation:
   - Python examples showing common workflows
   - Performance comparison to pure Python
   - Installation instructions

The bindings should be ergonomic and Pythonic while maintaining performance.
```

## Debugging and Troubleshooting Prompts

### Convergence Issues

```
The YTM calculation is not converging for certain bonds. Debug and fix:

1. Add detailed logging:
   - Log each iteration of Newton-Raphson
   - Show price function value and derivative
   - Display convergence criteria check

2. Identify problematic cases:
   - What bond characteristics cause issues?
   - Are there patterns (very low/high coupon, long/short maturity)?

3. Improve algorithm:
   - Add fallback to bisection if Newton-Raphson diverges
   - Adjust initial guess based on bond characteristics
   - Implement better bounds checking

4. Add guards:
   - Maximum iterations limit
   - Detect oscillation
   - Return appropriate error with context

Include test cases for previously failing bonds.
```

### Precision Issues

```
Some yield calculations differ from Bloomberg by more than acceptable tolerance. Investigate:

1. Compare step-by-step:
   - Cash flow dates and amounts
   - Day count fractions
   - Discount factors
   - Present values

2. Check numerical precision:
   - Are we using Decimal where needed?
   - Any float to decimal conversions?
   - Rounding differences?

3. Verify methodology:
   - Is sequential roll-forward implemented correctly?
   - Are we handling compounding frequency properly?
   - Edge cases (stub periods, irregular payments)?

4. Create detailed test:
   - Show expected vs actual at each step
   - Document any intentional differences
   - Add tolerance assertions

Ensure we match Bloomberg to within 1e-6 for all test cases.
```

## Code Review Prompts

### Review Checklist

```
Review the following module/PR for quality and correctness:

1. Correctness:
   - Are algorithms implemented correctly?
   - Do tests cover edge cases?
   - Are error conditions handled properly?

2. Performance:
   - Any obvious inefficiencies?
   - Unnecessary allocations?
   - Could benefit from parallelization?

3. API Design:
   - Is the API ergonomic?
   - Clear naming and documentation?
   - Follows Rust conventions?

4. Safety:
   - Any unsafe code? Is it necessary and documented?
   - Proper error handling, no panics?
   - Thread safety considerations?

5. Maintainability:
   - Clear code structure?
   - Good separation of concerns?
   - Sufficient comments for complex logic?

Provide specific feedback and suggestions for improvement.
```

## Maintenance and Evolution Prompts

### Add New Bond Type

```
Add support for [NEW_BOND_TYPE] (e.g., floating rate notes, inflation-linked):

1. Define the bond type:
   - Struct with relevant fields
   - Builder pattern for construction
   - Validation of inputs

2. Implement pricing:
   - Cash flow generation specific to this type
   - Any special discounting considerations
   - Integration with existing pricing engine

3. Add risk calculations:
   - Duration and convexity
   - Any type-specific risk metrics

4. Documentation and tests:
   - Examples of usage
   - Comparison to market standards
   - Edge case handling

Ensure backward compatibility with existing code.
```

### Performance Regression

```
Recent changes have caused performance degradation. Investigate and fix:

1. Run benchmarks:
   - Compare current performance to baseline
   - Identify which operations regressed
   - Measure severity of regression

2. Profile the code:
   - Find hot paths that changed
   - Look for new allocations
   - Check for algorithmic changes

3. Fix the regression:
   - Revert problematic changes if necessary
   - Optimize new code
   - Consider alternative approaches

4. Prevent future regressions:
   - Add benchmark to CI
   - Set performance thresholds
   - Document performance requirements

Document findings and solution in memory.md.
```

## Tips for Effective Prompts

### Be Specific
```
❌ "Implement bond pricing"
✅ "Implement fixed-rate bond pricing that calculates clean price from YTM using the formula P = Σ(CF_i / (1+y/f)^t_i), handling semi-annual coupons with 30/360 day count"
```

### Provide Context
```
❌ "Fix the bug"
✅ "The YTM calculation fails for zero-coupon bonds because the cash flow iterator returns empty. Update generate_cash_flows() to handle zero-coupon case by returning a single redemption payment."
```

### Reference Standards
```
❌ "Calculate duration"
✅ "Calculate Modified Duration using the formula D_mod = -1/P * dP/dy, matching Bloomberg's DUR function output. Test against US Treasury examples."
```

### Set Quality Bars
```
❌ "Write tests"
✅ "Write comprehensive tests achieving >95% coverage including: unit tests for each function, property tests for invariants, integration tests with real bond data, and validation tests comparing to Bloomberg reference values."
```

---

*These prompts are designed to guide Claude Code in building a production-quality fixed income analytics library.*
