# Convex Quick Start Guide

## Prerequisites

### Required Tools
- **Rust**: Latest stable version (1.75+)
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

- **Claude Code**: Install from [claude.ai/code](https://claude.ai/code)
  ```bash
  # macOS/Linux
  curl -fsSL https://claude.ai/download/code | sh
  
  # Verify installation
  claude --version
  ```

### Optional Tools (Recommended)
- **cargo-edit**: For managing dependencies
  ```bash
  cargo install cargo-edit
  ```

- **cargo-watch**: For auto-recompilation
  ```bash
  cargo install cargo-watch
  ```

- **cargo-nextest**: Faster test runner
  ```bash
  cargo install cargo-nextest
  ```

- **criterion**: Benchmarking (added to dependencies)

- **cargo-flamegraph**: Performance profiling
  ```bash
  cargo install flamegraph
  ```

## Project Setup

### 1. Initialize the Project

```bash
# Create project directory
mkdir convex
cd convex

# Copy the .claude directory with all config files
cp -r /path/to/downloaded/.claude .

# Initialize git repository
git init
git add .claude/
git commit -m "Initial Claude Code configuration"
```

### 2. Start Claude Code

```bash
# Start Claude Code in the project directory
claude code

# Or start with specific context
claude code --context .claude/context.md
```

### 3. First Prompt to Claude

Copy and paste this into Claude Code:

```
I'm starting the Convex fixed income analytics library. Please:

1. Create the Cargo workspace structure with all crates (convex-core, convex-curves, convex-bonds, convex-spreads, convex-math, convex-ffi)
2. Set up the workspace Cargo.toml with shared dependencies
3. Create basic module structure for each crate
4. Add essential dependencies (chrono, rust_decimal, thiserror, serde)
5. Create a basic example in the top-level README.md

Follow the architecture described in .claude/architecture.md and conventions in .claude/conventions.md.
```

## Development Workflow

### Typical Claude Code Session

1. **Start with Context**
   ```bash
   claude code
   ```

2. **Define Task Clearly**
   ```
   Implement the ACT/360 day count convention in convex-core with:
   - DayCounter trait implementation
   - Comprehensive unit tests
   - Documentation with examples
   - Comparison tests against known values
   ```

3. **Review Output**
   - Claude will generate code following conventions
   - Review for correctness and style
   - Ask for modifications if needed

4. **Iterate**
   ```
   The ACT/360 implementation looks good, but can you:
   - Add property-based tests for edge cases
   - Optimize the day_count_fraction calculation
   - Add more detailed documentation with LaTeX formulas
   ```

5. **Test and Commit**
   ```bash
   cargo test
   cargo clippy
   cargo fmt
   git add .
   git commit -m "Implement ACT/360 day count convention"
   ```

### Best Practices for Working with Claude Code

**Be Specific and Contextual**
```
❌ "Add bond pricing"
✅ "Implement fixed-rate bond clean price calculation in convex-bonds using 
   the present value formula with semi-annual compounding. Include YTM 
   solver using Newton-Raphson with convergence tolerance of 1e-10."
```

**Reference Documentation**
```
"Implement the yield curve bootstrap as described in .claude/context.md, 
following the conventions in .claude/conventions.md. Use the architecture 
outlined in .claude/architecture.md for module structure."
```

**Incremental Development**
```
Phase 1: "Create basic Bond struct with builder pattern"
Phase 2: "Add cash flow generation"
Phase 3: "Implement price calculation from yield"
Phase 4: "Add YTM calculation from price"
```

**Request Comprehensive Testing**
```
"Along with the implementation, create:
- Unit tests for each function
- Property tests for invariants
- Integration test with real US Treasury bond
- Validation test comparing to Bloomberg reference value"
```

## Common Development Tasks

### Adding a New Bond Type

```
I want to add support for floating rate notes. Please:

1. Create a FloatingRateNote struct in convex-bonds/src/instruments/
2. Implement cash flow generation with spread over reference rate
3. Add pricing calculation using forward projection
4. Create builder pattern for construction
5. Add comprehensive tests
6. Update documentation

The implementation should follow existing patterns for FixedRateBond 
and support LIBOR and SOFR reference rates.
```

### Implementing a New Spread Type

```
Implement I-Spread (Interpolated Spread) calculation in convex-spreads:

1. Interpolate government yield at bond's exact maturity
2. Calculate spread as bond YTM minus interpolated yield
3. Support multiple interpolation methods (linear, cubic spline)
4. Handle edge cases (maturity beyond curve, negative spreads)
5. Add benchmarks to ensure < 10 microsecond calculation time
6. Include tests comparing to Bloomberg I-Spread function
```

### Optimizing Performance

```
The bond portfolio pricing is taking too long. Please:

1. Profile the current implementation
2. Identify bottlenecks (likely in discount_cash_flows)
3. Apply optimizations:
   - Pre-allocate vectors with capacity
   - Use iterators instead of loops
   - Consider SIMD for discount factor calculations
   - Use rayon for parallel pricing of multiple bonds
4. Create before/after benchmarks
5. Document optimization decisions in memory.md

Target: Price 1000 bonds in under 10 milliseconds.
```

### Creating Language Bindings

```
Create Python bindings for the core bond pricing functionality:

1. Set up PyO3 in convex-python crate
2. Wrap Bond, YieldCurve, and pricing functions
3. Create Pythonic API with type hints
4. Add __repr__ and __str__ for friendly display
5. Include examples in docstrings
6. Create setup.py for pip installation
7. Write Python tests comparing to Rust implementation

The API should feel natural to Python users while maintaining performance.
```

## Testing Strategy

### Running Tests

```bash
# All tests
cargo test

# Specific crate
cargo test -p convex-core

# Specific test
cargo test test_act_360_day_count

# With output
cargo test -- --nocapture

# Fast test runner (if installed)
cargo nextest run
```

### Running Benchmarks

```bash
# All benchmarks
cargo bench

# Specific benchmark
cargo bench ytm_calculation

# With profiling
cargo bench --bench bond_pricing -- --profile-time=5
```

### Code Quality Checks

```bash
# Linting
cargo clippy -- -D warnings

# Formatting
cargo fmt --check

# Documentation
cargo doc --open

# Check everything
cargo check --all-features
```

## Troubleshooting

### Claude Doesn't Follow Conventions

```
Please review .claude/conventions.md and ensure the implementation:
- Uses newtypes for domain concepts
- Implements proper error handling with thiserror
- Includes comprehensive documentation
- Follows naming conventions (snake_case for functions, PascalCase for types)
- Has inline annotations for performance-critical functions
```

### Test Failures

```
The test_ytm_calculation is failing. Please:
1. Debug the issue by adding detailed logging
2. Compare expected vs actual values at each step
3. Identify where the calculation diverges
4. Fix the implementation
5. Add a test to prevent regression
```

### Performance Issues

```
Profiling shows discount_cash_flows is taking 80% of execution time. Please:
1. Review the implementation for inefficiencies
2. Look for unnecessary allocations or cloning
3. Consider using iterators and fold instead of collect
4. Add inline annotations
5. Benchmark the improvement
```

## Project Structure Reference

After initial setup, your project should look like:

```
convex/
├── .claude/
│   ├── context.md           # Domain knowledge
│   ├── architecture.md      # System design
│   ├── memory.md           # Decisions log
│   ├── conventions.md      # Coding standards
│   └── prompts.md          # Prompt examples
├── Cargo.toml              # Workspace config
├── README.md
├── LICENSE
├── crates/
│   ├── convex-core/
│   ├── convex-curves/
│   ├── convex-bonds/
│   ├── convex-spreads/
│   ├── convex-math/
│   └── convex-ffi/
├── examples/
├── benchmarks/
└── tests/
```

## Next Steps

1. **Start with Core Types**
   ```
   Let's begin by implementing the core types in convex-core:
   - Date with business day arithmetic
   - Price with currency support
   - Yield with convention support
   - Spread in basis points
   
   Include full documentation and tests for each.
   ```

2. **Build Day Count Foundations**
   ```
   Implement all day count conventions following Bloomberg methodology:
   - ACT/360, ACT/365, 30/360, ACT/ACT ICMA, ACT/ACT ISDA
   - With comprehensive tests comparing to known values
   ```

3. **Create Yield Curve Infrastructure**
   ```
   Build the yield curve construction framework:
   - Bootstrap algorithm
   - Linear and cubic spline interpolation
   - Tests with real market data
   ```

4. **Implement Bond Pricing**
   ```
   Create bond pricing engine:
   - Fixed-rate bonds
   - YTM calculation using Newton-Raphson
   - Clean and dirty price calculations
   - Validation against Bloomberg
   ```

## Resources

- **Context**: `.claude/context.md` - Domain knowledge and requirements
- **Architecture**: `.claude/architecture.md` - System design diagrams
- **Conventions**: `.claude/conventions.md` - Coding standards
- **Prompts**: `.claude/prompts.md` - Example prompts for common tasks
- **Memory**: `.claude/memory.md` - Decisions and progress tracking

## Support

When working with Claude Code:
- Reference the .claude files frequently
- Be specific in your requests
- Ask for tests and documentation
- Request performance benchmarks
- Have Claude update memory.md with decisions

---

**Ready to Start?**

Run `claude code` in the convex directory and begin with the first prompt above!
