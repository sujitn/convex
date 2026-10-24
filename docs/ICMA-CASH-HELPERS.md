# Contextual ACT/ACT ICMA cash helpers

`CashQuoteKind::ContextualIcmaYield { input }` admits an observed yield on a
complete fixed `CashBondHelper`, using the same `PreparedIcmaQuotation` as
[contextual quotations](ICMA-QUOTATIONS.md). Observation conversion, model
repricing, native cash calibration, dependent quote risk and snapshot replay
retain the full original helper. Coupon, schedule, nominal time, discount,
inverse, consideration and curve mathematics have their existing owners.

This is supplied-input engineering. CVX-063 remains partial, unaccepted and
uncertified; all 2,154 mandatory records remain Planned. The controls establish
the declared synthetic USD cases. They do not establish market Street yields,
all conventions/models, external source freshness, an independent financial
approval, actual unavailable hosts, production readiness or a latency SLA.

## Inputs and identity

Use the existing `helper` operation with `cash_quote` or `cash_reprice`, or an
existing complete `calibration` graph. A bond helper still supplies its full
terms, trade/settlement, payment entitlement, quote revision, actual last-cash
pillar, original native quote and absolute native-unit tolerance. The new tag
requires every field below; there is no inferred context or default budget.

| `quote_kind.input` field | Contract |
|---|---|
| `sources` | Complete security, observed quote and quotation-policy pins; four actual independent payment/reset/observation/exercise calendar pins. |
| `reference_context` | Exact revision, matching security revision, normal frequency and the full anchored contiguous nominal reference mesh covering original economic references and every consumed payment date. |
| `convention` | Version1, exact reference-context and quotation-policy pins, explicit normal frequency, independent yield compounding and reference-entry work ceiling. |
| `inverse` | Original closed decimal-yield lower/upper endpoints, dirty-price-per100 residual, annual parameter tolerance and 1..256 iterations. |
| `position` | Original signed quantity, unit, currency and original-face reference quantity, for complete consideration diagnostics. It never scales the fit. |
| `max_cash_evaluations` | Explicit positive cash-visit reservation ceiling across the observation and every model price/inverse in one build. |

The security ID equals the helper ID; its context pin equals its source pin.
The observed source version equals `helper.quote_version`. Changing both
security pins consistently declares a different supplied revision. There is no
registry in this pure library to assert that revision's freshness or approval.
Snapshot capture must retain the source revisions in its actual provenance.

Native callers bind `CashHelperResources::IcmaBond` with actual full calendars
and independently declared pins. Those pins must equal the helper's sources.
Shared callers use the existing calendar-pin resources, resolved against the
actual supplied calendar tables. Unversioned `Bond` resources cannot admit this
helper. Missing, conflicting, stale or incomplete bound references fail before
fitting. Normal coupon frequency, contractual accrued basis, yield compounding
and curve ACT365F time remain separate.

The quote unit is explicitly `DecimalRate`, `PercentRate` or `BasisPoints`.
The original observation is retained exactly. `PricePer100` and inferred unit
conversions are invalid here. The observation must lie in the original declared
inverse domain. Decimal contractual steps, principal allocations and signed
quantities retain the existing exact string serialization.

## Conversion, fitting and diagnostics

Observed yield conversion discounts every original entitled coupon and
principal payment through the same checked contextual cash/time owner. Dirty
price, actual accrued/rebate and clean price normalize by original face.
Consideration uses original signed quantity and dirty price. Stepped coupons,
amortization, split principal retirement/redemption, delayed claims, payment
Include/Exclude and actual ex-coupon ownership are preserved without averaging.
Accrued resets on the actual payment date even when same-day cash is included;
other unpaid future-period accrued remains under its original contract.

Model repricing first prices that same full cash using the existing government
or explicit issuer-basis model. Its dirty price is then inverted by the same
contextual closed-domain yield owner and converted to the helper's original
native unit. The model yield and native quote residual are separate from the
original dirty-price inverse residual. Inapplicable floating, optional,
mixed-sign inverse or other model requirements retain their explicit failures.

Conversion includes an optional complete `icma` quotation and `icma_work`.
Bond model valuations include the complete observed conversion, original model
cash price, full contextual model inverse and cumulative reservation diagnostics.
Old context-free results omit these optional fields. Direct `cash_reprice`
reports original native residuals and `within_tolerance`; it does not fit or
silently accept an off-market supplied curve.

