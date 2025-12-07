# convex-risk

Risk analytics for the Convex fixed income analytics library.

## Overview

`convex-risk` provides comprehensive risk measurement capabilities:

- **Duration**: Macaulay, modified, effective, key rate, spread, and partial durations
- **Convexity**: Analytical and effective convexity
- **DV01/PV01/PVBP**: Dollar value of a basis point
- **VaR**: Historical and parametric Value-at-Risk
- **Hedging**: Hedge ratio calculation and portfolio analysis

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
convex-risk = "0.1"
```

## Usage

### Duration Calculation

```rust
use convex_risk::duration::{MacaulayDuration, ModifiedDuration, EffectiveDuration};

let settlement = Date::from_ymd(2024, 1, 15);

// Macaulay duration
let mac_dur = MacaulayDuration::calculate(&bond, settlement)?;

// Modified duration
let mod_dur = ModifiedDuration::calculate(&bond, settlement)?;

// Effective duration (for bonds with options)
let eff_dur = EffectiveDuration::calculate(
    &callable_bond,
    settlement,
    &curve,
    bump_size: 0.0001,
)?;
```

### Key Rate Duration

```rust
use convex_risk::duration::KeyRateDuration;

let key_rates = vec![1.0, 2.0, 5.0, 10.0, 30.0];  // Pillar points

let krd = KeyRateDuration::calculate(
    &bond,
    settlement,
    &curve,
    &key_rates,
)?;

for (tenor, duration) in krd.iter() {
    println!("KRD at {}Y: {:.4}", tenor, duration);
}
```

### Convexity

```rust
use convex_risk::convexity::{Convexity, EffectiveConvexity};

// Analytical convexity for bullet bonds
let conv = Convexity::calculate(&bond, settlement)?;

// Effective convexity for bonds with options
let eff_conv = EffectiveConvexity::calculate(
    &callable_bond,
    settlement,
    &curve,
    bump_size: 0.0001,
)?;
```

### DV01 Calculation

```rust
use convex_risk::dv01::DV01;

let dv01 = DV01::calculate(&bond, settlement)?;

println!("DV01: ${:.2} per $1MM notional", dv01.per_million());
```

### Portfolio Risk

```rust
use convex_risk::hedging::Portfolio;

let portfolio = Portfolio::new()
    .add_position(&bond1, dec!(10_000_000))
    .add_position(&bond2, dec!(5_000_000))
    .add_position(&bond3, dec!(7_500_000));

// Total portfolio duration
let port_duration = portfolio.modified_duration(settlement)?;

// Total portfolio DV01
let port_dv01 = portfolio.dv01(settlement)?;

// Key rate exposures
let port_krd = portfolio.key_rate_durations(settlement, &curve, &key_rates)?;
```

### Value-at-Risk

```rust
use convex_risk::var::{HistoricalVaR, ParametricVaR};

// Historical VaR
let hist_var = HistoricalVaR::calculate(
    &portfolio,
    &historical_returns,
    confidence: 0.99,
    horizon_days: 10,
)?;

// Parametric VaR
let param_var = ParametricVaR::calculate(
    &portfolio,
    &covariance_matrix,
    confidence: 0.99,
    horizon_days: 10,
)?;
```

## License

This project is licensed under the MIT License - see the [LICENSE](../../LICENSE) file for details.
