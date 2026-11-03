# Mixed rate calibration

This document covers `convex::rate_calibration::build_rate_curve_set`. From dated
deposit, FRA, rate-futures, IRS, OIS and tenor-basis instruments that you pass
in, it builds an immutable OIS discount curve and independent index-projection
curves. It reuses the existing product pricers and bounded numerical kernels.
No provider, database, broker, runtime, registry or wall clock enters the
calculation.

[Product-price parametric fitting](PRODUCT-PARAMETRIC-CALIBRATION.md) adds an
NS/Svensson mode to the same helper code, with exact parameters, residuals in
source order, quote-risk rebuilding and schema-3 replay.

`convex::rate_calibration::reprice_rate_helpers` is a separate diagnostic. It
reprices a recipe on curves you pass in, without fitting or changing them. It
reuses the same input validation, history preflight, product valuations and
horizon guards, and returns residuals in the quote's own unit, the full product
valuations and `within_tolerance`. An off-market diagnostic is not an accepted
calibration. The shared request is documented in [the library API](LIBRARY-API.md).

Run the offline standalone consumer from the repository root:

```sh
cargo run --locked -p convex --example rate_calibration
cargo run --locked -p convex --example rate_calibration --no-default-features
cargo run --locked -p convex --example rate_calibration --features schemars
python3 tests/scripts/rate_calibration_reference.py check
cargo test --locked -p convex-analytics --test rate_calibration
```

The example reads synthetic contracts, calendars, histories and quotes from
[mixed-rates.json](../tests/fixtures/curves/mixed-rates.json), checks the
independent reference parameters and prints every residual and the helper
valuation count. The fixtures are synthetic, not live market observations.

## Inputs

Each `RateCurveRecipe` keeps its `CurveRecipe`, the ordered helper identities,
the parameter dates, the product terms, the `MarketQuote` and a positive
absolute tolerance in the quote's own unit. `RateCurveBuildInput` supplies one
matching `RateHelperResources` per helper, in the same order. Terms deserialize
strictly; deserializing alone does not validate a product.

| Helper | Economics and quote |
|---|---|
| Deposit | Business value/maturity dates, pinned calendar and context-free date day count; simple annualized rate in decimal, percent or basis points. |
| FRA | Separate index fixing, value start/end, explicit start/end settlement and eligible publication-aware fixing; annualized simple rate in its original rate unit. |
| Rate future | Full `FuturesContract`, actual observation/value window, calendar, clearing/history selection and convexity model pins; native `FuturePrice100MinusRate` price points. |
| IRS/OIS | Full `SwapTerms`, actual independent schedules, dated notional/spread/gearing profiles, fixing policies and exact discount/index mappings; original uniform par-fixed rate. |
| Tenor basis | Full `BasisSwapTerms`, independent floating schedules/projection indices and external collateral discount; original uniform absolute spread on an explicitly named paid or received leg. |

Deposit/FRA day counts are ACT/360, ACT/365F, ACT/ACT ISDA, 30/360 US or 30E/360.
Contextual ICMA money-market quotes need a reference-period model and fail. FRA
settlement is the declared value start or end; fitting the rate does not invent
a cash settlement price. A FRA that has already fixed can reprice from history
but cannot identify a new curve parameter by itself.

Swap and basis helpers require unrounded Allow-negative cash and a uniform
quoted fixed-rate or named spread profile. Other notional, spread and gearing
steps stay intact. The profile and the quote must agree exactly, in decimal-rate
or native floating-point units. Decimal strings are converted to floating point
with correct rounding for this check; unit scaling adds no financial tolerance.
A materially different embedded quote fails. Inverse quotes are distinct from
parallel shifts of all existing spread steps.

Rate futures keep arithmetic or compounded overnight holiday weights, partly
fixed windows and the separation of single-term fixing and value dates. They use
the [futures valuation](FUTURES-VALUATION.md) convention:
`forward rate = futures-implied rate + adjustment`, applied once to the whole
quote. An explicit zero adjustment is allowed; a missing one fails. Final
settlement precision stays separate from the unrounded calibration price and
exchange trading ticks. Synthetic or stale inputs keep their source blocks and
cannot become execution authority by passing calibration.

