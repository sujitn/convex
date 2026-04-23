# Convex Excel add-in smoke test

Validates the SafeCall refactor (`ExcelErrorHelper.cs`) at runtime against
the happy path, controlled errors, and unchecked exceptions. Tier 3.5.

## Prerequisites

Rebuilt artifacts (already staged by this session):

- `excel/Convex.Excel/bin/Release/net472/Convex.Excel64.xll` (Excel-DNA add-in)
- `excel/Convex.Excel/bin/Release/net472/convex_ffi.dll` (Rust FFI, must sit alongside the xll)

If either is stale, rebuild:

```bash
cargo build --release -p convex-ffi
cd excel/Convex.Excel && dotnet build --configuration Release
cp ../../target/release/convex_ffi.dll bin/Release/net472/
```

## Load the add-in

Option A — one-shot launcher (kills any running Excel first):

```bash
start excel /x "excel\Convex.Excel\bin\Release\net472\Convex.Excel64.xll"
```

Option B — from a running Excel: **File → Options → Add-ins → Manage: Excel Add-ins → Go → Browse…**
and pick the `Convex.Excel64.xll` above. Enable the "Convex" ribbon tab if prompted.

## Test matrix

Open a fresh workbook. Paste the four formulas into `A2:A5` (`A1` is the header).
Expected values are evaluated on a straight-par UST (coupon = yield → clean price = par, no accrued at settle = issue).

| Cell | Formula | Expected | What it validates |
|:---:|---|---|---|
| `A2` | `=CX.BOND.TSY("TEST123", 5.0, DATE(2035,12,31), DATE(2025,12,31))` | a handle like `#CX#100`, `#CX#101`, … | Bond creation succeeds and returns a formatted handle |
| `A3` | `=CX.PRICE(A2, DATE(2025,12,31), 5.0, 2)` | **100.000000** (±1e-6) | Happy path. Yield = coupon at issue → par |
| `A4` | `=CX.PRICE("GARBAGE_HANDLE", DATE(2025,12,31), 5.0, 2)` | `#REF!` (Excel error, not a text string) | **Controlled error path** — `HandleHelper.Parse` returns `INVALID_HANDLE`, UDF returns `ExcelError.ExcelErrorRef` |
| `A5` | `=CX.PRICE(A2, DATE(2025,12,31), 5.0, "not_a_number")` | text string starting `#ERROR:` — e.g. `#ERROR: Input string was not in a correct format.` (exact wording may vary by .NET locale) | **SafeCall exception path** — `Convert.ToInt32("not_a_number")` throws, `SafeCall` catches and returns the error string. Without the refactor this would surface as `#VALUE!` with no diagnostic |

## Pass criteria

1. `A2` returns a handle string (not `#VALUE!` / not blank).
2. `A3` returns **100** to six decimals. (Off by more → flag.)
3. `A4` renders as the native Excel error `#REF!`, not as text.
4. `A5` renders as a **text string** beginning with `#ERROR:`. The message
   must be human-readable — this is the whole point of the SafeCall refactor.
   A plain `#VALUE!` or `#NUM!` here would mean the exception was swallowed
   by Excel-DNA instead of by `SafeCall` and the refactor isn't live.

## Optional — quick expanded check

| Cell | Formula | Expected |
|:---:|---|---|
| `A6` | `=CX.PRICE(A2, DATE(2030,12,31), 5.0, 2)` | still 100.000000 (par yield throughout life) |
| `A7` | `=CX.PRICE(A2, DATE(2025,12,31), 6.0, 2)` | **92.561263** (±1e-4; QuantLib 1.40 reference) |
| `A8` | `=CX.PRICE(A2, DATE(2025,12,31), 4.0, 2)` | **108.175717** (±1e-4; QuantLib 1.40 reference) |

## After the test

Reply with which cells passed / failed. I'll mark 3.5 done in
`NEXT_STEPS.md` if everything lands, or dig into the specific failure if
one of the four assertions breaks.
