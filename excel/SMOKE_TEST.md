# Convex Excel add-in smoke test

Sanity-checks the cell layer end-to-end: handle construction, mark-driven
pricing (clean / yield round-trip), spread routing, and structured error
envelopes.

The Rust side has its own integration test suite — run

```bash
cargo test --locked -p convex-ffi --test smoke -- --test-threads=1
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
| `A6` | `=CX.PRICE("BAD_HANDLE", DATE(2025,4,15), "99.5C")` | `#REF!` | unknown ticker resolves engine-side to invalid_handle |
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
B4: =CX.SPREAD(A1, B1, DATE(2025,4,15), "99.5C", "G")            ' #VALUE!; CX.LASTERROR(B4) names params.government
```

`B4` must error; `=CX.LASTERROR(B4)` must name `params.government`.
Neither this UDF nor the Spread Ticket supplies actual government securities.
Use `CX.LIBRARY.RANGE` with a full `government_spread` request for the positive
path. A government zero-rate curve must not substitute for security yields.

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
D3: =CX.SCHEMA("SpreadRequest")                                  ' includes params.government, params.volatility, ...
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
F1: =CX.YAS(A1, DATE(2025,4,15), "99.5C", B1)                      ' yields, Z/ASW, risk, invoice; no government/benchmark output
F2: =CX.SCENARIO(A1, B1, DATE(2025,4,15), "99.5C", {-50,0,50})     ' ladder; the 0bp row reproduces the base price exactly
```

## Live cells (RTD)

Use Automatic calculation. Live cells update once per Excel RTD throttle
interval (Settings shows it; 2,000 ms by default).

1. `G1: =CX.PRICE.LIVE("TEST10Y5", DATE(2025,4,15), "99.5C", , , "ytm")`:
   shows the yield. The cell names the bond, so Excel does not know it
   depends on `A1`.
2. `G2: =CX.SPREAD.LIVE("TEST10Y5", "USD.SOFR", DATE(2025,4,15), "99.5C")`:
   shows the Z-spread.
3. `G3: =INDEX(CX.DIAG(), 6, 2)`: note the "Live computes" count.
4. Change the coupon in `A1` to `0.055`: `G1` and `G2` update by themselves,
   and `G3` rises by 2.
5. Change a rate in `B1`'s curve: `G2` updates, `G1` does not, and `G3`
   rises by 1 (only the cell that reads the curve recomputed).
6. `G4: =CX.PRICE.LIVE(A1, DATE(2025,4,15), "99.5C", , , "grid")`:
   `#VALUE!`, and `=CX.LASTERROR(G4)` names `CX.PRICE`.
7. `=CX.RELEASE(A1)` in a spare cell: `G1` and `G2` turn to `#REF!`. Delete
   that cell, then ribbon **Rebuild**: they heal.
8. Switch to Manual calculation and change the coupon again: `G1` changes
   only after F9.
9. Settings → "Excel RTD throttle": "Use 250 ms" makes the live cells update
   about four times sooner; "Excel default" restores 2,000 ms.

## Live server cells

Start the showcase (`docker build -t convex-showcase . && docker run -p
8080:8080 convex-showcase` from the repository root). Its feed moves
`INSTRUMENT-001` once a second. Use Automatic calculation.

1. Ribbon **Server**: Server URL `http://localhost:8080`, Sign in with
   *None*, **Save and connect**. The ribbon's connection button reads
   **Connected**; `=CX.STATUS()` shows `Connected`.
2. `H1: =CX.LIVE("INSTRUMENT-001", "clean_price_mid")`: a price near 99.5
   that moves by itself. `H2: =CX.LIVE("INSTRUMENT-001", "clean_price_mid",
   "quality")`: `Demo`. `H3: =CX.LIVE("INSTRUMENT-001", "ytm_mid",
   "revision")`: rises with each update.
3. `H4: =H1*2`: follows `H1` with no F9.
4. `H5: =CX.LIVE("INSTRUMENT-001", "no_such_mid")`: `#N/A`;
   `=CX.LASTERROR(H5)` lists the measures the result has.
5. `H6: =CX.LIVE("UNKNOWN-1", "clean_price_mid")`: `#N/A`, and
   `CX.LASTERROR` says the runtime has no result for it.
6. `H7: =CX.LIVE.GRID("INSTRUMENT-001", , H1)`: measure, value and quality
   rows that refresh with `H1`.
