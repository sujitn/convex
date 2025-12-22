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
| `CX.BOND.ZERO(isin, maturity, issue, compounding, dayCount)` | Create zero coupon bond |
| `CX.BOND.TBILL(cusip, maturity, issue)` | Create US Treasury Bill |
| `CX.BOND.FRN(isin, spread, rateIndex, maturity, issue, freq, dayCount, cap, floor)` | Create floating rate note |
| `CX.BOND.TSYFRN(cusip, spread, maturity, issue)` | Create US Treasury FRN |
| `CX.BOND.CALLABLE(isin, coupon%, maturity, issue, callDates, callPrices, freq)` | Create callable bond |
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

### Spread Functions

| Function | Description |
|----------|-------------|
| `CX.ZSPREAD(bond, curve, settle, price)` | Z-spread (constant spread over spot curve) |
| `CX.ISPREAD(bond, swapCurve, settle, yield)` | I-spread (spread over swap curve) |
| `CX.GSPREAD(bond, govtCurve, settle, yield)` | G-spread (spread over government curve) |
| `CX.ASW(bond, swapCurve, settle, price)` | Asset swap spread (par-par) |
| `CX.ZSPREAD.ANALYTICS(bond, curve, settle, price)` | Z-spread with DV01 and duration (array) |

### Price from Spread Functions

| Function | Description |
|----------|-------------|
| `CX.PRICE.ZSPREAD(bond, curve, settle, zSpreadBps)` | Clean price from Z-spread |
| `CX.DIRTY.PRICE.ZSPREAD(bond, curve, settle, zSpreadBps)` | Dirty price from Z-spread |
| `CX.PRICE.DM(frn, fwdCurve, discCurve, settle, dmBps)` | FRN price from discount margin |

### Callable Bond Analytics

| Function | Description |
|----------|-------------|
| `CX.YIELD.WORST(callable, settle, price, freq)` | Yield to worst |
| `CX.WORKOUT.DATE(callable, settle, price, freq)` | Workout date for YTW |
| `CX.OAS(callable, curve, settle, price, vol)` | Option-adjusted spread |

### FRN Analytics

| Function | Description |
|----------|-------------|
| `CX.DISCOUNT.MARGIN(frn, fwdCurve, discCurve, settle, price)` | Discount margin (Z-DM) |
| `CX.SIMPLE.MARGIN(frn, settle, price)` | Simple margin |

### Curve Bootstrapping

| Function | Description |
|----------|-------------|
| `CX.BOOTSTRAP(name, refDate, depTenors, depRates, swapTenors, swapRates, interp, dayCount)` | Bootstrap from deposits + swaps |
| `CX.BOOTSTRAP.OIS(name, refDate, tenors, rates, interp, dayCount)` | Bootstrap OIS curve |
| `CX.BOOTSTRAP.MIXED(name, refDate, types, tenors, rates, interp, dayCount)` | Bootstrap from mixed instruments |
| `CX.BOOTSTRAP.PIECEWISE(name, refDate, types, tenors, rates, interp, dayCount)` | Piecewise bootstrap (Brent solver) |

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

### RTD Functions (Real-Time Data) - Optional

**Note:** Regular `CX.*` functions now work correctly with Excel's dependency chain. When inputs change (e.g., BDP updates), curves/bonds are recreated with new handles, forcing dependent cells to recalculate.

**When to use regular functions:** Most scenarios - they now handle BDP/streaming data correctly.

**When to use RTD functions:** Only for advanced scenarios requiring:
- Server-side calculation throttling
- Explicit subscription management
- Integration with custom data feeds

RTD-enabled functions:

| Function | Description |
|----------|-------------|
| `CX.CURVE.RTD(name, refDate, tenors, rates, interp, dayCount)` | Create curve with real-time updates |
| `CX.CURVE.ZERO.RTD(curveName, tenor)` | Get zero rate with real-time updates |
| `CX.BOND.RTD(id, coupon%, freq, maturity, issue, dayCount, bdc)` | Create bond with real-time updates |
| `CX.BOND.CORP.RTD(id, coupon%, maturity, issue)` | Create US corporate bond (RTD) |
| `CX.YIELD.RTD(bondName, settle, price, freq)` | Calculate YTM with real-time updates |
| `CX.PRICE.RTD(bondName, settle, yield%, freq)` | Calculate price with real-time updates |
| `CX.DURATION.RTD(bondName, settle, price, freq)` | Modified duration with real-time updates |
| `CX.CONVEXITY.RTD(bondName, settle, price, freq)` | Convexity with real-time updates |
| `CX.DV01.RTD(bondName, settle, price, freq)` | DV01 with real-time updates |
| `CX.ZSPREAD.RTD(bondName, curveName, settle, price)` | Z-spread with real-time updates |
| `CX.PRICE.ZSPREAD.RTD(bondName, curveName, settle, zSpreadBps)` | Price from Z-spread (RTD) |
| `CX.RTD.STATS()` | Get RTD server statistics |
| `CX.RTD.REFRESH(pattern)` | Force refresh of topics matching pattern |

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

