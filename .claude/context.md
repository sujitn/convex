# Convex - Fixed Income Analytics Library

## Project Overview

Convex is a high-performance, production-grade fixed income analytics library written in Rust. It provides comprehensive bond pricing, yield curve construction, and risk analytics capabilities matching Bloomberg YAS (Yield Analysis System) functionality for ALL major bond types and curve methodologies.

**Mission:** Production-accurate fixed income analytics with sub-microsecond performance.

## Performance Targets

| Operation | Target | Notes |
|-----------|--------|-------|
| Single bond pricing | < 1μs | All bond types |
| YTM calculation | < 1μs | Newton-Raphson convergence |
| Full YAS analysis | < 100μs | All metrics |
| Z-spread calculation | < 50μs | Brent's method |
| OAS calculation | < 10ms | Binomial tree |
| MBS pricing | < 100ms | With prepayment model |
| Curve bootstrap (50 pts) | < 100μs | Any interpolation |
| Portfolio (1000 bonds) | < 100ms | Parallel processing |

## Accuracy Targets (Production Critical)

### By Bond Type

| Bond Type | Yield Tolerance | Spread Tolerance | Price Tolerance | Validation Source |
|-----------|-----------------|------------------|-----------------|-------------------|
| US Treasury | ±0.0001% | ±0.05 bps | ±1/256 (32nds) | Bloomberg, Treasury Direct |
| Corporate IG | ±0.00001% | ±0.1 bps | ±0.0001 | Bloomberg YAS |
| Corporate HY | ±0.001% | ±0.5 bps | ±0.001 | Bloomberg YAS |
| Municipal | ±0.001% | ±0.5 bps | ±0.001 | Bloomberg, EMMA |
| Agency | ±0.0001% | ±0.1 bps | ±0.0001 | Bloomberg |
| MBS Pass-Through | ±0.01% | ±1.0 bps | ±0.01 | Bloomberg, eMBS |
| TIPS | ±0.001% real | N/A | ±0.001 | Bloomberg, Treasury |
| Convertible | ±0.1% value | N/A | ±0.1 | Bloomberg OVCV |
| UK Gilt | ±0.0001% | ±0.1 bps | ±0.0001 | Bloomberg |
| German Bund | ±0.0001% | ±0.1 bps | ±0.0001 | Bloomberg |

### By Curve Operation

| Operation | Tolerance | Notes |
|-----------|-----------|-------|
| Discount factor | ±1e-10 | Absolute |
| Zero rate | ±0.0001% | At pillar points |
| Interpolated rate | ±0.001% | Between pillars |
| Forward rate | ±0.001% | Instantaneous |
| Curve bump | Exact | Must preserve shape |

---

## Domain Knowledge

### 1. Bond Types Supported

#### Government Bonds

**US Treasuries:**
- T-Bills (4, 8, 13, 17, 26, 52 week) - Discount basis
- T-Notes (2, 3, 5, 7, 10 year) - Semi-annual coupon
- T-Bonds (20, 30 year) - Semi-annual coupon
- TIPS (5, 10, 30 year) - Inflation-linked
- FRNs (2 year) - Floating rate

**International Government:**
- UK Gilts (including Index-Linked)
- German Bunds, Bobls, Schatz
- French OATs (including OATi, OAT€i)
- JGBs (Japanese Government Bonds)
- Other G10 sovereigns

#### Corporate Bonds

**Investment Grade (AAA to BBB-):**
- Fixed rate bullets
- Make-whole callable
- Sinking fund
- Medium-term notes (MTN)

**High Yield (BB+ and below):**
- Discrete callable (e.g., 5NC2)
- PIK (Payment-in-Kind)
- Step-up coupons
- Covenant structures

**Special Structures:**
- Convertible bonds
- Exchangeable bonds
- Contingent convertibles (CoCos)
- Perpetual bonds (AT1, AT2)

#### Municipal Bonds

- General Obligation (GO)
- Revenue bonds
- Tax-exempt
- Taxable (Build America Bonds)
- AMT-subject
- Pre-refunded/Escrowed

#### Agency & Mortgage-Backed

**Agency Debentures:**
- FNMA, FHLMC, FHLB
- TVA, Farm Credit

**MBS Pass-Through:**
- GNMA (full faith & credit)
- FNMA, FHLMC (GSE)
- 15Y, 20Y, 30Y pools

**Structured (Future):**
- CMO tranches
- ABS
- CLOs

