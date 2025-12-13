---
name: curve-construction
description: Comprehensive methodology for building all curve types required for bond pricing analytics. Use when implementing: (1) OIS-based discount curves (SOFR, SONIA, €STR), (2) Government yield curves from bond prices, (3) Multi-curve projection frameworks, (4) Cross-currency basis curves, (5) Credit/spread curves (Z-spread, OAS, CDS hazard rates), (6) Inflation-linked curves (TIPS, breakeven inflation), (7) Repo/financing curves, (8) Parametric models (Nelson-Siegel, Svensson). Covers bootstrapping algorithms, interpolation methods, spread calculations, and numerical optimization for high-performance curve construction in quantitative finance libraries.
---

# Curve Construction for Bond Pricing

Build production-quality curves for bond pricing, asset swaps, spread analysis, and derivatives valuation.

## Curve Type Selection

| Curve Type | When to Use | Reference |
|------------|-------------|-----------|
| OIS Discount | Swap/derivative discounting | [ois-curves.md](references/ois-curves.md) |
| Government Yield | Bond spread benchmarks | [govt-curves.md](references/govt-curves.md) |
| Projection (multi-curve) | Forward rate estimation | [multi-curve.md](references/multi-curve.md) |
| Cross-Currency | Multi-currency ASW | [cross-currency.md](references/cross-currency.md) |
| Credit/Spread | Corporate bond pricing | [spread-curves.md](references/spread-curves.md) |
| Inflation (Real) | TIPS/linker pricing | [inflation-curves.md](references/inflation-curves.md) |
| Repo | Carry calculations | [repo-curves.md](references/repo-curves.md) |
| Nelson-Siegel/Svensson | Smooth fitting, forecasting | [parametric-models.md](references/parametric-models.md) |

## Core Architecture

Modern curve construction uses the **multi-curve framework**:

```
┌─────────────────────────────────────────────────────────────┐
│                    CURVE HIERARCHY                          │
├─────────────────────────────────────────────────────────────┤
│  DISCOUNT CURVES (OIS-based)                                │
│  ├── USD: SOFR OIS                                          │
│  ├── EUR: €STR OIS                                          │
│  ├── GBP: SONIA OIS                                         │
│  └── Used for: Present value discounting                    │
├─────────────────────────────────────────────────────────────┤
│  PROJECTION CURVES (tenor-specific)                         │
│  ├── SOFR Term (1M, 3M)                                     │
│  ├── EURIBOR (1M, 3M, 6M)                                   │
│  └── Used for: Forward rate estimation                      │
├─────────────────────────────────────────────────────────────┤
│  GOVERNMENT YIELD CURVES                                    │
│  ├── Treasury (USD), Gilts (GBP), Bunds (EUR)              │
│  ├── Par, Zero, Forward representations                     │
│  └── Used for: Spread benchmarks, risk-free rates           │
├─────────────────────────────────────────────────────────────┤
│  SPREAD/CREDIT CURVES                                       │
│  ├── Z-spread curves by rating/sector                       │
│  ├── CDS hazard rate curves                                 │
│  └── Used for: Corporate bond pricing, credit analysis      │
├─────────────────────────────────────────────────────────────┤
│  INFLATION CURVES                                           │
│  ├── Real yield curves (TIPS, linkers)                      │
│  ├── Breakeven inflation                                    │
│  └── Used for: Inflation-linked bond pricing                │
├─────────────────────────────────────────────────────────────┤
│  CROSS-CURRENCY CURVES                                      │
│  ├── EUR collateralized in USD                              │
│  ├── GBP collateralized in USD                              │
│  └── Used for: Multi-currency asset swaps, XCCY pricing     │
├─────────────────────────────────────────────────────────────┤
│  REPO/FINANCING CURVES                                      │
│  ├── GC repo term structure                                 │
│  ├── Special repo rates by bond                             │
│  └── Used for: Carry, rolldown, funding analysis            │
└─────────────────────────────────────────────────────────────┘
```

## Build Order (Critical)

Always construct curves in this sequence:

1. **OIS discount curves** - SOFR, SONIA, €STR (anchor currency first, typically USD)
2. **Projection curves** - Using OIS for discounting
3. **Cross-currency curves** - Using domestic OIS + FX forwards + XCCY basis

## Quick Reference: Key Equations

### OIS Bootstrapping
```
D(0,Tᵢ) = [1 - sᵢ × Σⱼ₌₁ⁱ⁻¹ D(0,Tⱼ) × τⱼ] / [1 + sᵢ × τᵢ]
```

### Multi-Curve Forward Rate
```
Fₘ(t; T₁,T₂) = (1/τₘ) × [Pₘ(t,T₁)/Pₘ(t,T₂) - 1]
```

### Cross-Currency Basis
```
D_for^USD-coll(T) = D_USD(T) × S / F(T)
```

### Present Value (Multi-Curve)
```
PV = Σᵢ D_discount(Tᵢ) × τₘ × F_projection(Tᵢ₋₁, Tᵢ)
```

### Z-Spread
```
P = Σᵢ CFᵢ × exp(-(r(tᵢ) + z) × tᵢ)
```

### Breakeven Inflation
```
BEI(T) = y_nominal(T) - y_real(T)
```

