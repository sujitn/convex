# Credit and issuer-basis cash hedge risk

`cash_basis_hedge_risk` builds a complete common-factor cash-bond column from the
original cash helper, issuer valuation, calibration graph and five-price risk
owners. It supports fixed cash with the original coupon steps, principal
retirements, calendars, settlement and contextual ACT/ACT ICMA quotations.
The resulting vector enters the existing `hedge_aggregate` and `hedge_solve`
operations alongside CDS, IRS/OIS and admitted rates-futures columns.

Run the full native example, including independent cash comparisons, original
owner comparisons and USD/EUR mixed-book proposals:

```sh
cargo run --locked -p convex --example cash_basis_hedge
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test cash_basis_hedge -- --test-threads=1
```

The example emits fresh transport envelopes. Its `--emit-schema` switch requires
`--features schemars`. Default, minimal and schema builds use the same financial
owners. Request fixtures are in `tests/fixtures/library/cash-basis-hedge-api.json`,
`cash-basis-hedge-precise.json` and `cash-basis-hedge-book-columns.json`.

## Inputs and results

The strict operation contains `graph`, the complete `security` (original helper
and exact calendar pins), and `input`:

| Field | Meaning |
| --- | --- |
| `basis` | Exact issuer-spread curve key in the graph. |
| `model` | Original `ReferenceZeroSpread` or `CreditParRecovery` model. |
| `credit` | Optional full `BondCdsHedgeInput`, checked by the original legal mapping owner. Required for the credit model. |
| `basis_tolerance` | Explicit decimal-rate comparison bound in `(0, 1e-10]` for mapped baseline basis observations. |
| `hedge` | Complete original identity, positive native-face reference, ordered factors, snapshot and factor-work bound. |

The reference quantity must equal the positive original face in the actual
currency. Current, pending and proposed signed positions belong to the downstream
hedge book. A signed observed ICMA quotation remains signed in its quotation and
invoice; it does not replace the positive risk reference.

The result retains the complete helper, original model and checked mapping,
original observed conversion, full model valuation, model-implied native quote,
model-minus-observed residual, contextual inverse/work and the hedge vector.
Observed cash and model PV stay separate. A finite off-market residual is a
diagnostic, not an accepted calibration fit. Prices per 100, original-face money,
percentage-point yield, decimal contextual ICMA yield and common curve risk units
retain their original meanings.

Every factor returns the actual baseline, up/down and half-bump up/down PVs,
original key, settings, quantity, units and snapshot. An unrelated supplied
projection curve is still actually repriced; an observed zero is not filled in.
Curve versions are opaque exact keys. They are not coerced into the numeric
revisions used by security, mapping or calendar pins.

## Model and credit scope

`CreditParRecovery` uses the original promised-cash and par-recovery model.
The issuer basis applies to promised cash; recovery follows the existing
discount/survival integration over the actual principal profile. The original
ACT365F, log-linear discount-factor/survival grids, common origin, no-extrapolation,
immediate principal-payment and recovery-domain guards remain. Delayed principal,
default lifecycle cash and optional-model substitution are rejected by their
original owners. Accrual, quotation compounding, ICMA time and actual payment dates
are independent inputs.

The explicit mapping retains bond identity/revision, entity, seniority, CDS
documentation/restructuring/recovery, survival key, basis observations and model,
and a separate liquidity assumption. Mapped basis observations must match the
actual baseline curve at every supplied date within `basis_tolerance`; original
bond/CDS horizon admission applies. Calibration residuals are not substituted for
this comparison.

A reference-only column can also carry an explicit checked mapping for its
issuer-basis factor. With no mapping it has no credit scope, so the original hedge
owner rejects a nonzero credit-family factor that needs that scope. Neither case
manufactures hazard or recovery exposure. Direct yield risk remains a distinct
own-security factor, described in [ICMA yield risk](ICMA-YIELD-RISK.md).

For a recovery scenario the cash composition binds the same original model's
recovery parameter to the actual scenario credit role. Frozen supplied nodes and
market-quote/dependent rebuilds use the original graph owner. Unsupported
calibrated-node moves retain the original typed rejection.

ORE exposes discount, default, recovery and security-spread inputs separately;
its spread configuration and recovery cash handling must be aligned before a
numerical comparison. This is a design reference, not an executed ORE comparison
or model-equivalence claim. [ORE risky-bond engine](https://github.com/OpenSourceRisk/Engine/blob/master/QuantExt/qle/pricingengines/discountingriskybondengine.cpp)

## Bounds and failure behavior

Admission requires 1–16 distinct complete factors and an explicit 1–80 factor
evaluation bound. Five evaluations per factor are reserved before valuation.
These are graph callback counts, not a total calibration-work or latency bound.
Contextual cash/time preparation happens once. It retains original source-entry
bounds and conservatively reserves each complete model-price-plus-inverse pass
before valuation, including the baseline native-unit inverse. Graph calibration
and quotation solvers retain their separately supplied bounds.

Missing or incompatible mapping, basis observations, calendar revision, security,
quantity, factor, model, domain, contextual work or graph input returns the
existing typed error envelope. Failure produces no accepted partial column.
No provider, persistence, queue, clock, order or execution authority is added.
The pure implementation is in `convex-analytics`; the shared adapter resolves
the full original inputs and delegates. Existing C, MCP and native-compiled WASM
adapters continue through the single shared dispatcher.

## Qualified scope and next work

Predeclared independent Decimal60 controls cover positive/negative rates, clean
and dirty price, percentage yield, complete contextual ICMA yield, reference and
credit models, coupon/principal steps, full five-PV discount/hazard/basis/recovery
risk, actually evaluated unrelated projection, recovery endpoints, opaque
versions, and original helper calibration/dependent rebuilds.

The full mixed book uses discount, hazard, issuer basis and projection factors
with cash, CDS, IRS and term-futures candidates; it checks current/pending
positions, exact contract lots, side costs, constraints and explicit same-snapshot
USD/EUR FX. Recovery is separately covered by cash controls. The CDS graph owner
still needs its own recovery-coordinate binding before recovery can be added to
this common book. The original broad-limit solver numerical failure remains open;
the demonstrated proposals use explicitly declared financial limits and scales.

Full optional-model and all-member delivery-futures risk, actual JVM/JNI,
Windows/.NET/Excel and wasm32/browser execution, and independent model, market,
library, production and latency certification remain open. Native WASM compilation
does not prove browser execution. CVX-063 remains partial and unaccepted; these
bounded controls do not award a production certificate. CVX-025/031 own server
limit and order authority. The next harness increment should complete the original
CDS recovery graph owner and independently compare the full five-factor mixed
book while preserving these original financial controls and bounds.
