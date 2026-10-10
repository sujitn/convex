# Direct contextual ICMA yield risk and hedge columns

`workflow::PreparedIcmaQuotation::yield_sensitivity` prices the same complete
prepared plain fixed cash at the original yield and at +h, -h, +h/2 and -h/2.
`hedge_yield_risk` returns that full calculation and an existing
`constrained_hedge::HedgeRiskVector`. Shared `icma_yield_risk` actions
`yield_sensitivity` and `hedge_vector` reach generic native/JSON/C/MCP/WASM/
Java/Excel dispatch. Actual managed and browser host qualification is separate.

## Original context, units and factor

Supply complete bond economics, independent security/quote/policy/calendar
revisions, anchored nominal reference mesh, settlement/payment ownership,
normal frequency, compounding and signed original-face `RiskPosition`.
Existing checked preparation owns coupon/principal/steps/amortization, split
retirement/redemption, accrued/rebate, delayed payments and exact cash times.
Every time is prepared once. Risk cannot infer a mesh from a day-count tag.

Required `IcmaYieldRiskInput` carries `key`, `observed_quote`, `snapshot_id`,
original `QuoteRiskSettings` and `max_cash_evaluations`. Observations explicitly
use DecimalRate, PercentRate or BasisPoints. The original value is retained
exactly, including the baseline native observation. Bumps are annual decimals;
scenario quotes use the original unit. Prices are per100 original face; callback
PV is absolute native money. Sensitivity is signed currency per +1bp, with
quantity applied once by the existing reference owner.

The key must be InterestRate/Yield/RawQuote/FrozenCurves/CurrencyPerBp, target
the actual security ID and use Raw with the exact original quote ID/version.
Native unit matches the observation. `convention_version` equals the actual
quotation-policy revision; `model_version` equals its exact ID, identifying the
resolved contextual quotation/bump policy. Full policy, frequency/compounding,
context and sources survive in every quotation. The caller resolves those
immutable contents and the actual snapshot; comparing pins does not certify
a provider or manifest.

An instrument's own raw yield is its own coordinate. Other securities' yields,
Z spreads, curve nodes and recalibrated market quotes remain distinct. This
route does not calculate common curve-factor exposure for different bonds,
CDS, IRS or futures. Cross-instrument hedges require each product's complete
calculation on the same admitted factor set. There is no tenor mapping,
synthetic zero bucket or yield-to-curve proxy.

## Complete results and bounded work

`IcmaYieldSensitivity` retains the input, original five-PV `QuoteSensitivity`
and five ordered `IcmaYieldRiskValuation`s. Each includes canonical move,
native quote and full `IcmaQuotation`: original terms/cash, source/context/
policy/calendar revisions, quantity, accrued, clean/dirty/native PV and signed
unrounded dirty consideration. Hypothetical scenarios retain original source
revisions inside the supplied snapshot; they are not ingested observations.

The original `reference_quote_sensitivity` owns differentiation, normalization,
quantity and central/half-step stability. Original product/domain/finite-price
and stability failures abort an accepted result. Stability keeps the existing
native `invalid_input` category. No policy relaxation or bump retry is performed.

Before any price, all five annual-rate coordinates are admitted and
`5 × (original cashflow count + 2)` cash visits are reserved under the caller's
positive cap at most 67,108,864. One-time reference preparation retains its
original positive cap at most 16,777,216. Both reservations are reported.
Original schedule/profile/span and shared input/output limits remain. These
arithmetic bounds are not latency or memory certificates. No inverse, curve
calibration or reference regeneration occurs during the five prices.

Only unconditional plain fixed cash is admitted. Floating cash, unmodeled
optional contracts, conditional workouts, Street conventions and inferred
exercise paths keep explicit native failures. Missing/stale context/calendar/
policy, wrong factor/unit/face/currency, exhausted work and unstable or invalid
domains never produce an accepted partial result or successful zero fallback.

## Existing hedge owners

`hedge_vector` also requires `IcmaHedgeIdentity`: original TradeId, entity,
complete economics/state/market manifest pin and full calculation manifest pin.
It requires `quantity == reference_quantity` at positive original face. The
vector retains actual security revision, settlement and the unchanged native
sensitivity. Current, pending and proposed signed quantities remain explicit
inputs to the existing hedge owner, which scales the reference column once.
Signed trade risk cannot be scaled again as positive reference risk.

This vector covers one actual yield factor, with empty credit scope and absent
FX. Supply additional independently calculated factors and exact same-snapshot
reporting FX to the complete `HedgeProblem` when required. Existing admission
still rejects missing columns and incompatible keys/snapshots/quantity/credit/
FX. Results also remain usable by signed portfolio aggregation under its full
trade/factor manifest. The bridge does not resolve or certify manifest contents.

The standalone consumer passes freshly calculated columns through real
aggregate/solve, closing +2500 current and -500 pending face with a -2000 trade
in 1000-face lots. It checks independent full prices, exposures, quantities,
costs and one supplied 0.9 USD/EUR conversion. This is a same-security factor
control. Proposals remain `proposal_only`; CVX-025/031 own authoritative limits
and orders.

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

Decimal60 controls are declared before candidate execution. Raw requests/answers
and bounds remain; separate admission retains a valid unused-tail case and
adds actual delayed-payment coverage and fully framed conditional failures.
Fresh native envelopes establish transport only. CVX-063 remains partial,
unaccepted and uncertified; full market/model/product/host/release gates remain.

[QuantLib bond functions](https://github.com/lballabio/QuantLib/blob/master/ql/pricingengines/bond/bondfunctions.cpp)
delegate yield analytics to cashflows with explicit compounding and settlement.
[Strata's ICMA description](https://strata.opengamma.io/day_counts/) requires
nominal periods and frequency. [ORE sensitivity analysis](https://github.com/OpenSourceRisk/Engine/blob/master/OREAnalytics/orea/engine/sensitivityanalysis.cpp)
retains factor-specific shift data. These primary-source reads inform separation
of time, coordinates and valuation; no new external runtime certificate is claimed.