7. `H8: =CX.SNAP("INSTRUMENT-001", "clean_price_mid")`: a price that stays
   put while `H1` moves.
8. Stop the container: within a few seconds the connection button reads
   **Reconnecting**, `H2` turns `Stale`, and `H1` keeps its last price.
   Start it again: the button returns to **Connected**, `H2` to `Demo`, and
   `H1` moves again.
9. Switch to Manual calculation: `H1` changes only on F9, and
   `=CX.STATUS("calculation")` shows `Manual` after F9.

Single sign-on needs an identity provider and a server with
`[access.introspection]`; check that **Sign in** opens the browser, that
`=CX.STATUS("user")` names you afterwards, and that **Sign out** turns
`H2` to `Stale`.

## Limits

The Docker showcase refuses limit writes and the check route at its edge, so
run a local-development server you can write to: the showcase natively, as
in `excel/bench/README.md` (*Limits against a local server*), on
`http://127.0.0.1:8081` with its feed running. Its `SHOWCASE-BOOK` holds
1,000,000 face of `INSTRUMENT-001`. Point **Server** at that URL with Sign in
*None*.

1. `L1: =CX.LIMIT("SHOWCASE-BOOK", "mv")`: `#N/A`, and `=CX.LASTERROR(L1)`
   says the server has no limit set for the book.
2. Commit a limit set with an `mv` limit (`market_value`) whose upper bound
   is about 100 above the book's market value and `warning_fraction`
   `0.9998`, so the feed moves it across the band (the bench README has the
   requests). Within a second or two `L1` shows `within`, `warning` or
   `breach`, and changes by itself as the price moves.
3. `L2: =CX.LIMIT("SHOWCASE-BOOK", "mv", "value")`: about 1,012,000, the
   value of the evaluation shown; it moves only when the status changes.
   `"utilisation"` is close to 1, `"upper"` the bound, `"measure"`
   `market_value`, `"quality"` `Demo`, `"record_id"` rises with each change
   (`#N/A` until the first one arrives), `"evaluated_at"` a UTC time.
4. `L3: =IF(L1="breach", "STOP", "")`: follows `L1` with no F9.
5. `=CX.LIMIT("SHOWCASE-BOOK", "nope")`: `#N/A`; `CX.LASTERROR` says the set
   has no limit `nope`. `=CX.LIMIT("SHOWCASE-BOOK", "mv", "colour")`:
   `#NAME?`. `=CX.STATUS("books")`: the books followed.
6. Ribbon **Limit Check**: book `SHOWCASE-BOOK`, trade `INSTRUMENT-001`
   `1000000`, **Check**: `mv` goes to `breach` (bold, red) and `face` to
   `warning` if the set has a face limit at 2,000,000. Add a row
   `NO-SUCH-BOND 1000`: the check is refused and that row is marked
   `UNKNOWN_INSTRUMENT`. Delete the rows and **Check**: the current status.
   Select an empty cell and **Stamp CX.LIMIT**: live cells for each limit.
   Excel stays interactive while the ticket is open.
7. Stop the server: `L1` keeps its status and `"quality"` turns `Stale`.
   Start it again: `"quality"` returns to `Demo` and `L1` follows the feed.

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
Viewer/Scenario/Objects/Error Log/Limit Check are MODELESS — verify Excel stays
interactive (select cells, type) while each is open, and that re-clicking
the button focuses the existing window instead of duplicating it:

- **New Bond** → Fixed Rate tab → Build → status reads `OK — #CX#NNN`. Then
  **Build + paste handle** → the active cell holds the handle and Excel stays
  responsive. Every label reads in full, also at 150% display scale.
- **Pricing Ticket** → Refresh objects → pick the bond → mark `99.5C` →
  Compute → grid populates with finite numbers.
- **Spread Ticket** → pick bond + curve → spread `Z` → Compute → finite bps.
- **Curve Viewer** → pick a curve → chart renders an upward yield curve.
- **Object Browser** → handles match `=CX.OBJECTS()` → Describe shows
  bond fields (coupon, maturity, frequency, currency, face).
- **Error Log** → Connection tab lists the link's events after a sign-in or
  a reconnect. With a live server cell updating, `=CX.DIAG()` shows frames
  received and both latencies, and the log file it names holds the same events
  and no token.
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
