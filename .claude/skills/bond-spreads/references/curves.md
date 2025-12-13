# Curve Construction Reference

Post-LIBOR curve building with bootstrapping and interpolation for Bloomberg YAS parity.

## Dual-Curve Framework (Post-LIBOR)

Modern curve construction requires separate curves for:
1. **Discounting**: OIS curve (SOFR/SONIA/€STR)
2. **Projection**: Term rate curve (for legacy IBOR instruments)

For spread calculations, the **discount curve** is primary.

## Bootstrapping Algorithm

Process instruments in priority order:

### 1. Deposits (O/N to 3M)

Direct zero rate extraction:

```rust
fn bootstrap_deposit(rate: f64, dcf: f64) -> f64 {
    // Returns discount factor
    1.0 / (1.0 + rate * dcf)
}

fn zero_from_deposit(rate: f64, dcf: f64, t: f64) -> f64 {
    // Returns continuously compounded zero rate
    -(1.0 / (1.0 + rate * dcf)).ln() / t
}
```

### 2. Futures (3M - 2Y)

Apply **convexity adjustment** before bootstrapping:

```rust
fn convexity_adjustment(vol: f64, t1: f64, t2: f64) -> f64 {
    // Hull approximation
    0.5 * vol.powi(2) * t1 * (t2 - t1)
}

fn forward_from_future(future_price: f64, vol: f64, t1: f64, t2: f64) -> f64 {
    let implied_rate = (100.0 - future_price) / 100.0;
    let ca = convexity_adjustment(vol, t1, t2);
    implied_rate - ca
}

fn bootstrap_future(
    forward_rate: f64,
    dcf: f64,
    z_t1: f64,  // Known discount factor at t1
) -> f64 {
    // Returns discount factor at t2
    z_t1 / (1.0 + forward_rate * dcf)
}
```

### 3. Swaps (2Y+)

Iterative bootstrapping:

```rust
fn bootstrap_swap(
    par_rate: f64,
    payment_dates: &[f64],
    dcfs: &[f64],
    known_dfs: &[f64],  // Previously bootstrapped
) -> f64 {
    // Solve for final discount factor
    let n = payment_dates.len();
    let sum_known: f64 = (0..n-1)
        .map(|i| dcfs[i] * known_dfs[i])
        .sum();
    
    // Z(T_n) = (1 - R × Σ dcf_i × Z(T_i)) / (1 + R × dcf_n)
    (1.0 - par_rate * sum_known) / (1.0 + par_rate * dcfs[n-1])
}
```

**Full bootstrap loop**:
```rust
fn bootstrap_curve(instruments: &[Instrument]) -> Vec<(f64, f64)> {
    let mut nodes: Vec<(f64, f64)> = vec![];  // (time, discount_factor)
    
    // Sort by maturity
    let sorted = instruments.sorted_by_maturity();
    
    for inst in sorted {
        let df = match inst {
            Instrument::Deposit { rate, dcf, t } => {
                bootstrap_deposit(*rate, *dcf)
            }
            Instrument::Future { price, vol, t1, t2, dcf } => {
                let fwd = forward_from_future(*price, *vol, *t1, *t2);
                let z_t1 = interpolate_df(&nodes, *t1);
                bootstrap_future(fwd, *dcf, z_t1)
            }
            Instrument::Swap { rate, dates, dcfs } => {
                let known_dfs: Vec<f64> = dates[..dates.len()-1]
                    .iter()
                    .map(|t| interpolate_df(&nodes, *t))
                    .collect();
                bootstrap_swap(*rate, dates, dcfs, &known_dfs)
            }
        };
        nodes.push((inst.maturity(), df));
    }
    nodes
}
```

## Interpolation Methods

### Log-Linear on Discount Factors (Recommended for Stability)

```rust
fn interpolate_log_linear(t: f64, nodes: &[(f64, f64)]) -> f64 {
    let (t1, z1, t2, z2) = find_bracket(t, nodes);
    
    let log_z1 = z1.ln();
    let log_z2 = z2.ln();
    
    let log_z = log_z1 + (log_z2 - log_z1) * (t - t1) / (t2 - t1);
    log_z.exp()
}
```

**Properties**: 
- Produces continuous forward rates
- Guaranteed positive discount factors
- Simple and robust

### Monotone Convex (Hagan-West - Academically Preferred)

