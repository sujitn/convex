---
name: bond-risk-metrics
description: "Fixed income risk analytics library for Bloomberg-parity bond calculations. Use when implementing: (1) DV01/PV01/duration calculations, (2) Convexity including negative convexity for callables, (3) Key rate duration and partial DV01s, (4) Spread metrics (OAS, Z-spread, ASW, CS01), (5) Effective duration/convexity for bonds with embedded options, (6) FRN discount margin and spread duration, (7) Inflation-linked bond real duration and BEI01, (8) Portfolio-level risk aggregation. Targets sub-microsecond Rust implementation with Bloomberg YAS function parity."
---

# Bond Risk Metrics Implementation

High-performance fixed income risk analytics targeting Bloomberg YAS parity. All calculations use **dirty price** basis and **1bp symmetric shifts** unless otherwise noted.

## Core Calculation Workflow

1. **Determine bond type** → Select appropriate duration method
2. **Build/obtain yield curve** → Required for spread and KRD calculations  
3. **Calculate base metrics** → Duration, DV01, convexity
4. **Calculate spread metrics** → If credit bond: OAS, Z-spread, CS01
5. **Calculate KRD** → If curve risk decomposition needed
6. **Validate** → Sum of KRDs ≈ Total duration (within convexity tolerance)

## Duration Metrics

### DV01 and Modified Duration

Use **central finite difference** with **1bp shift**:

```
DV01 = (P₋ - P₊) / 2
Modified_Duration = DV01 × 10000 / P₀
```

Where P₋ = price at yield - 1bp, P₊ = price at yield + 1bp.

**Macaulay Duration** (for immunization):
```
Mac_D = Σ(t × PV_t) / Σ(PV_t)
Modified_D = Mac_D / (1 + y/n)  // n = compounding frequency
```

### Effective Duration (Bonds with Embedded Options)

**Critical:** Hold OAS constant, shift benchmark curve:

```
Effective_D = (P₋ - P₊) / (2 × P₀ × Δy)
```

Use **25-50bp shifts** for optioned bonds. Recalculate interest rate tree at each shifted curve.

**Bloomberg field:** `OAD`

## Convexity

### Standard Convexity

```
Convexity = (P₊ + P₋ - 2×P₀) / (P₀ × Δy²)
```

**Bloomberg scaling note:** Verify if Bloomberg reports convexity/100. Adjust for Taylor expansion:
```
ΔP/P ≈ -Duration × Δy + 0.5 × Convexity × Δy²
```

### Negative Convexity (Callables)

Occurs when rates fall below coupon. Price ceiling at call price causes concave price-yield curve.

**Detection:** Compare up/down price sensitivities. If |ΔP_down| < |ΔP_up|, bond exhibits negative convexity.

## Key Rate Duration (KRD)

### Standard Tenors
`[3M, 6M, 1Y, 2Y, 3Y, 5Y, 7Y, 10Y, 15Y, 20Y, 25Y, 30Y]`

For portfolio reporting: `[2Y, 5Y, 10Y, 30Y]`

### Triangular (Tent) Bump Method

```
Δr(t) = h × max(0, 1 - |t - T_key| / span)
```

Where span = distance to adjacent key rate tenor.

**Critical relationship:** `Σ KRD_i ≈ Effective_Duration` (verify as sanity check)

### Curve Interpolation Impact

| Method | Locality | Use Case |
|--------|----------|----------|
| Piecewise constant forwards | Perfect | Production KRD |
| Linear zeros | Perfect | Simple implementations |
| Cubic spline | Poor | Avoid for KRD (risk leakage) |
| Monotone convex | Good | Treasury curves |

Prefer **piecewise constant forwards** for clean bucket attribution.

## Spread Metrics

### Spread Hierarchy (Most to Least Theoretically Precise)

| Spread | Benchmark | Formula/Method |
|--------|-----------|----------------|
| OAS | Zero + option model | Iterative solve holding option model |
| Z-spread | Treasury zeros | Solve: P = Σ[CF_i / (1 + s_i + Z)^t_i] |
| ASW | Swap curve | (P_swap - Dirty_Price) / DV01_swap |
| I-spread | Interpolated swap | YTM - Swap_rate(maturity) |
| G-spread | Interpolated govt | YTM - Govt_yield(maturity) |

