# Bond Type Specific Calculations

## Fixed-Rate Vanilla Bonds

### Cash Flow Pattern
```
CF(t) = C/n    for t = 1/n, 2/n, ..., (T-1/n)
CF(T) = C/n + 100

Where:
  C = annual coupon rate (%)
  n = coupon frequency
  T = maturity (years)
```

### Duration/Convexity
Use standard modified duration and convexity formulas. No special handling required.

### Implementation Notes
- Most straightforward case
- All cash flows are known at issuance
- Duration increases with maturity, decreases with coupon

## Callable Bonds

### Price Decomposition
```
P_callable = P_straight - Call_Option_Value
```

### Effective Duration (OAS Method)

1. **Calibrate OAS** to market price using interest rate tree
2. **Shift curve** up/down by Δy (25-50bp)
3. **Rebuild tree** at shifted curve
4. **Price with same OAS** at each shifted curve
5. **Calculate** effective duration from prices

```rust
fn effective_duration_callable(
    bond: &CallableBond,
    curve: &YieldCurve,
    market_price: f64,
    shift_bp: f64,
) -> f64 {
    let oas = solve_oas(bond, curve, market_price);
    let shift = shift_bp / 10_000.0;
    
    let curve_up = curve.parallel_shift(shift);
    let curve_down = curve.parallel_shift(-shift);
    
    let price_up = price_with_oas(bond, &curve_up, oas);
    let price_down = price_with_oas(bond, &curve_down, oas);
    
    (price_down - price_up) / (2.0 * market_price * shift)
}
```

### Negative Convexity Detection
```
Convexity_up = (P_0 - P_up) / (P_0 × Δy) - D_eff
Convexity_down = (P_down - P_0) / (P_0 × Δy) - D_eff

If Convexity_down < Convexity_up significantly → Negative convexity
```

### One-Sided Durations
Report separate up/down sensitivities when option is near-the-money:
```
D_up = (P_0 - P_up) / (P_0 × Δy)
D_down = (P_down - P_0) / (P_0 × Δy)
```

### Vega
```
Vega = ∂P/∂σ

Calculate by bumping rate volatility parameter in tree model.
Callable bonds have negative vega (higher vol → lower price).
```

### Call Schedule Handling
- **Make-whole calls:** Treasury yield + fixed spread (no optionality before first par call)
- **Par calls:** Callable at 100 on specific dates
- **Step-down calls:** Call price decreases over time
- **Continuous American:** Callable any time after lockout

## Putable Bonds

### Price Decomposition
```
P_putable = P_straight + Put_Option_Value
```

### Effective Duration
Same OAS methodology as callable, but:
- Put value increases as rates rise
- Effective duration < modified duration when put is valuable
- Positive vega (higher vol → higher price)

### Implementation Note
Put bonds exhibit positive convexity that increases near put exercise boundary.

## Floating Rate Notes (FRNs)

### Cash Flow Pattern
```
CF(t) = (Index(t) + Quoted_Margin) × Notional / Frequency + Notional (at maturity)
```

### Discount Margin (DM)

Solve iteratively for DM:
```
P = Σ[(Index + QM) × N/m] / [(1 + (Index + DM)/m)^i] + N/[(1 + (Index + DM)/m)^n]
```

### Interest Rate Duration
```
D_rate ≈ T_reset / (1 + r × T_reset)

Where T_reset = time to next coupon reset (years)
```

FRN interest rate duration is typically < 0.5 years (time to next reset).

### Spread Duration
```
D_spread ≈ Modified_Duration_of_equivalent_fixed_bond ≈ Maturity
```

### DM01
```
DM01 = -∂P/∂DM × 0.0001
```

### Reset Risk
Between reset dates, FRN has exposure to difference between locked-in rate and current market rate:
```
Reset_Risk_Exposure = (Current_Index - Locked_Index) × T_to_next_reset × Notional
```

### Index Curves
Support multiple reference rates:
- SOFR (US)
- EURIBOR (EUR)
- SONIA (GBP)
- TONA (JPY)

## Inflation-Linked Bonds (TIPS)

