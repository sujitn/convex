# Convex Excel add-in smoke test

Sanity-checks the cell layer end-to-end: handle construction, mark-driven
pricing (clean / yield round-trip), spread routing, and structured error
envelopes.

The Rust side has its own integration test suite — run

```bash
cargo test -p convex-ffi --test smoke --release -- --test-threads=1
```

to validate every dispatcher arm. The checklist below is the manual
counterpart for the Excel side.

## Setup

Build artifacts:

```bash
cargo build --profile excel -p convex-ffi
cd excel/Convex.Excel && dotnet build --configuration Release
```

The build copies `convex_ffi.dll` next to the packed `.xll` automatically.
Launch Excel with the add-in:

```bash
start excel /x "excel\Convex.Excel\bin\Release\net472\publish\Convex.Excel64-packed.xll"
```

## Test matrix

Paste into `A1:A8` on a fresh sheet. Replace `A1` first; the rest reference it.

| Cell | Formula | Expected | Validates |
|:---:|---|---|---|
| `A1` | `=CX.BOND("TEST10Y5", 0.05, DATE(2035,1,15), DATE(2025,1,15))` | `#CX#100` (or similar) | `convex_bond_from_json` happy path |
| `A2` | `=CX.PRICE(A1, DATE(2025,4,15), "99.5C")` | clean ≈ `99.5` | mark-driven pricing, default field |
| `A3` | `=CX.PRICE(A1, DATE(2025,4,15), "99.5C", , , "ytm")` | YTM (%) ≈ `5.07` | yield from clean price, percent units |
| `A4` | `=CX.PRICE(A1, DATE(2025,4,15), "99-16+")` | clean ≈ `99.515625` | 32nds parser |
| `A5` | `=CX.PRICE(A1, DATE(2025,4,15), "abc")` | `#VALUE!` | invalid mark → native Excel error |
| `A6` | `=CX.PRICE("BAD_HANDLE", DATE(2025,4,15), "99.5C")` | `#VALUE!` (`#REF!` once ticker lookup lands) | bad handle text |
| `A7` | `=CX.OBJECTS()` | integer ≥ 1 | registry alive |
| `A8` | `=CX.RELEASE(A1)` then `=CX.OBJECTS()` | drops by 1 | release path |

**Load-bearing assertion (A5/A6)**: the cell contains a *native Excel
error* (`ISERROR(A5)` is TRUE, `IFERROR(A5,"x")` catches it), and
`=CX.LASTERROR(A5)` returns the structured `[code] message` detail.
The error taxonomy: `#VALUE!` malformed input · `#NAME?` unknown keyword
(frequency/day count/spread type/field) · `#REF!` unknown handle ·
`#NUM!` solver/analytics failure · `#N/A` native library not loaded
(diagnose with `=CX.DIAG()`).

## Spread sanity checks

```excel
B1: =CX.CURVE("USD.SOFR", DATE(2025,1,15), {0.5,1,2,5,10,30}, {0.04,0.04,0.04,0.04,0.04,0.04}, "zero_rate", "linear")
B2: =CX.SPREAD(A1, B1, DATE(2025,4,15), "99.5C", "Z")            ' ~80–120 bps
B3: =CX.SPREAD(A1, B1, DATE(2025,4,15), "99.5C", "I")            ' I-spread bps, finite
B4: =CX.SPREAD(A1, B1, DATE(2025,4,15), "99.5C", "G")            ' #VALUE!; CX.LASTERROR(B4) names params.govt_curve
```

`B4` must error — G-spread only computes against an explicitly-supplied
government curve; `=CX.LASTERROR(B4)` must name `params.govt_curve`. Use
the **Spread Ticket** ribbon form, which threads `params.govt_curve`
through, for the positive path.

## Risk + KRD

```excel
C1: =CX.RISK(A1, DATE(2025,4,15), "99.5C")                       ' grid: mod_dur, mac_dur, convexity, dv01
C2: =CX.RISK(A1, DATE(2025,4,15), "99.5C", , "dv01")             ' scalar
C3: =CX.RISK(A1, DATE(2025,4,15), "99.5C", B1, "krd", , "2,5,10")' KRD grid: 3 rows
```