### Nelson-Siegel Zero Rate
```
r(τ) = β₀ + β₁×[(1-e^(-τ/λ))/(τ/λ)] + β₂×[(1-e^(-τ/λ))/(τ/λ) - e^(-τ/λ)]
```

### CDS Hazard Rate (Approximation)
```
λ ≈ spread / (1 - Recovery)
```

### Carry
```
Carry = Coupon_Accrual - (Dirty_Price × Repo_Rate × Days/360)
```

## Implementation Workflow

### Step 1: OIS Curve Bootstrap

```rust
fn bootstrap_ois(instruments: &[OISSwap]) -> Curve {
    let mut nodes: Vec<(f64, f64)> = vec![(0.0, 1.0)]; // D(0,0) = 1
    
    for inst in instruments.iter().sorted_by_key(|i| i.maturity) {
        let df = solve_discount_factor(inst, &nodes);
        nodes.push((inst.maturity, df));
    }
    
    Curve::new(nodes, Interpolation::LogLinear)
}

fn solve_discount_factor(inst: &OISSwap, prior: &[(f64, f64)]) -> f64 {
    // Direct solve if no intermediate dates needed
    // Otherwise use Brent solver with 1e-12 tolerance
}
```

See [references/ois-curves.md](references/ois-curves.md) for overnight compounding, meeting-date construction, and turn-of-year handling.

### Step 2: Projection Curve Bootstrap

```rust
fn bootstrap_projection(
    instruments: &[SwapInstrument],
    discount_curve: &Curve,  // OIS curve
) -> Curve {
    // Use OIS curve for discounting
    // Solve for projection curve nodes
}
```

See [references/multi-curve.md](references/multi-curve.md) for tenor basis, convexity adjustments, and dual-curve stripping.

### Step 3: Cross-Currency Curve Bootstrap

```rust
fn bootstrap_xccy(
    fx_forwards: &[FXForward],      // Short end
    xccy_swaps: &[XCCYBasisSwap],   // Long end  
    domestic_ois: &Curve,
    spot_fx: f64,
) -> Curve {
    // Phase 1: FX forward implied (< 1Y)
    // Phase 2: XCCY basis swap bootstrap (> 1Y)
}
```

See [references/cross-currency.md](references/cross-currency.md) for basis swap mechanics, collateral switching, and simultaneous calibration.

## Interpolation Selection

| Curve Type | Recommended | Rationale |
|------------|-------------|-----------|
| OIS discount | Log-linear on DF | Positive forwards, stable, local |
| Projection | Monotone convex | Continuous forwards needed |
| XCCY basis | Log-linear on DF | Match OIS convention |

See [references/interpolation.md](references/interpolation.md) for algorithms and positivity constraints.

## Sensitivity Calculation

Bucketed PV01 requires the calibration Jacobian:

```rust
fn compute_key_rate_durations(
    curve: &Curve,
    portfolio: &Portfolio,
) -> Vec<f64> {
    // Use algorithmic differentiation (5x faster than bump)
    // Or finite difference with bump = 1e-4 to 1e-6
}
```

See [references/numerical.md](references/numerical.md) for AD implementation, solver selection, and performance optimization.

## Collateral Currency Rules

| CSA Terms | Discount Curve |
|-----------|---------------|
| USD cash | USD OIS (SOFR) |
| EUR cash | EUR OIS (€STR) |
| USD for EUR trade | EUR-implied from XCCY |
| Multi-currency | Cheapest-to-deliver |

## Performance Targets

| Operation | Target | Notes |
|-----------|--------|-------|
| Single curve bootstrap | < 1ms | Sequential, 30 nodes |
| Full multi-currency | < 20ms | 3 currencies with AD |
| Jacobian (AD) | < 5ms | 30 nodes × 30 sensitivities |

## Common Pitfalls

### Curve Construction
1. **Wrong build order** - Always OIS first, then projection, then XCCY
2. **Missing convexity adjustment** - Futures ≠ forwards (adjust 1-2bp short end)
3. **Ignoring turn-of-year** - Can be 5-500bp jump at year-end
4. **Single-curve pricing** - Post-2008 spreads can reach 200bp
5. **Wrong collateral curve** - 10%+ error on long-dated swaps possible

### Bond Spread Calculations
6. **Z-spread compounding mismatch** - Use semi-annual for USD, match convention
7. **Dirty vs clean price confusion** - Always use dirty for PV calculations
8. **Benchmark maturity mismatch** - G-spread uses wrong point; prefer Z-spread
9. **Ignoring accrued interest** - Critical for short-dated bonds

### Inflation-Linked Bonds
10. **Missing indexation lag** - TIPS use 3-month CPI lag
11. **Ignoring seasonality** - CPI seasonal patterns affect short-dated breakevens
12. **Forgetting deflation floor** - TIPS principal cannot go below par at maturity

### Credit/CDS
13. **Recovery rate assumption** - Typically 40% for senior unsecured, but varies
14. **Ignoring accrued premium** - CDS premium accrues to default date
15. **Hazard rate vs spread confusion** - λ ≈ spread/(1-R), not equal

### Repo/Financing
16. **Using wrong repo rate** - GC vs special can differ 50bp+
17. **Day count convention** - Repo uses ACT/360, bonds vary by market
18. **Ignoring haircuts** - Affects actual funding amount
