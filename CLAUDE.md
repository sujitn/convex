# Convex — Claude Code memory

High-performance fixed-income analytics in Rust, surfaced through:

- **Excel add-in** via Excel-DNA (.NET Framework 4.7.2 + P/Invoke).
- **MCP server** for tool/agent integration.
- **Convex umbrella crate** for direct embedding (CLI tools, services).

The boundary in every case is the same JSON wire format. Adding a new bond
shape, spread family, or pricing convention is a serde enum variant plus a
dispatch arm — no new C symbol, no new P/Invoke, no new Excel UDF.

## Crates

| Crate | Role |
|---|---|
| `convex-core` | Domain types: `Date`, `Currency`, `Frequency`, `Compounding`, `Mark`, `Spread`, `Yield`, `Price`, day-count conventions, calendars. |
| `convex-math` | Numerical kernels: Brent solver, Newton, Levenberg-Marquardt, interpolators. |
| `convex-curves` | Discrete curves, bootstrapping (global-fit + piecewise), bumping (parallel / key-rate / scenario). |
| `convex-bonds` | Instruments: `FixedRateBond`, `CallableBond`, `FloatingRateNote`, `ZeroCouponBond`, `SinkingFundBond`. |
| `convex-analytics` | Pricing (`price_from_mark`), risk, spreads (Z/G/I/ASW/OAS/DM), DTOs (`dto.rs`). |
| `convex-ffi` | C-ABI FFI: 13 symbols, JSON-in / JSON-out, opaque handle registry. |
| `convex` | Umbrella facade re-exporting the public API. |
| `convex-mcp` | MCP server speaking the same DTOs. |

## FFI surface (13 C symbols)

Construction (returns `u64` handle, `0` = invalid):

| Symbol | Purpose |
|---|---|
| `convex_bond_from_json(spec)` | Build a bond from a `BondSpec` JSON. |
| `convex_curve_from_json(spec)` | Build a curve from a `CurveSpec` JSON. |
| `convex_describe(handle)` | JSON description of a registered object. |
| `convex_release(handle)` | Drop a handle. |
| `convex_object_count()` | Number of registered objects. |
| `convex_clear_all()` | Drop every handle. |
| `convex_list_objects()` | JSON list `[{handle,kind,name?}]`. |

Stateless analytics (JSON RPC envelopes):

| Symbol | Request → Response |
|---|---|
| `convex_price` | `PricingRequest` → `PricingResponse` |
| `convex_risk` | `RiskRequest` → `RiskResponse` |
| `convex_spread` | `SpreadRequest` → `SpreadResponse` |
| `convex_cashflows` | `CashflowRequest` → `CashflowResponse` |
| `convex_curve_query` | `CurveQueryRequest` → `CurveQueryResponse` |

Utilities:

| Symbol | Purpose |
|---|---|
| `convex_schema(typeName)` | JSON schema for any DTO. |
| `convex_mark_parse(text)` | Parse a textual mark into canonical JSON. |
| `convex_last_error` / `convex_clear_error` / `convex_version` / `convex_string_free` | Boilerplate. |

The envelope is `{"ok":"true","result":...}` on success and
`{"ok":"false","error":{"code","message","field?"}}` on failure. Codes are
`invalid_input`, `invalid_handle`, `analytics`.

## Trader marks

Every pricing / risk / spread call accepts one textual mark string. The
parser (`Mark::from_str`) recognises:

| Form | Example |
|---|---|
| Decimal price (clean default) | `99.5`, `99.5C`, `99.5 dirty`, `99.5D` |
| 32nds | `99-16`, `99-16+` |
| Yield + frequency | `4.65%`, `4.65%@SA`, `4.65%@A` |
| Spread + benchmark (default Z) | `+125bps@USD.SOFR` |
| Spread with explicit type | `125 OAS@USD.TSY`, `-50 G@USD.TSY.10Y` |

Spread types: `Z` / `G` / `I` / `OAS` / `DM` / `ASW` / `ASW_PROC` / `CREDIT`.

## Conventions

| Field | Convention |
|---|---|
| Coupon rate | Decimal (0.05 = 5%). |
| Spread bps | Basis points (75 = 75 bp). |
| OAS volatility | Decimal (0.01 = 1%). |
| Prices | Per 100 face. |
| Dates | ISO-8601 strings on the wire; `DateTime` cells in Excel. |
| Day count | `Act360`, `Act365Fixed`, `ActActIsda`, `ActActIcma`, `Thirty360US`, `Thirty360E`. |
| Compounding | `Annual`, `SemiAnnual`, `Quarterly`, `Monthly`, `Continuous`, `Simple`. |
| Frequency | `Annual`, `SemiAnnual`, `Quarterly`, `Monthly`, `Zero`. |

## Excel UDFs (`CX.` prefix)

### Construction (returns `#CX#…` handle)
- `CX.BOND` · `CX.BOND.CALLABLE` · `CX.BOND.FRN` · `CX.BOND.ZERO`
- `CX.CURVE` · `CX.CURVE.BOOTSTRAP`

