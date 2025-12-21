# Convex Project - Claude Code Memory

## Project Overview
Convex is a high-performance fixed income analytics library built in Rust with an Excel add-in interface via Excel-DNA (.NET Framework 4.7.2).

## Architecture

### Rust Crates
- `convex-bonds` - Bond types, pricing, risk metrics
- `convex-curves` - Yield curve construction and interpolation
- `convex-ffi` - C FFI layer for Excel integration
- `convex-dates` - Date handling and day count conventions

### Excel Add-in (C#)
- Location: `excel/Convex.Excel/`
- Uses Excel-DNA for UDF and ribbon integration
- P/Invoke calls to `convex_ffi.dll`

## Key Technical Details

### Handle System
- Format: `#CX#100`, `#CX#101`, etc. (starts from 100)
- Defined in `HandleHelper.cs` and `registry.rs`
- Registry starts at handle 100 (changed from 6000)

### Object Types (registry.rs)
- 1 = Curve
- 2 = FixedBond
- 3 = ZeroBond
- 4 = FloatingRateNote
- 5 = CallableBond

### FFI Parameter Conventions
- `convex_bond_fixed`: coupon as decimal (0.05 for 5%)
- `convex_bond_us_corporate`: coupon as percentage (5.0 for 5%)
- `convex_bond_us_treasury`: coupon as percentage (5.0 for 5%)
- `convex_curve_from_zero_rates`: rates as decimal (0.04 for 4%)
- **Excel UDFs** (`CX.CURVE`, etc.): rates as percentage (4.0 for 4%) - automatically converted
- Dates passed as year, month, day integers

### NativeMethods Signature (convex_bond_fixed)
```csharp
public static extern ulong convex_bond_fixed(
    string isin,
    double couponRate,        // decimal (0.05)
    int maturityYear, int maturityMonth, int maturityDay,
    int issueYear, int issueMonth, int issueDay,
    int frequency,
    int dayCount,
    int currency,             // 0 = USD
    double faceValue);        // typically 100.0
```

## Excel Functions (CX. prefix)

### Curves
- `CX.CURVE` - Create curve from zero rates (input rates as %, e.g., 4.5 for 4.5%)
- `CX.CURVE.ZERO` - Query zero rate (returns %)
- `CX.CURVE.DISCOUNT` - Query discount factor
- `CX.CURVE.FORWARD` - Query forward rate (returns %)

### Bonds
- `CX.BOND` - Create generic fixed bond
- `CX.BOND.CORP` - Create US corporate bond
- `CX.BOND.TSY` - Create US Treasury bond
- `CX.BOND.CALLABLE` - Create callable bond
- `CX.BOND.ACCRUED` - Get accrued interest

### Pricing
- `CX.YIELD` - Calculate YTM (returns %)
- `CX.PRICE` - Calculate clean price from yield
- `CX.DIRTY.PRICE` - Calculate dirty price
- `CX.YIELD.CALL` - Yield to call

### Risk
- `CX.DURATION` - Modified duration
- `CX.DURATION.MAC` - Macaulay duration
- `CX.CONVEXITY` - Convexity
- `CX.DV01` - Dollar value of 1bp
- `CX.ANALYTICS` - All metrics at once

### Spreads
- `CX.ZSPREAD` - Z-spread
- `CX.ISPREAD` - I-spread
- `CX.GSPREAD` - G-spread
- `CX.ASW` - Asset swap spread

### Bootstrapping
- `CX.BOOTSTRAP` - Bootstrap curve from deposits and swaps (Global Fit method)
- `CX.BOOTSTRAP.OIS` - Bootstrap OIS curve
- `CX.BOOTSTRAP.MIXED` - Bootstrap curve from mixed instrument types (Deposit, FRA, Swap, OIS)
- `CX.BOOTSTRAP.PIECEWISE` - Bootstrap using piecewise/iterative method (Brent root-finding)

## Curve Bootstrapping

