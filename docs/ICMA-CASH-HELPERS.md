# ACT/ACT ICMA cash helpers

`CashQuoteKind::ContextualIcmaYield { input }` lets a fixed-rate `CashBondHelper`
be quoted as an observed yield under the ACT/ACT ICMA convention. It uses the
same `PreparedIcmaQuotation` as the [ICMA quotations](ICMA-QUOTATIONS.md). Yield
conversion, model repricing, cash calibration, dependent quote risk and snapshot
replay all keep the full helper. Coupon, schedule, nominal time, discount,
inverse, consideration and curve mathematics are handled by the existing code.

The checks cover the synthetic USD cases declared in the tests. They do not
cover market Street yields, other conventions and models, source freshness,
independent review of the numerics, hosts that are not available here, or latency.

## Inputs and identity

Use the existing `helper` operation with `cash_quote` or `cash_reprice`, or a
full `calibration` graph. A bond helper supplies its full terms, trade and
settlement dates, payment entitlement, quote revision, last-cash pillar, quote
and absolute tolerance in the quote's unit. The new tag requires every field
below. There is no inferred context and no default budget.

| `quote_kind.input` field | Contract |
|---|---|
| `sources` | Security, observed quote and quotation-policy pins; four independent payment/reset/observation/exercise calendar pins. |
| `reference_context` | Revision, matching security revision, normal frequency and the anchored contiguous nominal reference mesh. The mesh covers the economic reference periods and every consumed payment date. |
| `convention` | Version1, reference-context and quotation-policy pins, explicit normal frequency, independent yield compounding and reference-entry work ceiling. |
| `inverse` | Closed decimal-yield lower and upper endpoints, dirty-price-per100 residual, annual parameter tolerance and 1..256 iterations. |
| `position` | Signed quantity, unit, currency and original-face reference quantity, for full consideration diagnostics. It never scales the fit. |
| `max_cash_evaluations` | Positive ceiling on cash visits, shared by the observation and every model price and inverse in one build. |

The security ID equals the helper ID, and the context pin equals the source pin.
The observed source version equals `helper.quote_version`. Changing both security
pins consistently declares a different revision. This library has no registry to
check that revision's freshness or approval. Snapshot capture must keep the
source revisions in its provenance.

Rust callers bind `CashHelperResources::IcmaBond` with full calendars and
independently declared pins. Those pins must equal the helper's sources. Shared
callers use the existing calendar-pin resources, resolved against the calendar
tables they pass in. Unversioned `Bond` resources cannot be used with this
helper. Missing, conflicting, stale or incomplete references fail before fitting.
The normal coupon frequency, the accrued basis in the contract, yield compounding
and the curve's ACT365F time stay separate.

The quote unit is explicitly `DecimalRate`, `PercentRate` or `BasisPoints`. The
observation is kept exactly as given. `PricePer100` and inferred unit conversions
are invalid here. The observation must lie in the declared inverse domain.
Decimal contractual steps, principal allocations and signed quantities keep the
existing string serialization.

## Conversion, fitting and diagnostics

Converting an observed yield discounts every entitled coupon and principal
payment with the same validated cash and time code. Dirty price, accrued or
rebate, and clean price are normalized by original face. Consideration uses the
signed quantity and dirty price.

The following are preserved without averaging:

- stepped coupons and amortization;
- split principal retirement and redemption;
- delayed claims;
- payment Include/Exclude;
- ex-coupon ownership.

Accrued resets on the payment date even when same-day cash is included. Accrued
for other unpaid future periods stays under the original contract.

Model repricing first prices the same cash with the existing government or
issuer-basis model. Its dirty price is then inverted by the same closed-domain
yield solver and converted to the helper's quote unit. The model yield and the
quote residual are separate from the dirty-price inverse residual. Floating and
optional bonds, mixed-sign inverses and other unmet model requirements keep
their explicit failures.

