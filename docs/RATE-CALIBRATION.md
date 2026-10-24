# Supplied mixed rate calibration

`convex::rate_calibration::build_rate_curve_set` constructs a complete immutable
OIS discount and independent index-projection graph from original dated deposit,
FRA, exchange-rate-futures, IRS, OIS and tenor-basis instruments. It composes the
existing full product pricers and bounded numerical kernels. No provider,
database, broker, runtime, registry or wall clock enters the calculation.

[Full product-price parametric fitting](PRODUCT-PARAMETRIC-CALIBRATION.md) adds
explicit NS/Svensson mode to this same full helper owner, with original exact
parameters, source-order native residuals, quote-risk rebuilding and schema3 replay.

`convex::rate_calibration::reprice_rate_helpers` separately diagnoses a complete
recipe on exact supplied curves without fitting or changing them. It reuses the
original admission, history preflight, product valuations and horizon guards,
returning native-unit residuals, full product evidence and `within_tolerance`.
An off-market diagnostic is not an accepted calibration. The shared request
and client qualification are documented in [the library API](LIBRARY-API.md).

Run the offline standalone consumer from the repository root:

```sh
cargo run --locked -p convex --example rate_calibration
cargo run --locked -p convex --example rate_calibration --no-default-features
cargo run --locked -p convex --example rate_calibration --features schemars
python3 tests/scripts/rate_calibration_reference.py check
cargo test --locked -p convex-analytics --test rate_calibration
```

The consumer reads explicit synthetic contracts, calendars, histories and quotes
from [mixed-rates.json](../tests/fixtures/curves/mixed-rates.json), checks the
independent reference parameters and prints every original native residual and
the actual helper valuation count. These are engineering fixtures, not live
market observations or a quantitative, production or latency certificate.

## Economic inputs

Each `RateCurveRecipe` retains its full `CurveRecipe`, ordered original helper
identities, actual parameter dates, complete product terms, original `MarketQuote`
and positive absolute tolerance in the quote's own unit. `RateCurveBuildInput`
supplies one matching `RateHelperResources` per original helper, in that same
order. Terms deserialize strictly; deserialization alone does not admit a product.

| Helper | Actual economics and original quote |
|---|---|
| Deposit | Business value/maturity dates, pinned calendar and context-free date day count; simple annualized rate in decimal, percent or basis points. |
| FRA | Separate index fixing, value start/end, explicit start/end settlement and eligible publication-aware fixing; annualized simple rate in its original rate unit. |
| Rate future | Full `FuturesContract`, actual observation/value window, calendar, clearing/history selection and convexity model pins; native `FuturePrice100MinusRate` price points. |
| IRS/OIS | Full `SwapTerms`, actual independent schedules, dated notional/spread/gearing profiles, fixing policies and exact discount/index mappings; original uniform par-fixed rate. |
| Tenor basis | Full `BasisSwapTerms`, independent floating schedules/projection indices and external collateral discount; original uniform absolute spread on an explicitly named paid or received leg. |

Deposit/FRA day counts are ACT/360, ACT/365F, ACT/ACT ISDA, 30/360 US or 30E/360.
Contextual ICMA money-market quotes require another complete reference-period
model and fail. FRA settlement is the declared value start or end; fitting the
original rate does not invent a cash settlement price. A FRA already fixed can
reprice from history but cannot identify a new curve parameter by itself.

Swap and basis helpers require unrounded Allow-negative cash and a uniform
quoted fixed-rate or named spread profile. Other notional, spread and gearing
steps remain intact. Uniform profile/quote consistency requires exact equality
in decimal-rate or original native floating-point units. Decimal strings use
correctly rounded floating-point ingress for this check; unit scaling does not
introduce a financial tolerance. A materially different embedded quote fails.
Inverse quotes are distinct from parallel shifts of all existing spread steps.

Rate futures retain arithmetic or compounded overnight holiday weights, partly
fixed windows and single-term fixing/value-date separation. They use the existing
[supplied futures valuation](FUTURES-VALUATION.md) convention:
`forward rate = futures-implied rate + supplied adjustment`, applied once to the
whole quote. An explicit zero assumption is allowed; missing admission fails.
Final settlement precision remains separate from the unrounded calibration
price and exchange trading ticks. Synthetic or stale inputs retain their source
blocks and cannot become execution authority by passing calibration.

