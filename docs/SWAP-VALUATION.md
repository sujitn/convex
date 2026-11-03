# Multi-curve swap valuation and calibration

This page covers IRS and OIS valuation and full-contract rate-curve fitting. The
code is in `convex_rates` (valuation), `convex::swap_pricing` and
`convex::swap_calibration`, and it builds on the [swap contracts](RATES-CONTRACTS.md).

"Checked" below means the function validates its input and returns an error
instead of guessing.

The implementation has been compared with QuantLib 1.40 on ten cases. All 973
numeric checks passed at unchanged, predeclared tolerances. [The capture and
offline guide](SWAP-COMPARISON.md) records the scope and explains how to run the
comparison against current code. The manual reference remains a separate check.
This has not been validated against a vendor system, and market, model and release
sign-off are still outstanding.

## Start and integration

Run from the repository root:

```sh
python3 tests/scripts/swap_valuation_reference.py check
cargo test --locked -p convex-rates
cargo test --locked -p convex-analytics --test swap_calibration
cargo test --locked -p convex-curves -p convex-engine
cargo run --locked -p convex --example swap_valuation
```

The [example](../crates/convex/examples/swap_valuation.rs) prices a seasoned IRS
with published resets, checks the result against the independent reference, fits
its par-rate helper and prints the measured residuals. It also runs with
`--no-default-features` and `--features schemars`. The schema feature adds no
generated swap schemas or host dispatch.

1. Build `SwapTerms` with independently versioned calendars.
2. Pass an immutable `CurveSet`, eligible `FixingSeries`, `CashflowContext` and
   an administrator publication function. No mutable evaluation date is read.
3. Call `convex::swap_pricing::price_swap`. Keep the returned `SwapValuation` for
   cash, rates, accrued interest, par admissibility, queries and the curve
   versions used.
4. To fit curves, call `convex::swap_calibration::build_swap_curve_set` with full
   `SwapCurveRecipe` inputs and calendar bindings for every helper. The engine's
   `build_supplied_swap_curves` calls this same function.

The crates divide the work as follows:

- Core owns dates, profiles, fixing eligibility and compound arithmetic.
- Rates owns product economics and valuation.
- Curves owns representations, roles, queries and validated fitted construction.
- Math owns the bounded solvers.
- Analytics composes product fitting.
- Engine composes the application call.

Curves does not depend on rates. No database, MQ, provider, server state, runtime
or network dependency enters the pure dependency graph. Adapters supply resolved
data and pins.

## Valuation economics

| Result | Contract |
|---|---|
| Net/fixed/floating PV | Signed `ModelMoney` in the contract currency. Pay-fixed makes fixed cash negative and floating positive; receive-fixed reverses both exactly. |
| Expected coupons | Schedules, reset intervals, rounded or unrounded signed cash, and the fixing and projection lineage. No principal exchanges are inferred. |
| Discounting | Each payment uses `P(payment)/P(valuation)`, so a curve's reference date may precede the valuation date. The collateral currency and discount role are checked. |
| Projection | Simple annualized forward over the full index quotation interval, with that interval's day count. The index, cash currency and projection key and version are checked. |
| History | A complete check of outstanding published history runs before any projection. Missing due history is an error, and known eligible resets do not change under a projection bump. |
| Accrued | Signed expected cash accrued in outstanding coupons, computed with the step, reset and observation slices and rounded once per coupon. This is not a booked payment ledger. |
| Par fixed rate/spread | Decimal annualized uniform replacements and parallel additions to stepped profiles, with unsigned fixed and spread annuities. |

The floating PV is the discounted sum of the actual coupons. A discount curve does
not stand in for an index curve, and no par-floater identity replaces seasoned,
amortizing, differently geared or independently scheduled cash. Negative rates and
cash are valid under the `Allow` contract.

Par inverses require unrounded cash, `Allow` negative-coupon policies and positive
remaining fixed and floating annuities. Rounded or otherwise nonlinear contracts
still price but return `SwapParQuotes::Unavailable`, and no zero fair rate is
manufactured. A fixed replacement replaces all rate levels. A parallel shift keeps
their differences. The spread stays simple for OIS, with gearing applied once to
compounded interest.

Accrued interest is zero before accrual start. It clips each fixed or term segment
at valuation and includes the full owed coupon cash from accrual end through the
delayed payment. On the same day, `Include` keeps that cash and accrual and
`Exclude` omits it. A fully paid schedule returns zero PV and accrual without
needing unused curves or forecasting observations. Arrears, default and payment
state belong to the lifecycle and ledger routes.

