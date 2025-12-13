# Bond Risk Metrics - Complete Formulas

## Duration Formulas

### Macaulay Duration
```
D_mac = Σ(t_i × CF_i × DF_i) / P

Where:
  t_i   = time to cash flow i (years)
  CF_i  = cash flow at time i
  DF_i  = discount factor = 1/(1+y/n)^(n×t_i)
  P     = dirty price = Σ(CF_i × DF_i)
  n     = compounding frequency (2 for semi-annual)
```

### Modified Duration
```
D_mod = D_mac / (1 + y/n)

Alternatively (finite difference):
D_mod = (P₋ - P₊) / (2 × P₀ × Δy)
```

### Dollar Duration
```
Dollar_Duration = D_mod × P
DV01 = Dollar_Duration / 10000 = D_mod × P × 0.0001
```

### Effective Duration (OAS-based)
```
D_eff = (P₋_OAS - P₊_OAS) / (2 × P₀ × Δy)

Where:
  P₋_OAS = price with curve shifted down, OAS constant
  P₊_OAS = price with curve shifted up, OAS constant
  Δy     = shift size (typically 25-50bp for optioned bonds)
```

## Convexity Formulas

### Standard Convexity (Analytical)
```
C = [1/(P × (1+y/n)²)] × Σ[CF_i × t_i × (t_i + 1/n) × DF_i]
```

### Standard Convexity (Finite Difference)
```
C = (P₊ + P₋ - 2×P₀) / (P₀ × Δy²)
```

### Taylor Expansion Price Approximation
```
ΔP/P ≈ -D_mod × Δy + 0.5 × C × Δy²

For large yield changes, add higher order terms or use full repricing.
```

### Effective Convexity
```
C_eff = (P₊_OAS + P₋_OAS - 2×P₀) / (P₀ × Δy²)

Same as standard but with OAS-constant pricing.
```

## Key Rate Duration

### Triangular Bump Function
```
For key rate at tenor T_k with adjacent tenors T_{k-1} and T_{k+1}:

Δr(t) = 
  h × (t - T_{k-1})/(T_k - T_{k-1})     for T_{k-1} ≤ t < T_k
  h × (T_{k+1} - t)/(T_{k+1} - T_k)     for T_k ≤ t < T_{k+1}
  0                                       otherwise

Where h = bump size (typically 1bp)
```

### Key Rate Duration at Tenor k
```
KRD_k = -(1/P) × (∂P/∂r_k)
      ≈ (P₋_k - P₊_k) / (2 × P × h)
```

### Validation
```
Σ KRD_i ≈ D_eff (within convexity error)
```

## Spread Metrics

### Z-Spread
```
Solve for Z in:
P_dirty = Σ[CF_i / (1 + s_i + Z)^t_i]

Where s_i = spot rate at time t_i
```

### Asset Swap Spread (Par-Par)
```
ASW = (100 - P_dirty + AI) / Annuity + Coupon - Swap_Rate

Where:
  AI = accrued interest
  Annuity = PV01 of swap fixed leg
```

### I-Spread
```
I_spread = YTM - Swap_Rate(maturity)
```

### G-Spread
```
G_spread = YTM - Govt_Yield(maturity)
```

### OAS (Simplified)
```
Z_spread = OAS + Option_Cost

Option_Cost > 0 for callable
Option_Cost < 0 for putable
```

### CS01 (Credit Spread 01)
```
CS01 = -∂P/∂s × 0.0001
     = Spread_Duration × P × 0.0001
```

## Floating Rate Note Metrics

### Discount Margin
```
Solve for DM in:
P = Σ[(Index + QM) / m] / [(1 + (Index + DM)/m)^i] + 100/[(1 + (Index + DM)/m)^N]

Where:
  QM = quoted margin
  m  = payments per year
  N  = number of remaining payments
```

### Interest Rate Duration
```
D_rate ≈ (T_reset) / (1 + r×T_reset)

Where T_reset = time to next reset date (years)
```

### Spread Duration
```
D_spread ≈ Maturity (in years)

For FRN, spread duration >> interest rate duration
```

### DM01
```
DM01 = ∂P/∂DM × 0.0001
```

## Inflation-Linked Bond Metrics

### Real Duration
```
D_real = -(1/P) × (∂P/∂r_real)

Where r_real = real yield
```

### Breakeven Inflation Duration (BEI01)
```
BEI01 = ∂P/∂BEI × 0.0001

Where BEI = breakeven inflation rate
```

### Fisher Equation Decomposition
```
(1 + y_nominal) = (1 + y_real) × (1 + BEI)

Approximately: y_nominal ≈ y_real + BEI
```

### Inflation DV01
```
Inflation_DV01 = ∂P/∂π × 0.0001

Where π = inflation expectation
```

## Callable/Putable Bond Metrics

### Vega (Rate Volatility Sensitivity)
```
Vega = ∂V/∂σ

Where σ = rate volatility parameter
```

### Option-Adjusted Duration (One-Sided)
```
D_up = (P₀ - P₊) / (P₀ × Δy)      // sensitivity to rate increase
D_down = (P₋ - P₀) / (P₀ × Δy)    // sensitivity to rate decrease

For callable bonds near call: D_down < D_up
```

### Option Value
```
P_callable = P_straight - Call_Value
P_putable = P_straight + Put_Value
```

## Portfolio Metrics

### Portfolio DV01
```
DV01_portfolio = Σ(w_i × DV01_i)
               = Σ(MV_i × D_mod_i × 0.0001)
```

### Contribution to Duration
```
Contrib_i = (MV_i × D_i) / (Σ MV_j × D_j)
```

### Portfolio Convexity
```
C_portfolio = Σ(w_i × C_i × D_i²) / D_portfolio²

Note: Not simply weighted average due to duration weighting
```

## Jacobian Transformations

### Zero to Par Rate Sensitivity
```
∂P/∂y_par = ∂P/∂r_zero × J^(-1)

Where J = Jacobian matrix (∂r_zero/∂y_par)
```

### Forward Rate Sensitivity
```
For forward rate f(t₁,t₂):
∂P/∂f = (t₂ - t₁) × DF(t₂) × Σ[CF_i × DF_i] for t_i > t₂
```

## Day Count Factors

### 30/360 US
```
DCF = (360×(Y₂-Y₁) + 30×(M₂-M₁) + (D₂-D₁)) / 360

With adjustments:
  If D₁ = 31, set D₁ = 30
  If D₁ = 30 or D₁ was 31, and D₂ = 31, set D₂ = 30
```

### ACT/ACT ICMA
```
DCF = Days_in_period / (Frequency × Days_in_coupon_period)
```

### ACT/360
```
DCF = Actual_days / 360
```

### ACT/365 Fixed
```
DCF = Actual_days / 365
```

## Accrued Interest

### Standard Calculation
```
AI = (Coupon / Frequency) × (Days_since_last_coupon / Days_in_period)

Use appropriate day count convention for bond type.
```

### Dirty vs Clean Price
```
Dirty_Price = Clean_Price + Accrued_Interest
```
