# Convex Excel Add-In

Mark-driven fixed income analytics for Microsoft Excel, using the Rust library through `convex-ffi`. The add-in lives in `excel/` (C# project `excel/Convex.Excel`) and ships as a zip on the GitHub releases page; this page also covers building it from source.

The add-in is small: a compact C FFI surface, a typed JSON
wire format, about 30 cell functions (UDFs), and a ribbon. Adding a new bond shape, spread
family, or pricing convention is a serde enum variant on the Rust side plus
one dispatch arm — no new C symbol, no new P/Invoke, no new UDF.

Behaviors worth knowing: reference bonds by CUSIP/ISIN/name instead of
handles; `CX.YAS` spills a full yield-and-spread analysis from one call;
identical requests are served from a generation-gated cache so multi-field
blocks and repeated recalcs cost one engine call; errors are standard Excel
errors with per-cell detail via `CX.LASTERROR` and a ribbon Error Log;
IntelliSense tooltips document every argument in the grid; the pricing and spread forms are
modeless so Excel stays interactive. On add-in load a full rebuild
re-registers cell-built objects, so reopened workbooks heal automatically
(Settings → "Rebuild handles on open"; manual: ribbon **Rebuild**).

## Installation

### Prerequisites
- Microsoft Excel 2016 or later (64-bit)
- .NET Framework 4.7.2 (Windows 10 +)
- Visual Studio Build Tools or .NET SDK (only needed to build from source)

### Build from source

```powershell
# Build the Rust FFI (the `excel` profile builds with panic=unwind so a
# Rust panic becomes an in-cell error instead of killing Excel)
cargo build --profile excel -p convex-ffi

# Build the Excel add-in
cd excel/Convex.Excel
dotnet build --configuration Release
```

The Rust `convex_ffi.dll` is copied next to the `.xll` automatically by
the build.

### Loading the add-in

1. Open Excel → File → Options → Add-ins.
2. At the bottom, "Excel Add-ins" → Go.
3. Browse to `excel/Convex.Excel/bin/Release/net472/publish/Convex.Excel64-packed.xll`.
4. OK.

## Trader marks

Every pricing/risk/spread call accepts a single textual mark. The grammar:

| Form | Example | Meaning |
|---|---|---|
| Decimal price | `99.5`, `99.5C` | Clean price |
| Decimal price (dirty) | `99.5D`, `99.5 dirty` | Dirty price |
| 32nds | `99-16`, `99-16+` | Treasury 32nds (`+` = ½) |
| Yield | `4.65%`, `4.65%@SA` | Yield with optional frequency suffix |
| Spread | `+125bps@USD.SOFR` | Z-spread over benchmark (default) |
| Spread w/ type | `125 OAS@USD.TSY`, `-50 G@USD.TSY.10Y` | Explicit family + benchmark |

Frequency tokens: `A` (Annual), `SA` (SemiAnnual, default), `Q`, `M`, `Z`.
Spread types: `Z`, `G`, `I`, `OAS`, `DM`, `ASW`, `ASW_PROC`, `CREDIT`.

## Cell UDFs

Every UDF lives behind the `CX.` prefix and routes through the JSON FFI.
The full surface:

### Construction (returns a `#CX#…` handle)

| UDF | Description |
|---|---|
| `CX.BOND(id, couponDecimal, maturity, issue, [freq], [dayCount], [currency], [face])` | Fixed-rate bond. |
| `CX.BOND.CALLABLE(id, couponDecimal, maturity, issue, callDates, callPrices, [freq], [style], [dayCount])` | Callable bond. American/European/Bermudan. |
| `CX.BOND.FRN(id, spreadBps, maturity, issue, [index], [freq], [dayCount], [cap], [floor])` | Floating-rate note. |
| `CX.BOND.ZERO(id, maturity, issue, [compounding], [dayCount])` | Zero-coupon bond. |
| `CX.CURVE(name, refDate, tenors, values, [valueKind], [interp], [dayCount], [compounding])` | Discrete curve from `(tenor, value)` pairs. |
| `CX.CURVE.BOOTSTRAP(name, refDate, kinds, tenors, rates, [method], [interp], [dayCount])` | Bootstrap from market instruments. |

### Ticker-style referencing

Every analytics UDF accepts the bond/curve argument as **either** a `#CX#N`
handle **or** the identifier it was built with — CUSIP, ISIN, or free name
for bonds; the curve's own name for curves:

```excel
=CX.PRICE("912828YK0", DATE(2025,4,15), "99.5C")
=CX.SPREAD("912828YK0", "USD.SOFR", DATE(2025,4,15), "99.5C", "Z")
```

When the same identifier is built in two cells, the name resolves to the
most recent build (each cell's own handle is unaffected). Unknown names
return `#REF!`.

### Stateless analytics

| UDF | Description |
|---|---|
| `CX.PRICE(bond, settle, mark, [curve], [quoteFreq], [field])` | Returns clean / dirty / accrued / YTM. `field`: `clean` (default), `dirty`, `accrued`, `ytm`, `grid`. For a Z-spread use `CX.SPREAD`. |
| `CX.YAS(bond, settle, mark, curve, [quoteFreq], [swapCurve])` | Convenience yields, Z/ASW spreads, durations, convexity, DV01 and settlement invoice. |
| `CX.RISK(bond, settle, mark, [curve], [metric], [quoteFreq], [krdTenors])` | `metric`: `grid` (default), `mod_dur`, `mac_dur`, `convexity`, `dv01`, `spread_dur`, `krd`. |
| `CX.SPREAD(bond, curve, settle, mark, [type], [vol], [field])` | Z / G / I / ASW / ASW_PROC / OAS / DM / Credit. `field`: `bps` (default), `grid`. |
| `CX.SCENARIO(bond, curve, settle, mark, shifts, [kind], [pivot], [quoteFreq])` | Scenario ladder in one call — reprices under each bps shift holding the mark-implied Z fixed. `kind`: `parallel` (default), `steepener`, `flattener`, `key_rate`, `credit`. |
| `CX.CASHFLOWS(bond, settle)` | Cashflow grid: `Date, Amount, Kind`. |
| `CX.CURVE.QUERY(curve, tenor, [query], [tenorEnd])` | `query`: `zero` (default), `df`, `forward`. |

### Live cells (RTD)

A live cell recalculates by itself when a bond or curve it reads is rebuilt
or released (from a cell, a ribbon form or `CX.RELEASE`), and only then. No
F9 is needed under Automatic calculation. Each live cell returns one number.

| UDF | Field (default first) |
|---|---|
| `CX.PRICE.LIVE(bond, settle, mark, [curve], [quoteFreq], [field])` | `clean`, `dirty`, `accrued`, `ytm` |
| `CX.RISK.LIVE(bond, settle, mark, [curve], [metric], [quoteFreq])` | `mod_dur`, `mac_dur`, `convexity`, `dv01`, `spread_dur` |
| `CX.SPREAD.LIVE(bond, curve, settle, mark, [type], [vol], [field])` | `bps` |
| `CX.YAS.LIVE(bond, settle, mark, curve, [quoteFreq], [swapCurve], [field])` | `ytm`, `clean`, `dirty`, `accrued`, `accrued_days`, `current_yield`, `simple_yield`, `z_spread`, `mod_dur`, `mac_dur`, `convexity`, `dv01`, `principal`, `accrued_amount`, `settlement_amount` |

The arguments match the static functions, with two exceptions:
`CX.RISK.LIVE` has no key-rate tenors argument, and `CX.YAS.LIVE` adds
`field` because `CX.YAS` returns a grid. A grid field (`grid`, `krd`)
returns `#VALUE!`, and `=CX.LASTERROR(cell)` names the static function to
use instead.

How a change reaches the sheet:

1. The Rust registry records which names (CUSIP, ISIN, curve name, owning
   cell) and handles each rebuild or release changed.
2. The add-in's one RTD server (`Convex.Rtd`) recomputes only the live cells
   whose request names one of them. It sends a value to Excel only when the
   number changed.
3. Excel recalculates every formula that reads a changed live cell, as for
   any other cell.

A cell that references a handle cell (`=CX.PRICE.LIVE(A1, …)` with `#CX#101`
in `A1`) recalculates through Excel's own dependency graph. The registry's
change log matters for cells that name a bond or curve, such as `"USD.SOFR"`
or a CUSIP, which Excel cannot see. Identical live cells share one topic,
and live and static cells share one response cache, so they cost one engine
call per change.

Limits and settings:

- **Update rate.** Excel delivers changed live cells once per RTD throttle
  interval: 2,000 ms by default. Settings → "Excel RTD throttle" can set
  250 ms. That is Excel's own setting: it applies to every add-in and
  persists after Excel restarts. The add-in never changes it by itself.
- **Calculation mode.** Under Manual calculation, live cells update on F9.
- **Size.** At most 20,000 distinct live cells per Excel process; beyond
  that a cell returns `#VALUE!`.
- **Diagnostics.** `=CX.DIAG()` shows the live topic count, the recomputes
  and pushes so far, and the time the last change took.

This replaces the earlier live layer, which recomputed every live cell on
any registry change and had a "LIVE poll (ms)" setting. There is no
compatibility layer: `CX.PRICE` no longer has a `z_spread` field (use
`CX.SPREAD`), and the setting is gone.

### Hedge advisor

| UDF | Description |
|---|---|
| `CX.RISKPROFILE(bond, curve, settle, mark, notionalFace, [vol])` | Position risk profile (DV01, KRD buckets, market value) as JSON — feed to `CX.HEDGE`. |
| `CX.HEDGE(strategy, profileJson, curve, settle)` | Hedge proposal grid: trades, residual DV01, cost. `strategy`: `duration_futures`, `barbell_futures`, `cash_bond_pair`, `interest_rate_swap`, `key_rate_futures`. |

### Diagnostics / utilities

| UDF | Description |
|---|---|
| `CX.SCHEMA(typeName)` | JSON schema for any DTO (`Mark`, `BondSpec`, `YasRequest`, …). |
| `CX.MARK(text)` | Parse a textual mark and return its canonical JSON form. |
| `CX.DESCRIBE(handle)` | Inspect a registered object's key fields. |
| `CX.LASTERROR(cellRef)` | Full `[code] message` detail behind an error value in a cell. |
| `CX.DIAG()` | Rust-library load status, engine version, object count, live topics and recomputes. |
| `CX.VERSION()` | Library version. |
| `CX.OBJECTS()` | Number of registered objects. |
| `CX.RELEASE(handle)` | Release one handle. |
| `CX.CLEAR()` | Release every handle (and the error log). |
| `CX.LIBRARY(requestJson)` | Run a complete `LibraryRequest` JSON; returns the result envelope as text. |
| `CX.LIBRARY.RANGE(requestCells)` | Same, with the request split across text cells; returns the result in one-column chunks. |
| `CX.MW(...)` | Make-whole call price for a callable bond with a make-whole spread. |

## Examples

### Build a fixed-rate bond and price it three ways

```excel
A1: =CX.BOND("US037833100", 0.05, DATE(2035,1,15), DATE(2025,1,15))     ' returns #CX#100
A2: =CX.PRICE(A1, DATE(2025,4,15), "99.5C")                              ' clean price as input
A3: =CX.PRICE(A1, DATE(2025,4,15), "4.65%@SA")                           ' yield mark
A4: =CX.PRICE(A1, DATE(2025,4,15), "99-16+")                             ' 32nds
A5: =CX.PRICE(A1, DATE(2025,4,15), "99.5C", , , "grid")                  ' full grid: clean/dirty/accrued/ytm/z
```

### Build a curve and compute spreads

```excel
B1: =CX.CURVE("USD.SOFR", TODAY(),
              {0.5, 1, 2, 5, 10, 30},
              {0.045, 0.046, 0.047, 0.048, 0.049, 0.050},
              "zero_rate", "linear")
B2: =CX.SPREAD(A1, B1, DATE(2025,4,15), "99.5C", "Z")                    ' Z-spread bps
B3: =CX.SPREAD(A1, B1, DATE(2025,4,15), "99.5C", "Z", , "grid")          ' grid: bps + spread DV01
```

### Government spreads use benchmark securities

Use `CX.LIBRARY` / `CX.LIBRARY.RANGE` with a full `government_spread`
request that carries the benchmark bond/bill quotes and calendars. `CX.SPREAD` has no argument
for benchmark securities and cannot compute a G-spread. Its error names
`params.government`; a zero-rate curve is not accepted as a government benchmark.
See [removed routes and Excel argument migration](../docs/internal/ROUTE-MIGRATION.md).

### Bootstrap a curve from market instruments

```excel
B7: =CX.CURVE.BOOTSTRAP("USD.SOFR.LIVE", TODAY(),
        {"deposit","deposit","swap","swap","swap","swap"},
        {0.25, 0.5, 2, 5, 10, 30},
        {0.0525, 0.0530, 0.0475, 0.0425, 0.0410, 0.0425},
        "global_fit", "linear", "Act360")
```

### Risk grid + KRD

```excel
C1: =CX.RISK(A1, DATE(2025,4,15), "99.5C")                               ' default grid
C2: =CX.RISK(A1, DATE(2025,4,15), "99.5C", , "dv01")                     ' scalar
C3: =CX.RISK(A1, DATE(2025,4,15), "99.5C", B1, "krd", , "2,5,10")        ' KRD grid
```

### FRN price by discount margin

```excel
D1: =CX.BOND.FRN("US-FRN-001", 75, DATE(2030,1,15), DATE(2025,1,15), "sofr", "Quarterly", "Act360")
D2: =CX.PRICE(D1, DATE(2025,4,15), "100D", B1)                           ' price mark
D3: =CX.SPREAD(D1, B1, DATE(2025,4,15), "100D", "DM")                    ' DM in bps
```

## Ribbon

The Convex tab has five groups:

- **Bonds**: New Bond (tabbed: fixed-rate, callable, FRN, zero) · Pricing
  Ticket · Spread Ticket.
- **Curves**: New Curve (tabbed: discrete, bootstrap) · Curve Viewer
  (chart of zero + 1Y forward).
- **Scenarios**: Run Scenario (parallel shift bumps over a CSV bps list).
- **Tools**: Objects (browse / inspect / paste handle / release) · Schemas
  (JSON schema viewer for any DTO) · Settings (default frequency, day count,
  spread family, currency persisted to `%APPDATA%\Convex\settings.json`) ·
  Clear All.
- **Help**: About.

Every form routes through the same JSON RPC the cell UDFs use, so the
ribbon and the worksheet can never disagree.

## DTOs and the wire format

The complete request/response shapes are exposed via `=CX.SCHEMA(...)`:

```excel
=CX.SCHEMA("Mark")                ' textual + tagged JSON forms
=CX.SCHEMA("BondSpec")            ' fixed_rate / callable / floating_rate / zero_coupon / sinking_fund
=CX.SCHEMA("CurveSpec")           ' discrete | bootstrap
=CX.SCHEMA("PricingRequest")
=CX.SCHEMA("PricingResponse")
=CX.SCHEMA("RiskRequest")
=CX.SCHEMA("RiskResponse")
=CX.SCHEMA("SpreadRequest")
=CX.SCHEMA("SpreadResponse")
=CX.SCHEMA("CashflowRequest")
=CX.SCHEMA("CashflowResponse")
=CX.SCHEMA("CurveQueryRequest")
=CX.SCHEMA("CurveQueryResponse")
```

## Conventions

| Field | Convention |
|---|---|
| Coupon rates | Decimal (0.05 = 5%). |
| Spread bps | Basis points (75 = 75 bp). |
| OAS volatility | Decimal (0.01 = 1%). |
| Prices | Per 100 face. |
| Dates | ISO-8601 strings on the wire; Excel `DateTime` cells in UDFs. |
| Frequency | `Annual`, `SemiAnnual`, `Quarterly`, `Monthly`, `Zero` (or `A`/`SA`/`Q`/`M`). |
| Day count | `Act360`, `Act365Fixed`, `ActActIsda`, `ActActIcma`, `Thirty360US`, `Thirty360E`. |
| Compounding | `Annual`, `SemiAnnual`, `Quarterly`, `Monthly`, `Continuous`, `Simple`. |

## Architecture

```
   Excel cell  ─►  CX.* UDF  ─┐
                              ├─►  Cx (P/Invoke)  ─►  convex_ffi.dll  (Rust)
   Excel ribbon ─► form ──────┘                              │
                                                             ▼
                                              registry → typed bonds/curves
                                                             │
                                                             ▼
                                          convex-analytics → convex-bonds /
                                                              convex-curves /
                                                              convex-core
```

The C surface is small. Stateful calls build, describe, release, list, count
and clear bonds and curves. The stateless analytics calls are `convex_price`,
`convex_risk`, `convex_spread`, `convex_cashflows` and `convex_curve_query`,
plus `convex_library` for complete library requests. Utilities are
`convex_schema`, `convex_mark_parse`, `convex_string_free`, `convex_last_error`
and `convex_version`. See the [convex-ffi README](../crates/convex-ffi/README.md)
for the full list.

Bonds and curves live in a thread-safe Rust registry behind opaque
`u64` handles displayed in cells as `#CX#100`, `#CX#101`, ….
`convex_generation` counts registry changes, and `convex_changes_since`
returns the names and handles that changed after a given count, from a log
of the last 4,096 changes. The live layer (`Live/LiveHub.cs`) reads that log
to recompute only the live cells a change reaches.

## Errors

Stateless RPCs return a JSON envelope:

```json
{ "ok": "true",  "result": { ... } }
{ "ok": "false", "error": { "code": "invalid_input", "message": "...", "field": "mark" } }
```

Codes:

- `invalid_input` — bad JSON shape, unparseable mark, missing required field.
- `invalid_handle` — handle not in the registry, or wrong kind for the call.
- `analytics` — solver did not converge, settle ≥ maturity, or other domain error.
- `panic` — an internal engine error was caught at the FFI boundary
  (converted to an error envelope instead of crashing Excel).

The cell UDFs surface these as **standard Excel errors** so `IFERROR` /
`ISERROR` and error propagation work as in any built-in function:

| Excel error | Meaning |
|---|---|
| `#VALUE!` | malformed input — bad mark, unparseable cell, misaligned parallel ranges — or a caught engine `panic` |
| `#NAME?`  | unknown keyword — frequency, day count, spread type, field/metric name |
| `#REF!`   | handle (or ticker) not registered, or wrong object kind |
| `#NUM!`   | solver non-convergence or domain error |
| `#N/A`    | Rust library not loaded — run `=CX.DIAG()` |

The full message behind any error is one call away:
`=CX.LASTERROR(B2)` returns `[code] message` for the failing cell, and
the ribbon **Diagnostics** button reports whether the Rust library loaded.

## Demo workbook

`excel/ConvexDemo.xlsx` ships a Bonds / Curves / Spreads / Scenarios /
Schemas tour. The workbook is a derived artifact — regenerate it from
the canonical script:

```powershell
pip install openpyxl
python excel/build_demo.py
```

Open it with the add-in loaded; every formula should evaluate without
`#NAME?`. Any Excel error in a CX cell carries structured detail
(see *Errors*) — `=CX.LASTERROR(cell)` tells you which argument was
rejected.

## Smoke tests

```powershell
cargo test --locked -p convex-ffi --test smoke -- --test-threads=1
```

End-to-end tests cover: every spread family, FRN/zero/sinking-fund
pricing and risk, KRD, cashflow tag stability, schema introspection, mark
parsing, and the error envelope.

For a manual cell-side checklist see `excel/SMOKE_TEST.md`.

## Library API

See the [library API](../docs/LIBRARY-API.md) for request shapes and limits.
`CX.LIBRARY` returns the result envelope unchanged, with decimal strings and
error diagnostics intact. Coverage of the required operations and host
certification are still open.