OIS accrued interest clips the interest-weight windows at the **actual valuation
date**. Lookback and observation shift are already part of those windows, so the
cutoff is not shifted again. A partial holiday weight uses a partial simple factor
with the same resolved rate, followed by the simple spread on unshifted accrual
dates. The prefix operation validates every observation and requests no extra
fixing. The convention follows the [QuantLib 1.40 overnight pricer](https://github.com/lballabio/QuantLib/blob/v1.40/ql/cashflows/overnightindexedcouponpricer.cpp)
and [coupon accrued implementation](https://github.com/lballabio/QuantLib/blob/v1.40/ql/cashflows/overnightindexedcoupon.cpp).
The [discounting engine](https://raw.githubusercontent.com/lballabio/QuantLib/v1.40/ql/pricingengines/swap/discountingswapengine.cpp)
is a separate reference for NPV-date normalization. Reading that source is not the
same as running the engine and comparing results.

## Full-contract fitting

`SwapCalibrationHelper` holds full terms, an ID, a quote (decimal, percentage or
basis points), a pillar and an absolute tolerance in the quote's unit.

- The uniform fixed profile must equal the par quote.
- Unrounded cash and `Allow` policies allow the analytic quote.
- Stepped fixed profiles still price but are not relabeled as uniform par helpers.
- Notionals, floating spreads and gearing, seasoning, calendars, fixings and
  independent coupon and reset grids all carry through each trial valuation and
  the final repricing.

`SwapCurveRecipe` keeps the `CurveRecipe::Helpers` order and mode, strictly
ascending parameter dates, full helpers, dependencies and numerical settings. The
output is a dated continuous-zero representation, with the reference-date node
tied to the first parameter. Every projected index interval, payment and
valuation query that uses the fitted curve must fit within its helper pillar. A
shortened coupon stub does not shorten an `IndexTenor` quotation interval.
Extrapolation follows the curve policy.

An index fit uses a separate discount curve that you supply. Fitting a discount
curve to OIS needs a separate `overnight_projection` key and a declared
**zero-basis overnight projection** model. The resulting
`CurveSource::OvernightProjection` view carries the index ID, currency and
discount dependency you supplied, and preserves the discount function. It is never
inferred from a ticker or currency. It is a zero-basis model, not an independent
overnight basis calibration. All helpers in one such fit must use the same
overnight index. Curves for several indices, or basis curves, need their own
inputs and workflows.

Piecewise fitting uses the shared bounded Brent solver, linear continuous zeros,
one helper per positive parameter pillar, finite zero brackets and iteration
ceilings. Helpers are solved in pillar order, but outputs keep source order, so
reordering the quote input does not change the curve. The quotes at the bracket
endpoints must distinguish the parameter at the native tolerance. Domain,
missing-data and nonfinite errors stop the fit.

Global fitting uses the shared bounded least-squares solver with the parameter
starts and bounds, per-quote scales and weights, finite-difference controls and
convergence and rank diagnostics. It requires numerical convergence and each
quote's own tolerance, so a scaled RMS alone cannot approve a curve. These cases
fail without returning a partial curve:

- Bad dimensions.
- Unidentifiable or rank-deficient inputs.
- Unusable brackets.
- Exhausted work or iteration limits.

The batch validates all contracts, fixes administrator publication timestamps once
per unique index and date, and collects missing eligible history across the whole
batch before fitting. Exact-version dependencies give a stable build order with
dependencies first. Missing mappings or versions, duplicates and cycles fail as a
whole, and the curves you pass in are never modified. The final diagnostics keep
the model-minus-input residuals, tolerances, helper valuations and solver work.
The fitted constructor checks these records and keeps the calibrated recipe,
instead of labeling the solved nodes as direct input.

The bounds are 128 total helpers and parameters, 256 total curves, 32,768 unique
index and date publications and 8,192 helper valuations per batch, in addition to
the product and solver bounds. These are work ceilings, not measured throughput or
latency. Inputs and callbacks must be deterministic, must terminate and must not
panic. The solver is not meant for a live-price loop.

## Common results and scope

`SwapValuation::price_result` emits exactly net, fixed and floating PV in the
common v2 envelope. It checks the trade ID and version, the economic date, the
information cutoff, the calendar and collateral pins, the published fixing
revisions and all curve recipes used, including transitive bases. The caller binds
the opaque curve versions to numeric record revisions, and no version is parsed or
invented. Supplied curves that were not used are not reported as used.

The manifest is limited to 256 records. A large seasoned overnight input can price
without trouble and still exceed this limit with its per-fixing manifest. The
bridge then returns an error, so it is a conditional capability. Snapshot, config,
model, build and calculation time and data quality are values the application
supplies. "Complete" means the three requested PV measures are present. It does
not mean every product measure is available.

Quote-bump and rebuild sensitivities, trade events and lifecycle, host and schema
dispatch (FFI, Java, Excel, MCP, WASM) and server routing are handled elsewhere.
The tenor-only helper and hedge approximations keep their own routes. The new
entry points do not imply cached synthetic hedge risk, a market pack, pre-trade
checks, execution or latency guarantees.

## Evidence

[Twenty manual reference cases](../crates/convex-rates/tests/fixtures/valuation.json)
cover seasoned IRS and OIS, amortization and steps, negative and zero rates,
payment delay, stubs, lookback, shift and lockout, same-day entitlement and
maturity. The [independent Python oracle](../tests/scripts/swap_valuation_reference.py)
uses `datetime`, 60-digit Decimal, separately pinned dates and explicit zero grids,
and it imports neither Convex nor QuantLib. The bounds for the new fixtures are
1e-7 absolute currency PV and accrued interest at 1e6 notional, 1e-12 for decimal
par rates, 1e-13 for discount ratios, and exact dates. Existing artifacts,
expected values, tolerances and solver defaults are preserved. Cash details and
the repricing of fitted helpers have separate positive, boundary and failure
tests.

The QuantLib 1.40 comparison covers seasoned and amortizing IRS and OIS, with
matched calendars, eligible fixes, quotation dates, curves, payment lag and dated
economic steps. The runtime wheel and installed publisher bytes were verified
before import. Coupon objects and `DiscountingSwapEngine` computed the reference.
All ten cases and 973 numeric values match within the bounds, and the cashflow,
reset and fixing lineage match exactly. No pricing or calibration code changed for
the comparison.

[The capture](../tests/fixtures/comparisons/swap-quantlib-1.40/manifest.json)
records the execution and build details. It is frozen separately from the manual
fixtures and the retained bond pack. Follow the
[capture and offline guide](SWAP-COMPARISON.md) to compare a newly built candidate
without QuantLib or network access. The positive, boundary and failure checks and
the full-contract calibration checks remain required. Stubs, intracoupon resets,
rounding and other convention and model combinations outside the external suite
need more comparison coverage. All 2,154 mandatory library records remain Planned.
