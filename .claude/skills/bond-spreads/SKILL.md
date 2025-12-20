---
name: bond-spreads
description: "Fixed income bond spread calculations with Bloomberg YAS parity. Implements Z-Spread, I-Spread, G-Spread, ASW (Asset Swap Spread), and OAS (Option-Adjusted Spread) for USD/EUR/GBP markets. Use when: (1) Calculating any bond spread measure, (2) Building yield curve bootstrapping, (3) Implementing spread solvers, (4) Working with post-LIBOR curves (SOFR/SONIA/€STR), (5) Bond pricing analytics, (6) Comparing spreads across methodologies, (7) Implementing Bloomberg YAS-equivalent functionality."
---

# Bond Spread Calculations

Implements five spread methodologies for fixed income analytics with Bloomberg YAS parity and data-source neutral architecture.

## Spread Hierarchy (Simplest → Most Complex)

| Spread | What It Measures | Complexity | Curve Required |
|--------|------------------|------------|----------------|
| G-Spread | Yield over interpolated government bond | Low | Gov't par curve |
| I-Spread | Yield over interpolated swap rate | Low | Swap par curve |
| Z-Spread | Constant spread over zero curve matching dirty price | Medium | Zero/spot curve |
| ASW | Spread in swap market terms | Medium | Zero + forward curves |
| OAS | Spread after removing embedded option value | High | Zero + vol surface |

**Typical ordering**: `OAS ≤ Z-Spread ≈ ASW < I-Spread < G-Spread` for investment-grade bonds.

## Quick Reference

### Z-Spread (Primary Spread)

Solve for constant `Z` where:
```
P_dirty = Σ [CF_i / (1 + (s_i + Z)/2)^(2×t_i)]  // Semi-annual
P_dirty = Σ [CF_i × exp(-(s_i + Z) × t_i)]       // Continuous
```

**Solver**: Brent's method (recommended) or Newton-Raphson
- Tolerance: `1e-8` (~0.001 bp)
- Bounds: `[-0.10, 1.00]`
- Initial guess: `YTM - benchmark_YTM`

### I-Spread / G-Spread

```
I-Spread = Bond_YTM - Interpolated_Swap_Rate(maturity)
G-Spread = Bond_YTM - Interpolated_Govt_Yield(maturity)
```

**ISDA interpolation**: Linear on calendar days between bracketing tenors.

### ASW (Par-Par Convention - Bloomberg Default)

```
ASW = (PV_bond_coupons_at_swap_rates - P_dirty) / PV01_annuity
```

### OAS

Requires Hull-White model with trinomial tree or Monte Carlo. See [references/spreads.md](references/spreads.md).

## Currency Conventions (Post-LIBOR)

| Currency | RFR | Day Count | Spot | Fixed Freq |
|----------|-----|-----------|------|------------|
| USD | SOFR | ACT/360 | T+2 | Annual |
| EUR | €STR | ACT/360 | T+2 | Annual |
| GBP | SONIA | ACT/365F | T+0 | Annual |

**Government benchmarks**: USD=Treasuries, EUR=Bunds, GBP=Gilts

## Implementation Checklist

1. **Curve infrastructure** - Zero rates, discount factors, forward rates, par rates with interpolation
2. **Day count functions** - ACT/360, ACT/365F, ACT/ACT ICMA, 30/360
3. **Root-finding solver** - Brent's method with fallback to bisection
4. **Cash flow generation** - Handle settlement, ex-dividend, accrued interest

## Detailed References

- **Spread calculations**: See [references/spreads.md](references/spreads.md) for formulas, solver algorithms, edge cases
- **Curve construction**: See [references/curves.md](references/curves.md) for bootstrapping, interpolation methods
- **Market conventions**: See [references/conventions.md](references/conventions.md) for currency-specific details

## Rust Data Structures

```rust
pub trait YieldCurve {
    fn zero_rate(&self, t: f64) -> f64;
    fn discount_factor(&self, t: f64) -> f64;
    fn forward_rate(&self, t1: f64, t2: f64) -> f64;
    fn par_rate(&self, tenor: f64) -> f64;
}

pub struct BondSpreadResult {
    pub z_spread: Option<f64>,
    pub i_spread: Option<f64>,
    pub g_spread: Option<f64>,
    pub asw_par: Option<f64>,
    pub asw_market: Option<f64>,
    pub oas: Option<f64>,
}

pub enum DayCount { Act360, Act365Fixed, ActActICMA, ActActISDA, Thirty360, ThirtyE360 }
pub enum Compounding { Simple, Annual, SemiAnnual, Quarterly, Continuous }
pub enum InterpolationMethod { Linear, LogLinear, CubicSpline, MonotoneConvex }
```

## Validation Targets

- **Roundtrip**: Input swap rates must reprice exactly (< 1e-10)
- **Bloomberg parity**: Z-spread within ±0.05 bp, OAS within ±0.5 bp
- **Arbitrage-free**: Positive forwards, monotonic discount factors