### Compounding Frequencies (Zero Coupon Bonds)
- `0` = Continuous
- `1` = Annual
- `2` = Semi-Annual
- `4` = Quarterly

### Rate Index (FRN)
- `0` = SOFR
- `1` = SONIA
- `2` = EURIBOR
- `3` = LIBOR (legacy)

### Bootstrap Instrument Types
- `0` = Deposit (money market)
- `1` = FRA (forward rate agreement)
- `2` = Swap (interest rate swap)
- `3` = OIS (overnight index swap)

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

### Spread Calculations

```excel
' Calculate Z-spread from market price
=CX.ZSPREAD(bondHandle, curveHandle, TODAY()+2, 98.5)

' Calculate price from Z-spread (inverse)
=CX.PRICE.ZSPREAD(bondHandle, curveHandle, TODAY()+2, 150)

' Get full Z-spread analytics (spread, DV01, duration)
=CX.ZSPREAD.ANALYTICS(bondHandle, curveHandle, TODAY()+2, 98.5)
```

### Callable Bond Analytics

```excel
' Create a callable bond with call schedule
=CX.BOND.CALLABLE("XYZ5%2030", 5.0, DATE(2030,6,15), DATE(2020,6,15),
    {DATE(2025,6,15), DATE(2027,6,15)}, {100, 100}, 2)

' Calculate yield to worst
=CX.YIELD.WORST(callableHandle, TODAY()+2, 102.5, 2)

' Get workout date (when YTW occurs)
=CX.WORKOUT.DATE(callableHandle, TODAY()+2, 102.5, 2)

' Calculate OAS with 20% volatility
=CX.OAS(callableHandle, curveHandle, TODAY()+2, 102.5, 0.20)
```

### FRN Analytics

```excel
' Create an FRN (SOFR + 50bp, quarterly)
=CX.BOND.FRN("FRN2027", 50, 0, DATE(2027,3,15), DATE(2024,3,15), 4, 0, 0, 0)

' Calculate discount margin
=CX.DISCOUNT.MARGIN(frnHandle, fwdCurve, discCurve, TODAY()+2, 99.75)

' Calculate price from discount margin (inverse)
=CX.PRICE.DM(frnHandle, fwdCurve, discCurve, TODAY()+2, 55)
```

### Curve Bootstrapping

```excel
' Bootstrap curve from deposits and swaps
=CX.BOOTSTRAP("USD.SWAP", TODAY(),
    {0.25, 0.5, 1},          ' Deposit tenors
    {5.25, 5.30, 5.35},      ' Deposit rates (%)
    {2, 3, 5, 7, 10, 30},    ' Swap tenors
    {4.75, 4.50, 4.25, 4.15, 4.10, 4.25},  ' Swap rates (%)
    0, 0)  ' Linear interp, ACT/360

' Bootstrap from mixed instrument types
=CX.BOOTSTRAP.MIXED("USD.OIS", TODAY(),
    {0, 0, 3, 3, 3},         ' Types: Deposit, Deposit, OIS, OIS, OIS
    {0.25, 0.5, 1, 2, 5},    ' Tenors
    {5.25, 5.30, 5.00, 4.50, 4.00},  ' Rates (%)
    0, 0)
```

### Real-Time Data with Bloomberg BDP

```excel
' Create curve from Bloomberg real-time rates
' Rates in A1:A5 are fed by =BDP("UST 2Y", "YLD_YTM_MID") etc.
=CX.CURVE.RTD("USD.GOVT.LIVE", TODAY(), {2,5,10,20,30}, A1:A5, 0, 1)

' Create bond (static, doesn't change)
=CX.BOND.CORP.RTD("AAPL5%2030", 5.0, DATE(2030,2,15), DATE(2020,2,15))

' Calculate Z-spread using live curve - updates automatically when curve changes
=CX.ZSPREAD.RTD("AAPL5%2030", "USD.GOVT.LIVE", TODAY()+2, B1)
' Where B1 = =BDP("AAPL 5 02/15/30 Corp", "PX_LAST")

' Duration, convexity, DV01 all update when price changes
=CX.DURATION.RTD("AAPL5%2030", TODAY()+2, B1, 2)
=CX.CONVEXITY.RTD("AAPL5%2030", TODAY()+2, B1, 2)
=CX.DV01.RTD("AAPL5%2030", TODAY()+2, B1, 2)
```

**How RTD works:**
1. RTD functions subscribe to a topic (e.g., "curve:USD.GOVT.LIVE")
2. When inputs change (e.g., BDP updates), the curve is recalculated
3. All dependent topics (Z-spread, duration, etc.) automatically update
4. Updates are throttled (default 100ms) to prevent calculation storms

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

- **Curves Group**: New Curve, Curve Viewer, Bootstrap
- **Bonds Group**: New Bond (Fixed, Zero, FRN, Callable), Bond Analyzer
- **Analysis Group**: Price/Yield, Risk Metrics, Spreads
- **Tools Group**: Object Browser, Clear All, Help, About

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
