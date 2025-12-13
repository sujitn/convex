# Government Yield Curves

## Table of Contents
- [Curve Types and Relationships](#curve-types-and-relationships)
- [Bootstrapping from Bond Prices](#bootstrapping-from-bond-prices)
- [Treasury Curve Construction](#treasury-curve-construction)
- [Gilt and Bund Curves](#gilt-and-bund-curves)
- [Par Yield vs Zero Yield vs Forward](#par-yield-vs-zero-yield-vs-forward)

## Curve Types and Relationships

Government yield curves come in three equivalent representations:

```
┌─────────────────────────────────────────────────────────────┐
│                    CURVE RELATIONSHIPS                       │
├─────────────────────────────────────────────────────────────┤
│  PAR YIELD CURVE                                            │
│  └── Coupon rate at which bond prices at par                │
│  └── Published by central banks (Fed, BoE, ECB)             │
│                      ↓ Bootstrap                            │
│  ZERO (SPOT) CURVE                                          │
│  └── Yield on hypothetical zero-coupon bonds                │
│  └── Used for discounting individual cash flows             │
│                      ↓ Differentiate                        │
│  FORWARD CURVE                                              │
│  └── Implied future short-term rates                        │
│  └── Used for expectations analysis, derivatives            │
└─────────────────────────────────────────────────────────────┘
```

**Key relationships:**

Zero rate from discount factor:
```
r(T) = -ln(D(T)) / T
```

Forward rate from zero rates:
```
f(t₁, t₂) = [r(t₂) × t₂ - r(t₁) × t₁] / (t₂ - t₁)
```

Instantaneous forward:
```
f(t) = r(t) + t × dr(t)/dt
```

Par yield from discount factors:
```
y_par(T) = [1 - D(T)] / Σᵢ D(tᵢ)
```

## Bootstrapping from Bond Prices

### Core Algorithm

Given coupon bonds with prices P₁, P₂, ..., Pₙ sorted by maturity:

```rust
fn bootstrap_govt_curve(bonds: &[Bond]) -> Curve {
    let mut nodes: Vec<(f64, f64)> = vec![];  // (time, discount_factor)
    
    for bond in bonds.iter().sorted_by_maturity() {
        let df = solve_discount_factor(bond, &nodes);
        nodes.push((bond.maturity_years(), df));
    }
    
    Curve::new(nodes, Interpolation::MonotoneConvex)
}
```

### Solving for Discount Factors

For a bond with price P, coupon C, and maturity T:

```
P = Σᵢ C × D(tᵢ) + 100 × D(T)
```

Rearranging for the final discount factor:

```
D(T) = [P - Σᵢ₌₁ⁿ⁻¹ C × D(tᵢ)] / (C + 100)
```

```rust
fn solve_discount_factor(bond: &Bond, prior_nodes: &[(f64, f64)]) -> f64 {
    let coupon = bond.coupon / bond.frequency as f64;
    let price = bond.dirty_price();
    
    // Sum of known cash flows (using interpolation for intermediate dates)
    let known_pv: f64 = bond.coupon_dates()
        .iter()
        .filter(|d| **d < bond.maturity)
        .map(|d| coupon * interpolate_df(prior_nodes, d.years()))
        .sum();
    
    // Solve for final DF
    (price - known_pv) / (coupon + 100.0)
}
```

### Handling Accrued Interest

Bonds quote **clean price**; calculations use **dirty price**:

```
Dirty_Price = Clean_Price + Accrued_Interest
Accrued = Coupon × (Days_Since_Last_Coupon / Days_In_Period)
```

```rust
fn accrued_interest(bond: &Bond, settle_date: Date) -> f64 {
    let last_coupon = bond.previous_coupon_date(settle_date);
    let next_coupon = bond.next_coupon_date(settle_date);
    
    let days_accrued = day_count(last_coupon, settle_date, bond.day_count);
    let days_period = day_count(last_coupon, next_coupon, bond.day_count);
    
    bond.coupon / bond.frequency as f64 * days_accrued / days_period
}
```

## Treasury Curve Construction

### US Treasury Methodology (February 2025)

The US Treasury uses **monotone convex** method on par yields:

1. **Inputs**: Bid-side prices for on-the-run securities
   - Bills: 4, 6, 8, 13, 17, 26, 52 weeks
   - Notes: 2, 3, 5, 7, 10 years
   - Bonds: 20, 30 years

2. **Process**:
   - Convert prices to yields
   - Bootstrap instantaneous forwards at input maturities
   - Apply monotone convex interpolation on forwards
   - Minimize price error on inputs

3. **Output**: Par yield curve

```rust
fn build_treasury_curve(
    bills: &[TBill],
    notes_bonds: &[TNote],
) -> Curve {
    // Sort all instruments by maturity
    let mut instruments = Vec::new();
    
    // Bills: direct discount rate → zero rate
    for bill in bills {
        let zero_rate = bill.bond_equivalent_yield();
        instruments.push((bill.maturity_years(), zero_rate));
    }
    
    // Notes/Bonds: bootstrap from prices
    let mut curve_nodes = instruments.clone();
    
    for note in notes_bonds.iter().sorted_by_maturity() {
        let df = bootstrap_coupon_bond(note, &curve_nodes);
        let zero = -df.ln() / note.maturity_years();
        curve_nodes.push((note.maturity_years(), zero));
    }
    
    // Convert to par yields via monotone convex on forwards
    build_par_curve_monotone_convex(&curve_nodes)
}
```

### Bill Pricing

T-Bills quote on **discount basis**:

```
Discount_Rate = (100 - Price) / 100 × (360 / Days_to_Maturity)
```

Bond Equivalent Yield (BEY):

```
BEY = (100 - Price) / Price × (365 / Days_to_Maturity)
```

For days ≤ 182:
```
BEY = [−Days + √(Days² + (2×Days−1)(100−Price)×Days/Price)] / (Days−0.5)
```

```rust
fn tbill_bond_equivalent_yield(price: f64, days: u32) -> f64 {
    if days <= 182 {
        (100.0 - price) / price * (365.0 / days as f64)
    } else {
        // Semi-annual compounding adjustment for longer bills
        let term = days as f64 / 365.0;
        let y_simple = (100.0 - price) / price / term;
        2.0 * ((1.0 + y_simple * term).sqrt() - 1.0) / term
    }
}
```

## Gilt and Bund Curves

### UK Gilts

**Conventions:**
- Day count: ACT/ACT (ISDA)
- Settlement: T+1
- Coupon frequency: Semi-annual
- Yield convention: UK semi-annual

**Curve construction:**
- Bank of England publishes daily fitted curves
- Uses spline methodology with variable roughness penalty
- Separate nominal and index-linked curves

### German Bunds

**Conventions:**
- Day count: ACT/ACT (ICMA)
- Settlement: T+2
- Coupon frequency: Annual
- Yield convention: Annual ICMA

**Eurozone considerations:**
- Use OIS (€STR) for discounting derivatives
- Bund curve as risk-free benchmark
- Spread to Bunds for other sovereigns (BTP, OAT, etc.)

```rust
struct GovtBondConventions {
    day_count: DayCount,
    settlement_days: u32,
    coupon_frequency: u32,
    yield_compounding: Compounding,
}

impl GovtBondConventions {
    fn us_treasury() -> Self {
        Self {
            day_count: DayCount::ActAct,
            settlement_days: 1,
            coupon_frequency: 2,
            yield_compounding: Compounding::SemiAnnual,
        }
    }
    
    fn uk_gilt() -> Self {
        Self {
            day_count: DayCount::ActActISDA,
            settlement_days: 1,
            coupon_frequency: 2,
            yield_compounding: Compounding::SemiAnnual,
        }
    }
    
    fn german_bund() -> Self {
        Self {
            day_count: DayCount::ActActICMA,
            settlement_days: 2,
            coupon_frequency: 1,
            yield_compounding: Compounding::Annual,
        }
    }
}
```

## Par Yield vs Zero Yield vs Forward

### Conversion Between Representations

**Zero to Par:**
```rust
fn zero_to_par(zero_curve: &Curve, maturity: f64, frequency: u32) -> f64 {
    let periods = (maturity * frequency as f64) as usize;
    let dt = 1.0 / frequency as f64;
    
    let mut annuity = 0.0;
    for i in 1..=periods {
        let t = i as f64 * dt;
        annuity += zero_curve.df(t);
    }
    
    let final_df = zero_curve.df(maturity);
    frequency as f64 * (1.0 - final_df) / annuity
}
```

**Par to Zero (Bootstrap):**
```rust
fn par_to_zero(par_curve: &Curve, maturities: &[f64], frequency: u32) -> Curve {
    let mut zero_nodes = vec![];
    let dt = 1.0 / frequency as f64;
    
    for &mat in maturities {
        let par_yield = par_curve.rate(mat);
        let coupon = par_yield / frequency as f64;
        
        // Sum known DFs
        let known_sum: f64 = zero_nodes.iter()
            .filter(|(t, _)| *t < mat)
            .map(|(t, df)| *df)
            .sum();
        
        // Solve: 1 = coupon × Σ DF + (1 + coupon) × DF(T)
        let final_df = (1.0 - coupon * known_sum) / (1.0 + coupon);
        let zero_rate = -final_df.ln() / mat;
        
        zero_nodes.push((mat, final_df));
    }
    
    Curve::from_dfs(zero_nodes)
}
```

**Zero to Forward:**
```rust
fn instantaneous_forward(zero_curve: &Curve, t: f64) -> f64 {
    let dt = 0.0001;  // Small delta
    let r1 = zero_curve.rate(t);
    let r2 = zero_curve.rate(t + dt);
    
    // f(t) = r(t) + t × dr/dt
    r1 + t * (r2 - r1) / dt
}

fn forward_rate(zero_curve: &Curve, t1: f64, t2: f64) -> f64 {
    let r1 = zero_curve.rate(t1);
    let r2 = zero_curve.rate(t2);
    
    (r2 * t2 - r1 * t1) / (t2 - t1)
}
```

### Typical Curve Shapes

| Shape | Par | Zero | Forward | Economic Signal |
|-------|-----|------|---------|-----------------|
| Normal | Upward | Steeper upward | Steepest | Growth expected |
| Flat | Flat | Flat | Flat | Uncertainty |
| Inverted | Downward | Less inverted | Most inverted | Recession expected |
| Humped | Peak mid-term | Similar | Sharper hump | Policy transition |

### Spread Benchmarks

For bond pricing, key spread measures reference different curve points:

| Spread | Reference | Use Case |
|--------|-----------|----------|
| G-spread | On-the-run Treasury yield | Quick relative value |
| I-spread | Swap rate at maturity | Floating rate comparison |
| Z-spread | Entire zero curve | Comprehensive credit spread |
| ASW spread | SOFR/swap curve | Funding-adjusted value |

See [spread-curves.md](spread-curves.md) for detailed spread curve construction.
