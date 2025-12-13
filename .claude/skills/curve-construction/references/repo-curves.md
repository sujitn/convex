# Repo Curves for Bond Financing

## Table of Contents
- [Repo Market Overview](#repo-market-overview)
- [GC vs Special Repo](#gc-vs-special-repo)
- [Repo Curve Construction](#repo-curve-construction)
- [Carry and Rolldown Calculations](#carry-and-rolldown-calculations)
- [Term Repo Structure](#term-repo-structure)

## Repo Market Overview

Repos (repurchase agreements) are secured short-term borrowing using bonds as collateral.

```
┌─────────────────────────────────────────────────────────────┐
│                    REPO TRANSACTION                          │
├─────────────────────────────────────────────────────────────┤
│  DAY 0 (Start Leg)                                          │
│  Cash Lender ───── $99.50 cash ─────> Bond Holder           │
│  Cash Lender <──── Bond (collateral) ── Bond Holder         │
├─────────────────────────────────────────────────────────────┤
│  DAY N (End Leg)                                            │
│  Cash Lender <──── $99.60 cash ─────── Bond Holder          │
│  Cash Lender ───── Bond ──────────────> Bond Holder         │
│                                                             │
│  Repo Rate = ($99.60 - $99.50) / $99.50 × (360/N)          │
└─────────────────────────────────────────────────────────────┘
```

### Key Rates

| Rate | Description | Source |
|------|-------------|--------|
| SOFR | Secured Overnight Financing Rate | Fed/NY Fed |
| BGCR | Broad General Collateral Rate | NY Fed |
| TGCR | Tri-party GC Rate | NY Fed |
| €STR | Euro Short-Term Rate | ECB |
| SONIA | Sterling Overnight Index Average | BoE |

## GC vs Special Repo

### General Collateral (GC)

**Cash-driven:** Borrower needs cash, lender accepts any bond from a basket.

```
GC_Rate ≈ Policy_Rate - small_spread
```

Characteristics:
- Any bond in basket acceptable
- Rate determined by cash demand/supply
- Benchmark for funding costs

### Special Collateral (SC / Specific)

**Security-driven:** Lender needs a specific bond (for short covering, delivery, etc.).

```
Special_Rate < GC_Rate  (when bond is "on special")
Specialness = GC_Rate - Special_Rate
```

Characteristics:
- Specific CUSIP/ISIN required
- Rate determined by bond scarcity
- On-the-run issues often trade special

```rust
struct RepoRates {
    gc_rate: f64,          // General collateral rate
    special_rates: HashMap<String, f64>,  // Bond ID → rate
}

impl RepoRates {
    fn specialness(&self, bond_id: &str) -> f64 {
        match self.special_rates.get(bond_id) {
            Some(special) => self.gc_rate - special,
            None => 0.0,  // Trading at GC
        }
    }
    
    fn financing_rate(&self, bond_id: &str) -> f64 {
        self.special_rates
            .get(bond_id)
            .copied()
            .unwrap_or(self.gc_rate)
    }
}
```

### Specialness Drivers

| Factor | Effect on Specialness |
|--------|----------------------|
| On-the-run status | Higher (more demand) |
| Auction cycle | Peaks before auction |
| Short interest | Higher (delivery needs) |
| CTD for futures | Higher near delivery |
| QE purchases | Higher (reduced float) |

## Repo Curve Construction

### Term Structure of GC Rates

Build term repo curve from overnight to 1 year:

```rust
fn build_gc_repo_curve(
    overnight_rate: f64,
    term_rates: &[(u32, f64)],  // (days, rate)
) -> Curve {
    let mut nodes = vec![(0.0, overnight_rate)];
    
    for (days, rate) in term_rates {
        let t = *days as f64 / 360.0;  // ACT/360
        nodes.push((t, *rate));
    }
    
    Curve::new(nodes, Interpolation::Linear)
}
```

### Standard Term Points

| Tenor | Days | Typical vs O/N |
|-------|------|----------------|
| Overnight | 1 | Base |
| T/N | 2 | +0-2bp |
| 1 Week | 7 | +2-5bp |
| 2 Week | 14 | +3-7bp |
| 1 Month | 30 | +5-15bp |
| 3 Month | 90 | +10-25bp |
| 6 Month | 180 | +15-35bp |
| 1 Year | 360 | +20-50bp |

### Special Repo Term Structure

For specific bonds, build separate curve:

```rust
fn build_special_repo_curve(
    bond_id: &str,
    gc_curve: &Curve,
    specialness_by_term: &[(u32, f64)],  // (days, specialness_bp)
) -> Curve {
    let mut nodes = vec![];
    
    for (days, specialness) in specialness_by_term {
        let t = *days as f64 / 360.0;
        let gc = gc_curve.rate(t);
        let special = gc - specialness / 10000.0;
        nodes.push((t, special));
    }
    
    Curve::new(nodes, Interpolation::Linear)
}
```

## Carry and Rolldown Calculations

### Carry Definition

**Carry** = Income from holding position minus financing cost

```
Carry = Coupon_Income - Financing_Cost
      = (Accrued_Interest_Earned) - (Price × Repo_Rate × Days/360)
```

```rust
fn calculate_carry(
    bond: &Bond,
    dirty_price: f64,
    repo_rate: f64,
    holding_days: u32,
) -> f64 {
    // Coupon accrual over period
    let accrued_earned = bond.coupon / bond.frequency as f64 
        * holding_days as f64 / (365.0 / bond.frequency as f64);
    
    // Financing cost
    let financing_cost = dirty_price * repo_rate * holding_days as f64 / 360.0;
    
    accrued_earned - financing_cost
}
```

### Breakeven Repo Rate

Rate at which carry = 0:

```
Breakeven_Repo = (Coupon × Days / Coupon_Period) / (Dirty_Price × Days / 360)
               = Coupon / Dirty_Price × (360 / 365) × (1 / frequency)
```

```rust
fn breakeven_repo_rate(bond: &Bond, dirty_price: f64) -> f64 {
    let annual_coupon = bond.coupon;
    annual_coupon / dirty_price * (360.0 / 365.0)
}
```

### Rolldown

**Rolldown** = Price change from moving down the yield curve

```
Rolldown = Price(today, yield_at_shorter_maturity) - Price(today, current_yield)
```

```rust
fn calculate_rolldown(
    bond: &Bond,
    current_yield: f64,
    yield_curve: &Curve,
    horizon_days: u32,
) -> f64 {
    let current_maturity = bond.maturity_years();
    let future_maturity = current_maturity - horizon_days as f64 / 365.0;
    
    if future_maturity <= 0.0 {
        return 0.0;
    }
    
    // Yield at shorter maturity point
    let rolled_yield = yield_curve.rate(future_maturity);
    
    // Price at current yield
    let price_now = bond_price_from_yield(bond, current_yield);
    
    // Price at rolled yield (same bond, different yield)
    let price_rolled = bond_price_from_yield(bond, rolled_yield);
    
    price_rolled - price_now
}
```

### Total Return Components

```
Total_Return = Carry + Rolldown + Yield_Change_Effect
             = (Coupon - Financing) + (Curve_Roll) + (Duration × Δy)
```

```rust
struct TotalReturnDecomposition {
    carry: f64,
    rolldown: f64,
    yield_change: f64,
    total: f64,
}

fn decompose_total_return(
    bond: &Bond,
    dirty_price_start: f64,
    dirty_price_end: f64,
    repo_rate: f64,
    yield_curve_start: &Curve,
    yield_curve_end: &Curve,
    holding_days: u32,
) -> TotalReturnDecomposition {
    // Carry component
    let carry = calculate_carry(bond, dirty_price_start, repo_rate, holding_days);
    
    // Rolldown (assuming unchanged curve)
    let yield_start = bond.yield_to_maturity(dirty_price_start);
    let rolldown = calculate_rolldown(bond, yield_start, yield_curve_start, holding_days);
    
    // Residual = yield change effect
    let actual_return = (dirty_price_end - dirty_price_start + 
                         carry * dirty_price_start / 100.0);
    let yield_change = actual_return - carry - rolldown;
    
    TotalReturnDecomposition {
        carry,
        rolldown,
        yield_change,
        total: actual_return,
    }
}
```

## Term Repo Structure

### Forward-Starting Repos

Lock in financing for future period:

```
1x2 Repo: Start in 1 month, end in 2 months
Forward_Rate = (DF(1m) / DF(2m) - 1) × 360 / 30
```

```rust
fn forward_repo_rate(
    repo_curve: &Curve,
    start_days: u32,
    end_days: u32,
) -> f64 {
    let t1 = start_days as f64 / 360.0;
    let t2 = end_days as f64 / 360.0;
    
    let df1 = (-repo_curve.rate(t1) * t1).exp();
    let df2 = (-repo_curve.rate(t2) * t2).exp();
    
    (df1 / df2 - 1.0) * 360.0 / (end_days - start_days) as f64
}
```

### Repo vs OIS Spread

Repo rates typically trade tight to OIS but can diverge:

```
Repo-OIS Spread = Repo_Rate - OIS_Rate
```

| Condition | Spread Behavior |
|-----------|-----------------|
| Normal | 0-5bp |
| Quarter-end | 10-50bp+ (balance sheet pressure) |
| Year-end | 20-100bp+ |
| Fed tightening | Widens |
| QE | Tightens (abundant reserves) |

```rust
fn repo_ois_spread(
    repo_curve: &Curve,
    ois_curve: &Curve,
    tenor_days: u32,
) -> f64 {
    let t = tenor_days as f64 / 360.0;
    let repo = repo_curve.rate(t);
    let ois = ois_curve.rate(t);
    
    (repo - ois) * 10000.0  // In basis points
}
```

## Practical Applications

### Funding a Bond Position

```rust
fn funding_cost_analysis(
    bond: &Bond,
    position_size: f64,  // Face value
    dirty_price: f64,
    repo_rates: &RepoRates,
    holding_period: u32,
) -> FundingAnalysis {
    let market_value = position_size * dirty_price / 100.0;
    
    // Haircut (typically 2-5% for Treasuries)
    let haircut = 0.02;
    let borrowing_amount = market_value * (1.0 - haircut);
    
    // Financing rate
    let repo_rate = repo_rates.financing_rate(&bond.id);
    
    // Cost
    let financing_cost = borrowing_amount * repo_rate * holding_period as f64 / 360.0;
    
    // Income
    let coupon_income = position_size * bond.coupon / bond.frequency as f64
        * holding_period as f64 / (365.0 / bond.frequency as f64);
    
    FundingAnalysis {
        market_value,
        borrowing_amount,
        repo_rate,
        financing_cost,
        coupon_income,
        net_carry: coupon_income - financing_cost,
    }
}
```

### Relative Value with Repo

Compare two bonds adjusting for financing:

```rust
fn repo_adjusted_spread(
    bond1: &Bond,
    bond2: &Bond,
    repo_rates: &RepoRates,
    yield_curve: &Curve,
) -> f64 {
    // Z-spreads
    let z1 = bond1.z_spread(yield_curve);
    let z2 = bond2.z_spread(yield_curve);
    
    // Financing advantage
    let repo1 = repo_rates.financing_rate(&bond1.id);
    let repo2 = repo_rates.financing_rate(&bond2.id);
    let financing_diff = (repo2 - repo1) * 10000.0;  // bp
    
    // Adjusted spread: higher spread + cheaper financing = better
    (z1 - z2) + financing_diff
}
```

### CTD Analysis with Repo

For futures basis trading, repo is critical:

```rust
fn futures_basis_net_of_carry(
    bond: &Bond,
    futures_price: f64,
    conversion_factor: f64,
    repo_rate: f64,
    days_to_delivery: u32,
) -> f64 {
    // Gross basis
    let gross_basis = bond.clean_price - futures_price * conversion_factor;
    
    // Carry to delivery
    let carry = calculate_carry(bond, bond.dirty_price(), repo_rate, days_to_delivery);
    
    // Net basis (should be near zero for CTD)
    gross_basis - carry
}
```
