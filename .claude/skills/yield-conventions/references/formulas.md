# Yield Calculation Formulas

All formulas use conventions from the **bond itself** (day count, frequency).
YieldMethod only determines which formula to apply.

## Compounded Yield (Newton-Raphson)

### Price-Yield Equation
```
Dirty Price = Σ[k=1 to N] CF_k / (1 + y/f)^(f × t_k)
```
Where:
- `y` = annual yield (decimal)
- `f` = bond.coupon_frequency().periods_per_year()
- `t_k` = bond.day_count().year_fraction(settlement, cf_date)
- `CF_k` = cash flow at time k

### Newton-Raphson Iteration
```
y_{n+1} = y_n - f(y_n) / f'(y_n)

f(y) = Calculated Price - Market Price
f'(y) = -Duration × PV / (1 + y/f)
```

Parameters:
- Tolerance: 1e-10
- Max iterations: 100
- Initial guess: bond.coupon_rate() / price

### Implementation
```rust
fn solve_compounded<B: Bond>(
    bond: &B,
    settlement: Date,
    dirty_price: Decimal,
) -> Result<Yield, YieldError> {
    let day_count = bond.day_count();  // FROM BOND
    let freq = bond.coupon_frequency().periods_per_year() as f64;  // FROM BOND
    
    let cash_flows = bond.cash_flows_from(settlement);
    let mut y = initial_guess(bond, dirty_price);
    
    for _ in 0..MAX_ITERATIONS {
        let (pv, duration) = price_and_duration(&cash_flows, settlement, y, day_count, freq);
        let error = pv - dirty_price.to_f64();
        
        if error.abs() < TOLERANCE {
            return Ok(Yield::from_decimal(y));
        }
        
        let derivative = -duration * pv / (1.0 + y / freq);
        y -= error / derivative;
    }
    
    Err(YieldError::NoConvergence)
}
```

## Simple Yield (Japanese Convention)

### Formula
```
Simple Yield = (Annual Coupon + (Redemption - Price) / Years) / Price
```

### Implementation
```rust
fn calc_simple<B: Bond>(
    bond: &B,
    settlement: Date,
    price: CleanPrice,
) -> Yield {
    let day_count = bond.day_count();  // FROM BOND
    let years = day_count.year_fraction(settlement, bond.maturity());
    let annual_coupon = bond.coupon_rate() * bond.face_value();
    let redemption = bond.face_value();
    
    let capital_gain = (redemption - price.as_decimal()) / years;
    let simple_yield = (annual_coupon + capital_gain) / price.as_decimal();
    
    Yield::from_decimal(simple_yield)
}
```

## Discount Yield (T-Bills)

### Formula
```
Discount Yield = (Face - Price) / Face × (Basis / Days)
```

Where `Basis` = day count's year basis (360 for ACT/360)

### Implementation
```rust
fn calc_discount<B: Bond>(
    bond: &B,
    settlement: Date,
    price: CleanPrice,
) -> Yield {
    let day_count = bond.day_count();  // FROM BOND
    let days = day_count.day_count(settlement, bond.maturity());
    let basis = day_count.year_basis();  // 360 for ACT/360
    
    let discount = (bond.face_value() - price.as_decimal()) / bond.face_value();
    let annualized = discount * Decimal::from(basis) / Decimal::from(days);
    
    Yield::from_decimal(annualized)
}
```

## Add-On Yield (Money Market)

### Formula
```
Add-On Yield = (Face - Price) / Price × (Basis / Days)
```

### Implementation
```rust
fn calc_add_on<B: Bond>(
    bond: &B,
    settlement: Date,
    price: CleanPrice,
) -> Yield {
    let day_count = bond.day_count();  // FROM BOND
    let days = day_count.day_count(settlement, bond.maturity());
    let basis = day_count.year_basis();
    
    let return_pct = (bond.face_value() - price.as_decimal()) / price.as_decimal();
    let annualized = return_pct * Decimal::from(basis) / Decimal::from(days);
    
    Yield::from_decimal(annualized)
}
```

