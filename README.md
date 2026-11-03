# Convex

[![CI](https://github.com/sujitn/convex/actions/workflows/ci.yml/badge.svg)](https://github.com/sujitn/convex/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/convex-analytics.svg)](https://crates.io/crates/convex-analytics)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Convex is a fixed income analytics library written in Rust. It prices bonds,
solves yields and spreads, builds yield and credit curves, values interest rate
swaps and single-name CDS, and computes risk and hedges.

You can use it as a set of Rust crates, through a JSON API from C, Java, Excel,
WebAssembly or an MCP client, or as a pricing server. All of these call the
same Rust code.

Try it in the browser: [Convex Terminal](https://sujitn.github.io/convex/)
runs the WebAssembly build against free public market data (US Treasury, New
York Fed, ECB, Bank of Canada).

## Status

Version 0.14.0. Until 1.0, any minor release can change the API; the
[changelog](CHANGELOG.md) lists what changed.

CI compares Convex with stored outputs from QuantLib 1.40 (day counts, fixed
rate bonds and swaps) and the ISDA CDS Standard Model 1.8.3 (CDS). The bond
pack, 392 measures over 70 day-count cases and 42 bonds, currently passes in
full. [reconciliation/COMPARISON.md](reconciliation/COMPARISON.md) lists the
cases and tolerances. Nothing here has been reviewed by a vendor or regulator,
so check results against your own systems before relying on them.

## Rust crates

Seven crates are on crates.io:

| Crate | What it holds |
|---|---|
| [`convex-math`](crates/convex-math) | Root finders, interpolation, extrapolation, exact decimal sums, platform-independent `exp`/`log` |
| [`convex-core`](crates/convex-core) | Dates, prices, yields, day counts, holiday calendars, coupon schedules, fixings |
| [`convex-curves`](crates/convex-curves) | Discount, zero and credit curves; fitting to market quotes; curves described as data |
| [`convex-rates`](crates/convex-rates) | Fixed/floating and OIS swap contracts, their cash flows and valuation |
| [`convex-credit`](crates/convex-credit) | Single-name CDS valuation and running/upfront quote conversion |
| [`convex-bonds`](crates/convex-bonds) | Fixed, zero, floating, callable and sinking-fund bonds; cash flows; accrued; yield solver |
| [`convex-analytics`](crates/convex-analytics) | Yields, spreads, duration, DV01, callable bond models, curve calibration, futures, hedging |

They build on each other in that order. `convex-analytics` does not re-export
the lower crates, so add the ones you use:

```toml
[dependencies]
convex-analytics = "0.14"
convex-bonds = "0.14"
convex-core = "0.14"
convex-curves = "0.14"
rust_decimal_macros = "1.40"
```

The minimum Rust version is 1.95.

### Yield to maturity

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

    println!("Yield to Maturity: {:.4}%", ytm.yield_percent()?); // 5.3454%

    Ok(())
}
```

### Z-spread

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

    // Z-spread that reprices the bond to a dirty price of 97.50.
    let spread = z_spread(&bond, dec!(97.50), &curve, settlement)?;
    println!("Z-Spread: {:.2} bps", spread.as_bps()); // 57.58 bps

    Ok(())
}
```

Both programs are in `crates/convex-analytics/examples/` (`readme_quickstart`
and `readme_zspread`). Each crate's README has more examples, and they run as
doctests.

## The `library()` JSON API

`convex::library` takes one JSON request (the instrument terms, the market
inputs and the calculation you want) and returns one JSON response. The C ABI
(`convex_library`), the Java binding, Excel (`CX.LIBRARY`), WebAssembly
(`library()`) and the MCP server all use this same request format, so a request
that works in one works in the others. Exact decimal amounts travel as strings.

The `convex` crate that provides it is not on crates.io (the name belongs to
another project). Use a git dependency:

```toml
convex = { git = "https://github.com/sujitn/convex", tag = "v0.14.0" }
```

[docs/LIBRARY-API.md](docs/LIBRARY-API.md) describes the request format, and
`tests/fixtures/library/cases.json` has working requests for each operation.

## Other ways to run it

| | How to get it | Docs |
|---|---|---|
| WebAssembly | npm tarballs (`web` and `nodejs`) on each [GitHub release](https://github.com/sujitn/convex/releases), or build with `wasm-pack` | [crates/convex-wasm](crates/convex-wasm/README.md) |
| Excel add-in | Zip on each GitHub release; load `Convex.Excel64.xll` in 64-bit Excel. Functions are named `CX.*` (`CX.BOND`, `CX.PRICE`, `CX.YAS`, ...) | [excel/](excel/README.md) |
| Java | Build from source; needs JDK 22+ (uses the Foreign Function & Memory API). Not on Maven Central yet | [bindings/java](bindings/java/README.md) |
| C | `convex-ffi` builds a shared library with a JSON-in, JSON-out C interface | [crates/convex-ffi](crates/convex-ffi/README.md) |
| MCP server | `cargo run -p convex-mcp --bin convex-mcp-server` | [crates/convex-mcp](crates/convex-mcp/README.md) |
| Pricing server | `convex-server`: REST and WebSocket, live quotes, snapshots and replay. A demo front end streams from a hosted instance at <https://convex-demo.pages.dev/> | [crates/convex-server](crates/convex-server/README.md) |
| Browser app | Convex Terminal at <https://sujitn.github.io/convex/> | [www/](www/README.md) |

## Workspace

Besides the seven published crates, the repository holds:

| Crate | Purpose |
|---|---|
| `convex` | Facade that re-exports the library and provides `library()` |
| `convex-portfolio` | Portfolio and ETF analytics |
| `convex-ffi` | C ABI used by Excel and Java |
| `convex-wasm` | WebAssembly build |
| `convex-mcp` | MCP server |
| `convex-ports` | Interfaces for market data, reference data and storage |
| `convex-engine` | Pricing runtime: calculation graph, live valuation, snapshots |
| `convex-ext-file`, `convex-ext-redb` | File and redb storage adapters |
| `convex-server` | HTTP and WebSocket server |

## Building from source

You need the Rust toolchain pinned in `rust-toolchain.toml` (rustup installs it
automatically) and Python 3.

```sh
git clone https://github.com/sujitn/convex.git
cd convex

# Unpack the compressed test inputs the full test suite reads
python3 tools/fixtures.py materialize

cargo build --locked
cargo test --locked -p convex-analytics      # one crate
cargo test --locked --workspace              # everything (slow)
cargo doc --no-deps --open
```

[docs/TEST-DATA.md](docs/TEST-DATA.md) explains the packed fixtures. The
Excel and Java bindings load the C library built with
`cargo build --locked --profile excel -p convex-ffi`, which keeps panics from
crashing the host process.

## Documentation

- API reference: [docs.rs/convex-analytics](https://docs.rs/convex-analytics)
  and the other crates on docs.rs.
- Topic guides (curves, bond quotes, swaps, CDS, risk, hedging, lifecycle):
  [docs/](docs/README.md).
- What changed: [CHANGELOG.md](CHANGELOG.md).

## Contributing

Issues and pull requests are welcome at
<https://github.com/sujitn/convex/issues>. CI runs formatting, Clippy with
warnings as errors, the full test suite and the comparison checks above.

## License

MIT. See [LICENSE](LICENSE).

The CDS code in `convex-credit` follows the ISDA CDS Standard Model, and its
license requires this notice:

This application is based on the ISDA CDS Standard Model (version 1.8.3), developed and supported in collaboration with Markit Group Ltd.

The [ISDA CDS Standard Model Public License](licenses/ISDA-CDS-Standard-Model-Public-License-1.0.txt)
applies to that part.
