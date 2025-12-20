# Interpolation Methods for Yield Curves

## Table of Contents
- [Method Comparison](#method-comparison)
- [Log-Linear on Discount Factors](#log-linear-on-discount-factors)
- [Monotone Convex (Hagan-West)](#monotone-convex-hagan-west)
- [Cubic Spline Methods](#cubic-spline-methods)
- [Positivity and Arbitrage Constraints](#positivity-and-arbitrage-constraints)
- [Implementation](#implementation)

## Method Comparison

| Method | Positive Forwards | Continuous f(t) | Continuous f'(t) | Local | Best For |
|--------|------------------|-----------------|------------------|-------|----------|
| Log-linear DF | Yes | No | No | Yes | OIS, default |
| Linear DF | No | No | No | Yes | Avoid |
| Linear zero | No | No | No | Yes | Avoid |
| Monotone convex | Yes | Yes | No | Yes | Projection curves |
| Natural cubic | No | Yes | Yes | No | Avoid (finance) |
| Tension spline | Yes* | Yes | Yes | No | Smooth forwards |

**Key insight:** In finance, positive forwards are essential (arbitrage-free). Smoothness is secondary.

## Log-Linear on Discount Factors

Industry standard for OIS curves. Interpolate log(D(t)) linearly.

**Formula:**
```
ln(D(t)) = ln(D(tᵢ)) + (t - tᵢ)/(tᵢ₊₁ - tᵢ) × [ln(D(tᵢ₊₁)) - ln(D(tᵢ))]
```

Equivalently:
```
D(t) = D(tᵢ)^((tᵢ₊₁ - t)/(tᵢ₊₁ - tᵢ)) × D(tᵢ₊₁)^((t - tᵢ)/(tᵢ₊₁ - tᵢ))
```

**Forward rate (constant between nodes):**
```
f(t) = [ln(D(tᵢ)) - ln(D(tᵢ₊₁))] / (tᵢ₊₁ - tᵢ)  for t ∈ [tᵢ, tᵢ₊₁)
```

**Properties:**
- ✅ Always positive forwards (if discrete forwards positive)
- ✅ Completely local (change one node, affects only adjacent intervals)
- ✅ Numerically stable
- ❌ Discontinuous forward rates at nodes
- ❌ Can produce sawtooth forward patterns

```rust
pub struct LogLinearInterpolator {
    times: Vec<f64>,
    log_dfs: Vec<f64>,
}

impl LogLinearInterpolator {
    pub fn new(times: Vec<f64>, dfs: Vec<f64>) -> Self {
        let log_dfs = dfs.iter().map(|d| d.ln()).collect();
        Self { times, log_dfs }
    }
    
    pub fn discount_factor(&self, t: f64) -> f64 {
        if t <= self.times[0] {
            return self.log_dfs[0].exp();
        }
        if t >= *self.times.last().unwrap() {
            // Flat extrapolation on forward
            let n = self.times.len();
            let last_fwd = (self.log_dfs[n-2] - self.log_dfs[n-1]) 
                         / (self.times[n-1] - self.times[n-2]);
            return (self.log_dfs[n-1] - last_fwd * (t - self.times[n-1])).exp();
        }
        
        // Binary search for interval
        let i = self.find_interval(t);
        
        let dt = self.times[i+1] - self.times[i];
        let w = (t - self.times[i]) / dt;
        
        ((1.0 - w) * self.log_dfs[i] + w * self.log_dfs[i+1]).exp()
    }
    
    pub fn forward_rate(&self, t: f64) -> f64 {
        let i = self.find_interval(t);
        (self.log_dfs[i] - self.log_dfs[i+1]) / (self.times[i+1] - self.times[i])
    }
    
    fn find_interval(&self, t: f64) -> usize {
        match self.times.binary_search_by(|x| x.partial_cmp(&t).unwrap()) {
            Ok(i) => i.min(self.times.len() - 2),
            Err(i) => (i - 1).min(self.times.len() - 2),
        }
    }
}
```

## Monotone Convex (Hagan-West)

Produces continuous forward rates while preserving positivity. Industry standard when smooth forwards needed.

### Algorithm Overview

1. Calculate discrete forward rates from zero rates
2. Estimate instantaneous forwards at nodes
3. Apply monotonicity constraints
4. Construct piecewise quadratic interpolant

### Step 1: Discrete Forwards

```
fᵈᵢ = (rᵢτᵢ - rᵢ₋₁τᵢ₋₁) / (τᵢ - τᵢ₋₁)
```

Where rᵢ is the continuously compounded zero rate to time τᵢ.

### Step 2: Instantaneous Forward Estimates

At each node, estimate instantaneous forward as weighted average:

```
fᵢ = (τᵢ - τᵢ₋₁)/(τᵢ₊₁ - τᵢ₋₁) × fᵈᵢ₊₁ + (τᵢ₊₁ - τᵢ)/(τᵢ₊₁ - τᵢ₋₁) × fᵈᵢ
```

At boundaries:
- f₀ = fᵈ₁ - 0.5 × (f₁ - fᵈ₁)
- fₙ = fᵈₙ - 0.5 × (fₙ₋₁ - fᵈₙ)

### Step 3: Positivity Constraints

Collar instantaneous forwards to ensure positivity:

```
fᵢ = max(0, min(fᵢ, 2 × min(fᵈᵢ, fᵈᵢ₊₁)))
```

This ensures the quadratic interpolant stays non-negative.

### Step 4: Quadratic Interpolation

For τ ∈ [τᵢ₋₁, τᵢ], define x = (τ - τᵢ₋₁)/(τᵢ - τᵢ₋₁) ∈ [0,1].

The forward rate is:
```
f(τ) = g(x) + fᵈᵢ
```

Where g(x) is a piecewise quadratic satisfying:
- g(0) = fᵢ₋₁ - fᵈᵢ
- g(1) = fᵢ - fᵈᵢ
- ∫₀¹ g(x)dx = 0 (ensures discrete forward preserved)

```rust
pub struct MonotoneConvexInterpolator {
    times: Vec<f64>,
    discrete_fwds: Vec<f64>,
    inst_fwds: Vec<f64>,
}

impl MonotoneConvexInterpolator {
    pub fn new(times: Vec<f64>, zero_rates: Vec<f64>) -> Self {
        let n = times.len();
        
        // Step 1: Discrete forwards
        let mut discrete_fwds = vec![0.0; n];
        for i in 1..n {
            discrete_fwds[i] = (zero_rates[i] * times[i] - zero_rates[i-1] * times[i-1])
                             / (times[i] - times[i-1]);
        }
        discrete_fwds[0] = discrete_fwds[1];  // Extrapolate
        
        // Step 2: Instantaneous forward estimates
        let mut inst_fwds = vec![0.0; n];
        for i in 1..n-1 {
            let w = (times[i] - times[i-1]) / (times[i+1] - times[i-1]);
            inst_fwds[i] = (1.0 - w) * discrete_fwds[i] + w * discrete_fwds[i+1];
        }
        // Boundaries
        inst_fwds[0] = discrete_fwds[1] - 0.5 * (inst_fwds[1] - discrete_fwds[1]);
        inst_fwds[n-1] = discrete_fwds[n-1] - 0.5 * (inst_fwds[n-2] - discrete_fwds[n-1]);
        
        // Step 3: Positivity constraints
        for i in 0..n {
            let fd_left = if i > 0 { discrete_fwds[i] } else { discrete_fwds[1] };
            let fd_right = if i < n-1 { discrete_fwds[i+1] } else { discrete_fwds[n-1] };
            let upper = 2.0 * fd_left.min(fd_right);
            inst_fwds[i] = inst_fwds[i].max(0.0).min(upper.max(0.0));
        }
        
        Self { times, discrete_fwds, inst_fwds }
    }
    
    pub fn forward_rate(&self, t: f64) -> f64 {
        let i = self.find_interval(t);
        let x = (t - self.times[i]) / (self.times[i+1] - self.times[i]);
        
        let g0 = self.inst_fwds[i] - self.discrete_fwds[i+1];
        let g1 = self.inst_fwds[i+1] - self.discrete_fwds[i+1];
        
        // Quadratic g(x) satisfying constraints
        let g = self.quadratic_g(x, g0, g1);
        
        g + self.discrete_fwds[i+1]
    }
    
    fn quadratic_g(&self, x: f64, g0: f64, g1: f64) -> f64 {
        // Monotone quadratic interpolant
        // Multiple cases based on signs of g0, g1
        
        if (g0 < 0.0 && -0.5 * g0 <= g1 && g1 <= -2.0 * g0)
            || (g0 > 0.0 && -0.5 * g0 >= g1 && g1 >= -2.0 * g0) {
            // Zone 1: Monotone
            let eta = g1 + 2.0 * g0;
            if x < eta / (eta - g0) {
                g0 + (g0 - g1) * x.powi(2) / eta
            } else {
                g0 + (g0 - g1) * (1.0 - x).powi(2) / (eta - 2.0 * g0)
            }
        } else if (g0 < 0.0 && g1 > -2.0 * g0)
            || (g0 > 0.0 && g1 < -2.0 * g0) {
            // Zone 2: Has extremum
            let eta = 3.0 * g1 / (g1 - g0);
            if x < eta {
                g0 + (g1 - g0) * x.powi(2) / (eta * (2.0 - eta))
            } else {
                g1 + (g1 - g0) * (1.0 - x).powi(2) / ((1.0 - eta) * (2.0 - eta))
            }
        } else {
            // Zone 3: Simple quadratic
            g0 + (g1 - g0) * x
        }
    }
    
    fn find_interval(&self, t: f64) -> usize {
        // Same as log-linear
        match self.times.binary_search_by(|x| x.partial_cmp(&t).unwrap()) {
            Ok(i) => i.min(self.times.len() - 2),
            Err(i) => (i - 1).max(0).min(self.times.len() - 2),
        }
    }
}
```

## Cubic Spline Methods

### Natural Cubic Spline

**Avoid for yield curves** - can produce negative forwards.

Fits cubic polynomials with continuous second derivatives:
```
S(t) = aᵢ + bᵢ(t-tᵢ) + cᵢ(t-tᵢ)² + dᵢ(t-tᵢ)³
```

With S''(t₀) = S''(tₙ) = 0 (natural boundary conditions).

**Problem:** Non-local - changing one node affects entire curve. Can produce oscillations and negative forwards.

### Tension Spline

Adds tension parameter to reduce oscillations:
```
S''(t) - σ²S(t) = constant
```

Higher σ → more linear behavior → less oscillation.

**Still non-local** but can preserve positivity with sufficient tension.

```rust
// Only use if smoothness is paramount
pub struct TensionSpline {
    times: Vec<f64>,
    values: Vec<f64>,
    tension: f64,  // Typically 1.0 - 10.0
    // ... coefficients
}
```

## Positivity and Arbitrage Constraints

### Why Positive Forwards Matter

Negative forward rate implies:
```
D(T₁) < D(T₂)  for T₁ < T₂
```

This is arbitrage: borrow at T₁, lend at T₂, lock in riskless profit.

### Ensuring Positivity

**Log-linear:** Automatic if input discount factors are decreasing.

**Monotone convex:** Explicit constraints in step 3 guarantee positivity.

**Cubic splines:** Add inequality constraints (complex, loses closed-form).

### Checking Arbitrage

```rust
fn check_arbitrage_free(curve: &Curve) -> bool {
    let dt = 0.001;  // Small step
    let mut t = 0.0;
    
    while t < curve.max_maturity() {
        let fwd = curve.forward_rate(t);
        if fwd < -1e-10 {  // Allow tiny numerical noise
            return false;
        }
        t += dt;
    }
    true
}
```

## Implementation

### Generic Interpolator Trait

```rust
pub trait Interpolator: Send + Sync {
    fn discount_factor(&self, t: f64) -> f64;
    fn forward_rate(&self, t: f64) -> f64;
    fn zero_rate(&self, t: f64) -> f64 {
        if t <= 0.0 { return 0.0; }
        -self.discount_factor(t).ln() / t
    }
}

pub enum InterpolationMethod {
    LogLinear,
    MonotoneConvex,
    Linear,  // Not recommended
}

pub fn create_interpolator(
    method: InterpolationMethod,
    times: Vec<f64>,
    dfs: Vec<f64>,
) -> Box<dyn Interpolator> {
    match method {
        InterpolationMethod::LogLinear => {
            Box::new(LogLinearInterpolator::new(times, dfs))
        }
        InterpolationMethod::MonotoneConvex => {
            let zero_rates: Vec<f64> = times.iter()
                .zip(dfs.iter())
                .map(|(t, d)| if *t > 0.0 { -d.ln() / t } else { 0.0 })
                .collect();
            Box::new(MonotoneConvexInterpolator::new(times, zero_rates))
        }
        InterpolationMethod::Linear => {
            Box::new(LinearInterpolator::new(times, dfs))
        }
    }
}
```

### Extrapolation

**Flat forward extrapolation** (most common):

```rust
fn extrapolate_flat_forward(&self, t: f64) -> f64 {
    let n = self.times.len();
    let last_t = self.times[n-1];
    let last_df = self.dfs[n-1];
    
    // Forward rate from second-to-last to last node
    let fwd = (self.dfs[n-2].ln() - last_df.ln()) / (last_t - self.times[n-2]);
    
    // Extrapolate
    last_df * (-fwd * (t - last_t)).exp()
}
```

### Performance Considerations

**Binary search:** O(log n) per lookup. Cache last interval for sequential access.

```rust
pub struct CachedInterpolator<I: Interpolator> {
    inner: I,
    last_interval: AtomicUsize,
}

impl<I: Interpolator> CachedInterpolator<I> {
    fn find_interval_cached(&self, t: f64) -> usize {
        let hint = self.last_interval.load(Ordering::Relaxed);
        // Check if hint is still valid
        if self.times[hint] <= t && t < self.times[hint + 1] {
            return hint;
        }
        // Fall back to binary search
        let i = self.binary_search(t);
        self.last_interval.store(i, Ordering::Relaxed);
        i
    }
}
```

**SIMD for batch lookups:** When pricing portfolios, batch discount factor lookups.

```rust
#[cfg(target_arch = "x86_64")]
fn batch_discount_factors_simd(times: &[f64], curve: &Curve) -> Vec<f64> {
    // Use AVX2 for 4 simultaneous lookups
    // ... SIMD implementation
}
```
