# Changelog

Versions before 1.0 can break the API in any minor release. Each entry lists
the changes that matter when you upgrade.

## 0.14.0 (unreleased)

This is the first crates.io release since 0.11.1. The v0.12.0 and v0.13.0 tags
exist on GitHub, but their crates were never published, so this entry covers
everything since 0.11.1.

### Crates

- **New on crates.io:** `convex-rates` (fixed/floating and overnight index
  swap contracts, their cash flows and multi-curve valuation) and
  `convex-credit` (single-name CDS valuation and running/upfront quote
  conversion, following the ISDA CDS Standard Model 1.8.3).
- **Dependency order changed.** `convex-core` now depends on `convex-math`
  (the reverse of before). The published order is math, core, curves, then
  rates, credit and bonds, then analytics.
- **Minimum Rust version is 1.95** (was 1.75).
- Every published crate now ships its LICENSE file. `convex-credit` also
  ships the ISDA CDS Standard Model Public License.
- Workspace-only crates: `convex-traits` is now `convex-ports`;
  `convex-ext-json` was removed. A new `convex` facade crate (not on
  crates.io) re-exports the library and adds the JSON `library()` API.

### convex-math

- New modules: `decimal` (`BoundedDecimal`, exact decimal sums with a size
  limit), `portable` (`exp`, `log`, `pow` from `libm`, so results match across
  platforms), `stats` and `bounds`.
- `libm` is pinned to 0.2.15 with soft floats, and `nalgebra` uses
  `libm-force`. See [docs/PORTABLE-NUMERICS.md](docs/PORTABLE-NUMERICS.md).

### convex-core

- New modules: `cashflows` (shared coupon schedules and amounts), `contracts`
  (request and result types with explicit units), `fixings` (index fixing
  history), `futures` (futures reference terms) and `ids`.
- Many constructors and calculations now return `Result` and reject invalid
  input instead of panicking or rounding it away.

### convex-curves

- New `contracts` module: describe a curve as data (`CurveRecipe`), build a
  set with `build_curve_set`, and read it with `CurveQuery`. Supports node
  curves, fitting to market quotes, Nelson-Siegel and Svensson, Monotone
  Convex, and an ultimate-forward-rate tail. The tail converges forwards to
  the UFR; it is not the regulatory Smith-Wilson method.

### convex-bonds

- New modules: `contracts` (`BondTerms`, a serializable bond description),
  `arrc` (compounded overnight coupons, SOFR style) and `fixings`.
- Removed `risk` (use `convex-analytics`), `indices` (fixing history moved to
  `convex_core::fixings`; overnight compounding to `arrc`) and
  `curve_instruments` (calibration instruments are in `convex-curves`).
- Yield and price functions return `Result`; for example
  `YieldSolver::dirty_price_from_yield` and `clean_price_from_yield`.

### convex-analytics

- Curve calibration: `rate_calibration` (deposits, FRAs, futures, swaps, OIS
  and tenor basis), `swap_calibration`, `cash_calibration` (government and
  repo curves) and `credit_calibration` (CDS curves).
- Callable and putable bonds: `optional_bond`, and Hull-White one-factor
  calibration to swaptions in `calibration`.
- Futures valuation and risk (`futures`, `futures_reference`), quote
  sensitivities (`quote_risk`), credit risk for bonds (`credit_risk`), floating
  coupon caps and floors (`floating_coupon`), trade lifecycle events
  (`lifecycle`) and multi-product workflows (`workflow`).
- Hedge advisor in `risk::hedging`: DV01-neutral proposals using bond futures,
  cash bonds or swaps, a comparison report and a plain-text summary. Costs come
  from a fixed heuristic table unless you supply a `CostFeed`.
- Accuracy fixes to I-spread, OAS, asset swap spreads and several day counts.

### Bindings and applications (not on crates.io)

- JSON `library()` API: one request format shared by Rust (`convex::library`),
  the C ABI (`convex_library`), Java, Excel (`CX.LIBRARY`), WebAssembly and
  MCP. See [docs/LIBRARY-API.md](docs/LIBRARY-API.md).
- Java binding in `bindings/java` (JDK 22+, Panama FFM). Build from source;
  it is not on Maven Central yet.
- Excel add-in reworked around `CX.*` functions, including live-updating
  `.LIVE` variants and `CX.YAS`.
- Convex Terminal, a browser app on the WebAssembly engine with public market
  data: <https://sujitn.github.io/convex/>.
- `convex-server` rebuilt around a single pricing runtime with live quotes,
  snapshots and replay, stored in one redb database.

### Release packaging

- The release workflow publishes crates in dependency order, requires a green
  CI run on the tagged commit, and builds every artifact before uploading
  anything.
- Each GitHub release now carries npm tarballs of the WebAssembly engine
  (`web` and `nodejs` targets) next to the Excel add-in zip.

## 0.11.1 and earlier

See the [GitHub releases](https://github.com/sujitn/convex/releases).
