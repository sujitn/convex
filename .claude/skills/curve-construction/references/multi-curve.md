# Multi-Curve Framework

## Table of Contents
- [Why Multi-Curve](#why-multi-curve)
- [Fundamental Pricing Formula](#fundamental-pricing-formula)
- [Projection Curve Construction](#projection-curve-construction)
- [Tenor Basis Relationships](#tenor-basis-relationships)
- [Convexity Adjustments](#convexity-adjustments)
- [Dual-Curve Stripping](#dual-curve-stripping)

## Why Multi-Curve

Pre-2008: Single curve for both discounting and projection (LIBOR-OIS spread ~10bp, ignored).

Post-2008: EURIBOR 3M - OIS spread hit **195.5 bp** during Lehman collapse. Different tenors carry different credit/liquidity premia.

**Key insight:** A 3M rate embeds 3 months of bank credit risk; overnight rate embeds ~1 day. These are fundamentally different instruments requiring separate curves.

## Fundamental Pricing Formula

Swap present value under multi-curve:

```
PV(t) = Σᵢ Pₐ(t,Tᵢ) × γₘ(Tᵢ₋₁,Tᵢ) × FRAₘ(t; Tᵢ₋₁,Tᵢ)
```

Where:
- `Pₐ(t,Tᵢ)` = discount factor from **discount curve** (OIS)
- `FRAₘ(t; Tᵢ₋₁,Tᵢ)` = forward rate from **projection curve** of tenor m
- `γₘ` = day count fraction for tenor m

**Forward rate from projection curve:**
```
Fₘ(t; T₁,T₂) = (1/γₘ) × [Pₘ(t,T₁)/Pₘ(t,T₂) - 1]
```

Note: `Pₘ` are "pseudo-discount factors" - not true discount factors, but mathematical constructs that produce correct forwards.

## Projection Curve Construction

### Input Instruments

**SOFR Term Curve:**
- Short end: 1M/3M SOFR futures (with convexity adjustment)
- Long end: SOFR OIS swap rates (same curve serves both discount and projection)

**EURIBOR Curves:**
- Short end: EURIBOR futures, FRAs
- Long end: EUR IRS vs EURIBOR
- Requires €STR OIS for discounting

### Bootstrap Algorithm

```rust
fn bootstrap_projection_curve(
    instruments: Vec<ProjectionInstrument>,
    discount_curve: &Curve,  // OIS curve
) -> Curve {
    let mut nodes = vec![CurveNode { time: 0.0, df: 1.0 }];
    
    for inst in instruments.iter().sorted_by_maturity() {
        let df = match inst.instrument_type {
            InstrumentType::Future => {
                // Apply convexity adjustment first
                let adjusted_rate = inst.quote - convexity_adjustment(&inst);
                solve_from_future(adjusted_rate, &nodes, discount_curve)
            }
            InstrumentType::Swap => {
                solve_from_swap(&inst, &nodes, discount_curve)
            }
            InstrumentType::FRA => {
                solve_from_fra(&inst, &nodes, discount_curve)
            }
        };
        
        nodes.push(CurveNode { 
            time: inst.maturity_years(), 
            df 
        });
    }
    
    Curve::new(nodes, Interpolation::MonotoneConvex)
}
```

### Swap Stripping with OIS Discounting

For a par swap with fixed rate K:

```
K × Σᵢ D_OIS(Tᵢ) × τᵢ^fixed = Σⱼ D_OIS(Tⱼ) × τⱼ^float × F(Tⱼ₋₁, Tⱼ)
```

Rearranging for the last forward:

```rust
fn solve_swap_forward(
    swap: &SwapInstrument,
    proj_nodes: &[CurveNode],
    disc_curve: &Curve,
) -> f64 {
    let K = swap.fixed_rate;
    let n = swap.float_dates.len();
    
    // Fixed leg PV
    let fixed_pv: f64 = swap.fixed_dates.iter()
        .map(|d| disc_curve.df(d) * swap.fixed_dcf(d))
        .sum();
    
    // Known float leg PV (all but last period)
    let known_float_pv: f64 = swap.float_dates[..n-1].iter()
        .enumerate()
        .map(|(i, d)| {
            let fwd = forward_from_nodes(proj_nodes, swap.float_dates[i], *d);
            disc_curve.df(d) * swap.float_dcf(d) * fwd
        })
        .sum();
    
    // Solve for last forward
    let last_disc = disc_curve.df(swap.float_dates[n-1]);
    let last_dcf = swap.float_dcf_last();
    
    (K * fixed_pv - known_float_pv) / (last_disc * last_dcf)
}
```

## Tenor Basis Relationships

Different tenors trade at different levels. A 3M vs 6M basis swap:
- Leg 1: 3M rate + spread (quarterly)
- Leg 2: 6M rate flat (semi-annual)

**Bootstrap secondary tenors from primary:**

Given 3M curve, build 6M curve from basis swaps:

```
Σᵢ D(Tᵢ³ᴹ) × τ₃ₘ × [F₃ₘ(Tᵢ₋₁,Tᵢ) + spread] = Σⱼ D(Tⱼ⁶ᴹ) × τ₆ₘ × F₆ₘ(Tⱼ₋₁,Tⱼ)
```

```rust
fn bootstrap_tenor_basis(
    basis_swaps: &[TenorBasisSwap],
    primary_curve: &Curve,   // e.g., 3M
    discount_curve: &Curve,
) -> Curve {
    let mut secondary_nodes = vec![CurveNode { time: 0.0, df: 1.0 }];
    
    for swap in basis_swaps.iter().sorted_by_maturity() {
        // Primary leg PV (known)
        let primary_pv = compute_float_leg_pv(
            &swap.primary_leg,
            primary_curve,
            discount_curve,
        ) + swap.spread * compute_annuity(&swap.primary_leg, discount_curve);
        
        // Solve for secondary pseudo-DF
        let secondary_df = solve_basis_constraint(
            primary_pv,
            &swap.secondary_leg,
            &secondary_nodes,
            discount_curve,
        );
        
        secondary_nodes.push(CurveNode {
            time: swap.maturity_years(),
            df: secondary_df,
        });
    }
    
    Curve::new(secondary_nodes, Interpolation::MonotoneConvex)
}
```

## Convexity Adjustments

Futures settle daily (mark-to-market); forwards settle at maturity. This creates a timing mismatch.

**Hull-White approximation:**
```
CA ≈ (σ² × T₁ × T₂) / 2
```

Where:
- σ = interest rate volatility
- T₁ = futures expiry
- T₂ = underlying rate maturity

**Forward Rate:**
```
Forward = Futures_Rate - CA
```

**Typical magnitudes:**
| Tenor | CA (bp) |
|-------|---------|
| 3M    | 0.1-0.5 |
| 1Y    | 1-2     |
| 2Y    | 3-6     |
| 5Y    | 10-20   |

```rust
fn convexity_adjustment(
    futures_expiry: f64,
    underlying_maturity: f64,
    vol: f64,  // Typically 0.5-1.5%
) -> f64 {
    // Hull-White / Ho-Lee approximation
    0.5 * vol * vol * futures_expiry * underlying_maturity
}

fn adjusted_forward(futures_rate: f64, futures: &Future, vol: f64) -> f64 {
    let ca = convexity_adjustment(
        futures.expiry_years(),
        futures.underlying_maturity_years(),
        vol,
    );
    futures_rate - ca
}
```

## Dual-Curve Stripping

When bootstrapping projection curve, the discount curve must be fully built first.

**Complete algorithm:**

```rust
struct MultiCurveBuilder {
    discount_curve: Option<Curve>,
    projection_curves: HashMap<Tenor, Curve>,
}

impl MultiCurveBuilder {
    fn build(
        ois_instruments: Vec<OISInstrument>,
        projection_instruments: HashMap<Tenor, Vec<ProjectionInstrument>>,
        basis_swaps: HashMap<TenorPair, Vec<TenorBasisSwap>>,
    ) -> Self {
        // Step 1: Build OIS discount curve
        let discount = bootstrap_ois_curve(ois_instruments);
        
        // Step 2: Build primary projection curve (typically 3M)
        let primary_tenor = Tenor::M3;
        let primary = bootstrap_projection_curve(
            projection_instruments.get(&primary_tenor).unwrap(),
            &discount,
        );
        
        let mut projection_curves = HashMap::new();
        projection_curves.insert(primary_tenor, primary);
        
        // Step 3: Build secondary tenors from basis
        for (pair, swaps) in basis_swaps {
            let secondary = bootstrap_tenor_basis(
                &swaps,
                projection_curves.get(&pair.primary).unwrap(),
                &discount,
            );
            projection_curves.insert(pair.secondary, secondary);
        }
        
        Self {
            discount_curve: Some(discount),
            projection_curves,
        }
    }
    
    fn forward(&self, tenor: Tenor, start: f64, end: f64) -> f64 {
        let curve = self.projection_curves.get(&tenor).unwrap();
        let df_start = curve.df(start);
        let df_end = curve.df(end);
        let dcf = end - start; // Simplified
        
        (df_start / df_end - 1.0) / dcf
    }
    
    fn discount(&self, t: f64) -> f64 {
        self.discount_curve.as_ref().unwrap().df(t)
    }
    
    fn pv_float_leg(&self, leg: &FloatLeg) -> f64 {
        leg.periods.iter()
            .map(|p| {
                let fwd = self.forward(leg.tenor, p.start, p.end);
                self.discount(p.payment) * p.dcf * fwd * p.notional
            })
            .sum()
    }
}
```

## Curve Dependency Graph

```
                    ┌──────────────┐
                    │   OIS Swaps  │
                    └──────┬───────┘
                           │
                           ▼
                    ┌──────────────┐
                    │  OIS Curve   │ ◄── Discount for ALL
                    │ (SOFR/€STR)  │
                    └──────┬───────┘
                           │
            ┌──────────────┼──────────────┐
            │              │              │
            ▼              ▼              ▼
     ┌──────────┐   ┌──────────┐   ┌──────────┐
     │ Futures  │   │   IRS    │   │   FRA    │
     │ (adj)    │   │ vs 3M    │   │          │
     └────┬─────┘   └────┬─────┘   └────┬─────┘
          │              │              │
          └──────────────┼──────────────┘
                         ▼
                  ┌──────────────┐
                  │   3M Curve   │ ◄── Primary projection
                  └──────┬───────┘
                         │
          ┌──────────────┼──────────────┐
          │              │              │
          ▼              ▼              ▼
    ┌──────────┐   ┌──────────┐   ┌──────────┐
    │ 3M/1M    │   │ 3M/6M    │   │ 3M/12M   │
    │ Basis    │   │ Basis    │   │ Basis    │
    └────┬─────┘   └────┬─────┘   └────┬─────┘
         │              │              │
         ▼              ▼              ▼
    ┌─────────┐   ┌─────────┐   ┌─────────┐
    │1M Curve │   │6M Curve │   │12M Curve│
    └─────────┘   └─────────┘   └─────────┘
```
