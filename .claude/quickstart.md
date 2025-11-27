# Convex Quick Start Guide

## Prerequisites

### Required Tools

**Rust (1.75+):**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup update stable
```

**Claude Code:**
```bash
# Install from claude.ai/code
claude --version
```

### Recommended Tools

```bash
# Dependency management
cargo install cargo-edit

# Fast test runner
cargo install cargo-nextest

# Benchmarking (added to dev-dependencies)
# criterion is used in the project

# Profiling
cargo install flamegraph

# Code coverage
cargo install cargo-tarpaulin
```

---

## Project Setup

### 1. Create Project Directory

```bash
mkdir convex
cd convex

# Copy .claude configuration
cp -r /path/to/.claude .

# Initialize git
git init
git add .claude/
git commit -m "Initial Claude Code configuration"
```

### 2. Start Claude Code

```bash
claude code
```

### 3. First Prompt

```
I'm starting the Convex fixed income analytics library. Please:

1. Read all files in .claude/ to understand requirements
2. Create the Cargo workspace with crates:
   - convex-core (types, day counts, calendars)
   - convex-math (solvers, interpolation, optimization)
   - convex-curves (yield curve construction)
   - convex-bonds (all bond types)
   - convex-spreads (spread calculations)
   - convex-risk (risk analytics)
   - convex-yas (Bloomberg YAS replication)
   - convex-ffi (FFI layer)
3. Set up workspace Cargo.toml with shared dependencies
4. Create basic module structure in each crate

Follow the architecture in .claude/architecture.md.
```

---

## Development Workflow

### Session Start

Always begin with:

```
Please read .claude/context.md, .claude/architecture.md, and .claude/memory.md
to understand the project state. Then run `tree -L 2 src/` to see current structure.
```

### Feature Implementation Pattern

```
## Task: Implement [FEATURE] in [CRATE]

### Pre-Implementation
Read .claude/context.md section: [RELEVANT_SECTION]
Review existing code in [MODULE_PATH]

### Requirements
[Detailed requirements from prompts.md]

### Validation
- Tolerance: [±X]
- Test bond: [CUSIP or description]

### Deliverables
- [ ] Implementation with docs
- [ ] Unit tests
- [ ] Bloomberg validation test
- [ ] Benchmark (if performance-critical)
```

### Session End

```
Please update .claude/memory.md with:
- What was implemented today
- Any decisions made
- Validation status
- Open issues or next steps
```

---

## Implementation Order

### Phase 1: Foundation (Week 1-2)

```
1. Core types (Price, Yield, Spread, Rate)
   - Use Decimal for precision
   - Implement Display, From, TryFrom
   
2. Day count conventions
   - Start with ACT/360, 30/360 US
   - Must match Bloomberg exactly
   
3. Holiday calendars
   - SIFMA, US Government first
   - Bitmap storage for O(1) lookup
```

### Phase 2: Math Engine (Week 3-4)

```
1. Solvers
   - Newton-Raphson (primary)
   - Brent's method (fallback)
   
2. Interpolation
   - Linear (baseline)
   - Monotone Convex (production)
   - Cubic Spline
   
3. Extrapolation
   - Flat
   - Smith-Wilson (regulatory)
```

### Phase 3: Curves (Week 5-6)

```
1. Curve traits and types
2. Bootstrap from deposits
3. Bootstrap from swaps
4. Multi-curve framework
```

### Phase 4: Bond Pricing (Week 7-10)

```
1. Fixed-rate bonds + Boeing validation
2. US Treasury (Notes, Bonds, Bills)
3. Zero coupon
4. UK Gilts, German Bunds
5. TIPS
```

### Phase 5: Spreads & Risk (Week 11-14)

```
1. G-Spread, I-Spread
2. Z-Spread (Brent solver)
3. Duration (all types)
4. Convexity, DV01
```

### Phase 6: Advanced (Week 15-20)

```
1. Callable bonds (binomial tree)
2. OAS calculation
3. Municipal bonds
4. MBS Pass-through
5. Convertibles
```

### Phase 7: Production (Week 21-24)

```
1. Full Bloomberg validation
2. Performance optimization
3. FFI layer
4. Python bindings
5. Documentation complete
```

---

## Testing Commands

```bash
# Run all tests
cargo test --workspace

# Run specific crate
cargo test -p convex-bonds

# Run Bloomberg validation
cargo test --test bloomberg_validation

# Run with output
cargo test -- --nocapture

# Fast test runner
cargo nextest run

# Run benchmarks
cargo bench

# Coverage report
cargo tarpaulin --out Html
```

## Quality Commands

```bash
# Linting
cargo clippy --workspace -- -D warnings

# Formatting
cargo fmt --workspace

# Check formatting
cargo fmt --workspace -- --check

# Documentation
cargo doc --workspace --open

# Check all
cargo check --all-features
```

---

## Key Files Reference

| File | Purpose |
|------|---------|
| `.claude/context.md` | Domain knowledge, accuracy requirements |
| `.claude/architecture.md` | System design, crate structure |
| `.claude/memory.md` | Decisions, progress, validation status |
| `.claude/prompts.md` | Prompt templates for all features |

---

## Validation Targets

### Primary: Boeing 7.5% 06/15/2025

```
CUSIP: 097023AH7
Settlement: 04/29/2020
Price: 110.503

Expected Values:
├── Street Convention: 4.905895%  (±0.00001%)
├── G-Spread: 448.5 bps           (±0.1 bps)
├── Z-Spread: 444.7 bps           (±0.1 bps)
├── Modified Duration: 4.209      (±0.001)
├── Convexity: 0.219              (±0.001)
└── Accrued Interest: 26,986.11   (±0.01)
```

### Performance Targets

| Operation | Target |
|-----------|--------|
| Bond pricing | < 1μs |
| YTM calculation | < 1μs |
| Z-spread | < 50μs |
| OAS (100 steps) | < 10ms |
| Curve bootstrap | < 100μs |
| Portfolio (1000) | < 100ms |

---

## Common Issues & Solutions

### Precision Issues

```
Problem: Calculations differ from Bloomberg

Solution:
1. Use Decimal for all financial math
2. Check day count implementation (month-end rules)
3. Verify settlement date calculation
4. Check sequential roll-forward for short bonds
```

### Convergence Failures

```
Problem: YTM solver doesn't converge

Solution:
1. Check initial guess (use coupon rate)
2. Add bounds checking
3. Fall back to Brent's method
4. Log iterations for debugging
```

### Performance Issues

```
Problem: Calculations too slow

Solution:
1. Profile with flamegraph
2. Check for allocations in hot path
3. Use iterators instead of collect()
4. Pre-allocate vectors with capacity
5. Cache interpolation coefficients
```

---

## Next Steps After Setup

1. **Start with core types**: Get Price, Yield, Spread working
2. **Add day counts**: ACT/360 and 30/360 US first
3. **Build yield solver**: Newton-Raphson with tests
4. **Create first bond**: Fixed-rate with Boeing validation
5. **Iterate**: Add types, validate, optimize

---

## Resources

- **Bloomberg YAS Reference**: Internal docs
- **ISDA Day Count Definitions**: industry standard
- **ICMA Bond Calculation Rules**: coupon calculations
- **EIOPA Smith-Wilson**: regulatory curve extrapolation

---

**Ready?** Run `claude code` and start with the first prompt!
