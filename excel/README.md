# Convex Excel Add-In

Mark-driven fixed income analytics for Microsoft Excel, powered by Rust.

The add-in is intentionally small: a 13-function FFI surface, a typed JSON
wire format, ~15 cell UDFs, and a ribbon. Adding a new bond shape, spread
family, or pricing convention is a serde enum variant on the Rust side plus
one dispatch arm — no new C symbol, no new P/Invoke, no new UDF.

## Installation

### Prerequisites
- Microsoft Excel 2016 or later (64-bit)
- .NET Framework 4.7.2 (Windows 10 +)
- Visual Studio Build Tools or .NET SDK (only needed to build from source)

### Build from source

```powershell
# Build the Rust FFI (release)
cargo build --release -p convex-ffi

# Build the Excel add-in
cd excel/Convex.Excel
dotnet build --configuration Release
```

The native `convex_ffi.dll` is copied next to the `.xll` automatically by
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

### Stateless analytics

| UDF | Description |
|---|---|
| `CX.PRICE(bond, settle, mark, [curve], [quoteFreq], [field])` | Returns clean / dirty / accrued / YTM / Z-bps. `field`: `clean` (default), `dirty`, `accrued`, `ytm`, `z_spread`, `grid`. |
| `CX.RISK(bond, settle, mark, [curve], [metric], [quoteFreq], [krdTenors])` | `metric`: `grid` (default), `mod_dur`, `mac_dur`, `convexity`, `dv01`, `spread_dur`, `krd`. |
| `CX.SPREAD(bond, curve, settle, mark, [type], [vol], [field])` | Z / G / I / ASW / ASW_PROC / OAS / DM / Credit. `field`: `bps` (default), `grid`. |
| `CX.CASHFLOWS(bond, settle)` | Cashflow grid: `Date, Amount, Kind`. |
| `CX.CURVE.QUERY(curve, tenor, [query], [tenorEnd])` | `query`: `zero` (default), `df`, `forward`. |

### Diagnostics / utilities

| UDF | Description |
|---|---|
| `CX.SCHEMA(typeName)` | JSON schema for any DTO (`Mark`, `BondSpec`, `PricingRequest`, …). |
| `CX.MARK(text)` | Parse a textual mark and return its canonical JSON form. |
| `CX.DESCRIBE(handle)` | Inspect a registered object's key fields. |
| `CX.VERSION()` | Library version. |
| `CX.OBJECTS()` | Number of registered objects. |
| `CX.RELEASE(handle)` | Release one handle. |
| `CX.CLEAR()` | Release every handle. |

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

### G-spread requires a separate government curve

```excel
B4: =CX.CURVE("USD.TSY", TODAY(), {2,5,10,30}, {0.042,0.044,0.045,0.046}, "zero_rate", "linear")
B5: =CX.SPREAD(A1, B1, DATE(2025,4,15), "99.5C", "G")
    ' #ERROR: invalid_input (params.govt_curve): G-spread requires a separate government curve handle
B6: ' UDF doesn't yet expose params.govt_curve directly — call via the ribbon
    ' Spread Ticket form, which threads the field through.
```

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

The FFI is 13 C symbols total: 5 stateful (build bond, build curve,
describe, release, list, count, clear) and 5 stateless analytics RPCs
(`convex_price`, `convex_risk`, `convex_spread`, `convex_cashflows`,
`convex_curve_query`) plus 3 utilities (`convex_schema`,
`convex_mark_parse`, `convex_string_free`/`last_error`/`version`).

Bonds and curves live in a thread-safe Rust registry behind opaque
`u64` handles displayed in cells as `#CX#100`, `#CX#101`, ….

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

The cell UDFs surface these as `#ERROR: <code>: <message>` strings in the
cell so `=IFERROR(...)` and `=ISERROR(...)` continue to work.

## Demo workbook

`excel/ConvexDemo.xlsx` ships a Bonds / Curves / Spreads / Scenarios /
Schemas tour. The workbook is a derived artifact — regenerate it from
the canonical script:

```powershell
pip install openpyxl
python excel/build_demo.py
```

Open it with the add-in loaded; every formula should evaluate without
`#NAME?`. Any `#ERROR: ...` text in a cell is a structured FFI envelope
(see *Errors*) — the message tells you which argument was rejected.

## Smoke tests

```powershell
cargo test -p convex-ffi --test smoke --release -- --test-threads=1
```

19 end-to-end tests cover: every spread family, FRN/zero/sinking-fund
pricing and risk, KRD, cashflow tag stability, schema introspection, mark
parsing, and the error envelope.

For a manual cell-side checklist see `excel/SMOKE_TEST.md`.