All supplied cash recipes and original contexts are admitted before any graph
family's fit. No difficult or unselected helper is removed. Piecewise fitting
retains actual cash pillars and original endpoints/iteration limits; global
fitting retains original scales, weights, numerical controls and identification.
For contextual helpers the solve target additionally uses the supplied inverse
parameter precision, converted to original native units for fit residuals.
Piecewise parameter stopping also retains that precision in annual units.
Final acceptance still requires every original native helper tolerance and
original inverse price tolerance. No bracket expansion or retry is introduced.

Quote scenarios retain original keys and hypothetical source revisions, bump
the actual observed yield in its original units, then rebuild all declared
dependencies. An ICMA quote factor's convention version must match the actual
quotation policy. Tenor labels use the existing positive D/W/M/Y/parallel/spot
contract; actual helper identity and revision select the observation. Signed
risk retains the original five PVs and full/half-bump stability checks.

## Bounded work and reproducibility

Reference preparation visits the complete nominal mesh once per prepared
helper. Declared reference caps sum to at most16,777,216 entries; the mixed graph
reserves two admission/preparation passes under that aggregate ceiling before
any numerical fit. Actual per-helper prepared counts remain in diagnostics.

Cash-visit caps sum to at most67,108,864 per build or reprice. Observation
reserves one complete cash pass. Before each model price is evaluated, the owner
reserves one full model-price pass plus the unchanged inverse upper reservation
of `max_iterations + 8` full passes, each including the original cash count
plus two boundary visits. Exhaustion fails before that valuation. Existing
16,384 helper evaluations, 128 total helpers and 64-recipe graph limits also
apply. Fixed five-PV risk makes at most five bounded graph valuations; rebuilding
does not expand any helper's domain, precision or per-build reservation. These
explicit arithmetic bounds are work limits, not latency measurements.

Complete serialized recipes retain all source/context/policy/calendar/quantity,
native quotes and numerical controls. File snapshot capture/reopen rebuilds the
actual graph and compares complete diagnostics; it does not substitute fitted
nodes. Missing context, changed provenance or captured diagnostics fail replay.
Native capture output is reproducibility evidence, never a financial oracle.

The existing context-free `CashflowYield` and explicitly dated output quotation
routes remain valid with their own original conventions. Changing their day
count tag to ICMA cannot supply this context. Rust users matching or copying
`CashQuoteKind` must borrow or clone it; the new owned context makes the enum
`Clone` rather than `Copy`. No provider, storage, MQ, runtime or ledger authority
is introduced, and the C ABI retains its existing26 exports.

## Reproduce the supplied engineering controls

```sh
python3 tests/scripts/icma_helpers_reference.py
python3 tests/scripts/icma_helpers_qualification.py
python3 tests/scripts/icma_helpers_boundaries.py
python3 tests/scripts/icma_helpers_api.py
python3 tests/scripts/icma_helpers_terms.py
python3 tests/scripts/icma_helpers_admission.py
python3 tests/scripts/icma_helpers_failures.py
python3 tests/scripts/icma_helpers_compatibility.py
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test icma_helpers -- --test-threads=1
cargo test --locked --all-features -p convex-engine --test icma_helpers_snapshot
cargo run --locked -p convex --example icma_helpers
cargo run --locked -p convex --example icma_helpers --no-default-features
cargo run --locked -p convex --example icma_helpers --features schemars
```

The predeclared Decimal60 controls retain the raw requests, complete independent
cash/time/price/accrued/yield/invoice/fit/five-PV answers and every failed command.
Separate admission records retain strict framing failures and correct the
requests without changing financial answers, domains or bounds. Fresh native
envelopes test actual C and adapter transport only. CI also validates the actual
schema and prepares actual Java and Windows Excel transport tests; local
availability is reported in private execution evidence.

The nominal time context follows the need for frequency and schedule information
described in [Strata's ACT/ACT ICMA documentation](https://strata.opengamma.io/day_counts/).
Composition through the full original bond price is consistent with the
[QuantLib bond-helper implementation](https://github.com/lballabio/QuantLib/blob/master/ql/termstructures/yield/bondhelpers.cpp).
These design references are not new independent executions or certificates.

The existing cash-calibration test adds one explicit clone to reuse its quote
tag. Its exact original complete source is retained as an immutable fixture,
and a checked migration proves every input, formula, bound and assertion is
unchanged. Its live source fingerprint requires an explicit refresh with
independent reviewer approval under the repository assurance policy.
