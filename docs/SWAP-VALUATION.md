# Supplied multi-curve swap valuation and calibration

CVX-019 adds native IRS/OIS valuation and full-contract rate-curve fitting over
the [CVX-018 contracts](RATES-CONTRACTS.md). The native implementation is an
engineering implementation. Its ten-case QuantLib 1.40 acceptance comparison has
executed and passed all 973 numeric checks at unchanged predeclared tolerances.
[The capture and offline guide](SWAP-COMPARISON.md) records the scope and how to
run current-native regression comparisons. This satisfies the scoped CVX-019
engineering task; independent market/model, library, host and release certificates
remain pending. The manual oracle remains a separate reference.

## Start and integration

Run from the repository root on `codex/production-readiness`:

```sh
python3 tests/scripts/swap_valuation_reference.py check
cargo test --locked -p convex-rates
cargo test --locked -p convex-analytics --test swap_calibration
cargo test --locked -p convex-curves -p convex-engine
cargo run --locked -p convex --example swap_valuation
```

The [standalone consumer](../crates/convex/examples/swap_valuation.rs) prices a
seasoned IRS with actual published resets, checks its independent reference,
fits its full native par-rate helper and prints measured residuals. It also
runs with `--no-default-features` and `--features schemars`. Schema feature
compatibility does not add generated swap schemas or host dispatch.

1. Build supplied `SwapTerms` with actual independently versioned calendars.
2. Supply an immutable `CurveSet`, eligible `FixingSeries`, `CashflowContext`
   and administrator publication function; no mutable evaluation date is read.
3. Call `convex::swap_pricing::price_swap`. Keep its complete `SwapValuation`
   for cash, rates, accrued, par admissibility, queries and consumed versions.
4. For fitting, call `convex::swap_calibration::build_swap_curve_set` with
   full `SwapCurveRecipe` inputs and actual calendar bindings for every helper.
   The engine's `build_supplied_swap_curves` delegates to this same function.

Core owns dates, profiles, fixing eligibility and compound arithmetic; rates
owns product economics and valuation; curves owns representations, roles,
queries and validated fitted construction; math owns bounded solvers; analytics
composes product fitting; engine composes the application call. Curves does not
depend on rates. No DB, MQ, provider, server state, runtime or network dependency
enters the pure normal dependency graph. Adapters supply resolved data and pins.

## Valuation economics

| Result | Contract |
|---|---|
| Net/fixed/floating PV | Signed `ModelMoney` in absolute contract currency. Pay-fixed makes fixed cash negative and floating positive; receive-fixed reverses both exactly. |
| Expected coupons | Original actual schedules, reset intervals, rounded/unrounded signed cash and fixing/projection lineage. No principal exchanges are inferred. |
| Discounting | Each actual payment uses `P(payment)/P(valuation)`, allowing a curve reference date before valuation. Exact collateral currency and discount role are checked. |
| Projection | Simple annualized forward over the full represented index quotation interval, with that interval's day count. Exact index, cash currency and projection key/version are checked. |
| History | Complete outstanding published-history preflight precedes every projection. Missing due history returns an error; known eligible resets do not change under a projection bump. |
| Accrued | Signed expected cash accrued in outstanding coupons, computed with actual step/reset/observation slices and once-per-coupon rounding. This is not a booked payment ledger. |
| Par fixed rate/spread | Decimal annualized uniform replacements and parallel additions to original stepped profiles, with explicit unsigned fixed/spread annuities. |

The floating PV is the discounted actual coupon sum. A discount curve does not
stand in for an index curve and no par-floater identity replaces seasoned,
amortizing, differently geared or independently scheduled cash. Negative rates
and cash remain valid under the original `Allow` contract.

Par inverses require unrounded cash, `Allow` negative-coupon policies and
positive remaining fixed/floating annuities. Rounded or otherwise nonlinear
contracts still price, but return `SwapParQuotes::Unavailable`; there is no
manufactured zero fair rate. A fixed replacement replaces all rate levels;
a parallel shift retains their original differences. Spread remains simple
for OIS, with gearing applied once to compounded interest.