### Principal Adjustment
```
Adjusted_Principal = Par × Index_Ratio
Index_Ratio = CPI_ref(settle) / CPI_ref(issue)
```

### Cash Flow Pattern
```
CF(t) = (C/n) × Adjusted_Principal(t)
CF(T) = (C/n) × Adjusted_Principal(T) + max(Adjusted_Principal(T), Par)
```

Deflation floor: Principal cannot fall below par at maturity.

### Real Yield
Solve for y_real in:
```
P × Index_Ratio = Σ[CF(t) / (1 + y_real)^t]
```

### Real Duration
```
D_real = -(1/P) × ∂P/∂y_real
```

Calculated using finite differences on real yield.

### Breakeven Inflation (BEI)
```
BEI = Nominal_Yield - Real_Yield - Inflation_Risk_Premium

Simplified: BEI ≈ Nominal_Yield - Real_Yield
```

### BEI01
```
BEI01 = ∂P/∂BEI × 0.0001
```

For TIPS, BEI01 ≈ DV01 of equivalent nominal bond.

### Indexation Lag

| Market | Lag |
|--------|-----|
| US TIPS | 2-3 months |
| UK Linkers | 3 months |
| EUR Linkers | 3 months |

Lag creates basis risk for precise inflation hedging.

### Seasonality
CPI has seasonal patterns. For short-dated inflation exposure, consider seasonal adjustment or use seasonally-adjusted CPI.

## Zero-Coupon Bonds

### Cash Flow
```
CF(T) = 100 (at maturity only)
```

### Macaulay Duration
```
D_mac = T (equals maturity)
```

### Modified Duration
```
D_mod = T / (1 + y/n)
```

### Convexity
```
C = T × (T + 1/n) / (1 + y/n)²
```

Higher convexity than coupon bonds of same maturity.

## Sinking Fund Bonds

### Weighted Average Life (WAL)
```
WAL = Σ(t_i × P_i) / Σ(P_i)

Where:
  t_i = time to each principal payment
  P_i = principal amount at time i
```

### Duration Adjustment
Use WAL instead of maturity for duration estimates:
```
D_estimated ≈ D_similar_bullet(WAL)
```

### Delivery Option
Issuer can satisfy sinking fund by:
1. Open market purchases (if trading below par)
2. Calling at par

Creates uncertainty in actual average life.

### Implementation
Model as portfolio of bullets maturing at each sinking fund date, adjusted for delivery option.

## Amortizing Bonds

### Cash Flow Pattern
```
CF(t) = Interest(t) + Principal(t)

Interest(t) = Outstanding_Principal(t) × r / n
Principal(t) = Scheduled_Amortization(t)
```

### Weighted Average Life
```
WAL = Σ(t_i × Principal_Payment_i) / Total_Principal
```

### Duration
Use present value weighted approach:
```
D = Σ(t_i × PV_i) / Σ(PV_i)
```

Where PV_i includes both interest and principal components.

## Perpetual Bonds

### Price Formula
```
P = C / y

Where C = annual coupon (in $), y = yield (decimal)
```

### Duration
```
D_mac = (1 + y) / y
D_mod = 1 / y
```

### Convexity
```
C = 2 / y²
```

### Notes
- Very high duration (often 20+ years)
- Most perpetuals have call features - treat as callable
- Regulatory capital treatment affects pricing for bank perpetuals

## Convertible Bonds

### Price Decomposition
```
P_convertible = P_straight + Conversion_Option_Value
```

### Delta
```
Delta = ∂P/∂S

Where S = underlying stock price
```

### Bond-like vs Equity-like
```
If P_convertible >> Conversion_Value → Bond-like (high duration, low delta)
If P_convertible ≈ Conversion_Value → Equity-like (low duration, high delta)
```

### Duration
Effective duration varies with stock price:
- Deep out-of-money: Duration ≈ straight bond duration
- At-the-money: Duration < straight bond duration
- Deep in-the-money: Duration → 0

### Implementation
Full convertible pricing requires:
- Interest rate model for bond floor
- Equity model for conversion option
- Credit model for issuer default
- Correlation between rate, equity, credit