#### Floating Rate Notes

- SOFR-linked (daily compounded, term)
- EURIBOR-linked
- SONIA-linked
- Legacy LIBOR
- With caps/floors/collars

#### Money Market Instruments

- T-Bills (discount basis)
- Commercial Paper
- Certificates of Deposit
- Bankers' Acceptances
- Repo/Reverse Repo

---

### 2. Yield Curve Construction (Production Critical)

#### Curve Types

**Discount Curves:**
- OIS curves (SOFR, ESTR, SONIA, TONAR)
- Government curves (risk-free proxy)
- Repo curves

**Projection Curves:**
- Term rate curves (Term SOFR, EURIBOR)
- Basis-adjusted curves
- Legacy LIBOR curves (for legacy trades)

**Spread Curves:**
- Credit spread curves by rating
- Sector spread curves
- Issuer-specific curves

**Inflation Curves:**
- Breakeven inflation
- Real rate curves
- CPI/RPI projection

#### Input Instruments

**Money Market (Short End):**
```
Instrument          Typical Tenors       Day Count    Notes
─────────────────────────────────────────────────────────────
Overnight rate      O/N                  ACT/360      Direct input
Tom/Next            T/N                  ACT/360      
Deposits            1W, 2W, 1M-12M       ACT/360      Cash rates
FRAs                1x4, 2x5, 3x6, etc.  ACT/360      Forward starting
Futures             IMM dates            ACT/360      Convexity adjust
```

**Capital Market (Long End):**
```
Instrument          Typical Tenors       Day Count    Notes
─────────────────────────────────────────────────────────────
Interest Rate Swaps 2Y-50Y               ACT/360 vs   Par swap rates
                                         30/360
Basis Swaps         2Y-30Y               Various      For multi-curve
OIS Swaps           1W-30Y               ACT/360      Discounting curve
Government Bonds    On-the-run           ACT/ACT      Fitted curve
```

#### Interpolation Methods (All Must Be Supported)

**On Zero Rates:**
```rust
pub enum ZeroRateInterpolation {
    /// Simple linear interpolation - Fast, may have negative forwards
    Linear,
    
    /// Log-linear - Ensures positive forwards
    LogLinear,
    
    /// Cubic spline - Smooth, may oscillate
    CubicSpline,
    
    /// Monotone convex - Positive forwards, no oscillation
    MonotoneConvex,
    
    /// Bessel/Hermite - Smooth with tension control
    BesselHermite { tension: f64 },
}
```

**On Discount Factors:**
```rust
pub enum DiscountFactorInterpolation {
    /// Log-linear on DF (linear on continuously compounded zero)
    LogLinear,
    
    /// Cubic spline on log(DF)
    LogCubicSpline,
    
    /// Raw interpolation on DF (not recommended)
    Linear,
}
```

**On Forward Rates:**
```rust
pub enum ForwardRateInterpolation {
    /// Flat forward between pillars
    Flat,
    
    /// Linear forward interpolation
    Linear,
    
    /// Cubic spline on forwards
    CubicSpline,
}
```

**Parametric Models:**
```rust
pub enum ParametricCurve {
    /// Nelson-Siegel: y(t) = β₀ + β₁((1-e^(-t/τ))/(t/τ)) + β₂((1-e^(-t/τ))/(t/τ) - e^(-t/τ))
    NelsonSiegel {
        beta0: f64,  // Long-term level
        beta1: f64,  // Short-term component
        beta2: f64,  // Medium-term hump
        tau: f64,    // Decay factor
    },
    
    /// Svensson: Nelson-Siegel + additional hump
    Svensson {
        beta0: f64,
        beta1: f64,
        beta2: f64,
        beta3: f64,  // Second hump
        tau1: f64,
        tau2: f64,
    },
    
    /// Björk-Christensen (alternative parameterization)
    BjorkChristensen { /* params */ },
}
```

#### Extrapolation Methods (Critical for Long-Dated)

```rust
pub enum Extrapolation {
    /// No extrapolation - error if outside range
    None,
    
    /// Flat from last point
    Flat,
    
    /// Linear continuation of slope
    Linear,
    
    /// Smith-Wilson (regulatory standard for insurance)
    SmithWilson {
        ultimate_forward_rate: f64,  // e.g., 4.2% for EUR
        convergence_speed: f64,      // Alpha parameter
        last_liquid_point: f64,      // e.g., 20Y for EUR
    },
    
    /// Nelson-Siegel asymptotic
    NelsonSiegelAsymptotic { long_term_rate: f64 },
}
```

