# Credit and issuer-basis cash hedge risk

`cash_basis_hedge_risk` builds a hedge column for a cash bond that covers every
common factor: discount, hazard, issuer basis and projection. It combines the
cash helper, the issuer valuation, the calibration graph and the five-price risk
calculation. It supports fixed cash with coupon steps, principal retirements,
calendars, settlement and contextual ACT/ACT ICMA quotations. The resulting vector
goes into the existing `hedge_aggregate` and `hedge_solve` operations alongside
CDS, IRS/OIS and rates-futures columns.

Run the full example, which includes independent cash comparisons and USD/EUR
mixed-book proposals:

```sh
cargo run --locked -p convex --example cash_basis_hedge
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test cash_basis_hedge -- --test-threads=1
```

The example emits fresh transport envelopes. Its `--emit-schema` switch requires
`--features schemars`. The default, minimal and schema builds use the same
financial code. Request fixtures are in `tests/fixtures/library/cash-basis-hedge-api.json`,
`cash-basis-hedge-precise.json` and `cash-basis-hedge-book-columns.json`.

## Inputs and results

The strict operation contains `graph`, the complete `security` (the cash helper
and exact calendar pins), and `input`:

| Field | Meaning |
| --- | --- |
| `basis` | Exact issuer-spread curve key in the graph. |
| `model` | `ReferenceZeroSpread` or `CreditParRecovery` model. |
| `credit` | Optional full `BondCdsHedgeInput`, validated by the legal mapping step. Required for the credit model. |
| `basis_tolerance` | Explicit decimal-rate comparison bound in `(0, 1e-10]` for mapped baseline basis observations. |
| `hedge` | Complete identity, positive face reference in the bond's currency, ordered factors, snapshot and factor-work bound. |

The reference quantity must equal the positive face in the bond's currency.
Current, pending and proposed signed positions belong to the downstream hedge
book. A signed observed ICMA quotation stays signed in its quotation and invoice;
it does not replace the positive risk reference.

The result keeps:

- the helper, the model and the validated mapping;
- the observed conversion and the full model valuation;
- the model-implied quote and the model-minus-observed residual;
- the contextual inverse and its work count;
- the hedge vector.

Observed cash and model PV stay separate. A finite off-market residual is a
diagnostic, not an accepted calibration fit. Prices per 100, money on the face
amount, percentage-point yield, decimal contextual ICMA yield and common curve risk
units keep their usual meanings.

Every factor returns the baseline, up/down and half-bump up/down PVs, key,
settings, quantity, units and snapshot. An unrelated projection curve you pass in
is still repriced; an observed zero is not filled in. Curve versions are opaque
exact keys. They are not converted into the numeric revisions used by security,
mapping or calendar pins.

## Model and credit scope

`CreditParRecovery` uses the promised-cash and par-recovery model. The issuer
basis applies to promised cash. Recovery follows the existing discount and survival
integration over the principal profile.

These guards stay in place: ACT365F, log-linear discount-factor and survival
grids, a common origin, no extrapolation, immediate principal payment and the
recovery domain. Delayed principal, default lifecycle cash and optional-model
substitution are rejected. Accrual, quotation compounding, ICMA time and payment
dates are independent inputs.

The explicit mapping keeps these items:

- bond identity and revision, entity and seniority;
- CDS documentation, restructuring and recovery;
- survival key, basis observations and model;
- a separate liquidity assumption.

Mapped basis observations must match the baseline curve at every supplied date
within `basis_tolerance`, and the bond/CDS horizon check applies. Calibration
residuals are not substituted for this comparison.

A reference-only column can also carry an explicit validated mapping for its
issuer-basis factor. With no mapping it has no credit scope, so the hedge step
rejects a nonzero credit-family factor that needs that scope. Neither case
manufactures hazard or recovery exposure. Direct yield risk remains a separate
own-security factor, described in [ICMA yield risk](ICMA-YIELD-RISK.md).

For a recovery scenario, the cash composition binds the model's recovery parameter
to the scenario's credit role. Frozen nodes you pass in, and market-quote or
dependent rebuilds, go through the graph code. Unsupported calibrated-node moves
are rejected with the same typed error as before.

ORE exposes discount, default, recovery and security-spread inputs separately;
its spread configuration and recovery cash handling must be aligned before a
numerical comparison. This is a design reference only: no ORE comparison has been
run. See the [ORE risky-bond engine](https://github.com/OpenSourceRisk/Engine/blob/master/QuantExt/qle/pricingengines/discountingriskybondengine.cpp).

## Bounds and failure behavior

Input validation requires 1–16 distinct complete factors and an explicit 1–80
factor evaluation bound. Five evaluations per factor are reserved before
valuation. These are graph callback counts, not a bound on total calibration work
or latency.

Contextual cash and time preparation happens once. It keeps the source-entry
bounds and reserves each complete model-price-plus-inverse pass before valuation,
including the baseline inverse in native units. Graph calibration and quotation
solvers keep the bounds you supply for them.

Missing or incompatible mapping, basis observations, calendar revision, security,
quantity, factor, model, domain, contextual work or graph input returns the
existing typed error envelope. A failure produces no partial column. The
operation adds no provider, persistence, queue, clock, order or execution
authority.

The calculation is in `convex-analytics`. The shared adapter resolves the full
inputs and delegates to it. The C, MCP and WASM adapters go through the same
shared dispatcher.

## Scope and open items

Independent Decimal60 controls cover:

- positive and negative rates, clean and dirty price, percentage yield and
  contextual ICMA yield;
- the reference and credit models;
- coupon and principal steps;
- full five-PV discount, hazard, basis and recovery risk;
- an unrelated projection curve, recovery endpoints and opaque versions;
- helper calibration and dependent rebuilds.

The full mixed book uses discount, hazard, issuer basis and projection factors
with cash, CDS, IRS and term-futures candidates. It checks current and pending
positions, exact contract lots, side costs, constraints and explicit same-snapshot
USD/EUR FX. Recovery is covered separately by the cash controls. The CDS graph
code still needs its own recovery-coordinate binding before recovery can be added
to this common book. The solver fails numerically on the original broad
limits; the proposals shown use explicitly declared financial limits and scales.

Not yet covered: full optional-model risk, all-member delivery-futures risk,
execution on JVM/JNI, Windows/.NET/Excel and wasm32/browser, and independent
model, market and latency validation. Compiling the WASM build does not show that
it runs in a browser. Server limits and order authority are outside this library.
[CDS recovery scenarios and the full five-factor mixed book](CDS-RECOVERY-HEDGE-RISK.md)
extend this work.