### Supported Instrument Types
- **Deposit** (type=0): Money market deposits, simple interest
- **FRA** (type=1): Forward rate agreements
- **Swap** (type=2): Interest rate swaps (semi-annual, 30/360)
- **OIS** (type=3): Overnight index swaps

### Calibration Methods

**Global Fit (Default):** Uses Levenberg-Marquardt optimization to fit all instruments simultaneously.
- Better stability with over-determined systems
- Handles instrument interdependencies
- May not exactly match market quotes

**Piecewise Bootstrap:** Iterative bootstrap using Brent root-finding.
- Exact fit to market quotes (within tolerance)
- Faster for standard curves
- Industry standard approach
- Sensitive to instrument ordering

### FFI Functions
```rust
// Deposit + Swap bootstrap
convex_bootstrap_from_instruments(
    name, ref_year, ref_month, ref_day,
    deposit_tenors, deposit_rates, deposit_count,
    swap_tenors, swap_rates, swap_count,
    interpolation, day_count
) -> Handle

// OIS only bootstrap
convex_bootstrap_ois(
    name, ref_year, ref_month, ref_day,
    tenors, rates, count,
    interpolation, day_count
) -> Handle

// Mixed instruments bootstrap
convex_bootstrap_mixed(
    name, ref_year, ref_month, ref_day,
    instrument_types, tenors, rates, count,
    interpolation, day_count
) -> Handle
```

### C# Wrapper Methods
```csharp
ConvexWrapper.BootstrapCurve(name, refDate, depositTenors, depositRates, swapTenors, swapRates, interpolation, dayCount)
ConvexWrapper.BootstrapOISCurve(name, refDate, tenors, rates, interpolation, dayCount)
ConvexWrapper.BootstrapMixedCurve(name, refDate, instrumentTypes, tenors, rates, interpolation, dayCount)
```

## Ribbon Forms

### Implemented Forms
1. **NewCurveForm** - Create curves with data grid for tenor/rate points
2. **NewBondForm** - Create bonds with callable option support
3. **CurveViewerForm** - View curves with chart (zero rate + forward rate)
4. **BondAnalyzerForm** - Analyze bonds with cashflow chart
5. **ObjectBrowserForm** - Browse registered objects
6. **HelpForm** - Tabbed documentation
7. **BootstrapForm** - Bootstrap curves from market instruments (deposits, swaps, OIS, FRAs)

### Custom Ribbon Icons
- Icons are programmatically generated in `RibbonController.LoadImage()`
- 32x32 pixel bitmaps drawn with System.Drawing
- No external icon files required

## Common Issues & Solutions

### SplitContainer Error
Don't set Panel1MinSize/Panel2MinSize in initializer - can cause "SplitterDistance must be between..." error. Set them after construction or omit them.

### Bond Creation Fails
Check parameter order matches Rust FFI signature. `convex_bond_fixed` expects coupon as decimal, dates before frequency.

### Missing Microsoft.CSharp
Add `<Reference Include="Microsoft.CSharp" />` to .csproj for dynamic keyword usage.

## Build Commands

```bash
# Build Rust FFI
cargo build --release -p convex-ffi

# Build Excel add-in
cd excel/Convex.Excel
dotnet build --configuration Release

# Copy DLL (if needed)
copy target\release\convex_ffi.dll excel\Convex.Excel\bin\Release\net472\

# Launch Excel with add-in
start excel /x "excel\Convex.Excel\bin\Release\net472\Convex.Excel64.xll"
```

## Recent Changes (Session Summary)

### Handle Format Change
- Changed from `#XXXX` to `#CX#XXX`
- Registry starts at 100 instead of 6000

### New Forms Added
- CurveViewerForm with yield curve chart
- BondAnalyzerForm with cashflow chart
- NewCurveForm and NewBondForm for object creation
- HelpForm with tabbed documentation

### Custom Ribbon Icons
- Replaced imageMso with programmatically drawn icons
- LoadImage override in RibbonController

### Bug Fixes
- Fixed convex_bond_fixed signature mismatch
- Fixed SplitContainer initialization errors
- Fixed BondAnalyzerForm layout (input panel height 80→130)