Conversion includes an optional `icma` quotation and `icma_work`. Bond model
valuations include the observed conversion, the model cash price, the model
inverse and cumulative reservation diagnostics. Results from the context-free
quotes omit these optional fields. Direct `cash_reprice` reports residuals in the
quote's unit and `within_tolerance`. It does not fit, and it does not silently
accept an off-market curve.

All cash recipes and contexts are validated before any graph family is fitted.
No helper is dropped for being difficult or unselected. Piecewise fitting keeps
the cash pillars and the endpoints and iteration limits. Global fitting keeps its
scales, weights, numerical controls and identification checks.

For contextual helpers the solve target also uses the inverse parameter precision
you supply, converted to the quote's unit for fit residuals. Piecewise parameter
stopping also keeps that precision in annual units. Final acceptance still needs
every helper tolerance and the dirty-price inverse tolerance. No bracket is
expanded and nothing is retried.

Quote scenarios keep their keys and hypothetical source revisions. They bump the
observed yield in its own unit, then rebuild all declared dependencies. An ICMA
quote factor's convention version must match the quotation policy. Tenor labels
use the existing positive D/W/M/Y/parallel/spot contract, and the helper identity
and revision select the observation. Signed risk keeps the five PVs and the
full-bump and half-bump stability checks.

## Bounded work

Reference preparation visits the nominal mesh once per prepared helper. The
declared reference caps sum to at most 16,777,216 entries. The mixed graph
reserves two validation and preparation passes under that ceiling before any
fit. The prepared counts per helper stay in the diagnostics.

Cash-visit caps sum to at most 67,108,864 per build or reprice. The observation
reserves one full cash pass. Before each model price is evaluated, the code
reserves one full model-price pass plus the inverse upper reservation of
`max_iterations + 8` full passes, each including the cash count plus two
boundary visits. If the budget is exhausted, the valuation fails before it runs.

The existing limits of 16,384 helper evaluations, 128 helpers and 64 recipes per
graph also apply. Fixed five-PV risk makes at most five bounded graph valuations.
Rebuilding does not widen any helper's domain, precision or per-build
reservation. These are work limits, not latency measurements.

Serialized recipes keep all source, context, policy, calendar and quantity
settings, along with quotes and numerical controls. File snapshot capture and
reopen rebuilds the graph and compares the full diagnostics. It does not
substitute the fitted nodes. Missing context, changed provenance or changed
diagnostics fail the replay. Captured native output shows reproducibility only;
it is not a financial reference.

The existing context-free `CashflowYield` and the dated output quotation routes
remain valid with their own conventions. Changing their day count tag to ICMA
cannot supply this context.

Rust users who match on or copy `CashQuoteKind` must borrow or clone it. The new
owned context makes the enum `Clone` rather than `Copy`. No provider, storage,
message queue, runtime or ledger is involved, and the C ABI keeps its existing 26
exports.

## Reproduce the checks

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

The Decimal60 controls were declared before the comparison. They keep the raw
requests, the independent cash, time, price, accrued, yield, invoice, fit and
five-PV answers, and every failed command. Separate validation records keep the
strict framing failures and correct the requests without changing financial
answers, domains or bounds. Freshly captured native outputs test the C and
adapter transport only. CI also validates the schema and prepares the Java and
Windows Excel transport tests.

The nominal time context follows the need for frequency and schedule information
described in [Strata's ACT/ACT ICMA documentation](https://strata.opengamma.io/day_counts/).
Composing through the full bond price is consistent with the
[QuantLib bond-helper implementation](https://github.com/lballabio/QuantLib/blob/master/ql/termstructures/yield/bondhelpers.cpp).
These references informed the design. They are not independent runs of the
calculation.

The existing cash-calibration test adds one explicit clone to reuse the quote
tag. Its original source is kept as an immutable fixture, and a checked migration
shows that every input, formula, bound and assertion is unchanged. Refreshing its
source fingerprint needs an explicit reviewer approval under the repository
assurance policy.