## Basis pricing

`convex::rates::basis::BasisSwapTerms::build` and `price_basis_swap` are pure
public APIs for constructing and pricing a two-floating-leg swap. Paid cash/PV is
negative and received cash/PV positive; negative coupons keep their economic sign.
Results include the terms, signed expected cash/accrued/PV, every rate origin,
the projection/discount queries and the recipes used. No principal exchanges or
cross-currency economics are supported.

The implementation reuses the `SwapContract` floating-leg caches and the
`price_swap` engine through a private zero fixed-coupon adapter on that leg's
coverage. That fixed leg exists only for validation; it is absent from basis
inputs and reported results and contributes no nonzero cash. Reset, overnight,
profile, accrued and history calculations have one implementation.

For positive remaining unrounded spread exposure, `BasisParQuotes` provides
uniform replacement spreads on either leg and additive shifts to each leg's
existing steps, with separate positive spread annuities. These are linear spread
inverses, with spread outside overnight compounding. Rounded, clipped or expired
cash has an explicit unavailable inverse. Both histories are preflighted before
either leg forecasts; fully paid trades return zero with no invented inverse.

## Curve graph and history

Discount and projection identities are separate, versioned `CurveKey` values.
Projection role, index and currency must agree with the product. Basis fitting
calibrates exactly one independently projected leg against the other leg's
external projection curve and the external collateral discount curve. Same-index
or single-curve substitution fails.

Discount OIS fitting declares an `OvernightProjectionBinding`: a separately
named, versioned zero-basis overnight view of that discount curve. It is a model
assumption; it must be used and must match the named overnight index. Nothing is
inferred from the currency or index name.

Declared external dependencies must equal the union of all curve versions the
helpers use. Missing versions, unused declarations, duplicate identities and
cycles fail. Immutable curves you pass in cannot be replaced, even by another
version of the same identity. The full topology is checked before solving any
curve; output order is deterministic, with dependencies before their users and
views.

Pass a `RateCalibrationContext` with an economic timestamp, a matching UTC
valuation date, a known-at cutoff, an unpublished/final fixing policy and
payment-date entitlement. Administrator publication times are frozen once per
unique index and observation date across the whole graph. All due histories
across all products are resolved before any trial forecast. Every missing due
observation is kept in the typed missing list. Future projections cannot
overwrite an eligible known rate. Futures keep their explicit FirstPublished
versus LatestEligible clearing rules; swap/FRA history keeps its checked
eligible-revision selection. No calendar or fixing source is invented.

Parameter dates and helper pillars are explicit. Every projected quotation end
and fitted-curve payment date must fit that helper's pillar; delayed payments
and index-tenor dates can exceed contractual maturity. Enabling extrapolation
does not excuse an insufficient pillar. The curve horizon, representation,
interpolation and query postconditions still apply.

## Supported numerical combinations

| Mode | Representation/interpolation | Validation and diagnostics |
|---|---|---|
| Piecewise | Dated ACT/declared-time continuous zeros; Linear | One helper per positive parameter date; explicit finite closed zero bracket and bounded Brent iterations. Duplicate pillars fail. Origin zero ties to the first parameter. |
| Global | Same continuous-zero grid; Linear or CubicSpline | Caller-provided starts, closed zero bounds, positive quote scales and weights; shared bounded least squares, measured Jacobian rank/conditioning, objective and per-quote thresholds. |

Zero brackets lie within [-10,10] annualized decimal units. `RetainAllGlobal`
keeps every overlapping quote, weight, residual and threshold. It never drops a
helper or chooses a preferred source. These cases fail, with no partial result:

- inconsistent quotes;
- unidentifiable parameters;
- bad controls;
- exhausted work;
- unsuccessful convergence.

Global fitting solves each curve after its exact dependencies; simultaneous
cyclic equations are not supported. Other representations, interpolation
combinations and unsupported product conventions fail explicitly.

Scales express one decimal rate in the quote's own units: the example uses
decimal 1, percent 100, basis points 10,000 and futures price points 100.
Weights and scales are paired with the quotes before deterministic identity
ordering. Scaling an objective cannot waive a quote's tolerance.
`FitNumericalControls` applies to the global kernel; piecewise uses its own
bootstrap controls. Each successful curve is finally repriced with every full
instrument, without collapsing schedules or substituting raw forwards.

