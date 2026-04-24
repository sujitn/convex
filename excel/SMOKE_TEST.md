# Convex Excel add-in smoke test

Validates `ExcelErrorHelper.SafeCall` at runtime — happy path,
controlled errors, unchecked exceptions.

## Setup

Artifacts live at `excel/Convex.Excel/bin/Release/net472/`: the xll
and `convex_ffi.dll` must sit side-by-side. Rebuild if stale:

```bash
cargo build --release -p convex-ffi
cd excel/Convex.Excel && dotnet build --configuration Release
cp ../../target/release/convex_ffi.dll bin/Release/net472/
```

Launch:

```bash
start excel /x "excel\Convex.Excel\bin\Release\net472\Convex.Excel64.xll"
```

## Test matrix

Paste into `A2:A5` on a fresh sheet:

| Cell | Formula | Expected | Validates |
|:---:|---|---|---|
| `A2` | `=CX.BOND.TSY("TEST123", 5.0, DATE(2035,12,31), DATE(2025,12,31))` | `#CX#100` or similar | Bond creation |
| `A3` | `=CX.PRICE(A2, DATE(2025,12,31), 5.0, 2)` | `100.000000` | Happy path — par yield = coupon |
| `A4` | `=CX.PRICE("GARBAGE_HANDLE", DATE(2025,12,31), 5.0, 2)` | native `#REF!` | Controlled error — `INVALID_HANDLE` → `ExcelErrorRef` |
| `A5` | `=CX.PRICE(A2, DATE(2025,12,31), 5.0, "not_a_number")` | text `#ERROR: ...` | Exception — `Convert.ToInt32` throws, `SafeCall` catches |

**The load-bearing assertion is A5**: the cell must contain a *text
string* starting `#ERROR:`, not a native `#VALUE!`. A native Excel
error means the exception bypassed `SafeCall` and the refactor isn't
live.

## Optional expanded check

| Cell | Formula | Expected (QuantLib 1.40) |
|:---:|---|---|
| `A6` | `=CX.PRICE(A2, DATE(2030,12,31), 5.0, 2)` | `100.000000` |
| `A7` | `=CX.PRICE(A2, DATE(2025,12,31), 6.0, 2)` | `92.561263` ±1e-4 |
| `A8` | `=CX.PRICE(A2, DATE(2025,12,31), 4.0, 2)` | `108.175717` ±1e-4 |