## Basis pricing

`convex::rates::basis::BasisSwapTerms::build` and `price_basis_swap` are pure public
two-floating-leg construction/pricing APIs. Paid cash/PV is negative and received
cash/PV positive; negative coupons retain their natural economic sign. Results
include original terms, signed expected cash/accrued/PV, every rate origin and
actual projection/discount queries and consumed recipes. No principal exchanges
or cross-currency economics are admitted.

The implementation reuses the complete `SwapContract` floating-leg caches and
`price_swap` engine through a private zero fixed-coupon adapter on that leg's
actual coverage. That validation-only fixed leg is absent from basis inputs and
reported results; it contributes no nonzero basis cash. Reset, overnight,
profile, accrued and history calculations have one existing implementation.

For positive remaining unrounded spread exposure, `BasisParQuotes` provides
uniform replacement spreads on either leg and additive shifts to each leg's
existing steps, with separate positive spread annuities. These are linear spread
inverses, with spread outside overnight compounding. Rounded/clipped or expired
cash has an explicit unavailable inverse. Both histories are preflighted before
either leg forecasts; fully paid trades return zero with no invented inverse.

## Exact graph and history

Discount and projection identities are separate, versioned `CurveKey` values.
Projection role/index and currency must agree with the actual product. Basis
fitting calibrates exactly one independently projected leg against the other
external projection and external collateral discount curve. Same-index or
single-curve substitution fails.

Discount OIS fitting declares an `OvernightProjectionBinding`: a separately
named, versioned zero-basis overnight view of that discount curve. It is an
explicit model assumption; it must actually be consumed and match the named
overnight index. There is no inferred mapping from currency or index name.

Declared external dependencies must equal the union of all actual helper curve
versions. Missing versions, unused declarations, duplicate identities and cycles
fail. Supplied immutable curves cannot be replaced, including by another version
of the same identity. The full topology is checked before solving any curve;
output order is deterministic and dependencies precede their users and views.

Supply `RateCalibrationContext` with an explicit economic timestamp, matching UTC
valuation date, known-at cutoff, unpublished/final fixing policy and payment-date
entitlement. Administrator publication times are frozen once per unique index
and observation date across the entire graph. All due histories across all
products are resolved before any trial forecast. Every missing due observation
is retained in the typed missing list. Future projections cannot overwrite an
eligible known rate. Futures preserve their explicit FirstPublished versus
LatestEligible clearing rules; swap/FRA history preserves its checked eligible
revision selection. No calendar or fixing source is invented.

Parameter dates and helper pillars are explicit. Every actually projected
quotation end and fitted-curve payment date must fit that helper's pillar;
delayed payments and index-tenor dates can exceed contractual maturity. Enabling
extrapolation does not excuse an insufficient pillar. Existing curve horizon,
representation, interpolation and query postconditions still apply.

## Supported numerical combinations

| Mode | Representation/interpolation | Admission and diagnostics |
|---|---|---|
| Piecewise | Dated ACT/declared-time continuous zeros; Linear | One original helper per positive parameter date; explicit finite closed zero bracket and bounded Brent iterations. Duplicate pillars fail. Origin zero ties to the first parameter. |
| Global | Same continuous-zero grid; Linear or CubicSpline | Caller-supplied starts, closed zero bounds, positive native quote scales and weights; shared bounded least squares, measured Jacobian rank/conditioning, objective and original per-quote thresholds. |

Zero brackets lie within [-10,10] annualized decimal units. `RetainAllGlobal`
explicitly keeps every overlapping original quote, weight, residual and native
threshold. It never drops a helper or chooses a preferred source. Inconsistent
quotes, unidentifiable parameters, bad controls, exhausted work or unsuccessful
convergence fail; there is no partial accepted set. Global fitting solves each
curve after its exact dependencies; simultaneous cyclic equations are not
supported. Other representations, interpolation combinations and unsupported
product conventions fail explicitly.

Scales express one decimal rate in original native units: decimal 1, percent
100, basis points 10,000 and futures price points 100 are used in the consumer.
Weights and scales are paired with originals before deterministic identity
ordering. Scaling an objective cannot waive an original native tolerance.
`FitNumericalControls` applies to the global kernel; piecewise uses its explicit
bootstrap controls. Each successful curve is finally repriced with every full
original instrument, without collapsing schedules or substituting raw forwards.

