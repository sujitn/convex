# Spread Calculation Reference

Detailed methodologies for each spread type with Bloomberg YAS parity.

## Z-Spread (Zero-Volatility Spread)

### Definition

Constant spread added to each spot rate on the benchmark zero curve that equates PV of cash flows to market dirty price. Captures full term structure shape.

### Mathematical Formulation

**Semi-annual compounding (USD/corporate convention)**:
```
P_dirty = Σ(i=1 to n) [CF_i / (1 + (s_i + Z)/2)^(2×t_i)]
```

**Continuous compounding (academic)**:
```
P_dirty = Σ(i=1 to n) [CF_i × exp(-(s_i + Z) × t_i)]
```

Where:
- `P_dirty` = Clean price + accrued interest
- `CF_i` = Cash flow at time `t_i`
- `s_i` = Benchmark spot rate at maturity `t_i`
- `Z` = Z-spread (unknown, solve iteratively)

### Solver Implementation

**Brent's Method (Recommended)**:
```rust
fn solve_z_spread(
    dirty_price: f64,
    cash_flows: &[(f64, f64)],  // (time, amount)
    curve: &impl YieldCurve,
    tolerance: f64,
) -> Result<f64, SolverError> {
    let f = |z: f64| -> f64 {
        let pv: f64 = cash_flows.iter()
            .map(|(t, cf)| {
                let r = curve.zero_rate(*t) + z;
                cf / (1.0 + r / 2.0).powf(2.0 * t)
            })
            .sum();
        pv - dirty_price
    };
    
    brent_solver(f, -0.10, 1.00, tolerance, 100)
}
```

**Newton-Raphson (Faster but less robust)**:
```rust
fn newton_z_spread(/* same params */) -> Result<f64, SolverError> {
    let mut z = initial_guess;  // ytm - benchmark_ytm
    
    for _ in 0..MAX_ITER {
        let (pv, duration) = pv_and_duration(z, cash_flows, curve);
        let error = pv - dirty_price;
        
        if error.abs() < tolerance { return Ok(z); }
        
        z += error / duration;  // duration is -dPV/dz
    }
    Err(SolverError::NoConvergence)
}
```

**Parameters**:
- Tolerance: `1e-8` (≈0.001 bp)
- Initial guess: `bond_ytm - swap_ytm` or historical spread
- Bounds: `[-0.10, 1.00]` for Brent (supports negative spreads)
- Max iterations: 100

### Edge Cases

1. **Near-zero coupons**: Floor denominators in duration calc
2. **30+ year bonds**: Use log-space for discount factors
3. **Negative spreads**: Common post-2015; remove hardcoded positivity
4. **Solver failure**: Retry with bisection on wider bounds `[-0.5, 2.0]`

---

## I-Spread (Interpolated Swap Spread)

### Definition

Yield difference between bond and interpolated swap rate at same maturity.

### Formula

```
I-Spread = Bond_YTM - R_swap(T_maturity)
```

### ISDA 2021 Interpolation

Linear interpolation using **calendar days** (not business days):

```rust
fn interpolate_swap_rate(
    maturity_date: Date,
    swap_curve: &[(Date, f64)],  // Sorted by date
) -> f64 {
    let (t1, r1, t2, r2) = find_bracketing_points(maturity_date, swap_curve);
    
    let days_1 = (maturity_date - t1).days() as f64;
    let days_total = (t2 - t1).days() as f64;
    
    r1 + (r2 - r1) * days_1 / days_total
}
```

### Standard Swap Tenors

USD/EUR/GBP: 1M, 3M, 6M, 1Y, 2Y, 3Y, 4Y, 5Y, 6Y, 7Y, 8Y, 9Y, 10Y, 12Y, 15Y, 20Y, 25Y, 30Y

---

## G-Spread (Government Spread)

### Definition

Yield difference between bond and interpolated government bond yield.

### Formula

```
G-Spread = Bond_YTM - R_govt(T_maturity)
```

### Benchmark Securities

| Currency | Benchmarks | Source |
|----------|-----------|--------|
| USD | 2Y, 3Y, 5Y, 7Y, 10Y, 20Y, 30Y on-the-run Treasuries | Treasury Direct |
| EUR | German Bunds at standard tenors | Bundesbank |
| GBP | UK Gilts at standard tenors | DMO |

### Bloomberg Convention

Bloomberg interpolates based on **modified duration** rather than simple maturity for more accurate hedging.

---

## ASW (Asset Swap Spread)

### Par-Par ASW (Bloomberg Default)

Investor pays par regardless of market price. Most common convention.

**Formula**:
```
ASW = (PV_swap - P_dirty) / PV01_annuity

where:
PV_swap = Σ[coupon × Z(t_i)]           // Bond coupons discounted at swap zeros
PV01   = Σ[dcf_i × Z(t_i)]             // Annuity factor
Z(t)   = discount factor at time t
```