```rust
struct MonotoneConvex {
    times: Vec<f64>,
    discrete_forwards: Vec<f64>,  // f_i^d = -ln(Z_i/Z_{i-1}) / Δt
    slopes: Vec<f64>,             // Constrained slopes
}

impl MonotoneConvex {
    fn new(nodes: &[(f64, f64)]) -> Self {
        let n = nodes.len();
        let mut df: Vec<f64> = Vec::with_capacity(n - 1);
        
        // Calculate discrete forwards
        for i in 1..n {
            let dt = nodes[i].0 - nodes[i-1].0;
            let f = -(nodes[i].1 / nodes[i-1].1).ln() / dt;
            df.push(f);
        }
        
        // Calculate constrained slopes ensuring positivity
        let slopes = Self::calculate_slopes(&df);
        
        MonotoneConvex {
            times: nodes.iter().map(|(t, _)| *t).collect(),
            discrete_forwards: df,
            slopes,
        }
    }
    
    fn calculate_slopes(df: &[f64]) -> Vec<f64> {
        // Hagan-West algorithm for monotonicity-preserving slopes
        let n = df.len();
        let mut g: Vec<f64> = vec![0.0; n + 1];
        
        // Boundary conditions
        g[0] = df[0];
        g[n] = df[n - 1];
        
        // Interior points
        for i in 1..n {
            let f_prev = df[i - 1];
            let f_curr = df[i];
            
            // Constrain to ensure 0 < f < 2 × min(f_prev, f_curr)
            let f_max = 2.0 * f_prev.min(f_curr);
            g[i] = ((f_prev + f_curr) / 2.0).min(f_max).max(0.0);
        }
        g
    }
    
    fn forward_rate(&self, t: f64) -> f64 {
        let i = self.find_interval(t);
        let dt = self.times[i + 1] - self.times[i];
        let x = (t - self.times[i]) / dt;
        
        // Quadratic interpolation within interval
        let g0 = self.slopes[i];
        let g1 = self.slopes[i + 1];
        let fd = self.discrete_forwards[i];
        
        // f(x) = (1-4x+3x²)g0 + (4x-2x²)(fd - 0.5(g0+g1)) + (-2x+3x²)g1
        // Simplified for x ∈ [0,1]
        g0 * (1.0 - 4.0*x + 3.0*x*x) 
            + (fd - 0.5*(g0 + g1)) * (4.0*x - 2.0*x*x)
            + g1 * (-2.0*x + 3.0*x*x)
    }
}
```

**Properties**:
- Guarantees positive forwards when inputs positive
- Shape-preserving (monotonicity, convexity)
- Local: perturbing one input affects only 2 intervals
- Stability norm: ~1.5-2.0 vs >100,000 for quartic splines

### Cubic Spline (Traditional)

```rust
fn cubic_spline_interpolate(
    t: f64,
    nodes: &[(f64, f64)],
    second_derivatives: &[f64],  // From tridiagonal system
) -> f64 {
    let (i, t1, y1, t2, y2) = find_bracket_with_index(t, nodes);
    let h = t2 - t1;
    let a = (t2 - t) / h;
    let b = (t - t1) / h;
    
    let m1 = second_derivatives[i];
    let m2 = second_derivatives[i + 1];
    
    a * y1 + b * y2 
        + (a*a*a - a) * h*h / 6.0 * m1
        + (b*b*b - b) * h*h / 6.0 * m2
}
```

**Warning**: Can produce negative forwards in steep curve environments. Use natural boundary conditions.

### Bloomberg Methods

| Setting | Method | Use Case |
|---------|--------|----------|
| Raw | Flat forward (piecewise constant) | Most stable |
| 1 | Log-linear on discount factors | General purpose |
| 2 | Linear on spot rates | Simple |
| 3 | Cubic spline | Smooth curves |

## Curve Object Implementation

