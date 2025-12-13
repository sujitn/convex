# OIS Discount Curve Construction

## Table of Contents
- [Core Bootstrapping Equation](#core-bootstrapping-equation)
- [Overnight Rate Compounding](#overnight-rate-compounding)
- [Iterative Bootstrap Algorithm](#iterative-bootstrap-algorithm)
- [Global Optimization](#global-optimization)
- [Meeting-Date Construction](#meeting-date-construction)
- [Turn-of-Year Effects](#turn-of-year-effects)

## Core Bootstrapping Equation

OIS swaps exchange fixed rate for compounded overnight. Setting fixed = floating PV:

```
s_i × Σⱼ D(0,Tⱼ) × τⱼ = 1 - D(0,Tᵢ)
```

The RHS collapses via telescoping: compounded overnight from 0 to T equals `D(0,0)/D(0,T) - 1 = 1/D(0,T) - 1`.

**Iterative solution:**
```
D(0,Tᵢ) = [1 - sᵢ × Σⱼ₌₁ⁱ⁻¹ D(0,Tⱼ) × τⱼ] / [1 + sᵢ × τᵢ]
```

## Overnight Rate Compounding

### SOFR (ACT/360)
```
CompoundedRate = [∏ᵢ(1 + rᵢ × nᵢ/360) - 1] × (360/d)
```
Where:
- `rᵢ` = daily SOFR fixing
- `nᵢ` = calendar days rate applies (1 weekday, 3 weekend)
- `d` = total calendar days in period

### SONIA (ACT/365)
```
CompoundedRate = [∏ᵢ(1 + rᵢ × nᵢ/365) - 1] × (365/d)
```

### €STR (ACT/360)
Same formula as SOFR with ACT/360.

**Implementation:**
```rust
fn compound_overnight(
    fixings: &[(Date, f64)],
    start: Date,
    end: Date,
    day_count: DayCount,
) -> f64 {
    let mut accrued = 1.0;
    let base = day_count.year_fraction_base(); // 360 or 365
    
    for (date, rate) in fixings.iter() {
        if *date >= start && *date < end {
            let days = calendar_days_to_next(date);
            accrued *= 1.0 + rate * days as f64 / base;
        }
    }
    
    let total_days = (end - start).days() as f64;
    (accrued - 1.0) * base / total_days
}
```

## Iterative Bootstrap Algorithm

```rust
fn bootstrap_ois_curve(
    instruments: Vec<OISInstrument>,
    interpolation: InterpolationMethod,
) -> Result<Curve, CurveError> {
    // Sort by maturity
    let sorted = instruments.sorted_by(|a, b| a.maturity.cmp(&b.maturity));
    
    // Initialize with D(0,0) = 1
    let mut nodes = vec![CurveNode { time: 0.0, df: 1.0 }];
    
    for inst in sorted {
        let df = match inst.instrument_type {
            // Direct solve for standard OIS
            InstrumentType::OISSwap => {
                direct_solve_ois(&inst, &nodes)
            }
            // Brent solver when interpolation needed
            InstrumentType::OISFuture | InstrumentType::Meeting => {
                brent_solve_ois(&inst, &nodes, interpolation, 1e-12, 100)
            }
        };
        
        nodes.push(CurveNode { 
            time: inst.maturity_years(), 
            df 
        });
    }
    
    Ok(Curve::new(nodes, interpolation))
}

fn direct_solve_ois(inst: &OISInstrument, prior: &[CurveNode]) -> f64 {
    let rate = inst.quote;
    let tau_n = inst.accrual_fraction();
    
    // Sum of previous accruals
    let sum_prev: f64 = prior.iter()
        .skip(1)  // skip D(0,0)
        .zip(inst.payment_schedule())
        .map(|(node, tau)| node.df * tau)
        .sum();
    
    (1.0 - rate * sum_prev) / (1.0 + rate * tau_n)
}

fn brent_solve_ois(
    inst: &OISInstrument,
    prior: &[CurveNode],
    interp: InterpolationMethod,
    tol: f64,
    max_iter: usize,
) -> f64 {
    let objective = |df_target: f64| {
        let temp_curve = build_temp_curve(prior, inst.maturity_years(), df_target, interp);
        let implied = compute_par_rate(&temp_curve, inst);
        implied - inst.quote
    };
    
    // Bracket: discount factor must be in (0, 1) for positive rates
    brent_solver(objective, 0.001, 1.5, tol, max_iter)
}
```

### Brent Solver Implementation

Preferred over Newton-Raphson: guaranteed convergence via bracketing, ~1.6 convergence rate, no derivatives needed.

```rust
fn brent_solver<F>(f: F, a: f64, b: f64, tol: f64, max_iter: usize) -> f64 
where F: Fn(f64) -> f64 
{
    let mut a = a;
    let mut b = b;
    let mut fa = f(a);
    let mut fb = f(b);
    
    assert!(fa * fb < 0.0, "Root not bracketed");
    
    if fa.abs() < fb.abs() {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut fa, &mut fb);
    }
    
    let mut c = a;
    let mut fc = fa;
    let mut d = b - a;
    let mut e = d;
    
    for _ in 0..max_iter {
        if fb.abs() < tol {
            return b;
        }
        
        if fa != fc && fb != fc {
            // Inverse quadratic interpolation
            let s = a * fb * fc / ((fa - fb) * (fa - fc))
                  + b * fa * fc / ((fb - fa) * (fb - fc))
                  + c * fa * fb / ((fc - fa) * (fc - fb));
            // ... bisection fallback logic
        }
        // ... update brackets
    }
    b
}
```

## Global Optimization

Use when instruments overlap or smoothness constraints required.

**Objective function:**
```
min_x Σᵢ wᵢ × (model_priceᵢ(x) - market_priceᵢ)² + λ × R(x)
```

Where `R(x)` is regularization (e.g., forward curvature penalty).

### Levenberg-Marquardt

Interpolates gradient descent ↔ Gauss-Newton:

```
(JᵀJ + λ×diag(JᵀJ)) δ = Jᵀr
```

```rust
fn levenberg_marquardt(
    instruments: &[Instrument],
    initial_guess: Vec<f64>,
    lambda: f64,
    tol: f64,
) -> Vec<f64> {
    let mut x = initial_guess;
    let mut lambda = lambda;
    
    loop {
        let r = compute_residuals(instruments, &x);
        let J = compute_jacobian(instruments, &x);
        
        // Normal equations with damping
        let JtJ = J.transpose() * &J;
        let damped = &JtJ + lambda * Matrix::diag(&JtJ.diagonal());
        let delta = damped.solve(&(J.transpose() * &r));
        
        let x_new = &x - &delta;
        let r_new = compute_residuals(instruments, &x_new);
        
        if r_new.norm() < r.norm() {
            // Accept step, reduce damping
            x = x_new;
            lambda *= 0.1;
            if delta.norm() < tol { break; }
        } else {
            // Reject step, increase damping
            lambda *= 10.0;
        }
    }
    x
}
```

## Meeting-Date Construction

For short-end accuracy, model overnight rates as step functions at central bank meeting dates.

```
f(t) = fₖ  for t ∈ [meetingₖ, meetingₖ₊₁)
```

**CME Term SOFR Methodology:**

Optimize overnight rate levels between FOMC dates to match futures:

```rust
struct MeetingDateCurve {
    meetings: Vec<Date>,
    rates: Vec<f64>,  // Rate for each inter-meeting period
}

fn calibrate_meeting_dates(
    futures: &[SOFRFuture],
    fomc_dates: &[Date],
) -> MeetingDateCurve {
    // Objective: match SR1 and SR3 futures prices
    // Regularization: minimize jump sizes
    // λ × Σ(rₖ₊₁ - rₖ)²
}
```

## Turn-of-Year Effects

Year-end overnight rates spike 5-500+ bp due to:
- Balance sheet reporting pressures
- Regulatory capital requirements
- Reduced market liquidity

**Discrete jump model:**
```
f(τ) = f_base(τ) + J × 𝟙{τ ∈ turn_period}
```

**Implementation as discount factor adjustment:**
```rust
fn apply_turn_jump(
    curve: &mut Curve,
    turn_date: Date,
    jump_bp: f64,
) {
    let jump = jump_bp / 10000.0;
    let tau = day_count_to_next_business_day(turn_date);
    
    // Adjustment factor
    let B = 1.0 / (1.0 + jump * tau);
    
    // Apply to all discount factors after turn date
    for node in curve.nodes.iter_mut() {
        if node.date > turn_date {
            node.df *= B;
        }
    }
}
```

**Typical turn magnitudes:**
- Normal year: 5-20 bp
- Stressed conditions: 50-200 bp
- Crisis: 200-500+ bp

Calibrate from Dec/Jan OIS spreads or year-end futures basis.