#### Bootstrap Algorithm

```rust
/// Production bootstrap with multiple solver options
pub struct CurveBootstrapper {
    /// Interpolation method during bootstrap
    pub interpolation: InterpolationMethod,
    
    /// Extrapolation for long end
    pub extrapolation: Extrapolation,
    
    /// Solver for each instrument
    pub solver: BootstrapSolver,
    
    /// Convergence tolerance
    pub tolerance: f64,  // Default: 1e-12
    
    /// Maximum iterations per instrument
    pub max_iterations: u32,  // Default: 100
    
    /// Jacobian method for global solve
    pub jacobian: JacobianMethod,
}

pub enum BootstrapSolver {
    /// Sequential solve - instrument by instrument
    Sequential {
        local_solver: LocalSolver,
    },
    
    /// Global solve - all instruments simultaneously
    Global {
        optimizer: GlobalOptimizer,
    },
    
    /// Hybrid - sequential with global refinement
    Hybrid {
        local_solver: LocalSolver,
        global_optimizer: GlobalOptimizer,
    },
}

pub enum LocalSolver {
    NewtonRaphson { tolerance: f64, max_iter: u32 },
    Brent { tolerance: f64, max_iter: u32 },
    Secant { tolerance: f64, max_iter: u32 },
}

pub enum GlobalOptimizer {
    LevenbergMarquardt { tolerance: f64 },
    GaussNewton { tolerance: f64 },
    BFGS { tolerance: f64 },
}
```

#### Multi-Curve Framework (Post-2008)

```rust
/// Multi-curve environment for modern pricing
pub struct MultiCurveEnvironment {
    /// Discounting curve (typically OIS)
    pub discount_curve: Curve,
    
    /// Projection curves by index
    pub projection_curves: HashMap<RateIndex, Curve>,
    
    /// Basis spread curves
    pub basis_curves: HashMap<(RateIndex, RateIndex), Curve>,
    
    /// FX curves for cross-currency
    pub fx_curves: HashMap<CurrencyPair, Curve>,
}

pub enum RateIndex {
    // Overnight rates
    SOFR, ESTR, SONIA, TONAR,
    
    // Term rates
    TermSOFR { tenor: Tenor },
    EURIBOR { tenor: Tenor },
    TIBOR { tenor: Tenor },
    
    // Legacy (for existing trades)
    LIBOR { currency: Currency, tenor: Tenor },
}
```

#### Curve Validation (Production Requirement)

```rust
pub struct CurveValidation {
    /// Repricing tolerance for input instruments
    pub reprice_tolerance: f64,  // e.g., 1e-8
    
    /// Forward rate positivity check
    pub check_positive_forwards: bool,
    
    /// Smoothness metric (second derivative bound)
    pub max_forward_curvature: Option<f64>,
    
    /// Arbitrage checks
    pub check_no_arbitrage: bool,
}
```

---

### 3. Day Count Conventions (Must Match Bloomberg Exactly)

#### ACT Family

| Convention | Formula | Usage | Special Rules |
|------------|---------|-------|---------------|
| ACT/360 | actual_days / 360 | Money market, EUR swaps | None |
| ACT/365F | actual_days / 365 | UK Gilts, AUD | Fixed 365 |
| ACT/365L | actual_days / 365 or 366 | ISDA | Leap year aware |
| ACT/ACT ICMA | actual / (freq × period) | Government bonds | Period-based |
| ACT/ACT ISDA | day1/year1 + day2/year2 | Swaps | Year-based split |
| ACT/ACT AFB | actual_days / 366 if leap | French | Feb 29 rule |

#### 30 Family

| Convention | Usage | D1 Rules | D2 Rules |
|------------|-------|----------|----------|
| 30/360 US | US Corporate | If D1=31→30; If D1=EOM Feb→30 | If D2=31 and D1≥30→30 |
| 30E/360 | Eurobonds | If D1=31→30 | If D2=31→30 |
| 30E/360 ISDA | Swaps | If D1=EOM→30 | If D2=EOM and not maturity→30 |
| 30/360 German | German market | If D1=31→30; If D1=Feb EOM→30 | If D2=31→30; If D2=Feb EOM→30 |

#### Implementation Critical Points

