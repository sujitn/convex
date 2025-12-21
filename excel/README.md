# Convex Excel Add-In

High-performance fixed income analytics for Microsoft Excel, powered by Rust.

## Installation

### Prerequisites
- Microsoft Excel 2016 or later (64-bit)
- .NET 8.0 Runtime (Windows)

### Build from Source

1. Build the Rust FFI library (release mode):
   ```powershell
   cd convex
   cargo build --release -p convex-ffi
   ```

2. Build the Excel add-in:
   ```powershell
   cd excel/Convex.Excel
   dotnet build --configuration Release
   ```

3. Copy `target/release/convex_ffi.dll` to the output directory:
   ```powershell
   copy target\release\convex_ffi.dll excel\Convex.Excel\bin\Release\net472\
   copy target\release\convex_ffi.dll excel\Convex.Excel\bin\Release\net472\publish\
   ```

### Loading the Add-In

1. Open Excel
2. Go to File > Options > Add-ins
3. At the bottom, select "Excel Add-ins" and click "Go"
4. Click "Browse" and navigate to:
   - `excel/Convex.Excel/bin/Release/net472/publish/Convex.Excel64-packed.xll`
5. Click OK

## Excel Functions

All functions use the `CX.` prefix.

### Curve Functions

| Function | Description |
|----------|-------------|
| `CX.CURVE(name, refDate, tenors, rates, interp, dayCount)` | Create curve from zero rates |
| `CX.CURVE.DF(name, refDate, tenors, dfs, interp, dayCount)` | Create curve from discount factors |
| `CX.CURVE.ZERO(curve, tenor)` | Get zero rate at tenor |
| `CX.CURVE.DISCOUNT(curve, tenor)` | Get discount factor at tenor |
| `CX.CURVE.FORWARD(curve, start, end)` | Get forward rate between tenors |
| `CX.CURVE.SHIFT(curve, bps, newName)` | Parallel shift curve |
| `CX.CURVE.TWIST(curve, shortBp, longBp, pivot, newName)` | Twist curve |
| `CX.CURVE.BUMP(curve, tenor, bps, newName)` | Bump specific tenor |

### Bond Functions

| Function | Description |
|----------|-------------|
| `CX.BOND(isin, coupon%, freq, maturity, issue, dayCount, bdc)` | Create fixed-rate bond |
| `CX.BOND.CORP(isin, coupon%, maturity, issue)` | Create US corporate bond |
| `CX.BOND.TSY(cusip, coupon%, maturity, issue)` | Create US Treasury bond |
| `CX.BOND.ACCRUED(bond, settle)` | Get accrued interest |
| `CX.BOND.MATURITY(bond)` | Get maturity date |
| `CX.BOND.COUPON(bond)` | Get coupon rate |

### Pricing Functions

| Function | Description |
|----------|-------------|
| `CX.YIELD(bond, settle, price, freq)` | Calculate YTM from price |
| `CX.PRICE(bond, settle, yield, freq)` | Calculate price from yield |
| `CX.DIRTY.PRICE(bond, settle, yield, freq)` | Calculate dirty price |

### Risk Functions

| Function | Description |
|----------|-------------|
| `CX.DURATION(bond, settle, price, freq)` | Modified duration |
| `CX.DURATION.MAC(bond, settle, price, freq)` | Macaulay duration |
| `CX.CONVEXITY(bond, settle, price, freq)` | Convexity |
| `CX.DV01(bond, settle, price, freq)` | Dollar value of 1bp |
| `CX.ANALYTICS(bond, settle, price, freq)` | All metrics (array) |

### Utility Functions

| Function | Description |
|----------|-------------|
| `CX.YEARFRAC(start, end, dayCount)` | Day count fraction |
| `CX.VERSION()` | Library version |
| `CX.OBJECT.COUNT()` | Registered object count |
| `CX.TYPE(handle)` | Object type |
| `CX.LOOKUP(name)` | Find object by name |
| `CX.RELEASE(handle)` | Release object |
| `CX.CLEAR.ALL()` | Clear all objects |
| `CX.LAST.ERROR()` | Last error message |

## Parameters

### Interpolation Methods
- `0` = Linear
- `1` = Log-Linear
- `2` = Cubic Spline
- `3` = Monotone Convex

### Day Count Conventions
- `0` = ACT/360
- `1` = ACT/365 Fixed
- `2` = ACT/ACT ISDA
- `3` = ACT/ACT ICMA
- `4` = 30/360 US
- `5` = 30E/360

### Business Day Conventions
- `0` = Unadjusted
- `1` = Following
- `2` = Modified Following
- `3` = Preceding

## Example Usage

### Creating a Yield Curve

```excel
=CX.CURVE("USD.GOVT", TODAY(), {1,2,5,10}, {0.03,0.035,0.04,0.045}, 0, 1)
```

### Creating a Bond and Calculating Analytics

```excel
' Create a 5-year corporate bond
=CX.BOND.CORP("AAPL4.65%2026", 4.65, DATE(2026,2,15), DATE(2021,2,15))

' Calculate yield from price (returns handle in B1)
=CX.YIELD(B1, TODAY(), 102.5, 2)

' Calculate all risk metrics
=CX.ANALYTICS(B1, TODAY(), 102.5, 2)
```

### Curve Transformations

```excel
' Shift curve +50bp
=CX.CURVE.SHIFT(A1, 50, "USD.GOVT.+50")

' Flatten curve (short +25bp, long -25bp, pivot at 5Y)
=CX.CURVE.TWIST(A1, 25, -25, 5, "USD.GOVT.FLAT")

' Bump 10Y point by 10bp
=CX.CURVE.BUMP(A1, 10, 10, "USD.GOVT.10Y+10")
```

## Demo Workbook Layout

Create a workbook with these sheets:

### Sheet 1: Curves
- Row 1: Headers (Tenor, Rate, DF, Forward)
- Column A: Tenors (0.25, 0.5, 1, 2, 3, 5, 7, 10, 20, 30)
- Column B: Input rates
- Column C: `=CX.CURVE.DISCOUNT($B$1, A2)`
- Column D: `=CX.CURVE.FORWARD($B$1, A2, A3)`

### Sheet 2: Bonds
- Create several test bonds
- Calculate price/yield for each
- Show risk metrics side-by-side

### Sheet 3: Scenario Analysis
- Create base curve
- Apply parallel shifts (+50, +100, -50, -100 bp)
- Show price impact on portfolio

## Ribbon UI

The add-in includes a "Convex" ribbon tab with:

- **Curves Group**: New Curve, Curve Viewer
- **Bonds Group**: New Bond, Bond Analyzer
- **Analysis Group**: Price/Yield, Risk Metrics
- **Tools Group**: Object Browser, Clear All, About

## Troubleshooting

### DLL Not Found
Ensure `convex_ffi.dll` is in the same directory as the .xll file.

### Function Returns #VALUE!
Use `=CX.LAST.ERROR()` to see the error message.

### Handle Invalid
Handles are 64-bit integers. If a function returns 0, the operation failed.

## Architecture

```
Excel <-> Excel-DNA (.NET) <-> P/Invoke <-> convex_ffi.dll (Rust)
                                               |
                                               v
                                    convex-core, convex-curves,
                                    convex-bonds, convex-analytics
```

All complex objects (curves, bonds) are stored in a thread-safe Rust registry
and accessed via 64-bit handles. This ensures memory safety while providing
high performance.
