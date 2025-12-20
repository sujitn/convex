# Numerical Methods and Performance

## Table of Contents
- [Jacobian and Sensitivity Calculation](#jacobian-and-sensitivity-calculation)
- [Algorithmic Differentiation](#algorithmic-differentiation)
- [Solver Selection](#solver-selection)
- [Numerical Stability](#numerical-stability)
- [Performance Optimization](#performance-optimization)
- [Benchmarking Targets](#benchmarking-targets)

## Jacobian and Sensitivity Calculation

### Bucketed PV01 (Key Rate Duration)

Transform portfolio sensitivities from curve parameters to market quotes:

```
∂PV/∂qₖ = Σₗ (∂PV/∂pₗ) × (∂pₗ/∂qₖ)
```

Where:
- q = market quotes (what traders observe)
- p = curve parameters (what the model uses)
- The transformation matrix comes from the calibration process

### Calibration Jacobian

During bootstrapping, we solve S(p, q) = 0 where S is the implied quote minus market quote.

By implicit function theorem:
```
∂p/∂q = -(∂S/∂p)⁻¹ × (∂S/∂q) = -(∂S/∂p)⁻¹
```

Since ∂S/∂q = I (identity).

```rust
fn compute_calibration_jacobian(
    instruments: &[Instrument],
    curve: &Curve,
) -> Matrix {
    let n = instruments.len();
    let mut dS_dp = Matrix::zeros(n, n);
    
    // ∂Sᵢ/∂pⱼ = sensitivity of instrument i's implied rate to node j
    for i in 0..n {
        for j in 0..n {
            dS_dp[(i, j)] = instrument_sensitivity_to_node(
                &instruments[i],
                curve,
                j,
            );
        }
    }
    
    // Invert to get ∂p/∂q
    dS_dp.inverse()
}

fn bucket_portfolio_risk(
    portfolio: &Portfolio,
    curve: &Curve,
    instruments: &[Instrument],
) -> Vec<f64> {
    // Portfolio sensitivity to curve nodes
    let dp_pv = portfolio_node_sensitivities(portfolio, curve);
    
    // Calibration Jacobian
    let dq_p = compute_calibration_jacobian(instruments, curve);
    
    // Transform: ∂PV/∂q = ∂PV/∂p × ∂p/∂q
    dq_p.transpose() * &dp_pv
}
```

### Key Rate Duration

Parallel shift decomposes into bucket shifts. Use triangular bump profiles:

```
Profile_i(t) = max(0, 1 - |t - tᵢ| / Δt)
```

Where bumps at adjacent nodes sum to parallel shift.

```rust
fn key_rate_durations(
    portfolio: &Portfolio,
    curve: &Curve,
    bump_size: f64,  // Typically 1bp = 0.0001
) -> Vec<f64> {
    let n = curve.node_count();
    let base_pv = portfolio.present_value(curve);
    let mut krds = vec![0.0; n];
    
    for i in 0..n {
        // Bump up
        let curve_up = curve.with_node_bumped(i, bump_size);
        let pv_up = portfolio.present_value(&curve_up);
        
        // Bump down
        let curve_down = curve.with_node_bumped(i, -bump_size);
        let pv_down = portfolio.present_value(&curve_down);
        
        // Central difference
        krds[i] = (pv_down - pv_up) / (2.0 * bump_size * base_pv);
    }
    
    krds
}
```

## Algorithmic Differentiation

AD provides exact derivatives at cost of ~2-4x single evaluation. **5x faster than finite difference** with machine precision.

### Forward Mode (Dual Numbers)

Track value and derivative simultaneously:

```rust
#[derive(Clone, Copy)]
pub struct Dual {
    pub val: f64,
    pub der: f64,
}

impl Dual {
    pub fn var(x: f64) -> Self {
        Self { val: x, der: 1.0 }
    }
    
    pub fn constant(x: f64) -> Self {
        Self { val: x, der: 0.0 }
    }
}

impl std::ops::Add for Dual {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            val: self.val + rhs.val,
            der: self.der + rhs.der,
        }
    }
}

impl std::ops::Mul for Dual {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self {
            val: self.val * rhs.val,
            der: self.val * rhs.der + self.der * rhs.val,
        }
    }
}

impl Dual {
    pub fn exp(self) -> Self {
        let e = self.val.exp();
        Self { val: e, der: e * self.der }
    }
    
    pub fn ln(self) -> Self {
        Self { val: self.val.ln(), der: self.der / self.val }
    }
}
```

### Forward Mode for Single Sensitivity

```rust
fn discount_factor_sensitivity<T: Num>(
    curve_nodes: &[(f64, T)],  // Generic over f64 or Dual
    t: f64,
) -> T {
    // Same interpolation code works for both value and derivative
    let i = find_interval(&curve_nodes, t);
    let (t0, d0) = &curve_nodes[i];
    let (t1, d1) = &curve_nodes[i + 1];
    
    let w = (t - t0) / (t1 - t0);
    d0.ln() * (1.0 - w) + d1.ln() * w
}

// Usage
fn compute_delta(portfolio: &Portfolio, curve: &Curve, node_idx: usize) -> f64 {
    // Seed the node we're differentiating with respect to
    let dual_nodes: Vec<(f64, Dual)> = curve.nodes.iter()
        .enumerate()
        .map(|(i, (t, d))| {
            if i == node_idx {
                (*t, Dual::var(*d))
            } else {
                (*t, Dual::constant(*d))
            }
        })
        .collect();
    
    let pv = portfolio.present_value_generic(&dual_nodes);
    pv.der
}
```

### Reverse Mode (Adjoint)

For many outputs, few inputs (our case: one PV, many curve nodes), reverse mode is optimal.

```rust
// Tape-based AD
struct Tape {
    nodes: Vec<TapeNode>,
}

struct TapeNode {
    val: f64,
    deps: Vec<(usize, f64)>,  // (dependency_idx, partial_derivative)
}

impl Tape {
    fn backward(&self, output_idx: usize) -> Vec<f64> {
        let mut adjoints = vec![0.0; self.nodes.len()];
        adjoints[output_idx] = 1.0;
        
        // Reverse sweep
        for i in (0..self.nodes.len()).rev() {
            let adj = adjoints[i];
            for (dep_idx, partial) in &self.nodes[i].deps {
                adjoints[*dep_idx] += adj * partial;
            }
        }
        
        adjoints
    }
}
```

### Performance Comparison

| Approach | Time (10K swaps, 30 nodes) | Precision |
|----------|---------------------------|-----------|
| Forward mode AD (per node) | ~40ms total | Machine |
| Reverse mode AD | ~3ms | Machine |
| Finite difference (central) | ~60ms | 1e-6 to 1e-8 |
| Finite difference (forward) | ~30ms | 1e-4 to 1e-6 |

**Recommendation:** Use reverse mode AD for production. Fall back to central finite difference (bump = 1e-5) for validation.

## Solver Selection

### Single Equation: Brent vs Newton

| Solver | Convergence | Requires | Robustness |
|--------|-------------|----------|------------|
| Brent | Superlinear (~1.6) | Bracketing | Guaranteed |
| Newton | Quadratic | Derivative | Can diverge |
| Secant | Superlinear (~1.6) | Two points | Can diverge |

**Recommendation:** Brent for bootstrapping (guaranteed convergence, no derivative needed).

### System of Equations: Newton vs LM

| Solver | Best For | Convergence | Robustness |
|--------|----------|-------------|------------|
| Newton-Raphson | Well-conditioned | Quadratic | Poor far from solution |
| Levenberg-Marquardt | Ill-conditioned, overdetermined | Linear to quadratic | Excellent |
| Gauss-Newton | Near solution | Quadratic | Poor far from solution |

**Recommendation:** LM for global curve fitting. Newton for well-posed sequential bootstrap.

```rust
fn levenberg_marquardt<F, J>(
    f: F,           // Residual function
    jacobian: J,    // Jacobian function
    x0: &[f64],
    tol: f64,
    max_iter: usize,
) -> Vec<f64>
where
    F: Fn(&[f64]) -> Vec<f64>,
    J: Fn(&[f64]) -> Matrix,
{
    let mut x = x0.to_vec();
    let mut lambda = 1e-3;  // Initial damping
    let mut r = f(&x);
    
    for _ in 0..max_iter {
        let J = jacobian(&x);
        let JtJ = J.transpose() * &J;
        let Jtr = J.transpose() * &r;
        
        // Damped normal equations
        let mut A = JtJ.clone();
        for i in 0..A.nrows() {
            A[(i, i)] *= 1.0 + lambda;
        }
        
        let delta = A.solve(&Jtr);
        let x_new: Vec<f64> = x.iter().zip(delta.iter()).map(|(a, b)| a - b).collect();
        let r_new = f(&x_new);
        
        // Evaluate step quality
        let rho = (norm_sq(&r) - norm_sq(&r_new)) 
                / (delta.dot(&(lambda * &delta + &Jtr)));
        
        if rho > 0.0 {
            // Accept step
            x = x_new;
            r = r_new;
            lambda *= (1.0_f64 / 3.0).max(1.0 - (2.0 * rho - 1.0).powi(3));
        } else {
            // Reject, increase damping
            lambda *= 2.0;
        }
        
        if norm(&r) < tol {
            break;
        }
    }
    
    x
}
```

## Numerical Stability

### Sources of Ill-Conditioning

1. **Time scale disparity:** 1 day to 50 years (ratio 1:15,000)
2. **Close nodes:** Near-linear dependence when maturities cluster
3. **Non-local interpolation:** Changes propagate globally (cubic splines)
4. **Exponential functions:** exp() can overflow/underflow

### Mitigation Strategies

**Work in appropriate units:**
```rust
// Bad: raw year fractions
let t = 0.00274;  // 1 day

// Good: normalized time
let t_normalized = t / max_maturity;  // Scale to [0, 1]
```

**Use log discount factors:**
```rust
// Numerically better for long maturities
let log_df = -r * t;  // Instead of exp(-r * t)
```

**Regularization for ill-conditioned systems:**
```rust
fn solve_regularized(A: &Matrix, b: &Vector, lambda: f64) -> Vector {
    // Tikhonov regularization
    let AtA = A.transpose() * A;
    let reg = &AtA + lambda * Matrix::identity(AtA.nrows());
    reg.solve(&(A.transpose() * b))
}
```

**Condition number monitoring:**
```rust
fn check_conditioning(J: &Matrix) -> f64 {
    let svd = J.svd();
    let cond = svd.singular_values[0] / svd.singular_values.last().unwrap();
    
    if cond > 1e12 {
        warn!("Jacobian poorly conditioned: κ = {}", cond);
    }
    cond
}
```

### Day Count Considerations

**Prefer simple day counts internally:**
- ACT/360, ACT/365 are additive: τ(t₁,t₃) = τ(t₁,t₂) + τ(t₂,t₃)
- 30/360 is NOT additive (avoid for curve construction)

```rust
enum DayCount {
    Act360,
    Act365,
    Thirty360,  // Use only for display/quoting
}

impl DayCount {
    fn year_fraction(&self, d1: Date, d2: Date) -> f64 {
        match self {
            DayCount::Act360 => (d2 - d1).days() as f64 / 360.0,
            DayCount::Act365 => (d2 - d1).days() as f64 / 365.0,
            DayCount::Thirty360 => {
                // Complex logic, avoid for curve math
                thirty_360_fraction(d1, d2)
            }
        }
    }
}
```

## Performance Optimization

### Complexity Analysis

| Operation | Sequential Bootstrap | Global Fit |
|-----------|---------------------|------------|
| Single curve (n nodes) | O(n × k) | O(n³ × iter) |
| Multi-curve (m curves) | O(m × n × k) | O((m×n)³ × iter) |
| With AD sensitivities | O(n² × k) | O((m×n)³ × iter) |

Where k = average solver iterations per node (~5-10).

### Lazy Evaluation

```rust
pub struct LazyCurve {
    quotes: Vec<f64>,
    nodes: Option<Vec<CurveNode>>,
    dirty: bool,
}

impl LazyCurve {
    pub fn set_quote(&mut self, idx: usize, value: f64) {
        self.quotes[idx] = value;
        self.dirty = true;
    }
    
    pub fn discount_factor(&mut self, t: f64) -> f64 {
        if self.dirty {
            self.rebuild();
            self.dirty = false;
        }
        self.interpolate(t)
    }
    
    fn rebuild(&mut self) {
        self.nodes = Some(bootstrap(&self.quotes));
    }
}
```

### Incremental Updates

When only one quote changes, avoid full rebuild:

```rust
impl Curve {
    pub fn update_single_quote(&mut self, idx: usize, new_quote: f64) {
        // Only re-bootstrap from affected node onwards
        let affected_start = self.quote_to_node_mapping[idx];
        
        // Keep nodes before affected
        let prior_nodes = &self.nodes[..affected_start];
        
        // Re-bootstrap from affected node
        self.quotes[idx] = new_quote;
        let new_nodes = bootstrap_from(&self.quotes[idx..], prior_nodes);
        
        self.nodes = [prior_nodes.to_vec(), new_nodes].concat();
    }
}
```

### Parallelization

**Independent curves:** Build USD, EUR, GBP OIS in parallel (no dependencies).

```rust
fn build_ois_curves_parallel(
    quotes: &HashMap<Currency, Vec<f64>>,
) -> HashMap<Currency, Curve> {
    quotes.par_iter()
        .map(|(ccy, q)| (*ccy, bootstrap_ois(q)))
        .collect()
}
```

**Jacobian columns:** Each column independent.

```rust
fn compute_jacobian_parallel(
    portfolio: &Portfolio,
    curve: &Curve,
) -> Matrix {
    let n = curve.node_count();
    
    (0..n).into_par_iter()
        .map(|i| compute_delta(portfolio, curve, i))
        .collect::<Vec<_>>()
        .into()
}
```

**Scenario calculations:** Embarrassingly parallel.

### Memory Layout

**Contiguous storage for cache efficiency:**

```rust
// Good: Contiguous arrays
pub struct Curve {
    times: Vec<f64>,      // Contiguous
    log_dfs: Vec<f64>,    // Contiguous
}

// Bad: Array of structs (poor cache locality)
pub struct CurveBad {
    nodes: Vec<CurveNode>,  // Each node allocated separately
}
```

**Pre-allocate workspaces:**

```rust
pub struct CurveBuilder {
    // Reusable workspace
    workspace: Vec<f64>,
    jacobian_workspace: Matrix,
}

impl CurveBuilder {
    pub fn bootstrap(&mut self, quotes: &[f64]) -> Curve {
        // Reuse workspace instead of allocating
        self.workspace.clear();
        self.workspace.resize(quotes.len(), 0.0);
        // ... build curve
    }
}
```

## Benchmarking Targets

Based on industry standards (OpenGamma, QuantLib benchmarks):

| Operation | Target | Notes |
|-----------|--------|-------|
| Single OIS curve (30 nodes) | < 500 μs | Sequential bootstrap |
| Multi-curve EUR (OIS + 3M + 6M) | < 2 ms | With tenor basis |
| Full XCCY (3 currencies) | < 10 ms | Including XCCY bootstrap |
| Portfolio PV (1000 swaps) | < 5 ms | Against single curve |
| Full Jacobian (30 nodes) | < 20 ms | With reverse AD |
| Jacobian (finite diff) | < 100 ms | Central difference |

### Rust-Specific Optimizations

```rust
// Use stack allocation for small arrays
use smallvec::SmallVec;
type NodeVec = SmallVec<[CurveNode; 64]>;

// Avoid bounds checks in hot loops
fn interpolate_unchecked(&self, t: f64) -> f64 {
    let i = self.find_interval(t);
    unsafe {
        let t0 = *self.times.get_unchecked(i);
        let t1 = *self.times.get_unchecked(i + 1);
        // ... interpolate
    }
}

// Use SIMD for batch operations
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

fn batch_exp_simd(vals: &mut [f64]) {
    // Process 4 at a time with AVX
}
```

### Profiling Checklist

1. **Measure before optimizing** - Use `criterion` for benchmarks
2. **Profile allocations** - `dhat` or `heaptrack`
3. **Check cache misses** - `perf stat`
4. **Identify hot loops** - `flamegraph`

```rust
// Criterion benchmark example
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_bootstrap(c: &mut Criterion) {
    let quotes = generate_test_quotes();
    
    c.bench_function("ois_bootstrap_30_nodes", |b| {
        b.iter(|| bootstrap_ois(&quotes))
    });
}

criterion_group!(benches, bench_bootstrap);
criterion_main!(benches);
```
