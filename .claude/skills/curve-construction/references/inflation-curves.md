# Inflation-Linked Curves

## Table of Contents
- [Real vs Nominal Curves](#real-vs-nominal-curves)
- [TIPS Cash Flow Mechanics](#tips-cash-flow-mechanics)
- [Real Yield Curve Construction](#real-yield-curve-construction)
- [Breakeven Inflation](#breakeven-inflation)
- [Inflation Seasonality](#inflation-seasonality)
- [Index Ratio and Accrued Inflation](#index-ratio-and-accrued-inflation)

## Real vs Nominal Curves

```
┌─────────────────────────────────────────────────────────────┐
│                    CURVE RELATIONSHIPS                       │
├─────────────────────────────────────────────────────────────┤
│  NOMINAL CURVE                                              │
│  └── Treasury / Government bonds                            │
│  └── Yields include inflation expectations + risk premium   │
├─────────────────────────────────────────────────────────────┤
│  REAL CURVE                                                 │
│  └── TIPS / Index-linked bonds                              │
│  └── Yields exclude inflation (purchasing power terms)      │
├─────────────────────────────────────────────────────────────┤
│  BREAKEVEN INFLATION = Nominal - Real                       │
│  └── Market-implied inflation expectations                  │
│  └── Contains inflation risk premium + liquidity effects    │
└─────────────────────────────────────────────────────────────┘
```

**Fisher Equation (approximate):**
```
Nominal_Rate ≈ Real_Rate + Expected_Inflation + Inflation_Risk_Premium
```

**Breakeven Inflation:**
```
BEI(T) = y_nominal(T) - y_real(T)
```

## TIPS Cash Flow Mechanics

### Principal Indexation

TIPS principal adjusts with CPI:

```
Adjusted_Principal(t) = Face × Index_Ratio(t)
Index_Ratio(t) = CPI(t) / CPI(issue_date)
```

**Deflation floor:** Principal at maturity cannot be less than original face value.

```rust
struct TIPS {
    face_value: f64,
    coupon_rate: f64,  // Real coupon
    maturity: Date,
    issue_date: Date,
    ref_cpi_at_issue: f64,
}

impl TIPS {
    fn index_ratio(&self, settlement: Date, cpi_curve: &CPICurve) -> f64 {
        let ref_cpi = self.reference_cpi(settlement, cpi_curve);
        ref_cpi / self.ref_cpi_at_issue
    }
    
    fn adjusted_principal(&self, settlement: Date, cpi_curve: &CPICurve) -> f64 {
        self.face_value * self.index_ratio(settlement, cpi_curve)
    }
    
    fn coupon_payment(&self, settlement: Date, cpi_curve: &CPICurve) -> f64 {
        let adjusted = self.adjusted_principal(settlement, cpi_curve);
        adjusted * self.coupon_rate / 2.0  // Semi-annual
    }
    
    fn redemption_value(&self, cpi_at_maturity: f64) -> f64 {
        let index_ratio = cpi_at_maturity / self.ref_cpi_at_issue;
        let adjusted = self.face_value * index_ratio;
        adjusted.max(self.face_value)  // Deflation floor
    }
}
```

### Indexation Lag

TIPS use a 3-month lag for CPI reference:

```
Reference_CPI(settlement) = interpolated CPI from 3 months prior
```

For settlement in month M:
```
Ref_CPI = CPI(M-3) + (day_of_month - 1)/(days_in_month) × [CPI(M-2) - CPI(M-3)]
```

```rust
fn reference_cpi(settlement: Date, cpi_data: &HashMap<Date, f64>) -> f64 {
    let ref_month = settlement.add_months(-3);
    let next_month = ref_month.add_months(1);
    
    let cpi_ref = cpi_data.get(&ref_month.start_of_month()).unwrap();
    let cpi_next = cpi_data.get(&next_month.start_of_month()).unwrap();
    
    let day = settlement.day();
    let days_in_month = settlement.days_in_month();
    let weight = (day - 1) as f64 / days_in_month as f64;
    
    cpi_ref + weight * (cpi_next - cpi_ref)
}
```

## Real Yield Curve Construction

### Bootstrapping from TIPS Prices

Similar to nominal curve but cash flows are indexed:

```rust
fn bootstrap_real_curve(
    tips: &[TIPS],
    settlement: Date,
    cpi_curve: &CPICurve,
) -> Curve {
    let mut real_dfs: Vec<(f64, f64)> = vec![];
    
    for tip in tips.iter().sorted_by_maturity() {
        let dirty_price = tip.dirty_price;
        let real_df = solve_real_discount_factor(tip, &real_dfs, settlement, cpi_curve);
        real_dfs.push((tip.maturity_years(settlement), real_df));
    }
    
    Curve::new(real_dfs, Interpolation::MonotoneConvex)
}

fn solve_real_discount_factor(
    tip: &TIPS,
    prior_dfs: &[(f64, f64)],
    settlement: Date,
    cpi_curve: &CPICurve,
) -> f64 {
    // TIPS price in real terms (adjusted for current inflation)
    let index_ratio = tip.index_ratio(settlement, cpi_curve);
    let real_price = tip.dirty_price / index_ratio;
    
    // Cash flows in real terms
    let real_coupons: f64 = tip.coupon_dates()
        .iter()
        .filter(|d| **d < tip.maturity)
        .map(|d| {
            let t = years_between(settlement, *d);
            let df = interpolate_df(prior_dfs, t);
            tip.real_coupon() * df
        })
        .sum();
    
    // Solve for final DF
    (real_price - real_coupons) / (tip.real_coupon() + tip.face_value)
}
```

### Fed Model Approach

Federal Reserve uses Svensson model on TIPS:

```rust
fn fit_tips_svensson(
    tips: &[TIPS],
    settlement: Date,
    cpi_curve: &CPICurve,
) -> Svensson {
    let real_yields: Vec<(f64, f64)> = tips.iter()
        .map(|tip| {
            let mat = tip.maturity_years(settlement);
            let real_ytm = calculate_real_ytm(tip, settlement, cpi_curve);
            (mat, real_ytm)
        })
        .collect();
    
    fit_svensson_two_stage(&real_yields)
}
```

## Breakeven Inflation

### Point Breakeven

At specific maturity:
```
BEI(T) = y_nominal(T) - y_real(T)
```

```rust
fn breakeven_inflation(
    nominal_curve: &Curve,
    real_curve: &Curve,
    maturity: f64,
) -> f64 {
    nominal_curve.rate(maturity) - real_curve.rate(maturity)
}
```

### Forward Breakeven

Expected inflation between two future dates:

```
BEI_forward(t1, t2) = [BEI(t2) × t2 - BEI(t1) × t1] / (t2 - t1)
```

```rust
fn forward_breakeven(
    nominal_curve: &Curve,
    real_curve: &Curve,
    t1: f64,
    t2: f64,
) -> f64 {
    let bei1 = breakeven_inflation(nominal_curve, real_curve, t1);
    let bei2 = breakeven_inflation(nominal_curve, real_curve, t2);
    
    (bei2 * t2 - bei1 * t1) / (t2 - t1)
}

// Example: 5y5y forward breakeven (5-year rate starting in 5 years)
let bei_5y5y = forward_breakeven(&nominal, &real, 5.0, 10.0);
```

### Decomposition

Breakeven inflation includes:
```
BEI = Expected_Inflation + Inflation_Risk_Premium - Liquidity_Premium
```

- **Inflation risk premium:** Compensation for inflation uncertainty (typically positive)
- **Liquidity premium:** TIPS less liquid than nominals (widens BEI)

## Inflation Seasonality

CPI has predictable seasonal patterns affecting short-dated TIPS:

```rust
struct SeasonalFactors {
    monthly_factors: [f64; 12],  // Average seasonal effect by month
}

impl SeasonalFactors {
    fn from_historical_cpi(cpi_history: &[(Date, f64)]) -> Self {
        // Decompose CPI into trend + seasonal + irregular
        // Extract average monthly factors
        let factors = x13_seasonal_adjustment(cpi_history);
        Self { monthly_factors: factors }
    }
    
    fn adjust_breakeven(&self, raw_bei: f64, months_to_maturity: u32) -> f64 {
        // Adjust for seasonal carry
        let seasonal_carry = self.cumulative_seasonal(months_to_maturity);
        raw_bei - seasonal_carry / months_to_maturity as f64 * 12.0
    }
    
    fn cumulative_seasonal(&self, months: u32) -> f64 {
        let current_month = today().month() as usize;
        let mut total = 0.0;
        
        for i in 0..months {
            let month = (current_month + i as usize) % 12;
            total += self.monthly_factors[month];
        }
        total
    }
}
```

### Typical US CPI Seasonality

| Month | Typical Effect |
|-------|---------------|
| Jan | Strong positive (reset effects) |
| Feb | Moderate positive |
| Mar | Moderate positive |
| Apr | Strong positive (apparel) |
| May | Positive |
| Jun | Positive |
| Jul | Negative |
| Aug | Negative |
| Sep | Slight positive |
| Oct | Moderate positive |
| Nov | Slight negative |
| Dec | Slight negative |

## Index Ratio and Accrued Inflation

### TIPS Pricing Components

```
Dirty_Price = Clean_Price + Accrued_Interest + Accrued_Inflation
```

Actually, TIPS quote clean price but in "real" terms:

```
Invoice_Price = Clean_Price × Index_Ratio + Accrued_Interest × Index_Ratio
```

```rust
fn tips_invoice_price(
    tips: &TIPS,
    clean_price: f64,
    settlement: Date,
    cpi_curve: &CPICurve,
) -> f64 {
    let index_ratio = tips.index_ratio(settlement, cpi_curve);
    let accrued = tips.accrued_interest(settlement);
    
    (clean_price + accrued) * index_ratio
}

fn tips_accrued_interest(tips: &TIPS, settlement: Date) -> f64 {
    let last_coupon = tips.previous_coupon_date(settlement);
    let next_coupon = tips.next_coupon_date(settlement);
    
    let days_accrued = day_count(last_coupon, settlement, DayCount::ActAct);
    let days_period = day_count(last_coupon, next_coupon, DayCount::ActAct);
    
    // Real coupon (not indexed)
    let coupon = tips.coupon_rate / 2.0 * tips.face_value;
    coupon * days_accrued / days_period
}
```

### Real Yield Calculation

Given market price, solve for real yield:

```rust
fn tips_real_yield(
    tips: &TIPS,
    clean_price: f64,
    settlement: Date,
    cpi_curve: &CPICurve,
) -> f64 {
    let invoice_price = tips_invoice_price(tips, clean_price, settlement, cpi_curve);
    let index_ratio = tips.index_ratio(settlement, cpi_curve);
    
    // Cash flows in real terms (known part)
    let real_cfs: Vec<(f64, f64)> = tips.future_cash_flows(settlement)
        .iter()
        .map(|(date, cf)| {
            let t = years_between(settlement, *date);
            // Future inflation unknown, but in real terms CF is known
            (t, *cf / index_ratio)
        })
        .collect();
    
    // Solve for yield
    solve_yield(&real_cfs, invoice_price)
}
```

## Multi-Currency Inflation Curves

### UK Index-Linked Gilts

**Reference index:** RPI (transitioning to CPIH from 2030)
**Indexation lag:** 3 months (new style) or 8 months (old style)

```rust
fn uk_linker_index_ratio(
    gilt: &IndexLinkedGilt,
    settlement: Date,
    rpi_data: &HashMap<Date, f64>,
) -> f64 {
    let lag_months = if gilt.is_new_style { 3 } else { 8 };
    let ref_rpi = reference_index(settlement, rpi_data, lag_months);
    ref_rpi / gilt.base_rpi
}
```

### Euro Area Linkers

**Reference index:** HICP ex-tobacco
**Markets:** France (OATi, OAT€i), Germany (DBRi), Italy (BTPi)

```rust
enum EuroLinkerType {
    French(HICPFrType),  // OATi (French HICP) or OAT€i (Euro HICP)
    German,              // DBRi (Euro HICP)
    Italian,             // BTPi (Euro HICP)
}

fn euro_linker_index_ratio(
    bond: &EuroInflationBond,
    settlement: Date,
    hicp_data: &HashMap<Date, f64>,
) -> f64 {
    // 3-month lag, linear interpolation
    let ref_hicp = reference_index(settlement, hicp_data, 3);
    ref_hicp / bond.base_index
}
```

### Building Consistent Curves

For cross-currency inflation analysis:

```rust
struct MultiCurrencyInflationCurves {
    // Real curves
    real_usd: Curve,  // TIPS
    real_gbp: Curve,  // Linkers
    real_eur: Curve,  // OATi/DBRi
    
    // Nominal curves
    nominal_usd: Curve,
    nominal_gbp: Curve,
    nominal_eur: Curve,
}

impl MultiCurrencyInflationCurves {
    fn breakeven(&self, ccy: Currency, maturity: f64) -> f64 {
        match ccy {
            Currency::USD => {
                self.nominal_usd.rate(maturity) - self.real_usd.rate(maturity)
            }
            Currency::GBP => {
                self.nominal_gbp.rate(maturity) - self.real_gbp.rate(maturity)
            }
            Currency::EUR => {
                self.nominal_eur.rate(maturity) - self.real_eur.rate(maturity)
            }
        }
    }
    
    fn relative_breakeven(&self, ccy1: Currency, ccy2: Currency, maturity: f64) -> f64 {
        self.breakeven(ccy1, maturity) - self.breakeven(ccy2, maturity)
    }
}
```
