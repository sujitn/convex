# convex-spreads

Spread analytics for the Convex fixed income analytics library.

## Overview

`convex-spreads` provides comprehensive spread calculation capabilities:

- **G-Spread**: Spread over interpolated government bond yield
- **I-Spread**: Spread over interpolated swap rate
- **Z-Spread**: Zero-volatility spread over spot curve
- **Asset Swap Spread**: Par-par and proceeds methodologies
- **OAS**: Option-adjusted spread for bonds with embedded options
- **CDS Basis**: Bond spread vs CDS spread for arbitrage analysis
- **Discount Margin**: Spread over reference rate for floating rate notes

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
convex-spreads = "0.1"
```

## Usage

### G-Spread Calculation

```rust
use convex_spreads::GSpread;
use convex_bonds::FixedRateBond;
use convex_curves::DiscountCurve;

let g_spread = GSpread::calculate(
    &bond,
    dec!(105.5),        // Clean price
    settlement,
    &treasury_curve,
)?;

println!("G-Spread: {:.1} bps", g_spread.as_bps());
```

### Z-Spread Calculation

```rust
use convex_spreads::ZSpread;

let z_spread = ZSpread::calculate(
    &bond,
    dec!(105.5),
    settlement,
    &spot_curve,
)?;

println!("Z-Spread: {:.1} bps", z_spread.as_bps());
```

### Asset Swap Spread

```rust
use convex_spreads::asw::{AssetSwapSpread, AswMethod};

// Par-par asset swap
let asw_par = AssetSwapSpread::calculate(
    &bond,
    dec!(105.5),
    settlement,
    &discount_curve,
    AswMethod::ParPar,
)?;

// Proceeds asset swap
let asw_proceeds = AssetSwapSpread::calculate(
    &bond,
    dec!(105.5),
    settlement,
    &discount_curve,
    AswMethod::Proceeds,
)?;
```

### Discount Margin for FRNs

```rust
use convex_spreads::DiscountMargin;
use convex_bonds::floating::FloatingRateNote;

let dm = DiscountMargin::calculate(
    &frn,
    dec!(99.75),
    settlement,
    &forward_curve,
    &discount_curve,
)?;

println!("Discount Margin: {:.1} bps", dm.as_bps());
```

### OAS Calculation

```rust
use convex_spreads::OAS;
use convex_bonds::corporate::CallableBond;

let oas = OAS::calculate(
    &callable_bond,
    dec!(102.5),
    settlement,
    &curve,
    &vol_surface,
    tree_steps: 100,
)?;

println!("OAS: {:.1} bps", oas.as_bps());
```

## Features

- `parallel` - Enable parallel spread calculations with Rayon

## License

This project is licensed under the MIT License - see the [LICENSE](../../LICENSE) file for details.