Accrued is zero before accrual start, clips each actual fixed/term segment at
valuation, and includes full owed coupon cash from accrual end through delayed
payment. Same-day `Include` retains that cash/accrual; `Exclude` omits it.
Fully paid schedules return zero PV/accrual without requiring unused curves or
forecasting invented observations. Arrears/default/payment state needs the
owning lifecycle and ledger routes.

OIS accrued clips the already represented interest-weight windows at the
**actual valuation date**. Lookback/observation shift is already incorporated
in those windows; the cutoff is not shifted again. A partial holiday weight
uses a partial simple factor with the same resolved rate, followed by the
simple spread on unshifted accrual dates. The prefix operation validates every
represented observation and requests no additional fixing. This convention is
grounded in the [QuantLib 1.40 overnight pricer](https://github.com/lballabio/QuantLib/blob/v1.40/ql/cashflows/overnightindexedcouponpricer.cpp)
and [coupon accrued implementation](https://github.com/lballabio/QuantLib/blob/v1.40/ql/cashflows/overnightindexedcoupon.cpp).
The [discounting engine](https://raw.githubusercontent.com/lballabio/QuantLib/v1.40/ql/pricingengines/swap/discountingswapengine.cpp)
provides a separate reference for NPV-date normalization. Source inspection is
not executed external-engine evidence.

## Full-contract fitting

`SwapCalibrationHelper` contains full supplied terms, original ID, native
decimal/percentage/basis-point quote, explicit pillar and absolute tolerance in
that original unit. The uniform fixed profile must equal the input par quote;
unrounded cash and `Allow` policies admit the analytic quote. General stepped
fixed profiles remain priceable but are not relabeled as uniform par helpers.
Notionals, floating spreads/gearing, seasoning, calendars, fixings and independent
coupon/reset grids survive each trial valuation and final repricing.

`SwapCurveRecipe` retains the original `CurveRecipe::Helpers` order and mode,
strictly ascending parameter dates, full helpers, exact dependencies and
explicit numerical settings. Output is a dated continuous-zero representation
with the reference-date node tied to the first parameter. Every projected
index interval, payment and valuation query that uses the fitted curve must
fit within its helper pillar. A shortened coupon stub does not shorten an
`IndexTenor` quotation interval. Extrapolation remains the explicit curve policy.

An index fit consumes a separate exact supplied discount curve. Discount-curve
OIS fitting requires a separate `overnight_projection` key and a declared
**zero-basis overnight projection** model. The resulting
`CurveSource::OvernightProjection` view carries the actual supplied index ID,
currency and exact discount dependency, and preserves the discount function.
It is never inferred from a ticker or currency. It is a zero-basis model, not
an independent overnight basis calibration. Helpers in one such discount fit
must use that same overnight index. Multi-index projection/basis curves require
their own explicit inputs and owning workflows.

Piecewise fitting uses the shared bounded Brent solver, linear continuous
zeros, one helper per positive parameter pillar, explicit finite zero brackets
and iteration ceilings. Helpers are solved in pillar order while original
source order survives in outputs; reordering quote input does not change the
curve. Bracket endpoint quotes must distinguish the parameter at the supplied
native tolerance. Domain, missing-data and nonfinite errors stop fitting.

Global fitting uses shared bounded least squares with the original parameter
starts/bounds, per-quote scales and weights, finite-difference controls and
convergence/rank diagnostics. It requires both numerical convergence and each
original native quote's tolerance; scaled RMS alone cannot approve a curve.
Bad dimensions, unidentifiable/rank-deficient inputs, unusable brackets and
exhausted work/iteration limits fail without a partial accepted curve.

The batch validates all contracts, freezes administrator publication timestamps
once per unique index/date, and collects missing eligible history across the
whole batch before fitting. Exact-version dependencies determine a stable
dependency-first build order. Missing mappings/versions, duplicates and cycles
fail atomically; supplied curves remain immutable. Final diagnostics retain
native model-minus-input residuals, tolerances, full helper valuations and
actual solver work. The fitted constructor checks these records and preserves
the original calibrated recipe instead of labeling solved nodes as direct input.

Bounds are 128 total helpers/parameters, 256 total curves, 32,768 unique
index/date publications and 8,192 individual helper valuations per atomic
batch, plus existing product and shared solver bounds. These are work ceilings,
not measured throughput or latency guarantees. Inputs and callbacks must be
deterministic, terminate and avoid panic; the solver is not a live-price loop.

## Common results and scope

`SwapValuation::price_result` emits exactly net/fixed/floating PV in the common
v2 envelope. It checks trade ID/version, economic date, information cutoff,
calendar and collateral pins, actual published fixing revisions and all consumed
curve recipes including transitive bases. The caller explicitly binds opaque
native curve versions to canonical numeric record revisions; no version is
parsed or invented. Unused supplied curves are not falsely reported consumed.

The canonical manifest has a 256-record bound. Large seasoned overnight inputs
may price natively while their per-fixing manifest exceeds this bridge; the
bridge returns an error and is a conditional capability. An authoritative
aggregate immutable manifest/schema belongs to the owning common host work.
Snapshot/config/model/build/calculation time and data quality remain supplied
application values. Complete means the three requested PV measures are present,
not that every product measure or production certificate is complete.

Quote-bump/rebuild sensitivities remain CVX-057; trade events and lifecycle
remain CVX-061/062; host/schema/FFI/Java/Excel/MCP/WASM dispatch remains CVX-063
and server routing CVX-033. Existing tenor-only helper and hedge approximations
retain their distinct unqualified routes. No cached synthetic hedge risk, actual
market pack, pretrade/execution, operational or colocation certification is
implied by the new native entry points.

## Evidence and remaining comparator gate

[Twenty new manual reference cases](../crates/convex-rates/tests/fixtures/valuation.json)
cover seasoned IRS/OIS, amortization/steps, negative/zero rates, payment delay,
stubs, lookback/shift/lockout, same-day entitlement and maturity. The
[independent Python oracle](../tests/scripts/swap_valuation_reference.py) uses
`datetime`, 60-digit Decimal, separately pinned dates and explicit zero grids;
it imports neither Convex nor QuantLib. New fixture bounds are 1e-7 absolute
currency PV/accrued at 1e6 notional, 1e-12 decimal par rates, 1e-13 discount
ratios and exact dates. Existing artifact bytes, expectations, tolerances and
solver defaults are preserved. Native cash details and full calibrated helper
repricing have separate positive, boundary and failure tests.

CVX-019 AC2 now has actual seasoned **and amortizing** IRS/OIS outputs from
QuantLib 1.40, with matched supplied calendars, eligible fixes, quotation dates,
curves, payment lag and dated economic steps. The runtime wheel and installed
publisher bytes were verified before import; actual coupon objects and
`DiscountingSwapEngine` computed the reference. All ten cases and 973 numeric
values match within the original bounds. Exact cashflow/reset/fixing lineage
matches too. No native pricing or calibration code changed for this comparison.

[The complete capture](../tests/fixtures/comparisons/swap-quantlib-1.40/manifest.json)
retains actual execution/build provenance and is frozen separately from the
original manual fixtures and retained bond pack. Follow the
[capture/offline guide](SWAP-COMPARISON.md) to compare a newly built native
candidate without QuantLib or network access. Positive/boundary/failure native
and full-contract calibration checks remain required. Stub, intracoupon reset,
rounding and other convention/model combinations outside the supplied external
suite require further comparison coverage.

CVX-019 may enter the engineering completion set. All 2,154 mandatory library
records remain Planned and no library, production or low-latency release gate
is awarded.