`RateCurveBuildOutput` retains the immutable `CurveSet`, exact build order,
original recipes, complete final helper valuations, native `QuoteRepricing`,
frozen publication requirements and measured numerical work. Piecewise returns
each actual root result; global returns the full shared `FitDiagnostics`.

## Rebuilding changed quotes

Use `RateCalibrationHelper::with_quote` to replace a native quote and the uniform
fixed-rate or quoted-leg spread profile together. Other dates, schedules,
economic steps and known history remain intact. Ingestion owns the new immutable
curve revisions and must update all dependent term mappings, aliases and numeric
futures projection stamps consistently. No automatic source/version is minted.

Call `build_rate_curve_set` again with the complete affected graph and only its
unchanged external supplied dependencies. Retaining an old graph as a supplied
dependency does not authorize replacing its identities. Publish the successful
complete new graph through the application snapshot/activation layer; an error
leaves the caller's last-good set untouched. Rebuilt quote risk and raw forward
sensitivity are different operations; this API does not label a partly-fixed
future's raw risk as calibrated market-quote PV01.

## Bounds, verification and remaining work

One atomic graph has at most 128 original helpers and 128 parameters in total,
256 curves including supplied curves/views, 32,768 unique frozen observations and
16,384 actual helper valuations including trial and final repricing. Product
schedule/profile/reset/history bounds, shared solver/SVD ceilings and curve
horizons remain enforced. Publication callbacks must terminate and return checked
data without panicking. These resource ceilings are not latency guarantees.

The independent standard-library Python builder uses Decimal precision 60 and
manual dated discount, simple reset, weighted overnight, profile and cash sums.
Two nonflat positive/negative scenarios supply six calibrated curves and eighteen
original helpers, plus separately derived arithmetic and single-term futures
variants. Seasoned OIS, holiday weights, published revisions, delayed payments,
amortizing/geared IRS and 3M–6M basis cash are compared at bounds declared before
native numerical execution: zero 1e-9 absolute + 1e-10 relative, cash 1e-4 currency
absolute + 1e-11 relative, rate 1e-12 and futures price 1e-10. Native basis-point
and percent residual bounds correspond to the same original rate precision.
The original fixture values and bounds are never generated from a fitted native
candidate. Earlier quantitative artifacts and external captures stay unchanged.

Read-only design research inspected the actual [ORE tenor-basis implementation](https://github.com/OpenSourceRisk/Engine/blob/master/QuantExt/qle/instruments/tenorbasisswap.cpp),
[ORE yield-curve configuration](https://github.com/OpenSourceRisk/Engine/blob/master/OREData/ored/configuration/yieldcurveconfig.hpp)
and [QuantLib 1.40 dated rate helpers](https://github.com/lballabio/QuantLib/blob/v1.40/ql/termstructures/yield/ratehelpers.cpp).
Independent leg projection, spread inverses, explicit duplicate policy and exact
dated dependency handling informed this design. ORE master URLs are mutable
source reads, not captured execution evidence; no new ORE comparison is claimed.
QuantLib futures adjustment signs must be translated explicitly before reuse.

Government/issuer/repo/bond basis is CVX-058. Canonical portfolio risk and quote
risk aggregation, stochastic convexity and full convention packs, lifecycle/P&L,
strict host construction/schema dispatch, independent quantitative approval and
library/production/multi-host/colocation certificates remain their owning work.
Existing legacy helper routes retain their earlier qualification; this task
adds the complete native route without certifying those separate paths.


Complete original product graph capture, fitted-output/solver/helper diagnostic
comparison and original quote-risk replay are documented in
[the schema-3 replay contract](CALIBRATION-SNAPSHOT-REPLAY.md). Pure native calculations remain provider/storage/MQ/clock neutral.
Existing schema1/2 identities and original numerical controls remain; CVX-063
and all mandatory library/market/model/host/production/latency certificates stay open.

Full-product exponential UFR/speed fitting over exact immutable analytic bases, required one-sided joined forwards, original quote-risk and schema3 replay use [the explicit UFR product contract](PRODUCT-UFR-CALIBRATION.md). Complete Helpers recipes and actual mixed-unit parameters/diagnostics are retained. No sampled objective, regulatory procedure or unsupported base is substituted.