The sum of the three KRDs should fall well within `[0, 2 × mod_dur]`.

## Schema browser

```excel
D1: =CX.SCHEMA("Mark")                                           ' multi-line JSON schema text
D2: =CX.SCHEMA("PricingRequest")                                 ' includes forward_curve field
D3: =CX.SCHEMA("SpreadRequest")                                  ' includes params.govt_curve, params.volatility, ...
```

If any of these errors (`#VALUE!` with `CX.LASTERROR` naming the type)
the schemas table in `crates/convex-ffi/src/schemas.rs` is out of sync
with the DTOs.

## Ticker referencing

```excel
E1: =CX.PRICE("TEST10Y5", DATE(2025,4,15), "99.5C")   ' by name — matches A2
E2: =CX.PRICE("NO_SUCH", DATE(2025,4,15), "99.5C")    ' #REF!; CX.LASTERROR(E2) names it
```

## One-call grids

```excel
F1: =CX.YAS(A1, DATE(2025,4,15), "99.5C", B1)                      ' spills ~18 rows: yields, G/Z/benchmark/ASW, risk, invoice
F2: =CX.SCENARIO(A1, B1, DATE(2025,4,15), "99.5C", {-50,0,50})     ' ladder; the 0bp row reproduces the base price exactly
```

## Live cells (RTD)

1. `G1: =CX.PRICE.LIVE(A1, DATE(2025,4,15), "99.5C")` — shows the price.
2. Rebuild the same bond from the ribbon (New Bond, same ID, different
   coupon) — `G1` updates within the poll interval with **no manual recalc**.
3. `=CX.RELEASE(...)` the bond — `G1` flips to an error; rebuild — it heals.

## Recalc-cascade cutoff (idempotent handles)

1. Build a builder cell + 10 dependent `CX.PRICE` cells.
2. Force-recalc the builder (F2+Enter) with unchanged inputs: the handle
   string must NOT change and dependents must not visibly recompute.
3. Edit the coupon: the handle changes and dependents recompute once.

## Workbook reopen

1. Build bonds/curves from cells, add analytics referencing them by name,
   save, quit Excel entirely.
2. Reopen the workbook (add-in loaded): the auto rebuild re-registers
   everything; no `#REF!` remains without touching a key. With
   Settings → "Rebuild handles on open" off, ribbon **Rebuild** does it.

## Ribbon

Open the **Convex** tab. Smoke each form once. Pricing/Spread/Curve
Viewer/Scenario/Objects/Error Log are MODELESS — verify Excel stays
interactive (select cells, type) while each is open, and that re-clicking
the button focuses the existing window instead of duplicating it:

- **New Bond** → Fixed Rate tab → Build → status reads `OK — #CX#NNN`.
- **Pricing Ticket** → Refresh objects → pick the bond → mark `99.5C` →
  Compute → grid populates with finite numbers.
- **Spread Ticket** → pick bond + curve → spread `Z` → Compute → finite bps.
- **Curve Viewer** → pick a curve → chart renders an upward yield curve.
- **Object Browser** → handles match `=CX.OBJECTS()` → Describe shows
  bond fields (coupon, maturity, frequency, currency, face).
- **Schemas** → flip through types → JSON renders without error.
- **Settings** → save → re-open → values persisted to
  `%APPDATA%\Convex\settings.json`.

If any step throws into a Windows error dialog, the form is bypassing the
JSON RPC layer — that's a regression.

## Demo workbook

`excel/ConvexDemo.xlsx` is regenerated by `excel/build_demo.py` (openpyxl).
Open it with the add-in loaded for an end-to-end tour: every sheet
(Bonds, Curves, Spreads, Scenarios, Schemas) exercises a different slice
of the surface. If every CX cell shows `#NAME?` the add-in is not loaded;
individual `#VALUE!`/`#REF!`/`#NUM!` errors carry per-cell detail readable
via `=CX.LASTERROR(cell)` or the ribbon Diagnostics button.
