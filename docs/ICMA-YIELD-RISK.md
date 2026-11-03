# ICMA yield risk and hedge columns

This document covers yield sensitivity for plain fixed-coupon bonds quoted on an
ICMA basis, in `workflow::PreparedIcmaQuotation::yield_sensitivity`.
It prices the prepared cash at the quoted yield and at +h, -h, +h/2 and -h/2.

`hedge_yield_risk` returns that full calculation and an existing
`constrained_hedge::HedgeRiskVector`. The shared `icma_yield_risk` actions
`yield_sensitivity` and `hedge_vector` are reachable through the generic
Rust/JSON/C/MCP/WASM/Java/Excel dispatch. Managed and browser hosts are not yet
validated.

## Inputs, units and factor

You pass in complete bond economics, independent security, quote, policy and
calendar revisions, an anchored nominal reference mesh, settlement/payment
ownership, the normal frequency, compounding and a signed face `RiskPosition`.
The existing checked preparation handles coupon, principal, steps, amortization,
split retirement/redemption, accrued/rebate, delayed payments and exact cash
times. Every time is prepared once. Risk cannot infer a mesh from a day-count
tag.

`IcmaYieldRiskInput` carries `key`, `observed_quote`, `snapshot_id`,
`QuoteRiskSettings` and `max_cash_evaluations`. Observations use DecimalRate,
PercentRate or BasisPoints. The value is kept exactly, including the baseline
observation. Bumps are annual decimals; scenario quotes use the original unit.
Prices are per 100 original face; callback PV is absolute money. Sensitivity is signed
currency per +1bp, with quantity applied once by the existing reference code.

The key must be InterestRate/Yield/RawQuote/FrozenCurves/CurrencyPerBp, target
the security ID and use Raw with the exact quote ID/version. The native unit
matches the observation. `convention_version` equals the quotation-policy
revision; `model_version` equals its exact ID, identifying the resolved
quotation/bump policy. The full policy, frequency/compounding, context and
sources are kept in every quotation. The caller resolves those immutable
contents and the snapshot; comparing pins does not validate a provider or
manifest.

An instrument's own raw yield is its own coordinate. Other securities' yields,
Z spreads, curve nodes and recalibrated market quotes are distinct. This route
does not calculate common curve-factor exposure for different bonds, CDS, IRS or
futures. Cross-instrument hedges need each product's full calculation on the
same factor set. There is no tenor mapping, synthetic zero bucket or
yield-to-curve proxy.

## Results and bounded work

`IcmaYieldSensitivity` keeps the input, the five-PV `QuoteSensitivity` and five
ordered `IcmaYieldRiskValuation`s. Each includes the canonical move, the quote
and the full `IcmaQuotation`: terms and cash, source/context/policy/calendar
revisions, quantity, accrued, clean/dirty/native PV and signed unrounded dirty
consideration. Hypothetical scenarios keep the source revisions inside the
snapshot; they are not ingested observations.

`reference_quote_sensitivity` handles differentiation, normalization, quantity
and central/half-step stability. Product, domain, finite-price and stability
failures abort the result. Stability keeps the existing `invalid_input`
category. No policy relaxation or bump retry is performed.

Before any price, all five annual-rate coordinates are validated and
`5 × (cashflow count + 2)` cash visits are reserved under your positive cap
(at most 67,108,864). One-time reference preparation has its own positive cap
(at most 16,777,216). Both reservations are reported. The schedule, profile,
span and shared input/output limits remain. These arithmetic bounds are not
latency or memory guarantees. No inverse, curve calibration or reference
regeneration occurs during the five prices.

Only unconditional plain fixed cash is accepted. Floating cash, unmodeled
optional contracts, conditional workouts, Street conventions and inferred
exercise paths fail with explicit errors. These cases never produce a partial
result or a successful zero fallback:

- missing or stale context, calendar or policy;
- wrong factor, unit, face or currency;
- exhausted work budget;
- unstable or invalid domains.

## Hedge columns

`hedge_vector` also requires `IcmaHedgeIdentity`: TradeId, entity, the
economics/state/market manifest pin and the calculation manifest pin. It
requires `quantity == reference_quantity` at positive original face. The vector keeps the
security revision, settlement and the unchanged sensitivity. Current, pending
and proposed signed quantities remain explicit inputs to the existing hedge
code, which scales the reference column once. Signed trade risk cannot be scaled
again as positive reference risk.

This vector covers one yield factor, with empty credit scope and absent FX. Pass
additional independently calculated factors and same-snapshot reporting FX to the
full `HedgeProblem` when required. It still rejects missing columns and
incompatible keys, snapshots, quantity, credit and FX. Results can also feed
signed portfolio aggregation under its trade/factor manifest. The bridge does
not resolve or validate manifest contents.

The standalone example passes freshly calculated columns through the real
aggregate and solve steps, closing +2500 current and -500 pending face with a
-2000 trade in 1000-face lots. It checks independent prices, exposures,
quantities, costs and one 0.9 USD/EUR conversion. This is a same-security factor
control. Proposals remain `proposal_only`; limits and orders belong to the
server.

## Reproduce

```sh
python3 tests/scripts/icma_risk_reference.py
python3 tests/scripts/icma_risk_admission.py
python3 tests/scripts/icma_risk_failures.py
python3 tests/scripts/icma_risk_protocol.py
python3 tests/scripts/icma_risk_api.py
python3 tests/scripts/icma_risk_owner.py
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test icma_risk -- --test-threads=1
cargo run --locked -p convex --example icma_risk
cargo run --locked -p convex --example icma_risk --no-default-features
cargo run --locked -p convex --example icma_risk --features schemars
python3 tests/scripts/icma_risk_schema.py --binary target/debug/examples/icma_risk
python3 tests/scripts/icma_risk_parity.py --binary target/debug/examples/icma_risk --library target/debug/libconvex_ffi.so
```

The Decimal60 controls are declared before the code under test runs. The raw
requests, answers and bounds are kept. The input-validation script keeps a valid
unused-tail case and adds delayed-payment coverage and fully framed conditional
failures. Fresh Rust envelopes only show that the transports agree. Market,
model, product and host validation is not yet done.

[QuantLib bond functions](https://github.com/lballabio/QuantLib/blob/master/ql/pricingengines/bond/bondfunctions.cpp)
delegate yield analytics to cashflows with explicit compounding and settlement.
[Strata's ICMA description](https://strata.opengamma.io/day_counts/) requires
nominal periods and frequency. [ORE sensitivity analysis](https://github.com/OpenSourceRisk/Engine/blob/master/OREAnalytics/orea/engine/sensitivityanalysis.cpp)
retains factor-specific shift data. These references informed how time,
coordinates and valuation are kept separate; the code does not run against them.

## Common-factor hedge columns

The separate `graph_hedge_risk` operation reuses the product and graph
revaluation code to produce bond, CDS, IRS, OIS and rates-futures columns for the
existing hedge aggregator and proposal solver. See
[quantity binding, inputs, controls and remaining limits](GRAPH-HEDGE-RISK.md).
Own-security ICMA yield remains a separate factor.

Not yet covered: optional models, credit-survival cash products, delivery-futures
all-member risk, managed and browser hosts, and production and latency
qualification. This work is partial and not certified.