### CS01 (Credit Spread 01)

```
CS01 = Spread_Duration × Price × 0.0001
```

**Key distinction:**
- Fixed-rate corporates: CS01 ≈ DV01
- Floating-rate notes: CS01 >> DV01 (rate duration ≈ 0)

### OAS to Z-spread Relationship

```
Z-spread = OAS + Option_Cost
```

- Option-free: Z = OAS
- Callable: Z > OAS
- Putable: Z < OAS

## Bond-Type Specific Calculations

See `references/bond-types.md` for detailed formulas.

### Quick Reference

| Bond Type | Duration Measure | Key Metrics |
|-----------|------------------|-------------|
| Fixed vanilla | Modified Duration | DV01, Convexity |
| Callable | Effective Duration (OAS) | OAD, OAC, Vega |
| Putable | Effective Duration (OAS) | OAD, OAC, Vega |
| FRN | Spread Duration | DM01, Reset Risk |
| TIPS | Real Duration | BEI01, Inflation DV01 |
| Sinking Fund | WAL-adjusted Duration | Average Life |

## Bloomberg Field Mapping

See `references/bloomberg-fields.md` for complete mapping.

### Essential Fields

| Metric | Bloomberg Field | Notes |
|--------|-----------------|-------|
| Modified Duration | `MOD_DUR` | % price change per 100bp |
| Macaulay Duration | `MACD` | Years |
| DV01 | `RISK` | Bloomberg's DV01 |
| OAS Duration | `OAD` | For optioned bonds |
| Convexity | `CONVEXITY` | Check scaling |

## Day Count Conventions

| Convention | Markets | Code |
|------------|---------|------|
| 30/360 US | US corporates, agencies | `DC_30_360_US` |
| ACT/ACT ICMA | Treasuries, Gilts | `DC_ACT_ACT_ICMA` |
| ACT/360 | Money markets, FRNs | `DC_ACT_360` |
| ACT/365 Fixed | Sterling bonds | `DC_ACT_365F` |

## Rust Implementation Patterns

### Core Traits

```rust
pub trait RiskMetrics {
    fn dv01(&self, curve: &YieldCurve) -> f64;
    fn modified_duration(&self, curve: &YieldCurve) -> f64;
    fn convexity(&self, curve: &YieldCurve) -> f64;
}

pub trait SpreadRisk {
    fn z_spread(&self, curve: &YieldCurve, price: f64) -> f64;
    fn cs01(&self, curve: &YieldCurve) -> f64;
}

pub trait KeyRateRisk {
    fn key_rate_durations(&self, curve: &YieldCurve, tenors: &[f64]) -> Vec<f64>;
}
```

### Performance Optimizations

1. **Parallel KRD:** Use `rayon` for independent tenor calculations
2. **Cache discount factors:** Reuse across bump scenarios
3. **SIMD:** Vectorize discount factor array operations
4. **Jacobian reuse:** Transform between rate types without full reprice

### Finite Difference Defaults

```rust
const DV01_SHIFT_BP: f64 = 1.0;
const EFFECTIVE_DUR_SHIFT_BP: f64 = 25.0;  // For optioned bonds
const KRD_SHIFT_BP: f64 = 1.0;
```

## Validation Checklist

- [ ] DV01 × 10000 / Price ≈ Modified Duration
- [ ] Σ KRD ≈ Effective Duration (within 0.1%)
- [ ] OAS ≤ Z-spread for callable bonds
- [ ] Convexity positive for vanilla bonds
- [ ] FRN interest rate duration ≈ time to next reset

## Regulatory Context (2025)

### ISDA SIMM v2.8
- Delta tenors: 2w, 1m, 3m, 6m, 1y, 2y, 3y, 5y, 10y, 15y, 20y, 30y
- Sub-curves: OIS, SOFR, inflation, cross-currency basis
- Semiannual calibration cycle

### FRTB SA
- Sensitivities-Based Method: Delta + Vega + Curvature
- Three correlation scenarios (high/medium/low)

## Reference Files

- `references/formulas.md` - Complete mathematical formulas with derivations
- `references/bloomberg-fields.md` - Full Bloomberg field mapping and conventions
- `references/bond-types.md` - Type-specific calculation details (FRN, TIPS, callables)
