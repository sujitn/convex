# Convex

**High-Performance Fixed Income Analytics Library in Rust**

[![Build Status](https://github.com/sujitn/convex/workflows/CI/badge.svg)](https://github.com/sujitn/convex/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**[Live Demo](https://convex-demo.pages.dev/)** | **[WASM Demo](https://sujitn.github.io/convex/)** - Try the interactive bond analytics calculator

Convex is a production-grade fixed income analytics library providing comprehensive bond pricing, yield curve construction, and risk analytics with industry-standard methodologies.

## Features

- **Bond Pricing**: All major bond types (government, corporate, callable, putable, sinking fund)
- **Yield Calculations**: YTM, YTC, YTW with industry-standard methodology
- **Yield Curves**: Bootstrap from market data with multiple interpolation methods
- **Spread Analytics**: Z-spread, G-spread, I-spread, Asset Swap spreads, OAS
- **Risk Metrics**: Duration (Macaulay, Modified, Effective, Key Rate), Convexity, DV01
- **Hedge Advisor**: AI-friendly proposal layer — DV01-neutral hedges via bond futures or IRS, with structured tradeoffs and a deterministic narrator (research tool, not an execution recommender)
- **Day Count Conventions**: ACT/360, ACT/365, 30/360, ACT/ACT (ICMA, ISDA)
- **Holiday Calendars**: SIFMA, TARGET2, UK, Japan with O(1) lookups + dynamic calendars
- **High Performance**: Microsecond-level pricing
- **Type Safety**: Leverage Rust's type system to prevent errors
- **WebAssembly**: Full browser support via wasm-pack ([Live Demo](https://convex-demo.pages.dev/) | [WASM Demo](https://sujitn.github.io/convex/))
- **Language Bindings**: C FFI and Java (published to Maven Central) available; Python and C# planned

## Quick Start

### Installation

The core library crates are published to [crates.io](https://crates.io). Most
users only need `convex-analytics`, which re-exports the lower layers (core,
math, curves, bonds):

```toml
[dependencies]
convex-analytics = "0.13"
```

The full set of published crates: `convex-core`, `convex-math`,
`convex-curves`, `convex-bonds`, `convex-analytics`.

> **Note:** the `convex` umbrella crate is *not* on crates.io — that name
> belongs to an unrelated project ([convex.dev](https://convex.dev)). The
> umbrella facade, along with the FFI, WASM, MCP and pricing-server crates,
> ships in-repo only. Depend on them via git or a path dependency:
>
> ```toml
> convex = { git = "https://github.com/sujitn/convex.git" }
> ```

> The two snippets below are mirrored by compile-checked examples — run them
> with `cargo run -p convex-analytics --example readme_quickstart` and
> `--example readme_zspread`.

### Example: Build a Bond and Compute Yield

```rust
use convex_analytics::functions::yield_to_maturity;
use convex_bonds::instruments::FixedRateBond;
use convex_core::types::{Date, Frequency};
use rust_decimal_macros::dec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A 5% semi-annual bond. Coupon is a decimal (0.05 == 5%); day count and
    // calendar default to 30/360 US and SIFMA.
    let bond = FixedRateBond::builder()
        .cusip_unchecked("912828Z29")
        .coupon_rate(dec!(0.05))
        .issue_date(Date::from_ymd(2020, 5, 15)?)
        .maturity(Date::from_ymd(2030, 5, 15)?)
        .frequency(Frequency::SemiAnnual)
        .build()?;

    // Yield to maturity from a clean price of 98.50 (per 100 face).
    let settlement = Date::from_ymd(2025, 5, 15)?;
    let ytm = yield_to_maturity(&bond, settlement, dec!(98.50), Frequency::SemiAnnual)?;

    println!("Yield to Maturity: {:.4}%", ytm.yield_percent());

    Ok(())
}
```

### Example: Z-Spread Calculation

```rust
use convex_analytics::spreads::z_spread;
use convex_bonds::instruments::FixedRateBond;
use convex_core::types::{Date, Frequency};
use convex_curves::curves::DiscountCurveBuilder;
use rust_decimal_macros::dec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 3.75% semi-annual corporate bond.
    let bond = FixedRateBond::builder()
        .cusip_unchecked("459200KJ1")
        .coupon_rate(dec!(0.0375))
        .issue_date(Date::from_ymd(2018, 11, 15)?)
        .maturity(Date::from_ymd(2028, 11, 15)?)
        .frequency(Frequency::SemiAnnual)
        .build()?;

    // A flat 4% continuously-compounded discount curve to spread against.
    let settlement = Date::from_ymd(2025, 1, 15)?;
    let rate = 0.04_f64;
    let curve = DiscountCurveBuilder::new(settlement)
        .add_pillar(1.0, (-rate * 1.0).exp())
        .add_pillar(2.0, (-rate * 2.0).exp())
        .add_pillar(5.0, (-rate * 5.0).exp())
        .add_pillar(10.0, (-rate * 10.0).exp())
        .with_extrapolation()
        .build()?;

    // Z-spread that reprices the bond to a dirty price of 102.50.
    let spread = z_spread(&bond, dec!(102.50), &curve, settlement)?;
    println!("Z-Spread: {:.2} bps", spread.as_bps());

    Ok(())
}
```

## Hedge Advisor

> **Research tool, not an execution recommender.** Costs come from a
> labeled `heuristic_v1` table; futures use a synthetic 6%-coupon
> deliverable (no live CTD basket). Every output stamps
> `provenance.cost_model` and `ComparisonRow.cost_source`.

DV01-neutral hedge proposals with structured tradeoffs. Strategies:
`DurationFutures`, `BarbellFutures`, `KeyRateFutures`, `CashBondPair`,
`InterestRateSwap`.

### MCP tools

```text
compute_position_risk(bond, settlement, mark, notional_face, curve, [key_rate_tenors])
  -> RiskProfile { dv01, modified_duration, key_rate_buckets, …, provenance }

aggregate_book_risk(positions, [book_id])
  -> RiskProfile  # net book DV01, KRD union, DV01-weighted durations

propose_hedges(risk, curve, [constraints], [basket_overrides])
  -> { proposals: HedgeProposal[…], skipped_strategies: [{strategy, reason}] }

propose_book_hedges(groups: BookGroup[…])
  -> { groups: [{ group_name, aggregate_risk, contributions, proposals,
                  skipped_strategies }] }

compare_hedges(position, proposals, [constraints])
  -> ComparisonReport { rows, recommendation }

narrate_recommendation(comparison)
  -> { text }   # deterministic template; no LLM call
```

`propose_book_hedges` hedges multiple sleeves with **per-group policy** —
each `BookGroup` carries its own positions, curve, constraints, and
basket overrides. Useful when a rates sleeve and a credit sleeve need
different `allowed_strategies` / cost caps in one call. The output's
`contributions` field decomposes each group's aggregate DV01 by
position (signed DV01 + share of gross), so traders can see who's
driving the book.

End-to-end test:

```bash
cargo run  -p convex-mcp --bin convex-mcp-server
cargo test -p convex-mcp --lib hedge_advisor_e2e
```

See `docs/hedge-advisor-{investigation,gaps,plan}.md` for design and
`docs/perf-baselines.md` for current benchmark numbers.

### Deferred

CTD live basket feed (heuristic synthetic deliverable in place); live
cost feed (heuristic table only); FX delta / cross-currency hedging;
LLM narrator (deterministic template only); CDS / credit DV01;
inflation-linked instruments; ETF-proxy strategy; per-benchmark partial
spread DV01.

## Architecture

Convex is organized into focused crates. The five **library** crates are
published to crates.io; the rest are internal (the facade, language bindings,
and the pricing-engine/server stack) and ship in-repo only.

```
convex/
├── convex-core        # Core types (Date, Price, Yield, calendars, day counts)   [published]
├── convex-math        # Solvers and interpolators (Brent, Newton, LM)            [published]
├── convex-curves      # Yield/credit curves, bootstrapping, multi-curve          [published]
├── convex-bonds       # Bond instruments (fixed, FRN, callable, zero, sinker)    [published]
├── convex-analytics   # Unified analytics: pricing, yields, spreads, risk        [published]
├── convex             # Single-import facade re-exporting the public API          (internal)
├── convex-portfolio   # Portfolio and ETF analytics                               (internal)
├── convex-ffi         # C-ABI FFI for language bindings (Java, Excel)             (internal)
├── convex-wasm        # WebAssembly bindings for the browser demo                 (internal)
├── convex-mcp         # MCP server for tool/agent integration                     (internal)
├── convex-ports       # Hexagonal port traits (market/reference data, storage)    (internal)
├── convex-engine      # Reactive pricing engine with a calculation graph          (internal)
├── convex-ext-file    # File-backed market/reference data adapter                 (internal)
├── convex-ext-redb    # redb embedded-storage adapter                             (internal)
└── convex-server      # REST + WebSocket pricing server (deployed to Fly.io)      (internal)
```

## Performance

Convex is designed for production trading systems with strict performance requirements:

| Operation | Time | Notes |
|-----------|------|-------|
| Bond Price | < 1 μs | Single fixed-rate bond |
| YTM Calculation | < 10 μs | Newton-Raphson convergence |
| Bootstrap 50-point curve | < 100 μs | Parallel processing |
| Z-Spread | < 50 μs | Iterative solver |
| Holiday Lookup | < 10 ns | O(1) bitmap lookup |
| Portfolio (1000 bonds) | < 10 ms | Parallel pricing |

*Benchmarked on AMD Ryzen 9 5950X @ 3.4GHz*

## Day Count Conventions

Convex implements all major day count conventions with industry-standard compliance:

- **ACT/360**: Actual days / 360 (Money market)
- **ACT/365**: Actual days / 365 (UK Gilts)
- **30/360 US**: 30-day months, 360-day year (US Corporate bonds)
- **30E/360**: European 30/360 convention
- **ACT/ACT ICMA**: Actual/Actual per period (Government bonds)
- **ACT/ACT ISDA**: Actual/Actual with year convention (Swaps)

## Holiday Calendars

Convex provides comprehensive holiday calendar support with O(1) bitmap-based lookups:

### Built-in Calendars

| Calendar | Description | Usage |
|----------|-------------|-------|
| `SIFMACalendar` | US fixed income (bond market) | Corporate bonds, Munis |
| `USGovernmentCalendar` | US Treasury securities | Treasuries |
| `Target2Calendar` | Eurozone payments | EUR swaps, Bunds |
| `UKCalendar` | UK bank holidays | Gilts |
| `JapanCalendar` | Japan holidays | JGBs |

### Dynamic Calendars

Load calendars from JSON or create custom calendars at runtime:

```rust
use convex_core::calendars::{DynamicCalendar, CustomCalendarBuilder, WeekendType, Calendar};

// Load from JSON
let cal = DynamicCalendar::from_json(r#"{
    "name": "My Calendar",
    "weekend": "SaturdaySunday",
    "holidays": ["2025-01-01", "2025-12-25"]
}"#)?;

// Build custom calendar
let cal = CustomCalendarBuilder::new("Trading Calendar")
    .weekend(WeekendType::SaturdaySunday)
    .add_fixed_holiday(1, 1)           // New Year's Day
    .add_nth_weekday(1, Weekday::Mon, 3) // MLK Day
    .add_good_friday()
    .add_custom(|year| fetch_holidays_from_db(year))
    .build();

// Dynamic modification
let mut cal = DynamicCalendar::new("Custom", WeekendType::SaturdaySunday);
cal.add_holiday(date);
cal.merge_from(&SIFMACalendar::new());
```

## Yield Calculation Methodology

Convex uses industry-standard methodology for all yield calculations:

1. **Sequential Roll-Forward**: Starting from settlement date, roll forward through each period
2. **Exact Day Counts**: Use actual calendar days with appropriate day count convention
3. **Newton-Raphson Solver**: Converge to 1e-10 tolerance within 100 iterations
4. **Compounding Frequency**: Support annual, semi-annual, quarterly, monthly

This ensures accurate results matching professional trading systems.

## Language Bindings

### Python (PyO3)

```python
from convex import Bond, YieldCurve, Date
from decimal import Decimal

bond = Bond(
    isin="US912828Z229",
    coupon_rate=Decimal("2.5"),
    maturity=Date(2030, 5, 15),
    frequency="semi-annual"
)

curve = YieldCurve.from_market_data(...)
price = bond.price(curve, Date.today())

print(f"Clean Price: {price.clean():.4f}")
print(f"YTM: {bond.ytm(price.clean()):.4f}%")
```

### Java (JNI)

```java
import com.convex.Bond;
import com.convex.YieldCurve;
import java.math.BigDecimal;

Bond bond = new Bond.Builder()
    .isin("US912828Z229")
    .couponRate(new BigDecimal("2.5"))
    .maturity(LocalDate.of(2030, 5, 15))
    .build();

YieldCurve curve = YieldCurve.fromMarketData(...);
Price price = bond.price(curve, LocalDate.now());

System.out.printf("Clean Price: %.4f%n", price.clean());
```

### Excel (XLL Plugin)

```excel
=CONVEX.BOND.PRICE("US912828Z229", TODAY(), "CURVE_USD")
=CONVEX.BOND.YTM("US912828Z229", 98.50, TODAY())
=CONVEX.BOND.DURATION("US912828Z229", "CURVE_USD", TODAY())
```

## Supported Bond Types

### Current Support
- ✅ Fixed-rate bonds (government, corporate)
- ✅ Zero-coupon bonds (T-Bills, discount bonds)
- ✅ Floating-rate notes (SOFR, SONIA, EURIBOR with caps/floors)
- ✅ Callable bonds (American, Bermudan, European, Make-Whole)
- ✅ Putable bonds
- ✅ Sinking fund bonds (with average life calculations)

### Future Support
- 🔜 Convertible bonds
- 🔜 Inflation-linked bonds (TIPS, Linkers)
- 🔜 Asset-backed securities
- 🔜 Mortgage-backed securities

## Building from Source

```bash
# Clone the repository
git clone https://github.com/sujitn/convex.git
cd convex

# Build all crates
cargo build --release

# Run tests
cargo test --all

# Run benchmarks
cargo bench

# Generate documentation
cargo doc --open
```

## Development

### Prerequisites

- Rust 1.75 or later
- Cargo

### Running Tests

```bash
# All tests
cargo test

# Specific crate
cargo test -p convex-core

# With output
cargo test -- --nocapture

# Integration tests
cargo test --test '*'
```

### Code Quality

```bash
# Linting
cargo clippy -- -D warnings

# Formatting
cargo fmt --check

# Check for common mistakes
cargo audit
```

## Contributing

Contributions are welcome! Please open an issue or submit a pull request on GitHub.

### Areas for Contribution

- Additional bond types (inflation-linked, convertibles)
- Additional language bindings
- Performance improvements
- Documentation and examples
- Bug fixes and testing

## Roadmap

### Completed
- [x] Core infrastructure (Date, Price, Yield types)
- [x] Day count conventions (all major conventions)
- [x] Yield curve construction and bootstrapping
- [x] Fixed-rate bond pricing with industry-standard methodology
- [x] Spread calculations (G-spread, Z-spread, I-spread, OAS, ASW)
- [x] Holiday calendars (SIFMA, TARGET2, UK, Japan)
- [x] Dynamic calendar system (JSON loading, custom builders)
- [x] Floating rate notes (SOFR, SONIA, EURIBOR)
- [x] Callable/putable bonds with OAS
- [x] Sinking fund bonds
- [x] Risk metrics (Duration, Convexity, DV01, VaR)
- [x] Multi-curve framework (OIS discounting)
- [x] WebAssembly support with interactive demo
- [x] Advanced interpolation (Nelson-Siegel, Svensson, Monotone Convex)

### In Progress
- [ ] Python bindings (PyO3)
- [ ] Comprehensive documentation
- [ ] Performance optimizations

### Planned
- [ ] Java and C# bindings
- [ ] Excel plugin
- [ ] Convertible bonds
- [ ] Inflation-linked bonds (TIPS, Linkers)
- [ ] REST API service

## Validation

Convex is validated against:
- Industry-standard reference implementations
- Known academic test cases
- Historical market data

All pricing and risk calculations are tested to match professional systems within 1e-6 tolerance.

## Performance Benchmarks

Run benchmarks with:

```bash
cargo bench
```

## Documentation

- API Documentation — generate locally with `cargo doc --no-deps --workspace --open`
- [Live Demo](https://convex-demo.pages.dev/)
- [WASM Demo](https://sujitn.github.io/convex/)

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Inspired by QuantLib design patterns
- Industry-standard fixed income methodologies
- Rust Financial community

## Citation

If you use Convex in academic work, please cite:

```bibtex
@software{convex2025,
  title = {Convex: High-Performance Fixed Income Analytics Library},
  author = {Sujit Nair},
  year = {2025},
  url = {https://github.com/sujitn/convex}
}
```

## Contact

- GitHub Issues: [Bug reports and feature requests](https://github.com/sujitn/convex/issues)

---

Built with ❤️ in Rust 🦀
