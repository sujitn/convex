# Parametric Yield Curve Models

## Table of Contents
- [When to Use Parametric Models](#when-to-use-parametric-models)
- [Nelson-Siegel Model](#nelson-siegel-model)
- [Svensson Extension](#svensson-extension)
- [Parameter Estimation](#parameter-estimation)
- [Objective Functions](#objective-functions)
- [Optimization Algorithms](#optimization-algorithms)

## When to Use Parametric Models

**Parametric models** (Nelson-Siegel, Svensson) vs **Bootstrapping**:

| Aspect | Parametric | Bootstrap |
|--------|------------|-----------|
| Fit quality | Approximate | Exact |
| Smoothness | Guaranteed smooth | Depends on interpolation |
| Extrapolation | Well-behaved | Can be unstable |
| Parameters | 4-6 interpretable | N curve nodes |
| Best for | Economists, forecasting | Trading, relative value |

**Use parametric when:**
- Limited market data (few bonds)
- Need smooth curves for analysis
- Forecasting/time-series applications
- Central bank style curve fitting

**Use bootstrapping when:**
- Exact repricing required
- Rich/cheap analysis
- Derivatives pricing
- Arbitrage-free requirements

## Nelson-Siegel Model

The Nelson-Siegel (1987) model parameterizes the instantaneous forward curve:

```
f(τ) = β₀ + β₁×exp(-τ/λ) + β₂×(τ/λ)×exp(-τ/λ)
```

Integrating gives the zero rate (spot rate):

```
r(τ) = β₀ + β₁×[(1-exp(-τ/λ))/(τ/λ)] + β₂×[(1-exp(-τ/λ))/(τ/λ) - exp(-τ/λ)]
```

### Parameter Interpretation

| Parameter | Interpretation | Typical Range |
|-----------|---------------|---------------|
| β₀ | Long-term level (asymptote) | 2-5% |
| β₁ | Short-term component (slope) | -3% to +3% |
| β₂ | Medium-term hump/curvature | -3% to +3% |
| λ | Decay rate (shape) | 0.5-5.0 |

**Factor loadings:**

```
Level factor:     1                          (constant)
Slope factor:     (1-exp(-τ/λ))/(τ/λ)       (starts at 1, decays to 0)
Curvature factor: above - exp(-τ/λ)          (starts at 0, humps, decays)
```

### Implementation

```rust
pub struct NelsonSiegel {
    pub beta0: f64,  // Level
    pub beta1: f64,  // Slope
    pub beta2: f64,  // Curvature
    pub lambda: f64, // Decay
}

impl NelsonSiegel {
    /// Zero rate (continuously compounded) at maturity tau
    pub fn zero_rate(&self, tau: f64) -> f64 {
        if tau < 1e-10 {
            return self.beta0 + self.beta1;
        }
        
        let x = tau / self.lambda;
        let exp_x = (-x).exp();
        let factor1 = (1.0 - exp_x) / x;
        let factor2 = factor1 - exp_x;
        
        self.beta0 + self.beta1 * factor1 + self.beta2 * factor2
    }
    
    /// Instantaneous forward rate at maturity tau
    pub fn forward_rate(&self, tau: f64) -> f64 {
        if tau < 1e-10 {
            return self.beta0 + self.beta1;
        }
        
        let x = tau / self.lambda;
        let exp_x = (-x).exp();
        
        self.beta0 + self.beta1 * exp_x + self.beta2 * x * exp_x
    }
    
    /// Discount factor
    pub fn discount_factor(&self, tau: f64) -> f64 {
        (-self.zero_rate(tau) * tau).exp()
    }
    
    /// Factor loadings for time tau
    pub fn factor_loadings(&self, tau: f64) -> (f64, f64, f64) {
        if tau < 1e-10 {
            return (1.0, 1.0, 0.0);
        }
        
        let x = tau / self.lambda;
        let exp_x = (-x).exp();
        let loading1 = (1.0 - exp_x) / x;
        let loading2 = loading1 - exp_x;
        
        (1.0, loading1, loading2)
    }
}
```

## Svensson Extension

Svensson (1994) adds a second hump term for better long-end fit:

```
r(τ) = β₀ + β₁×L₁(τ,λ₁) + β₂×L₂(τ,λ₁) + β₃×L₂(τ,λ₂)
```

Where:
```
L₁(τ,λ) = (1-exp(-τ/λ))/(τ/λ)
L₂(τ,λ) = L₁(τ,λ) - exp(-τ/λ)
```

### Parameter Interpretation

| Parameter | Interpretation |
|-----------|---------------|
| β₀ | Long-term level |
| β₁ | Short-term component |
| β₂ | First hump (medium-term) |
| β₃ | Second hump (long-term) |
| λ₁ | First decay rate |
| λ₂ | Second decay rate |

**Constraint:** λ₁ ≠ λ₂ (otherwise reduces to Nelson-Siegel)

### Implementation

```rust
pub struct Svensson {
    pub beta0: f64,
    pub beta1: f64,
    pub beta2: f64,
    pub beta3: f64,
    pub lambda1: f64,
    pub lambda2: f64,
}

impl Svensson {
    pub fn zero_rate(&self, tau: f64) -> f64 {
        if tau < 1e-10 {
            return self.beta0 + self.beta1;
        }
        
        let x1 = tau / self.lambda1;
        let x2 = tau / self.lambda2;
        let exp_x1 = (-x1).exp();
        let exp_x2 = (-x2).exp();
        
        let l1_1 = (1.0 - exp_x1) / x1;
        let l2_1 = l1_1 - exp_x1;
        let l2_2 = (1.0 - exp_x2) / x2 - exp_x2;
        
        self.beta0 
            + self.beta1 * l1_1 
            + self.beta2 * l2_1 
            + self.beta3 * l2_2
    }
    
    pub fn forward_rate(&self, tau: f64) -> f64 {
        if tau < 1e-10 {
            return self.beta0 + self.beta1;
        }
        
        let x1 = tau / self.lambda1;
        let x2 = tau / self.lambda2;
        let exp_x1 = (-x1).exp();
        let exp_x2 = (-x2).exp();
        
        self.beta0 
            + self.beta1 * exp_x1 
            + self.beta2 * x1 * exp_x1 
            + self.beta3 * x2 * exp_x2
    }
}
```

## Parameter Estimation

### Constraints

For economically sensible curves:

```rust
struct NSConstraints {
    // β₀ > 0 (positive long rate)
    beta0_min: f64,  // e.g., 0.0
    
    // β₀ + β₁ > 0 (positive short rate)
    // This is the instantaneous rate at τ=0
    
    // λ > 0 (positive decay)
    lambda_min: f64,  // e.g., 0.1
    lambda_max: f64,  // e.g., 10.0
    
    // For Svensson: λ₁ ≠ λ₂
    lambda_separation: f64,  // e.g., 0.5
}

fn check_constraints(params: &NelsonSiegel) -> bool {
    params.beta0 > 0.0 &&
    params.beta0 + params.beta1 > 0.0 &&
    params.lambda > 0.1 &&
    params.lambda < 10.0
}
```

### Initial Values

Good starting points are critical for convergence:

```rust
fn initial_guess_from_yields(yields: &[(f64, f64)]) -> NelsonSiegel {
    // β₀: long-term rate (longest maturity)
    let beta0 = yields.iter()
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
        .map(|(_, y)| *y)
        .unwrap_or(0.03);
    
    // β₁: short - long spread (typically negative for normal curve)
    let short_rate = yields.iter()
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
        .map(|(_, y)| *y)
        .unwrap_or(0.02);
    let beta1 = short_rate - beta0;
    
    // β₂: curvature (start at 0)
    let beta2 = 0.0;
    
    // λ: typical value around 1-2
    let lambda = 1.5;
    
    NelsonSiegel { beta0, beta1, beta2, lambda }
}
```

## Objective Functions

### Yield-Based Fitting

Minimize squared yield errors:

```
min Σᵢ [r_model(τᵢ) - r_market(τᵢ)]²
```

```rust
fn yield_objective(params: &[f64], data: &[(f64, f64)]) -> f64 {
    let ns = NelsonSiegel::from_params(params);
    
    data.iter()
        .map(|(tau, market_yield)| {
            let model_yield = ns.zero_rate(*tau);
            (model_yield - market_yield).powi(2)
        })
        .sum()
}
```

### Price-Based Fitting

Minimize squared price errors (better for coupon bonds):

```
min Σᵢ [P_model(bondᵢ) - P_market(bondᵢ)]²
```

```rust
fn price_objective(params: &[f64], bonds: &[Bond]) -> f64 {
    let ns = NelsonSiegel::from_params(params);
    
    bonds.iter()
        .map(|bond| {
            let model_price = bond_price(&ns, bond);
            let market_price = bond.dirty_price;
            (model_price - market_price).powi(2)
        })
        .sum()
}
```

### Duration-Weighted Price Fitting

Weight by inverse duration to balance short and long bonds:

```
min Σᵢ [(P_model(bondᵢ) - P_market(bondᵢ)) / Dᵢ]²
```

This prevents long-duration bonds from dominating the fit.

```rust
fn duration_weighted_objective(params: &[f64], bonds: &[BondWithDuration]) -> f64 {
    let ns = NelsonSiegel::from_params(params);
    
    bonds.iter()
        .map(|b| {
            let model_price = bond_price(&ns, &b.bond);
            let error = model_price - b.bond.dirty_price;
            (error / b.duration).powi(2)
        })
        .sum()
}
```

## Optimization Algorithms

### Nelder-Mead (Simplex)

Good for small problems (4-6 parameters), derivative-free:

```rust
fn fit_nelson_siegel_nelder_mead(
    yields: &[(f64, f64)],
    initial: NelsonSiegel,
) -> NelsonSiegel {
    let objective = |params: &[f64]| {
        let ns = NelsonSiegel::from_params(params);
        yields.iter()
            .map(|(tau, y)| (ns.zero_rate(*tau) - y).powi(2))
            .sum::<f64>()
    };
    
    let x0 = initial.to_params();
    
    // Nelder-Mead simplex
    let result = nelder_mead(objective, &x0, 1e-8, 1000);
    
    NelsonSiegel::from_params(&result)
}
```

### Levenberg-Marquardt

Better for least-squares problems, uses Jacobian:

```rust
fn fit_nelson_siegel_lm(
    yields: &[(f64, f64)],
    initial: NelsonSiegel,
) -> NelsonSiegel {
    let residuals = |params: &[f64]| -> Vec<f64> {
        let ns = NelsonSiegel::from_params(params);
        yields.iter()
            .map(|(tau, y)| ns.zero_rate(*tau) - y)
            .collect()
    };
    
    let jacobian = |params: &[f64]| -> Matrix {
        let ns = NelsonSiegel::from_params(params);
        let n = yields.len();
        let mut J = Matrix::zeros(n, 4);
        
        for (i, (tau, _)) in yields.iter().enumerate() {
            let (l0, l1, l2) = ns.factor_loadings(*tau);
            J[(i, 0)] = l0;  // ∂r/∂β₀
            J[(i, 1)] = l1;  // ∂r/∂β₁
            J[(i, 2)] = l2;  // ∂r/∂β₂
            J[(i, 3)] = dr_dlambda(&ns, *tau);  // ∂r/∂λ
        }
        J
    };
    
    let x0 = initial.to_params();
    let result = levenberg_marquardt(residuals, jacobian, &x0, 1e-8, 100);
    
    NelsonSiegel::from_params(&result)
}

fn dr_dlambda(ns: &NelsonSiegel, tau: f64) -> f64 {
    // Numerical derivative w.r.t. lambda
    let eps = 1e-6;
    let ns_up = NelsonSiegel { lambda: ns.lambda + eps, ..*ns };
    let ns_dn = NelsonSiegel { lambda: ns.lambda - eps, ..*ns };
    (ns_up.zero_rate(tau) - ns_dn.zero_rate(tau)) / (2.0 * eps)
}
```

### Differential Evolution

Global optimizer, good for avoiding local minima:

```rust
fn fit_svensson_de(
    yields: &[(f64, f64)],
    bounds: &[(f64, f64)],  // Parameter bounds
) -> Svensson {
    let objective = |params: &[f64]| {
        let sv = Svensson::from_params(params);
        yields.iter()
            .map(|(tau, y)| (sv.zero_rate(*tau) - y).powi(2))
            .sum::<f64>()
    };
    
    // DE parameters
    let population_size = 50;
    let mutation_factor = 0.8;
    let crossover_prob = 0.9;
    let max_generations = 500;
    
    let result = differential_evolution(
        objective, 
        bounds, 
        population_size,
        mutation_factor,
        crossover_prob,
        max_generations,
    );
    
    Svensson::from_params(&result)
}
```

### Two-Stage Fitting (Recommended)

For Svensson, first fit Nelson-Siegel, then extend:

```rust
fn fit_svensson_two_stage(yields: &[(f64, f64)]) -> Svensson {
    // Stage 1: Fit Nelson-Siegel
    let ns_initial = initial_guess_from_yields(yields);
    let ns = fit_nelson_siegel_lm(yields, ns_initial);
    
    // Stage 2: Extend to Svensson
    let sv_initial = Svensson {
        beta0: ns.beta0,
        beta1: ns.beta1,
        beta2: ns.beta2,
        beta3: 0.0,  // Start with no second hump
        lambda1: ns.lambda,
        lambda2: ns.lambda * 2.0,  // Different decay
    };
    
    fit_svensson_lm(yields, sv_initial)
}
```

## Model Selection

### Choosing Between NS and NSS

| Criterion | Use NS | Use Svensson |
|-----------|--------|--------------|
| Data points | < 10 | > 10 |
| Curve shape | Simple | Double hump |
| Maturity range | < 10Y | > 20Y |
| Stability | Higher priority | Better fit priority |

### Goodness of Fit Metrics

```rust
fn fit_statistics(model: &impl YieldCurve, data: &[(f64, f64)]) -> FitStats {
    let n = data.len();
    let residuals: Vec<f64> = data.iter()
        .map(|(tau, y)| model.zero_rate(*tau) - y)
        .collect();
    
    let sse: f64 = residuals.iter().map(|r| r.powi(2)).sum();
    let rmse = (sse / n as f64).sqrt();
    
    let mean_y: f64 = data.iter().map(|(_, y)| y).sum::<f64>() / n as f64;
    let sst: f64 = data.iter().map(|(_, y)| (y - mean_y).powi(2)).sum();
    let r_squared = 1.0 - sse / sst;
    
    let max_error = residuals.iter()
        .map(|r| r.abs())
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(0.0);
    
    FitStats { rmse, r_squared, max_error }
}
```

### Central Bank Practices

| Central Bank | Model | Notes |
|--------------|-------|-------|
| Fed | Svensson | Published daily |
| ECB | Svensson | AAA-rated sovereigns |
| BoE | Spline + smoothing | Variable roughness penalty |
| Bundesbank | Svensson | Extended methodology |
| BoJ | Nelson-Siegel | Simpler approach |