```rust
/// 30/360 US - Bloomberg exact implementation
pub fn thirty_360_us(d1: Date, d2: Date) -> Decimal {
    let (y1, m1, mut day1) = (d1.year(), d1.month(), d1.day());
    let (y2, m2, mut day2) = (d2.year(), d2.month(), d2.day());
    
    // Rule 1: If D1 is last day of February, change D1 to 30
    if is_last_day_of_february(d1) {
        day1 = 30;
    }
    // Rule 2: If D1 is 31, change D1 to 30
    else if day1 == 31 {
        day1 = 30;
    }
    
    // Rule 3: If D2 is last day of Feb AND D1 was last day of Feb, change D2 to 30
    if is_last_day_of_february(d2) && is_last_day_of_february(d1) {
        day2 = 30;
    }
    // Rule 4: If D2 is 31 AND D1 is now 30, change D2 to 30
    else if day2 == 31 && day1 == 30 {
        day2 = 30;
    }
    
    let days = 360 * (y2 - y1) + 30 * (m2 as i32 - m1 as i32) + (day2 as i32 - day1 as i32);
    Decimal::from(days) / dec!(360)
}
```

---

### 4. Settlement Conventions by Market

| Market | Settlement | Calendar | Day Count | Ex-Dividend |
|--------|------------|----------|-----------|-------------|
| US Treasury | T+1 | US Gov | ACT/ACT ICMA | None |
| US Corporate | T+2 | SIFMA | 30/360 US | None |
| US Municipal | T+1 | SIFMA | 30/360 US | None |
| US Agency | T+2 | SIFMA | 30/360 US | None |
| UK Gilt | T+1 | UK | ACT/ACT | 7 business days |
| German Bund | T+2 | TARGET2 | ACT/ACT | None |
| French OAT | T+2 | TARGET2 | ACT/ACT | None |
| JGB | T+2 | Japan | ACT/365F | None |
| Eurobond | T+2 | TARGET2 | 30E/360 | None |

---

### 5. Pricing Methodologies

#### Yield-to-Maturity (Newton-Raphson)

```
Convergence Criteria (Bloomberg Standard):
- Tolerance: 1e-10 (10 decimal places)
- Max iterations: 100
- Initial guess: Current yield or coupon rate
- Fallback: Brent's method if Newton fails
```

#### Sequential Roll-Forward (Money Market)

**Critical for Bloomberg Match on Short-Dated Bonds:**
```
For bonds < 1 year to maturity:
1. Start at settlement date
2. Project to each cash flow date sequentially
3. Apply appropriate day count at each step
4. Compound through to final cash flow
5. Solve for yield that equates to dirty price
```

#### Option-Adjusted Spread (Callable/Putable)

```
OAS Methodology:
- Interest rate model: Hull-White or BDT
- Tree: Binomial (100+ steps) or Trinomial
- Calibration: To ATM swaption vols
- Exercise: Optimal at each node
- Convergence: OAS tolerance 0.1 bps
```

---

### 6. Risk Metrics

#### Duration

| Type | Formula | Use Case |
|------|---------|----------|
| Macaulay | Σ(t × PV) / Price | Academic, theoretical |
| Modified | Macaulay / (1 + y/f) | Price sensitivity |
| Effective | (P₋ - P₊) / (2 × P₀ × Δy) | Bonds with options |
| Key Rate | ∂P/∂yₖ for each pillar | Curve risk |
| Spread | ∂P/∂spread | Credit risk |

#### Convexity

| Type | Formula | Notes |
|------|---------|-------|
| Analytical | Σ(t(t+1/f) × PV) / (P × (1+y/f)²) | Closed form |
| Effective | (P₋ + P₊ - 2P₀) / (P₀ × Δy²) | For optionality |

#### DV01 / PV01

```
DV01 = Modified Duration × Dirty Price × Face Value × 0.0001
     = -(∂P/∂y) × 0.0001
```

---

### 7. Spread Analytics

| Spread | Definition | Use Case |
|--------|------------|----------|
| G-Spread | YTM - Interpolated Gov Yield | Quick comparison |
| I-Spread | YTM - Interpolated Swap Rate | Swap-based |
| Z-Spread | Constant spread over spot curve | Credit analysis |
| ASW | Asset swap spread (par-par or proceeds) | Relative value |
| OAS | Spread over tree with option exercise | Callable bonds |
| CDS Basis | Bond spread - CDS spread | Arbitrage signal |
| Discount Margin | Spread over index for FRN | Floaters |