`RateCurveBuildOutput` keeps the immutable `CurveSet`, the build order, the
recipes, the final helper valuations, `QuoteRepricing`, the frozen publication
requirements and the measured numerical work. Piecewise returns each root result;
global returns the shared `FitDiagnostics`.

## Rebuilding changed quotes

Use `RateCalibrationHelper::with_quote` to replace a quote and the uniform
fixed-rate or quoted-leg spread profile together. Other dates, schedules,
economic steps and known history stay intact. Ingestion owns the new immutable
curve revisions and must update all dependent term mappings, aliases and numeric
futures projection stamps consistently. No source or version is minted
automatically.

Call `build_rate_curve_set` again with the whole affected graph and only its
unchanged external dependencies. Keeping an old graph as a dependency does not
allow replacing its identities. Publish the new graph through the application
snapshot/activation layer; on error the caller's last-good set is untouched.
Rebuilt quote risk and raw forward sensitivity are different operations; this API
does not label a partly-fixed future's raw risk as calibrated market-quote PV01.

## Bounds and verification

One atomic graph has at most 128 helpers and 128 parameters in total, 256 curves
including supplied curves and views, 32,768 unique frozen observations and 16,384
helper valuations including trial and final repricing. The product
schedule/profile/reset/history bounds, shared solver/SVD ceilings and curve
horizons remain enforced. Publication callbacks must terminate and return checked
data without panicking. These resource ceilings are not latency guarantees.

The independent Python builder uses only the standard library, Decimal precision
60 and manual dated discount, simple reset, weighted overnight, profile and cash
sums. Two nonflat scenarios (positive and negative rates) supply six calibrated
curves and eighteen helpers, plus separately derived arithmetic and single-term
futures variants. Seasoned OIS, holiday weights, published revisions, delayed
payments, amortizing/geared IRS and 3M–6M basis cash are compared at bounds
declared before the Rust numerics ran: zero 1e-9 absolute + 1e-10 relative, cash
1e-4 currency absolute + 1e-11 relative, rate 1e-12 and futures price 1e-10. The
basis-point and percent residual bounds correspond to the same rate precision.
The fixture values and bounds are never generated from a fitted Rust result.

For design, we read the [ORE tenor-basis implementation](https://github.com/OpenSourceRisk/Engine/blob/master/QuantExt/qle/instruments/tenorbasisswap.cpp),
[ORE yield-curve configuration](https://github.com/OpenSourceRisk/Engine/blob/master/OREData/ored/configuration/yieldcurveconfig.hpp)
and [QuantLib 1.40 dated rate helpers](https://github.com/lballabio/QuantLib/blob/v1.40/ql/termstructures/yield/ratehelpers.cpp).
Independent leg projection, spread inverses, an explicit duplicate policy and
exact dated dependency handling came from that reading. The ORE links point at
the master branch, which can change, and no ORE comparison was run. QuantLib
futures adjustment signs must be translated explicitly before reuse.

## Related work and gaps

Not yet covered: government/issuer/repo/bond basis, canonical portfolio and
quote risk aggregation, stochastic convexity, full convention packs,
lifecycle/P&L, strict host construction and schema dispatch, and independent
validation against a vendor system. Existing legacy helper routes keep their
earlier behavior; this document covers only the route above. Library,
production and latency certification are still open.

Product graph capture, comparison of fitted output, solver and helper
diagnostics, and quote-risk replay are documented in
[the schema-3 replay contract](CALIBRATION-SNAPSHOT-REPLAY.md). The Rust
calculations stay neutral to providers, storage, MQ and clocks. Existing schema-1
and schema-2 identities and numerical controls are unchanged.

Exponential UFR/speed fitting over exact immutable analytic bases, required
one-sided joined forwards, quote-risk and schema-3 replay are covered by
[the UFR product contract](PRODUCT-UFR-CALIBRATION.md). Helpers recipes and
mixed-unit parameters and diagnostics are kept. No sampled objective,
regulatory procedure or unsupported base is substituted.
