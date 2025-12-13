# Cross-Currency Basis Curves

## Table of Contents
- [Cross-Currency Basis Explained](#cross-currency-basis-explained)
- [FX Forward-Implied Curves](#fx-forward-implied-curves)
- [XCCY Basis Swap Bootstrapping](#xccy-basis-swap-bootstrapping)
- [Collateral Currency Effects](#collateral-currency-effects)
- [Simultaneous Multi-Currency Calibration](#simultaneous-multi-currency-calibration)
- [Implementation Patterns](#implementation-patterns)

## Cross-Currency Basis Explained

The cross-currency basis represents deviation from Covered Interest Parity (CIP):

**CIP (theoretical):**
```
F/S = (1 + r_foreign) / (1 + r_domestic)
```

**CIP with basis:**
```
F/S = (1 + r_foreign + basis) / (1 + r_domestic)
```

**Why basis exists:**
- USD funding scarcity (structural demand)
- Balance sheet constraints at dealers
- Regulatory capital costs
- Credit risk differentials

**Typical values (USD basis):**
| Currency | Typical Range | Direction |
|----------|---------------|-----------|
| EUR      | -20 to -50 bp | USD premium |
| JPY      | -30 to -80 bp | USD premium |
| GBP      | -10 to -30 bp | USD premium |
| AUD      | +10 to -20 bp | Variable |

Negative USD basis means borrowing USD via FX swap costs more than cash market.

## FX Forward-Implied Curves

For short maturities (< 1Y), derive foreign discount curve from:
- Domestic OIS curve
- FX spot and forward rates

**Core equation:**
```
D_foreign(T) = D_domestic(T) × S / F(T)
```

Where:
- S = spot FX rate (domestic per foreign)
- F(T) = forward FX rate to time T

```rust
fn build_fx_implied_curve(
    domestic_ois: &Curve,
    fx_spot: f64,           // e.g., 1.08 EUR/USD
    fx_forwards: &[(f64, f64)],  // (time, forward_rate)
) -> Curve {
    let mut nodes = vec![CurveNode { time: 0.0, df: 1.0 }];
    
    for (t, fwd) in fx_forwards {
        let domestic_df = domestic_ois.df(*t);
        let foreign_df = domestic_df * fx_spot / fwd;
        nodes.push(CurveNode { time: *t, df: foreign_df });
    }
    
    Curve::new(nodes, Interpolation::LogLinear)
}
```

**FX forward points convention:**
```
Forward = Spot + Forward_Points / 10000  (typically)
```

Note: Check if points are quoted in pips (10000) or percentage terms per currency pair.

## XCCY Basis Swap Bootstrapping

For longer maturities, use cross-currency basis swaps.

### XCCY Basis Swap Structure

**Constant notional (most common):**
- Start: Exchange notionals at spot
- Periodic: Pay foreign floating + basis, receive domestic floating
- End: Re-exchange notionals at original spot

**Mark-to-market (MTM):**
- Notionals reset periodically to current spot
- Reduces FX exposure during life

### Valuation Equations

**USD leg NPV:**
```
NPV_USD = -D_USD(T₀) + Σₙ[D_USD(Tₙ) × τₙ × L_USD(Tₙ₋₁,Tₙ)] + D_USD(Tₙ)
```

**Foreign leg NPV (with basis b):**
```
NPV_FOR = -D_FOR(T₀) + Σₙ[D_FOR(Tₙ) × τₙ × (L_FOR(Tₙ₋₁,Tₙ) + b)] + D_FOR(Tₙ)
```

**At inception (fair value):**
```
NPV_USD × S₀ = NPV_FOR
```

### Bootstrap Algorithm

```rust
struct XCCYCurveBuilder {
    usd_ois: Curve,
    usd_projection: Curve,
    foreign_ois: Curve,        // From FX forwards (short end)
    foreign_projection: Curve,
}

impl XCCYCurveBuilder {
    fn bootstrap(
        usd_ois: Curve,
        usd_proj: Curve,
        fx_forwards: &[(f64, f64)],  // Short end
        xccy_swaps: &[XCCYBasisSwap], // Long end
        fx_spot: f64,
    ) -> Self {
        // Phase 1: FX forward implied (< 1Y)
        let mut foreign_nodes = build_fx_implied_nodes(&usd_ois, fx_spot, fx_forwards);
        
        // Phase 2: XCCY basis swaps (> 1Y)
        for swap in xccy_swaps.iter().sorted_by_maturity() {
            let df = solve_xccy_constraint(
                swap,
                &usd_ois,
                &usd_proj,
                &foreign_nodes,
                fx_spot,
            );
            foreign_nodes.push(CurveNode {
                time: swap.maturity_years(),
                df,
            });
        }
        
        let foreign_ois = Curve::new(foreign_nodes.clone(), Interpolation::LogLinear);
        
        // Foreign projection typically same as OIS for SOFR-based currencies
        // Or build separately from domestic swaps if needed
        let foreign_projection = foreign_ois.clone();
        
        Self {
            usd_ois,
            usd_projection: usd_proj,
            foreign_ois,
            foreign_projection,
        }
    }
}

fn solve_xccy_constraint(
    swap: &XCCYBasisSwap,
    usd_ois: &Curve,
    usd_proj: &Curve,
    foreign_nodes: &[CurveNode],
    fx_spot: f64,
) -> f64 {
    let objective = |df_foreign: f64| {
        // Build temp foreign curve
        let temp_foreign = build_temp_curve(foreign_nodes, swap.maturity_years(), df_foreign);
        
        // USD leg NPV
        let usd_npv = xccy_leg_npv(
            &swap.usd_leg,
            usd_ois,
            usd_proj,
            0.0,  // No basis on USD leg
        );
        
        // Foreign leg NPV (with basis)
        let foreign_npv = xccy_leg_npv(
            &swap.foreign_leg,
            &temp_foreign,
            &temp_foreign,  // Using OIS as projection for simplicity
            swap.basis,
        );
        
        // Constraint: USD_NPV * spot = Foreign_NPV
        usd_npv * fx_spot - foreign_npv
    };
    
    brent_solver(objective, 0.01, 2.0, 1e-12, 100)
}

fn xccy_leg_npv(
    leg: &XCCYLeg,
    disc: &Curve,
    proj: &Curve,
    spread: f64,
) -> f64 {
    let mut npv = 0.0;
    
    // Initial exchange
    npv -= disc.df(leg.start) * leg.notional;
    
    // Float coupons
    for period in &leg.periods {
        let fwd = proj.forward(period.start, period.end);
        npv += disc.df(period.payment) * period.dcf * (fwd + spread) * leg.notional;
    }
    
    // Final exchange
    npv += disc.df(leg.end) * leg.notional;
    
    npv
}
```

## Collateral Currency Effects

Under CSA, present value uses the collateral rate for discounting.

### Discount Curve Selection

| Trade Currency | Collateral | Discount Curve |
|----------------|------------|----------------|
| EUR | EUR cash | EUR OIS (€STR) |
| EUR | USD cash | EUR discounted at XCCY-implied |
| USD | EUR cash | USD discounted at EUR-implied |
| Any | Multi-currency | Cheapest-to-deliver |

### USD-Collateralized Foreign Curve

For EUR trade with USD collateral:

```
D_EUR^USD-coll(T) = D_USD(T) × S / F(T)
```

This is the FX-implied curve - what we bootstrap from XCCY swaps.

```rust
enum CollateralType {
    Domestic,
    Foreign(Currency),
    MultiCurrency(Vec<Currency>),
}

fn select_discount_curve(
    curves: &MultiCurrencyCurves,
    trade_ccy: Currency,
    collateral: CollateralType,
) -> &Curve {
    match collateral {
        CollateralType::Domestic => {
            curves.ois(trade_ccy)
        }
        CollateralType::Foreign(coll_ccy) => {
            curves.xccy_implied(trade_ccy, coll_ccy)
        }
        CollateralType::MultiCurrency(ccys) => {
            // Cheapest to deliver
            curves.ctd_curve(trade_ccy, &ccys)
        }
    }
}
```

### Multi-Currency CSA (Cheapest-to-Deliver)

Collateral poster chooses cheapest currency at each margin call:

```
D_CTD(T) = max{D_EUR(T), D_USD(T) × S_EUR/USD / F_EUR/USD(T), ...}
```

This creates optionality - requires modeling or approximation.

**Simplified approach:** Use the currency with highest discount factor (lowest rates).

```rust
fn ctd_discount_factor(
    curves: &MultiCurrencyCurves,
    trade_ccy: Currency,
    eligible_collateral: &[Currency],
    t: f64,
) -> f64 {
    eligible_collateral.iter()
        .map(|coll_ccy| {
            if *coll_ccy == trade_ccy {
                curves.ois(trade_ccy).df(t)
            } else {
                curves.xccy_implied(trade_ccy, *coll_ccy).df(t)
            }
        })
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap()
}
```

**Value impact:** 10%+ on long-dated swaps possible between different collateral assumptions.

## Simultaneous Multi-Currency Calibration

When circular dependencies exist (e.g., EUR/USD and USD/JPY both reference USD), solve simultaneously.

### System Formulation

```
Find x such that f(x) = 0
```

Where:
- x = all curve node values across currencies
- f = pricing equations for all instruments

### Block Jacobian Structure

```
J = | J_USD,USD    J_USD,EUR    J_USD,JPY   |
    | J_EUR,USD    J_EUR,EUR    J_EUR,JPY   |
    | J_JPY,USD    J_JPY,EUR    J_JPY,JPY   |
```

- Diagonal blocks: Dense (single-currency instruments)
- Off-diagonal: Sparse (XCCY instruments only)

### Newton-Raphson Iteration

```rust
fn simultaneous_calibration(
    instruments: &AllInstruments,
    initial: MultiCurrencyNodes,
    tol: f64,
) -> MultiCurrencyNodes {
    let mut x = initial.to_vector();
    
    for _ in 0..100 {
        let f = compute_all_residuals(instruments, &x);
        
        if f.norm() < tol {
            break;
        }
        
        let J = compute_block_jacobian(instruments, &x);
        
        // Solve J * delta = f
        // Use block LU or iterative method for efficiency
        let delta = J.solve(&f);
        
        x = &x - &delta;
    }
    
    MultiCurrencyNodes::from_vector(&x)
}

fn compute_block_jacobian(
    instruments: &AllInstruments,
    x: &Vector,
) -> BlockMatrix {
    let mut J = BlockMatrix::zeros(3, 3);  // 3 currencies
    
    // Diagonal blocks: single-currency sensitivities
    for (i, ccy) in [USD, EUR, JPY].iter().enumerate() {
        J.set_block(i, i, 
            compute_single_ccy_jacobian(&instruments.single_ccy[ccy], x)
        );
    }
    
    // Off-diagonal: XCCY sensitivities
    for xccy in &instruments.xccy_swaps {
        let (i, j) = currency_indices(xccy.ccy_pair);
        let sens = compute_xccy_jacobian(xccy, x);
        J.add_to_block(i, j, &sens.foreign_sens);
        J.add_to_block(j, i, &sens.domestic_sens);
    }
    
    J
}
```

## Implementation Patterns

### Complete Multi-Currency Curve Set

```rust
pub struct MultiCurrencyCurves {
    // OIS curves (domestic collateral)
    ois: HashMap<Currency, Curve>,
    
    // Projection curves by tenor
    projection: HashMap<(Currency, Tenor), Curve>,
    
    // XCCY-implied curves (foreign collateral)
    // Key: (trade_ccy, collateral_ccy)
    xccy_implied: HashMap<(Currency, Currency), Curve>,
    
    // FX spots
    fx_spots: HashMap<CurrencyPair, f64>,
}

impl MultiCurrencyCurves {
    pub fn discount(&self, ccy: Currency, collateral: Currency, t: f64) -> f64 {
        if ccy == collateral {
            self.ois.get(&ccy).unwrap().df(t)
        } else {
            self.xccy_implied.get(&(ccy, collateral)).unwrap().df(t)
        }
    }
    
    pub fn forward(&self, ccy: Currency, tenor: Tenor, start: f64, end: f64) -> f64 {
        self.projection.get(&(ccy, tenor)).unwrap().forward(start, end)
    }
    
    pub fn fx_forward(&self, pair: CurrencyPair, t: f64) -> f64 {
        let spot = self.fx_spots.get(&pair).unwrap();
        let df_dom = self.ois.get(&pair.domestic).unwrap().df(t);
        let df_for = self.ois.get(&pair.foreign).unwrap().df(t);
        spot * df_for / df_dom
    }
}
```

### Asset Swap Spread with XCCY

For cross-currency asset swaps, need both domestic and XCCY curves:

```rust
fn cross_currency_asset_swap_spread(
    bond: &Bond,
    curves: &MultiCurrencyCurves,
    bond_price: f64,  // Dirty price
) -> f64 {
    let bond_ccy = bond.currency;
    let swap_ccy = Currency::USD;  // Swapping into USD
    
    // Bond PV in bond currency (using domestic OIS)
    let bond_pv_domestic = bond_present_value(
        bond,
        curves.ois.get(&bond_ccy).unwrap(),
    );
    
    // Convert to USD via spot
    let fx = curves.fx_spots.get(&CurrencyPair::new(swap_ccy, bond_ccy)).unwrap();
    let bond_pv_usd = bond_pv_domestic * fx;
    
    // USD float leg annuity
    let usd_annuity = compute_float_annuity(
        bond.maturity,
        curves.ois.get(&swap_ccy).unwrap(),
    );
    
    // Spread over USD SOFR
    (bond_pv_usd - bond_price * fx) / usd_annuity
}
```