---

### 8. Bloomberg Validation Reference Bonds

#### Primary: Boeing 7.5% 06/15/2025 (Corporate IG)

```
CUSIP: 097023AH7
Settlement: 04/29/2020
Price: 110.503

Expected Values:
├── Street Convention: 4.905895%  (±0.00001%)
├── True Yield: 4.903264%         (±0.00001%)
├── Current Yield: 6.561%         (±0.001%)
├── G-Spread: 448.5 bps           (±0.1 bps)
├── Z-Spread: 444.7 bps           (±0.1 bps)
├── Modified Duration: 4.209      (±0.001)
├── Convexity: 0.219              (±0.001)
├── Accrued Days: 134             (exact)
└── Accrued Interest: 26,986.11   (±0.01)
```

#### Secondary Validation Bonds

**US Treasury 10Y Note:**
```
CUSIP: 912828ZT6 (4.125% 11/15/2032)
Day Count: ACT/ACT ICMA
Price Quote: 32nds
Validation: Treasury Direct
```

**US Treasury Bill:**
```
Discount basis pricing
BEY conversion
Money market yields
```

**High Yield Callable:**
```
Structure: 5NC2
YTW across call schedule
OAS calculation
```

**Municipal GO:**
```
Tax-equivalent yield
De minimis validation
```

**MBS 30Y FNMA:**
```
Prepayment: PSA speeds
Yield table validation
Factor adjustment
```

**10Y TIPS:**
```
Real yield
Index ratio
Breakeven inflation
```

---

## Technical Architecture Principles

### Numerical Precision

```rust
// Use Decimal for all financial calculations
use rust_decimal::Decimal;

// Use f64 only for intermediate math where precision is not critical
// Always convert back to Decimal for final results

// Tolerances
const YIELD_TOLERANCE: Decimal = dec!(0.0000000001);    // 1e-10
const PRICE_TOLERANCE: Decimal = dec!(0.00000001);      // 1e-8
const SPREAD_TOLERANCE: Decimal = dec!(0.00000001);     // 1e-8 (0.0001 bps)
const DISCOUNT_FACTOR_TOLERANCE: f64 = 1e-12;
```

### Performance Optimization

1. **SIMD Vectorization**: For discount factor and PV calculations
2. **Zero-Copy Operations**: Minimize allocations in hot paths
3. **Cache Optimization**: Structure data for cache-friendly access
4. **Parallel Processing**: Rayon for portfolio and curve building
5. **LTO and PGO**: Enable link-time and profile-guided optimization

### Rust Best Practices

1. **Type Safety**: Newtypes for all domain concepts
2. **Error Handling**: Result types, never panic in library code
3. **Generic Programming**: Support different curve/bond types
4. **Zero-Cost Abstractions**: Trait-based design
5. **Memory Safety**: No unsafe unless documented and audited

---

## Project Structure

```
convex/
├── Cargo.toml                 # Workspace definition
├── crates/
│   ├── convex-core/          # Core types, day counts, calendars
│   ├── convex-math/          # Solvers, interpolation, optimization
│   ├── convex-curves/        # Yield curve construction
│   ├── convex-bonds/         # Bond instruments and pricing
│   ├── convex-spreads/       # Spread calculations
│   ├── convex-risk/          # Risk analytics
│   ├── convex-yas/           # Bloomberg YAS replication
│   └── convex-ffi/           # FFI for language bindings
├── bindings/
│   ├── python/               # PyO3 bindings
│   ├── java/                 # JNI bindings
│   └── excel/                # XLL plugin
├── tests/
│   ├── bloomberg_validation/ # Bloomberg-verified test cases
│   ├── curve_validation/     # Curve accuracy tests
│   └── scenarios/            # Production scenarios
└── benches/                  # Performance benchmarks
```

---

## References

### Industry Standards
- Bloomberg YAS Function Reference
- ISDA Definitions (day counts, conventions)
- ICMA Bond Calculation Rules
- ARRC SOFR Conventions
- EIOPA Smith-Wilson Technical Specification

### Academic
- *Fixed Income Securities* - Bruce Tuckman
- *Interest Rate Models* - Brigo & Mercurio
- *The Handbook of Fixed Income Securities* - Fabozzi
- Hagan, P. "Interpolation Methods for Curve Construction"
- Le Floc'h, F. "Monotone Convex Interpolation"