## Money Market Equivalent Yield (Sequential Roll-Forward)

For short-dated coupon bonds. Uses bond's day count for basis.

### Algorithm
```rust
fn calc_mmy_rollforward<B: Bond>(
    bond: &B,
    settlement: Date,
    dirty_price: Decimal,
) -> Result<Yield, YieldError> {
    let day_count = bond.day_count();  // FROM BOND
    let basis = day_count.year_basis() as f64;
    let cash_flows = bond.cash_flows_from(settlement);
    
    // Newton-Raphson to find y where PV(roll_forward(y)) = dirty_price
    let mut y = initial_guess(bond, dirty_price);
    
    for _ in 0..MAX_ITERATIONS {
        let (fv, fv_deriv) = roll_forward(&cash_flows, y, basis, day_count);
        
        let days_to_mat = day_count.day_count(settlement, bond.maturity()) as f64;
        let discount = 1.0 + y * days_to_mat / basis;
        let pv = fv / discount;
        
        let error = pv - dirty_price.to_f64();
        if error.abs() < TOLERANCE {
            return Ok(Yield::from_decimal(y));
        }
        
        // Derivative calculation...
        y -= error / pv_deriv;
    }
    
    Err(YieldError::NoConvergence)
}

fn roll_forward(
    cash_flows: &[CashFlow],
    y: f64,
    basis: f64,
    day_count: &dyn DayCount,
) -> (f64, f64) {
    let mut acc = 0.0;
    let mut acc_deriv = 0.0;
    
    for i in 0..cash_flows.len() {
        let days = if i + 1 < cash_flows.len() {
            day_count.day_count(cash_flows[i].date, cash_flows[i + 1].date) as f64
        } else {
            0.0  // Last CF: no rolling
        };
        
        if i == 0 {
            let factor = 1.0 + y * days / basis;
            acc = cash_flows[i].amount * factor;
            acc_deriv = cash_flows[i].amount * days / basis;
        } else if i + 1 < cash_flows.len() {
            let factor = 1.0 + y * days / basis;
            let total = cash_flows[i].amount + acc;
            acc_deriv = acc_deriv * factor + total * days / basis;
            acc = total * factor;
        } else {
            acc += cash_flows[i].amount;
        }
    }
    
    (acc, acc_deriv)
}
```

## Bond Equivalent Yield (182-Day Threshold)

### Short-Dated (≤182 days)
```
BEY = (Face - Price) / Price × (365 / Days)
```

### Long-Dated (>182 days)
```
BEY = (-b + √(b² - 4ac)) / 2a

where:
  a = Days / (2 × 365) - 0.25
  b = Days / 365
  c = (Price - Face) / Price
```

## Compounding Frequency Conversion

### General Formula
```
(1 + r₁/n₁)^n₁ = (1 + r₂/n₂)^n₂
r₂ = n₂ × [(1 + r₁/n₁)^(n₁/n₂) - 1]
```

### Common Conversions
```rust
// Semi-annual to Annual
let annual = (1.0 + semi / 2.0).powi(2) - 1.0;

// Annual to Semi-annual  
let semi = 2.0 * ((1.0 + annual).sqrt() - 1.0);

// Periodic to Continuous
let continuous = n * (1.0 + periodic / n).ln();
```

## Accrued Interest

Uses bond's day count convention:

```rust
fn accrued_interest<B: Bond>(bond: &B, settlement: Date) -> Decimal {
    let day_count = bond.day_count();  // FROM BOND
    let freq = bond.coupon_frequency();
    
    let (prev_coupon, next_coupon) = bond.coupon_dates_around(settlement);
    let period_coupon = bond.coupon_rate() * bond.face_value() / freq.periods_per_year();
    
    let accrued_fraction = day_count.year_fraction_with_period(
        prev_coupon, settlement, prev_coupon, next_coupon, freq
    );
    
    period_coupon * accrued_fraction * freq.periods_per_year()
}
```
