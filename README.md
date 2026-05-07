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
- **Language Bindings**: C FFI bindings available; Python, Java, C# (coming soon)

## Quick Start

### Installation

Convex is an internal workspace — it is not published to crates.io. Depend on the
crate(s) you need via a git dependency:

```toml
[dependencies]
convex-bonds    = { git = "https://github.com/sujitn/convex.git" }
convex-analytics = { git = "https://github.com/sujitn/convex.git" }
convex-curves   = { git = "https://github.com/sujitn/convex.git" }
```

or clone the repo and use a path dependency.

### Example: Pricing a US Treasury Bond

```rust
use convex::prelude::*;
use rust_decimal_macros::dec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a US Treasury bond
    let bond = Bond::builder()
        .isin("US912828Z229")
        .coupon_rate(dec!(2.5))
        .maturity(Date::from_ymd(2030, 5, 15))
        .frequency(Frequency::SemiAnnual)
        .day_count(DayCount::ActAct)
        .build()?;

    // Build a yield curve from market data
    let curve = YieldCurve::bootstrap()
        .add_deposit(dec!(0.015), Period::Months(3))
        .add_deposit(dec!(0.018), Period::Months(6))
        .add_bond(dec!(98.50), Date::from_ymd(2027, 5, 15))
        .add_bond(dec!(97.25), Date::from_ymd(2032, 5, 15))
        .interpolation(Interpolation::CubicSpline)
        .build()?;

    // Price the bond
    let settlement = Date::today();
    let price = bond.price(&curve, settlement)?;
    
    println!("Clean Price: {:.4}", price.clean());
    println!("Dirty Price: {:.4}", price.dirty());
    println!("Accrued Interest: {:.4}", price.accrued());

    // Calculate yield
    let ytm = bond.yield_to_maturity(price.clean(), settlement)?;
    println!("Yield to Maturity: {:.4}%", ytm.as_percentage());

    // Calculate risk metrics
    let duration = bond.modified_duration(&curve, settlement)?;
    let convexity = bond.convexity(&curve, settlement)?;
    let dv01 = bond.dv01(&curve, settlement)?;

    println!("Modified Duration: {:.4}", duration);
    println!("Convexity: {:.4}", convexity);
    println!("DV01: ${:.2}", dv01);

    Ok(())
}
```

### Example: Z-Spread Calculation

```rust
use convex::prelude::*;
use rust_decimal_macros::dec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bond = Bond::builder()
        .isin("US459200KJ18")  // IBM Corporate Bond
        .coupon_rate(dec!(3.75))
        .maturity(Date::from_ymd(2028, 11, 15))
        .frequency(Frequency::SemiAnnual)
        .day_count(DayCount::Thirty360)
        .build()?;

    // Government curve for discounting
    let gov_curve = YieldCurve::from_market_data(/* ... */)?;
    
    // Market price
    let market_price = Price::new(dec!(102.50), Currency::USD);
    
    // Calculate Z-spread
    let z_spread = bond.z_spread(market_price, &gov_curve, Date::today())?;
    
    println!("Z-Spread: {:.2} bps", z_spread.as_basis_points());

    Ok(())
}
```

## Hedge Advisor

> **Research tool, not an execution recommender.** Costs come from a
> labeled `heuristic_v1` table; futures use a synthetic 6%-coupon
> deliverable (no live CTD basket). Every output stamps
> `provenance.cost_model` and `ComparisonRow.cost_source`.

DV01-neutral hedge proposals with structured tradeoffs. Strategies:
`DurationFutures`, `BarbellFutures`, `CashBondPair`, `InterestRateSwap`.

### MCP tools

```
compute_position_risk(bond, settlement, mark, notional_face, curve, [key_rate_tenors])
  -> RiskProfile { dv01, modified_duration, key_rate_buckets, …, provenance }

propose_hedges(risk, curve, [constraints])
  -> { proposals: HedgeProposal[…], skipped_strategies: [{strategy, reason}] }

compare_hedges(position, proposals, [constraints])
  -> ComparisonReport { rows, recommendation }

narrate_recommendation(comparison)
  -> { text }   # deterministic template; no LLM call
```

End-to-end test:

```bash
cargo run  -p convex-mcp --bin convex-mcp-server
cargo test -p convex-mcp --lib hedge_advisor_e2e
```

See `docs/hedge-advisor-{investigation,gaps,plan}.md` for design and
`docs/perf-baselines.md` for current benchmark numbers.

### Deferred

OAS marks; CTD basket + repo financing; multi-position book hedging; FX
delta; LLM narration; real cost feeds; KRD constraints.

## Architecture

Convex is organized into several crates for modularity:

```
convex/
├── convex-core        # Core types (Date, Price, Yield, etc.)
├── convex-math        # Mathematical utilities and solvers
├── convex-curves      # Yield curve construction and interpolation
├── convex-bonds       # Bond instruments and definitions
├── convex-analytics   # Unified analytics (yields, spreads, risk)
├── convex-wasm        # WebAssembly bindings
└── convex-ffi         # Foreign Function Interface for language bindings
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
