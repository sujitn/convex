# Spread Curves for Bond Pricing

## Table of Contents
- [Spread Measures Overview](#spread-measures-overview)
- [Z-Spread Calculation](#z-spread-calculation)
- [I-Spread and G-Spread](#i-spread-and-g-spread)
- [Option-Adjusted Spread (OAS)](#option-adjusted-spread-oas)
- [Credit Spread Curves](#credit-spread-curves)
- [CDS-Implied Hazard Rate Curves](#cds-implied-hazard-rate-curves)

## Spread Measures Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    SPREAD HIERARCHY                          │
├─────────────────────────────────────────────────────────────┤
│  G-SPREAD (Gross Spread)                                    │
│  └── YTM - Benchmark Treasury YTM                           │
│  └── Simplest, but maturity mismatch                        │
├─────────────────────────────────────────────────────────────┤
│  I-SPREAD (Interpolated Spread)                             │
│  └── YTM - Interpolated swap rate                           │
│  └── Maturity matched, single point                         │
├─────────────────────────────────────────────────────────────┤
│  Z-SPREAD (Zero-Volatility Spread)                          │
│  └── Constant spread over entire zero curve                 │
│  └── Full term structure, no optionality                    │
├─────────────────────────────────────────────────────────────┤
│  OAS (Option-Adjusted Spread)                               │
│  └── Z-spread minus option value                            │
│  └── Accounts for embedded options (calls, puts)            │
└─────────────────────────────────────────────────────────────┘
```

| Spread | Formula | Best For |
|--------|---------|----------|
| G-spread | YTM_bond - YTM_treasury | Quick relative value |
| I-spread | YTM_bond - Swap_rate(maturity) | Floating rate comparison |
| Z-spread | Solve: P = Σ CF/(1+r(t)+z)^t | Credit analysis |
| OAS | Z-spread - option_value | Callable/putable bonds |
| ASW | See cross-currency.md | Funding comparison |

## Z-Spread Calculation

The Z-spread is the constant spread added to every point on the benchmark zero curve that equates discounted cash flows to market price:

```
P = Σᵢ CFᵢ × exp(-(r(tᵢ) + z) × tᵢ)
```

Or in discrete form:
```
P = Σᵢ CFᵢ / (1 + r(tᵢ) + z)^tᵢ
```

### Algorithm

```rust
fn calculate_z_spread(
    bond: &Bond,
    market_price: f64,  // Dirty price
    zero_curve: &Curve,
    tolerance: f64,
) -> f64 {
    let objective = |z: f64| {
        let model_price: f64 = bond.cash_flows()
            .iter()
            .map(|(t, cf)| {
                let r = zero_curve.rate(*t);
                cf * (-(r + z) * t).exp()
            })
            .sum();
        model_price - market_price
    };
    
    // Brent solver with reasonable bounds
    brent_solver(objective, -0.10, 0.50, tolerance, 100)
}
```

### Compounding Convention

Z-spread can be computed with different compounding:

```rust
enum ZSpreadCompounding {
    Continuous,       // exp(-(r+z)*t)
    SemiAnnual,       // 1/(1+(r+z)/2)^(2t)
    Annual,           // 1/(1+r+z)^t
}

fn discount_with_spread(
    t: f64,
    r: f64,
    z: f64,
    compounding: ZSpreadCompounding,
) -> f64 {
    match compounding {
        ZSpreadCompounding::Continuous => (-(r + z) * t).exp(),
        ZSpreadCompounding::SemiAnnual => {
            (1.0 + (r + z) / 2.0).powf(-2.0 * t)
        }
        ZSpreadCompounding::Annual => {
            (1.0 + r + z).powf(-t)
        }
    }
}
```

### Bloomberg Z-Spread Convention

Bloomberg uses semi-annual compounding for USD bonds:

```rust
fn bloomberg_z_spread(
    bond: &Bond,
    dirty_price: f64,
    treasury_curve: &Curve,  // Treasury zero curve
) -> f64 {
    let objective = |z: f64| {
        let pv: f64 = bond.cash_flows()
            .iter()
            .map(|(t, cf)| {
                let r = treasury_curve.rate(*t);
                let df = (1.0 + (r + z) / 2.0).powf(-2.0 * t);
                cf * df
            })
            .sum();
        pv - dirty_price
    };
    
    brent_solver(objective, -0.05, 0.30, 1e-8, 100)
}
```

## I-Spread and G-Spread

### G-Spread (Gross Spread)

```
G-spread = YTM_bond - YTM_benchmark_treasury
```

Simple but flawed: benchmark may not match maturity exactly.

```rust
fn g_spread(bond_ytm: f64, benchmark_ytm: f64) -> f64 {
    bond_ytm - benchmark_ytm
}

fn select_benchmark(maturity_years: f64) -> &'static str {
    match maturity_years {
        t if t <= 1.0 => "1Y Treasury",
        t if t <= 2.5 => "2Y Treasury",
        t if t <= 4.0 => "3Y Treasury",
        t if t <= 6.0 => "5Y Treasury",
        t if t <= 8.5 => "7Y Treasury",
        t if t <= 15.0 => "10Y Treasury",
        t if t <= 25.0 => "20Y Treasury",
        _ => "30Y Treasury",
    }
}
```

### I-Spread (Interpolated Spread)

```
I-spread = YTM_bond - Swap_rate(maturity)
```

Interpolated swap rate matches exact maturity:

```rust
fn i_spread(
    bond_ytm: f64,
    bond_maturity: f64,
    swap_curve: &Curve,
) -> f64 {
    let swap_rate = swap_curve.par_rate(bond_maturity);
    bond_ytm - swap_rate
}
```

## Option-Adjusted Spread (OAS)

For bonds with embedded options:

```
OAS = Z-spread - Option_Value_in_Spread_Terms
```

### Binomial Tree Approach

```rust
fn calculate_oas(
    bond: &CallableBond,
    market_price: f64,
    zero_curve: &Curve,
    volatility: f64,  // Interest rate vol
    time_steps: usize,
) -> f64 {
    let objective = |oas: f64| {
        let model_price = price_callable_bond_tree(
            bond,
            zero_curve,
            oas,
            volatility,
            time_steps,
        );
        model_price - market_price
    };
    
    brent_solver(objective, -0.05, 0.30, 1e-8, 100)
}

fn price_callable_bond_tree(
    bond: &CallableBond,
    zero_curve: &Curve,
    oas: f64,
    vol: f64,
    steps: usize,
) -> f64 {
    let dt = bond.maturity / steps as f64;
    
    // Build interest rate tree
    let rate_tree = build_bdt_tree(zero_curve, vol, steps);
    
    // Backward induction with OAS
    let mut values = vec![vec![0.0; steps + 1]; steps + 1];
    
    // Terminal values (par + final coupon)
    for j in 0..=steps {
        values[steps][j] = bond.face_value + bond.coupon / bond.frequency as f64;
    }
    
    // Backward induction
    for i in (0..steps).rev() {
        for j in 0..=i {
            let r = rate_tree[i][j] + oas;
            let df = (-r * dt).exp();
            
            // Expected value
            let continuation = 0.5 * (values[i+1][j] + values[i+1][j+1]) * df;
            
            // Add coupon if payment date
            let value = continuation + coupon_at_step(bond, i, steps);
            
            // Check call constraint
            if bond.is_callable_at_step(i, steps) {
                values[i][j] = value.min(bond.call_price);
            } else {
                values[i][j] = value;
            }
        }
    }
    
    values[0][0]
}
```

### OAS vs Z-Spread Relationship

| Bond Type | Z-spread | OAS | Relationship |
|-----------|----------|-----|--------------|
| Bullet (no options) | X | X | Equal |
| Callable | X | Y | Z > OAS (call hurts holder) |
| Putable | X | Y | Z < OAS (put helps holder) |

Option value in spread terms:
```
Option_Cost = Z-spread - OAS
```

## Credit Spread Curves

### Issuer-Specific Spread Curve

Build spread curve for a single issuer from multiple bonds:

```rust
fn build_issuer_spread_curve(
    issuer_bonds: &[Bond],
    benchmark_curve: &Curve,
) -> SpreadCurve {
    let mut spread_points: Vec<(f64, f64)> = issuer_bonds
        .iter()
        .map(|bond| {
            let z = calculate_z_spread(bond, bond.dirty_price, benchmark_curve, 1e-8);
            (bond.maturity_years(), z)
        })
        .collect();
    
    spread_points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    
    SpreadCurve::new(spread_points, Interpolation::Linear)
}
```

### Sector Spread Curve

Aggregate spreads by rating and sector:

```rust
fn build_sector_spread_curve(
    bonds: &[Bond],
    rating: Rating,
    sector: Sector,
    benchmark_curve: &Curve,
) -> SpreadCurve {
    let filtered: Vec<_> = bonds.iter()
        .filter(|b| b.rating == rating && b.sector == sector)
        .collect();
    
    // Bucket by maturity
    let mut buckets: HashMap<u32, Vec<f64>> = HashMap::new();
    
    for bond in filtered {
        let z = calculate_z_spread(bond, bond.dirty_price, benchmark_curve, 1e-8);
        let bucket = (bond.maturity_years() as u32).max(1).min(30);
        buckets.entry(bucket).or_default().push(z);
    }
    
    // Median spread per bucket
    let spread_points: Vec<(f64, f64)> = buckets.iter()
        .map(|(mat, spreads)| {
            let median = median(spreads);
            (*mat as f64, median)
        })
        .collect();
    
    SpreadCurve::new(spread_points, Interpolation::Linear)
}
```

## CDS-Implied Hazard Rate Curves

### Survival Probability Model

Default modeled as first jump of Poisson process with hazard rate λ(t):

```
Q(t) = exp(-∫₀ᵗ λ(s) ds)
```

With piecewise constant hazard rates:
```
Q(T) = exp(-Σᵢ λᵢ × Δtᵢ)
```

### CDS Valuation

**Premium leg** (protection buyer pays):
```
PL = s × Σₙ Δtₙ × D(tₙ) × [Q(tₙ) + 0.5×(Q(tₙ₋₁) - Q(tₙ))]
```

**Protection leg** (protection buyer receives on default):
```
DL = (1-R) × Σₙ D(tₙ) × [Q(tₙ₋₁) - Q(tₙ)]
```

At fair value: PL = DL

### Bootstrapping Hazard Rates

```rust
fn bootstrap_cds_curve(
    cds_spreads: &[(f64, f64)],  // (maturity, spread)
    zero_curve: &Curve,
    recovery_rate: f64,
) -> HazardRateCurve {
    let mut hazard_rates: Vec<(f64, f64)> = vec![];
    let mut survival_probs: Vec<(f64, f64)> = vec![(0.0, 1.0)];
    
    for (maturity, spread) in cds_spreads.iter().sorted_by_key(|x| x.0 as i64) {
        let lambda = solve_hazard_rate(
            *maturity,
            *spread,
            &survival_probs,
            zero_curve,
            recovery_rate,
        );
        
        hazard_rates.push((*maturity, lambda));
        
        // Update survival probability
        let prev_q = survival_probs.last().unwrap().1;
        let prev_t = survival_probs.last().unwrap().0;
        let q = prev_q * (-lambda * (maturity - prev_t)).exp();
        survival_probs.push((*maturity, q));
    }
    
    HazardRateCurve { hazard_rates, survival_probs }
}

fn solve_hazard_rate(
    maturity: f64,
    spread: f64,
    prior_survival: &[(f64, f64)],
    zero_curve: &Curve,
    recovery: f64,
) -> f64 {
    let objective = |lambda: f64| {
        // Build survival curve with this lambda
        let survival = extend_survival(prior_survival, maturity, lambda);
        
        // Calculate premium and protection leg PVs
        let premium_pv = premium_leg_pv(spread, maturity, &survival, zero_curve);
        let protection_pv = protection_leg_pv(maturity, &survival, zero_curve, recovery);
        
        premium_pv - protection_pv
    };
    
    brent_solver(objective, 0.0001, 0.50, 1e-10, 100)
}

fn premium_leg_pv(
    spread: f64,
    maturity: f64,
    survival: &[(f64, f64)],
    zero_curve: &Curve,
) -> f64 {
    let freq = 4.0;  // Quarterly payments
    let dt = 1.0 / freq;
    let n_periods = (maturity * freq) as usize;
    
    let mut pv = 0.0;
    for i in 1..=n_periods {
        let t = i as f64 * dt;
        let df = zero_curve.df(t);
        let q_curr = interpolate_survival(survival, t);
        let q_prev = interpolate_survival(survival, t - dt);
        
        // Regular premium
        pv += spread * dt * df * q_curr;
        
        // Accrued premium on default
        pv += spread * dt / 2.0 * df * (q_prev - q_curr);
    }
    
    pv
}

fn protection_leg_pv(
    maturity: f64,
    survival: &[(f64, f64)],
    zero_curve: &Curve,
    recovery: f64,
) -> f64 {
    let lgd = 1.0 - recovery;
    let dt = 1.0 / 12.0;  // Monthly default intervals
    let n_periods = (maturity * 12.0) as usize;
    
    let mut pv = 0.0;
    for i in 1..=n_periods {
        let t = i as f64 * dt;
        let df = zero_curve.df(t);
        let q_curr = interpolate_survival(survival, t);
        let q_prev = interpolate_survival(survival, t - dt);
        
        pv += lgd * df * (q_prev - q_curr);
    }
    
    pv
}
```

### Quick Hazard Rate Approximation

For rough estimates (ignores accrued premium and discounting):

```
λ ≈ spread / (1 - R)
```

```rust
fn approximate_hazard_rate(spread: f64, recovery: f64) -> f64 {
    spread / (1.0 - recovery)
}

// Example: 100bp spread, 40% recovery
// λ ≈ 0.01 / 0.6 = 1.67% annual hazard rate
```

### Using Hazard Rates for Bond Pricing

Price risky bond using survival probabilities:

```rust
fn price_risky_bond(
    bond: &Bond,
    zero_curve: &Curve,
    hazard_curve: &HazardRateCurve,
    recovery: f64,
) -> f64 {
    let mut pv = 0.0;
    
    for (t, cf) in bond.cash_flows() {
        let df = zero_curve.df(t);
        let q = hazard_curve.survival_probability(t);
        
        // Survival-contingent payment
        pv += cf * df * q;
    }
    
    // Expected recovery on default
    let face = bond.face_value;
    for i in 1..=((bond.maturity * 12.0) as usize) {
        let t = i as f64 / 12.0;
        let df = zero_curve.df(t);
        let q_prev = hazard_curve.survival_probability(t - 1.0/12.0);
        let q_curr = hazard_curve.survival_probability(t);
        let default_prob = q_prev - q_curr;
        
        pv += recovery * face * df * default_prob;
    }
    
    pv
}
```

### CDS-Bond Basis

Theoretical arbitrage relationship:
```
CDS_Spread ≈ Z-spread (for same reference entity)
```

**Basis = CDS - Z-spread**

| Basis | Interpretation |
|-------|---------------|
| Positive | Bond cheap relative to CDS |
| Negative | CDS cheap relative to bond |
| Large positive | Funding advantage for bond holders |
| Large negative | Default concerns, liquidity issues |

```rust
fn cds_bond_basis(
    bond_z_spread: f64,
    cds_spread: f64,
) -> f64 {
    cds_spread - bond_z_spread
}
```