### Stateless analytics
- `CX.PRICE(bond, settle, mark, [curve], [quoteFreq], [field])` — clean / dirty / accrued / ytm / z_spread / grid.
- `CX.RISK(bond, settle, mark, [curve], [metric], [quoteFreq], [krdTenors])` — grid / mod_dur / mac_dur / convexity / dv01 / spread_dur / krd.
- `CX.SPREAD(bond, curve, settle, mark, [type], [vol], [field])` — Z / G / I / ASW / ASW_PROC / OAS / DM / Credit.
- `CX.CASHFLOWS(bond, settle)` — `Date · Amount · Kind` grid.
- `CX.CURVE.QUERY(curve, tenor, [query], [tenorEnd])` — zero / df / forward.

### Diagnostics
- `CX.SCHEMA(typeName)` · `CX.MARK(text)` · `CX.DESCRIBE(handle)` ·
  `CX.VERSION()` · `CX.OBJECTS()` · `CX.RELEASE(handle)` · `CX.CLEAR()`.

## Ribbon

Convex tab, five groups:

- **Bonds**: `BondBuilderForm` · `PricingTicketForm` · `SpreadTicketForm`.
- **Curves**: `CurveBuilderForm` · `CurveViewerForm`.
- **Scenarios**: `ScenarioForm`.
- **Tools**: `ObjectBrowserForm` · `SchemaBrowserForm` · `SettingsForm` ·
  Clear All.
- **Help**: About.

Every form routes through the same JSON RPC the cell UDFs use, so the
worksheet and the ribbon can never disagree. JSON-spec construction lives in
`excel/Convex.Excel/helpers/BondSpecs.cs` and `CurveSpecs.cs` — single
source of truth shared by UDFs and forms. Ribbon icons live in
`helpers/IconAtlas.cs` (programmatic GDI+ paint, cached on first use).

## Handle registry

- Format: `#CX#100`, `#CX#101`, … (starts at 100 for visual contrast).
- Type-erased `Box<dyn Any + Send + Sync>` keyed by `u64`.
- Bond shapes are tracked via `BondKind { FixedRate, Callable, FloatingRate, ZeroCoupon, SinkingFund }`.
- Curves stored as `RateCurve<DiscreteCurve>`.
- Re-registering the same name releases the prior handle (so dependent cells
  see a fresh handle and recalc).

## Routing model in `convex_ffi::dispatch`

- **Fixed-coupon shapes** (FixedRate, Callable, SinkingFund) share a single
  generic body via `with_fixed_bond!`.
- **FRN** doesn't impl `FixedCouponBond`; gets dedicated `price_frn` /
  `risk_frn` / `spread_dm` paths driven off price marks and DM spread marks.
- **Zero-coupon** uses closed-form yield ↔ price (`ZeroCouponBond` exposes
  `price_from_yield` / `yield_from_price`); risk is closed-form
  Macaulay = years to maturity.
- **OAS** routes through `CallableBond`; effective duration / convexity are
  ±1bp **curve** parallel shifts holding OAS constant. Option value =
  bullet PV at the implied Z-spread minus callable model price.
- **G-spread** requires an explicit `params.govt_curve` handle — a synthesised
  benchmark from the discount curve is dishonest (G-spread on its own curve
  is identically zero), so the dispatcher refuses without one.
- **DM** uses `params.forward_curve` (defaults to the discount curve when
  omitted). Simple-margin shortcut: passing `params.current_index` returns
  the closed-form simple margin instead of the iterative DM solver.
- **KRD**: hold the implied Z-spread fixed, bump the discount curve at each
  key tenor (triangular weight), reprice. Output is per-tenor partial
  duration.

## Build commands

```bash
# Rust
cargo build --release -p convex-ffi
cargo test  --release -p convex-ffi --test smoke -- --test-threads=1

# Excel add-in
cd excel/Convex.Excel
dotnet build --configuration Release

# Launch Excel with the packed .xll
start excel /x "excel\Convex.Excel\bin\Release\net472\publish\Convex.Excel64-packed.xll"
```

## Conventions for adding new functionality

| Goal | Where to change |
|---|---|
| New bond shape | Add a variant to `convex_analytics::dto::BondSpec` + a builder arm in `convex_ffi::build` + (if new trait surface) a dedicated dispatch path in `convex_ffi::dispatch`. |
| New spread family | Add a variant to `convex_core::types::SpreadType` (already broad) + an arm in `dispatch::spread_fixed` (or a dedicated handler if it needs a different bond trait). |
| New curve type | Add a variant to `convex_analytics::dto::CurveSpec` + a builder arm in `convex_ffi::build`. |
| New analytic | Pick the right RPC (`convex_price` / `convex_risk` / `convex_spread`) and add a field to its response DTO. |

The C FFI surface, P/Invoke layer, and Excel UDFs do not need to change for
any of the above.

## Documentation map

- `excel/README.md` — Excel-side user docs (UDF surface + examples).
- `excel/SMOKE_TEST.md` — manual smoke-test checklist.
- `excel/build_demo.py` — regenerates `excel/ConvexDemo.xlsx` from scratch
  whenever the cell API changes (treat the `.xlsx` as a derived artifact).
- `crates/convex-ffi/README.md` — FFI integration guide.
- `crates/convex-mcp/README.md` — MCP server.
- `docs/mcp-audit.md` / `docs/mcp-gaps.md` — MCP-specific notes.
