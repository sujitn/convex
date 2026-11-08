# Changelog

Versions before 1.0 can break the API in any minor release. Each entry lists
the changes that matter when you upgrade.

## Unreleased

### Excel add-in

- **Live cells recompute only what a change reaches.** One RTD server
  (`Convex.Rtd`) now backs every live cell. When a bond or curve is rebuilt
  or released, only the live cells whose request names it recompute, and a
  cell updates only when its number changed. The old layer recomputed every
  live cell on any change.
- **Breaking, with no compatibility layer:**
  - `CX.PRICE.LIVE`, `CX.RISK.LIVE`, `CX.SPREAD.LIVE` and `CX.YAS.LIVE`
    return one number. `grid` and `krd` give `#VALUE!` and name the static
    function to use.
  - `CX.RISK.LIVE` drops the key-rate tenors argument and defaults to
    `mod_dur`. `CX.YAS.LIVE` adds a `field` argument (default `ytm`).
  - `CX.PRICE` no longer has a `z_spread` field (it returned `#N/A` for most
    marks); use `CX.SPREAD`. Its grid drops the Z row.
  - The "LIVE poll (ms)" setting is gone. Settings shows Excel's RTD
    throttle instead and can set 250 ms on request.
- Excel-DNA 1.9.0 (was 1.7.0), still on .NET Framework 4.7.2, 64-bit only.
- `CX.DIAG()` reports live topics, recomputes, pushes and the last drain
  time.

### convex-ffi

- New C export `convex_changes_since(cursor)`: the names and handles whose
  resolution changed after a registry generation, from a 4,096-entry log.

### convex-server

- **A valuation subscription sends current results first.** After
  `subscribe_valuations`, the session sends the current result of each listed
  instrument that has one, then the `subscribed` acknowledgement. A client no
  longer needs to read `GET /api/v1/runtime/results` to seed. Subscribing again
  to instruments already followed resends their results, which is how a client
  recovers after `RESYNC_REQUIRED`.
- **One session can follow every instrument and every limited book.** The
  per-session topic quota is now 320 by default and at most (was 256): 256
  instruments, as many as a runtime values, plus the 64 books the limit monitor
  watches. Before, a session following 256 instruments could follow no book.
- Clients should still keep the newer valuation by `(runtime_id, revision)`: a
  result sent at subscription can be newer than one already on its way.
- **A result too large for one 64 KiB message no longer closes the session.** It
  arrives as a `RESULT_TOO_LARGE` error naming the instrument, `runtime_id` and
  `revision`; read the result from `GET /api/v1/runtime/results`.
- The `subscribed` acknowledgement's `count` is the number of distinct instruments
  listed; a repeated id counts once.
- **A book's recorded limit status changes can be read.**
  `GET /api/v1/books/{book_id}/limits/records?after=&limit=` returns the changes
  the limit monitor recorded for a book, oldest first, each as its
  `limit_status` message carried it, with a cursor for the next page. A reader
  following new records passes the last record id it has. It needs what status
  needs, and a record naming an instrument the caller cannot read is left out
  and counted. Without a limit monitor it answers 503.

### convex-ports

- `AuditStore::entity_page(entity_type, entity_id, after, limit)` returns up to
  1,000 of one entity's entries after a cursor, and whether there are more. It
  has a default that scans through `query`, so existing stores still compile.

### convex-ext-redb

- **Breaking:** `RedbAuditStore::new` returns `Result`. The audit store keeps
  an index by entity, written with each entry. The first open of a store
  written before it indexes every entry once (about 5 s for 1,000,000 entries);
  later opens find nothing to index.

## 0.14.0

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
  (`web` and `nodejs` targets) next to the Excel add-in zip. Each tarball
  includes the MIT license and the ISDA CDS Standard Model Public License.

## 0.11.1 and earlier

See the [GitHub releases](https://github.com/sujitn/convex/releases).