**Implementation**:
```rust
fn par_par_asw(
    dirty_price: f64,
    coupons: &[(f64, f64)],      // (time, coupon_amount)
    curve: &impl YieldCurve,
) -> f64 {
    let pv_coupons: f64 = coupons.iter()
        .map(|(t, c)| c * curve.discount_factor(*t))
        .sum();
    
    // Add principal at maturity
    let maturity = coupons.last().unwrap().0;
    let pv_principal = 100.0 * curve.discount_factor(maturity);
    let pv_swap = pv_coupons + pv_principal;
    
    // PV01 using annual day count fractions
    let pv01: f64 = coupons.iter()
        .map(|(t, _)| day_count_fraction(*t) * curve.discount_factor(*t))
        .sum();
    
    (pv_swap - dirty_price) / pv01
}
```

### Market (Proceeds) ASW

Floating notional equals price with principal exchange at maturity.

**Solve for spread M**:
```
Σ[coupon × Z(t_i)] - Σ[(forward_rate + M) × dcf_i × (P/100) × Z(t_i)] 
+ (P - 100) × Z(T) = 0
```

Use Brent's method to solve for M.

### Key Differences

| Aspect | Par-Par | Market |
|--------|---------|--------|
| Notional | Par (100) | Market price |
| Principal exchange | None | At maturity |
| Counterparty risk | Higher if off-par | Lower |
| Bloomberg default | ✓ | |

---

## OAS (Option-Adjusted Spread)

### Definition

Spread over benchmark curve after removing embedded option value. Required for callable/puttable bonds and MBS.

### Hull-White One-Factor Model (Bloomberg Default)

```
dr(t) = [θ(t) - a × r(t)]dt + σ × dW(t)
```

**Parameters**:
- `a` = mean reversion (0.03 - 0.10, calibrated to swaptions)
- `σ` = volatility (from cap/floor or swaption market)
- `θ(t)` = drift calibrated to match initial term structure

### Trinomial Tree (Callable Bonds)

```rust
fn calculate_oas_callable(
    dirty_price: f64,
    bond: &CallableBond,
    curve: &impl YieldCurve,
    model: &HullWhiteModel,
) -> f64 {
    // Binary search for OAS
    brent_solver(|oas| {
        let tree = build_hull_white_tree(curve, model, bond.maturity);
        let model_price = price_callable_backward(tree, bond, oas);
        model_price - dirty_price
    }, -0.05, 0.50, 1e-6, 100)
}

fn price_callable_backward(
    tree: &TrinomialTree,
    bond: &CallableBond,
    oas: f64,
) -> f64 {
    // Start at maturity with redemption value
    let mut values = vec![bond.redemption; tree.width];
    
    // Backward induction
    for step in (0..tree.steps).rev() {
        for node in 0..tree.width_at(step) {
            let continuation = expected_value(tree, step, node, &values);
            let r = tree.short_rate(step, node) + oas;
            let df = (-r * tree.dt).exp();
            
            let hold_value = (continuation + bond.coupon_at(step)) * df;
            
            // Apply call constraint
            values[node] = if bond.is_callable_at(step) {
                hold_value.min(bond.call_price_at(step))
            } else {
                hold_value
            };
        }
    }
    values[tree.root_node]
}
```

### Monte Carlo (MBS/Path-Dependent)

For prepayment-sensitive securities:

```rust
fn calculate_oas_mbs(
    dirty_price: f64,
    mbs: &MBS,
    curve: &impl YieldCurve,
    model: &HullWhiteModel,
    n_paths: usize,  // 2000-10000
) -> f64 {
    brent_solver(|oas| {
        let paths = generate_rate_paths(curve, model, mbs.maturity, n_paths);
        let prices: Vec<f64> = paths.iter()
            .map(|path| price_mbs_path(path, mbs, oas))
            .collect();
        
        prices.iter().sum::<f64>() / n_paths as f64 - dirty_price
    }, -0.05, 0.50, 1e-5, 100)
}
```

**PSA Prepayment Model**:
```rust
fn cpr(loan_age_months: u32, psa_speed: f64) -> f64 {
    let base_cpr = 0.06 * (loan_age_months as f64 / 30.0).min(1.0);
    base_cpr * psa_speed / 100.0
}

fn smm(cpr: f64) -> f64 {
    1.0 - (1.0 - cpr).powf(1.0 / 12.0)
}
```

### Volatility Surface Requirements

| Expiry | Tenors Required |
|--------|-----------------|
| 1M, 3M, 6M, 1Y | 1Y, 2Y, 5Y, 10Y, 30Y |
| 2Y, 3Y, 5Y | 1Y, 2Y, 5Y, 10Y, 30Y |
| 7Y, 10Y | 1Y, 2Y, 5Y, 10Y, 30Y |

Use ATM swaption implied vols. Smile modelling adds complexity without proportional accuracy gains.

---

## Numerical Stability

### Precision

Always use **64-bit double precision**. Single precision accumulates errors producing multi-bp spread errors on 30-year bonds.

### Kahan Summation

For long cash flow schedules:

```rust
fn kahan_sum(values: &[f64]) -> f64 {
    let mut sum = 0.0;
    let mut c = 0.0;
    for &v in values {
        let y = v - c;
        let t = sum + y;
        c = (t - sum) - y;
        sum = t;
    }
    sum
}
```

### Date Arithmetic

Use integer day counts internally. Apply day count fractions only at final calculation.