```rust
pub struct YieldCurveImpl {
    reference_date: Date,
    nodes: Vec<(f64, f64)>,  // (year_fraction, discount_factor)
    interpolator: InterpolationMethod,
    day_count: DayCount,
    compounding: Compounding,
}

impl YieldCurve for YieldCurveImpl {
    fn discount_factor(&self, t: f64) -> f64 {
        match self.interpolator {
            InterpolationMethod::LogLinear => 
                interpolate_log_linear(t, &self.nodes),
            InterpolationMethod::MonotoneConvex => 
                self.monotone_convex.discount_factor(t),
            InterpolationMethod::CubicSpline =>
                cubic_spline_interpolate(t, &self.nodes, &self.spline_coeffs),
            InterpolationMethod::Linear =>
                linear_interpolate(t, &self.nodes),
        }
    }
    
    fn zero_rate(&self, t: f64) -> f64 {
        let df = self.discount_factor(t);
        match self.compounding {
            Compounding::Continuous => -df.ln() / t,
            Compounding::Annual => df.powf(-1.0 / t) - 1.0,
            Compounding::SemiAnnual => 2.0 * (df.powf(-1.0 / (2.0 * t)) - 1.0),
            Compounding::Quarterly => 4.0 * (df.powf(-1.0 / (4.0 * t)) - 1.0),
            Compounding::Simple => (1.0 / df - 1.0) / t,
        }
    }
    
    fn forward_rate(&self, t1: f64, t2: f64) -> f64 {
        let df1 = self.discount_factor(t1);
        let df2 = self.discount_factor(t2);
        let dcf = t2 - t1;
        
        match self.compounding {
            Compounding::Simple => (df1 / df2 - 1.0) / dcf,
            Compounding::Continuous => (df1 / df2).ln() / dcf,
            _ => {
                let n = match self.compounding {
                    Compounding::Annual => 1.0,
                    Compounding::SemiAnnual => 2.0,
                    Compounding::Quarterly => 4.0,
                    _ => 1.0,
                };
                n * ((df1 / df2).powf(1.0 / (n * dcf)) - 1.0)
            }
        }
    }
    
    fn par_rate(&self, tenor: f64) -> f64 {
        // Calculate par swap rate for given tenor
        let payment_dates = generate_annual_dates(tenor);
        let dcfs: Vec<f64> = payment_dates.iter()
            .zip(payment_dates.iter().skip(1))
            .map(|(t1, t2)| t2 - t1)
            .collect();
        
        let annuity: f64 = payment_dates.iter()
            .zip(dcfs.iter())
            .map(|(t, dcf)| dcf * self.discount_factor(*t))
            .sum();
        
        let df_final = self.discount_factor(tenor);
        
        (1.0 - df_final) / annuity
    }
}
```

## Standard Tenors by Currency

### USD SOFR

```rust
const USD_TENORS: &[&str] = &[
    "O/N", "1W", "1M", "2M", "3M", "6M", "9M", "1Y",
    "18M", "2Y", "3Y", "4Y", "5Y", "6Y", "7Y", "8Y", "9Y", "10Y",
    "12Y", "15Y", "20Y", "25Y", "30Y"
];
```

### EUR €STR

```rust
const EUR_TENORS: &[&str] = &[
    "O/N", "1W", "2W", "1M", "2M", "3M", "6M", "9M", "1Y",
    "15M", "18M", "2Y", "3Y", "4Y", "5Y", "6Y", "7Y", "8Y", "9Y", "10Y",
    "11Y", "12Y", "15Y", "20Y", "25Y", "30Y", "40Y", "50Y"
];
```

### GBP SONIA

```rust
const GBP_TENORS: &[&str] = &[
    "O/N", "1W", "2W", "1M", "2M", "3M", "6M", "9M", "1Y",
    "18M", "2Y", "3Y", "4Y", "5Y", "6Y", "7Y", "8Y", "9Y", "10Y",
    "12Y", "15Y", "20Y", "25Y", "30Y", "40Y", "50Y"
];
```

## Validation Tests

### Roundtrip Test

Input swap rates must reprice exactly:

```rust
#[test]
fn test_swap_roundtrip() {
    let swap_rates = vec![(2.0, 0.04), (5.0, 0.045), (10.0, 0.05)];
    let curve = bootstrap_from_swaps(&swap_rates);
    
    for (tenor, rate) in &swap_rates {
        let repriced = curve.par_rate(*tenor);
        assert!((repriced - rate).abs() < 1e-10);
    }
}
```

### Arbitrage Test

Discount factors must be monotonically decreasing:

```rust
#[test]
fn test_no_arbitrage() {
    let curve = build_test_curve();
    let times: Vec<f64> = (1..=30).map(|y| y as f64).collect();
    
    for window in times.windows(2) {
        let df1 = curve.discount_factor(window[0]);
        let df2 = curve.discount_factor(window[1]);
        assert!(df2 < df1, "Discount factors must decrease");
        assert!(df2 > 0.0, "Discount factors must be positive");
    }
}
```

### Forward Positivity

```rust
#[test]
fn test_positive_forwards() {
    let curve = build_test_curve();
    
    for t in (1..300).map(|m| m as f64 / 12.0) {
        let fwd = curve.forward_rate(t, t + 0.25);
        assert!(fwd > -0.05, "Forward rate too negative at t={}", t);
    }
}
```
