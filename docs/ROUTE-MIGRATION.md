# Removed government spread conveniences

Complete contextual fixed workout paths use strict shared `icma_workout`; see
[the contract](ICMA-WORKOUTS.md). Explicit single-path `icma_quotation` and
context-free dated `workout` remain valid distinct owners. Missing ICMA reference
context cannot route through either as a proxy. Removed compact browser workout
outputs remain absent; generic full requests retain every legal path and original
notice, principal, accrual, payment and quotation inputs.

The government spread convenience routes below have been removed. They could
not carry actual benchmark securities and common yield conventions. A government
zero rate does not represent a government bond yield. There is no compatibility
alias or approximate fallback.

| Removed surface | Migration |
|---|---|
| MCP `compute_spread.kind = g_spread` | Use `library` with `government_spread`. `compute_spread` admits `z_spread` and `i_spread` only. |
| C/JSON `SpreadParams.govt_curve` and Java `SpreadParams.govtCurve(...)` | Use actual `params.government` securities/yields for the existing checked fixed-bond spread route, or full `government_spread` through `convex_library` / `Convex.library`. Removed fields are unknown input errors, including `null`. |
| C/JSON `YasRequest.govt_curve` and YAS G-spread marks | Use a supported price/yield/Z/I mark for the convenience analysis and calculate government spreads separately with actual supplied securities. YAS cannot price a G-spread mark. |
| YAS `g_spread_bps`, `benchmark_spread_bps`, `benchmark_tenor` | These fields are absent from `YasResponse` and its schema. Update parsers and displays. |
| Excel `CX.YAS` / `CX.YAS.LIVE` government-curve argument | Signature is now `(bond, settle, mark, curve, [quoteFreq], [swapCurve])`. Remove the old fifth argument and shift following arguments left. The grid omits government/benchmark spreads. |
| WASM `price_from_g_spread`, `price_from_benchmark_spread` | These exports are absent. Rebuild the WASM package and consuming application together. Use generic `library` with the full supplied `government_spread_price` dated-cash operation; see [the inverse contract](GOVERNMENT-SPREAD-INVERSE.md). Street/market conventions and actual wasm32/browser qualification remain open. |
| WASM `AnalysisResult.g_spread`, `benchmark_spread`, `benchmark_tenor` | These result fields are absent. Use `library(request_json)` for actual-security government-spread calculation. |
| Browser G-spread editor and synthetic benchmark-bond panel | Removed with their callbacks. The remaining spread panel displays actual Z/ASW/OAS results; unavailable OAS is not replaced by Z-spread. |
| Browser mock analytics after engine-load failure | Removed. Failure to load the engine displays an error and does not mount the analytics application. |

The [full supplied-input API](LIBRARY-API.md) calculates government spreads from
complete cash securities, actual quote/calendar/reference revisions, a common
explicit dated-cash yield convention and exact or adjacent contractual maturity
dates. It preserves fractional basis points. It does not assert Street/workout
convention coverage. The separate full supplied dated-cash inverse is now
available through generic `government_spread_price`; see
[G-spread-to-price](GOVERNMENT-SPREAD-INVERSE.md).

The C symbol set and pointer ownership contract are unchanged. JSON result
shapes, Java source calls, WASM exports and Excel positional arguments above
are deliberate breaking changes; recompile callers and update formulas.

This removal review covers government spread conveniences and browser fallback
paths. Other convenience pricing, tenor-only bootstrap and indicative hedge
routes still need their original CVX-063 product/convention/host qualification.
Underlying native diagnostic models and their historical numerical controls
remain separate from the supplied-input API. Removing an incorrect convenience
does not remove any required standalone-library capability or release gate.

For future work, delete an obsolete or incorrect route together with its unused
types, callers and advertisement. Record its real replacement or remaining
feature owner, and reject obsolete inputs explicitly. Keep storage, messaging,
provider selection and execution authority outside pure analytics.

## Incomplete browser workout outputs removed

WASM `AnalysisResult` no longer contains `ytc`, `ytw`, `workout_date` or
`workout_price`. Their incomplete convenience calculation, browser display
callers and unused styles are removed. The compact call-date/price convenience
cannot supply original notice, principal or separate cash-settlement terms.
Use the strict shared `workout` operation through `convex_wasm::library`,
`convex_library` or other existing shared clients, with complete native bond
terms, actual independent calendar revisions, original trade/purchase dates,
explicit notice state and dated quotation/inverse controls. See
[workout quotations](WORKOUT-QUOTATIONS.md). No compatibility result or proxy
is substituted. Existing generic portfolio yield fields can carry separately
supplied actual metrics; this removal does not waive the required native
workout, floating/model/convention or actual browser qualification work.

The existing browser yield-risk column is labelled `Yield`; it no longer
advertises those ordinary yield sensitivities as workout risk.

## Complete curve-family construction

Bare `NelsonSiegel`/`Svensson` method tags on discrete nodes still fail rather
than constructing a sampled substitute. Use complete `CurveSource::Parametric`
or the shared `parametric_fit` operation for original zero observations. Complete
product quotes use [Parametric mode in the existing full calibrators](PRODUCT-PARAMETRIC-CALIBRATION.md).
`Segmented`/`Delegated` tags require
their complete native source contracts, exact dependencies, bounds/fallback and
join rules. See [curve families](CURVE-FAMILIES.md). Older tenor-only bootstrap
and native compatibility methods retain their separate unqualified scope; this
addition does not waive their full helper/convention/host qualification.

## Explicit step-zero and flat-forward sources

The native `InterpolationMethod::PiecewiseConstant` tag now holds the preceding
supplied value over each right-owned interval and changes at an exact knot.
Callers that actually require linear interpolation must explicitly select
`Linear`. The former linear proxy is removed. Jump derivatives fail.

The older raw `DiscreteCurve` FlatForward tail no longer returns a flat raw
value under a forward label. Its checked route remains Unsupported and its
infallible compatibility query returns NaN there. Use complete
`CurveSource::FlatForward { nodes }` with continuous-zero rate representation,
matching method and explicit extension through native/shared `curves` and
`curve_query`. It integrates the original native last forward. The original
checked `Nodes` route retains its rejection constraints. An explicit
`PiecewiseConstant { nodes }` source supplies the checked step-zero contract.
Full recipe/units/time basis/roles/currency/versions remain required; there is
no automatic migration, fallback field or sampled proxy. `FlatForwardRate`
requires `Left`/`Right` and reports actual extension at boundary sides.
See [curve methods](CURVE-METHODS.md). Full product-price calibration, quote-risk
rebuilding, complete hazard integration and remaining method/host coverage
retain their original owners and release requirements.

The older engine's default and flat-zero pricing/benchmark fixtures explicitly
select `Flat`, replacing their misleading `FlatForward` labels. Numerical input
values and pricing assertions are unchanged. `CurveBuilder` with `FlatForward`
now fails before provider access or publication, and `BuiltCurve::from_snapshot`
rejects a `ZeroExtrapolation::FlatForward` record. Such a snapshot must not be
silently rewritten as flat zero: supply the complete checked flat-forward recipe
to the shared library. Full direct recipes use [schema-2 replay](CURVE-SNAPSHOT-REPLAY.md); full original
product/risk graphs use [schema-3 replay](CALIBRATION-SNAPSHOT-REPLAY.md). The fictional constant-zero snapshot fixture selects `FlatZero`;
previous captured snapshot bytes and historical evidence are preserved.

The older infallible `DiscreteCurve` UFR tail no longer substitutes a zero
derivative, creates a short-end flat value, or calls a panicking constructor
when its inputs are invalid. It delegates to `checked_value_at`; failures return
NaN through the infallible compatibility signature. Callers should use checked
errors. The native UFR integral now uses stable `expm1` evaluation so small
alpha/time changes retain their integrated forward. Original numerical controls
and tolerances are unchanged.

Use the [explicit UFR source](CURVE-UFR.md) for the full supplied LLP/exact-base
contract and analytic `UfrForward` dispatch. Existing `Nodes` admission is not
widened. Other base families, complete method/product calibration and quote-risk
rebuilding need their owning contracts. Direct native recipes now use the
[complete engine/snapshot replay contract](CURVE-SNAPSHOT-REPLAY.md).

## Complete MonotoneConvex source

The original native MonotoneConvex owner now uses complete nonnegative Hagan–West regimes with one analytic forward/primitive. Query clipping, sampled zero derivatives and inconsistent flat-zero/flat-forward mathematics are removed. Exactly one zero endpoint-deviation axis uses the documented original quadratic; the native origin is derived analytically. `with_extrapolation()` now integrates the terminal instantaneous forward. Negative times and a negative first interval reject. Use `CurveSource::MonotoneConvex { nodes }`, continuous-zero rate representation and matching method for the complete dated contract; `MonotoneConvexForward { point, side }` exposes the actual native or declared terminal side. Original protected test sections remain unchanged. Bare Nodes/DF/hazard aliases, signed-forward or ameliorated variants, product calibration, other UFR joins and unqualified old host conveniences remain unsupported or have their original owner. See [full conventions and limits](CURVE-MONOTONE-CONVEX.md).

## Complete curve recipe snapshot replay

`SnapshotAssembly.curve_recipes` now carries full versioned native CurveRecipe
payloads and explicit provenance into schema-2 snapshots. All original date,
unit, role, time basis, model/LLP, helper/settings, method/tail and exact dependency
versions survive capture and reconstruction. Schema-1 omitted fields and content
identities remain unchanged; its unsupported FlatForward policy is rejected.
Migration needs a genuinely supplied complete recipe and new identity, without
sampling or relabelling old economics. Read [the replay contract](CURVE-SNAPSHOT-REPLAY.md).

The engine's changed-input rebuild now returns Result and clears an invalid
representation. Failed builds never attach the previous model to new points,
substitute linear/zero rates or publish a failed candidate. Callers must handle
the error. The older day/zero graph conversion now returns Result and rejects
fractional pillars and full recipe curves. Use immutable snapshots for those
inputs; no partial grid or integer-day truncation survives that route. Existing
exact whole-day zero inputs keep their declared path. Full product-helper
calibration and original quote-risk graphs use
[schema-3 replay](CALIBRATION-SNAPSHOT-REPLAY.md). Remaining raw-value/convenience
qualification retains its original owners.

`POST /api/v1/curves` continues to use checked MonotoneConvex over supplied
ACT/365F continuous-zero pillars. It requires at least two distinct positive
pillars; empty and single-pillar inputs return HTTP 400 before installation.
The former single-point success lacked a validated curve and depended on a
fallback that is now removed. Supply valid original pillars or a complete
native recipe with its explicit model; no synthetic second point or implicit
flat model is constructed by the server.


Complete original product graph capture, fitted-output/solver/helper diagnostic
comparison and original quote-risk replay are documented in
[the schema-3 replay contract](CALIBRATION-SNAPSHOT-REPLAY.md). Pure native calculations remain provider/storage/MQ/clock neutral.
Existing schema1/2 identities and original numerical controls remain; CVX-063
and all mandatory library/market/model/host/production/latency certificates stay open.

Full-product exponential UFR/speed fitting over exact immutable analytic bases, required one-sided joined forwards, original quote-risk and schema3 replay use [the explicit UFR product contract](PRODUCT-UFR-CALIBRATION.md). Complete Helpers recipes and actual mixed-unit parameters/diagnostics are retained. No sampled objective, regulatory procedure or unsupported base is substituted.

## Complete-factor proposals versus indicative advisor

Shared `hedge` (`aggregate` / `solve`) is the real remaining owner for supplied
complete-factor constrained proposals; see [its contract](CONSTRAINED-HEDGES.md).
Existing `propose_hedges`, `propose_book_hedges`, scalar ratios and per-instrument
round/roll math keep their explicitly indicative meanings. They do not implement
common-factor constrained solving, legal credit coverage or authoritative pending
orders. Migrate callers requiring those calculations to complete original
`HedgeProblem` inputs through the generic library entry point. The source-risk
and economics/calculation manifests remain original native/application owners;
no guessed CDS risk, tenor-only future or heuristic cost is introduced. The new
operation never grants order authority or waives separate server/host work.

## Complete floating conditional workout owner

Use `BondContract::floating_workouts` and the strict shared `floating_workout`
price/discount_margin/paths actions for actual term/overnight conditional cash.
Supply complete original history/publication decisions, independent calendar
revisions, exact index projection and collateral discount, purchase/notice state
and explicit partial shifted weight choices. The original plain floating and
optional-model owners retain their separate meanings. Standalone FRN convenience
DM-to-worst helpers cannot supply these economics and remain unqualified; their
outputs cannot substitute for full original conditional cash or model value.

Fixed and floating native compositions now share `WorkoutProjection`. Replace
imports of the removed `FixedWorkoutProjection` name; its full cash/date/amount
layout is retained. There is one checked composition, without a compatibility
type alias or duplicated floating financial formulas. No C symbol or generic
Java/Excel/MCP/WASM call changes. See [fields and controls](FLOATING-WORKOUTS.md).

## Explicit contextual ICMA cash quotation

Use `workflow::PreparedIcmaQuotation` or strict `icma_quotation` price/yield
for full original fixed/workout cash with an explicit versioned nominal mesh.
Supply complete original reference periods, independent quote/reference/policy/
calendar pins, frequency, compounding, coverage and original-face quantity.
The existing context-free `DatedYieldConvention` route still rejects missing
ICMA context; it cannot become an implicit ISDA, fixed365 or coupon-count proxy.
Its other admitted conventions are unchanged. Low-level approximate day-count
helpers are not an alternative to this checked original-cash quotation owner.
No existing correctly owned route is removed. Actual government benchmark
selection, market Street and stochastic OAS require their distinct full owners;
this contextual time composition grants none of those certificates.
See [ICMA quotations](ICMA-QUOTATIONS.md).

Contextual ICMA actual-government-security spread and inverse now have the
explicit `icma_government_spread` / `icma_government_price` shared operations.
Supply every target/benchmark's own complete context, observed helper/source
revisions and original quantity, plus an explicit common output policy and
compounding. See [the full contract](ICMA-GOVERNMENT-SPREADS.md). The original
context-free government operations and direct `icma_quotation` remain correctly
owned; changing a context-free day-count tag cannot supply missing ICMA context.


For contextual ICMA helper conversion/calibration/repricing, supply `CashQuoteKind::ContextualIcmaYield` with its complete original context and bind native `CashHelperResources::IcmaBond` or the existing shared independently resolved calendar pins. `CashflowYield` remains the correct context-free route. `CashQuoteKind` is now Clone rather than Copy; borrow matches or clone owned values. See [the full contract](ICMA-CASH-HELPERS.md).

For direct contextual ICMA own-yield risk, use `icma_yield_risk` actions
`yield_sensitivity` / `hedge_vector` and their full observed-unit/snapshot/factor
inputs. Context-free bond quote risk and recalibrated helper risk retain their
correct distinct owners. An ICMA day-count tag cannot supply their missing
context, and a conditional workout cannot become unconditional hedge risk.
See [ICMA yield risk](ICMA-YIELD-RISK.md). No obsolete alias is introduced.


## Complete common-factor hedge columns

The distinct `graph_hedge_risk` operation composes the original full product and
graph revaluation owners into complete bond/CDS/IRS/OIS/rates-futures columns
for the existing hedge aggregator and proposal solver. See
[quantity binding, inputs, controls and remaining limits](GRAPH-HEDGE-RISK.md).
Own-security ICMA yield remains a separate factor. Optional models, credit-survival
cash products and delivery-futures all-member risk remain owning work; actual
managed/browser and production/latency qualification remain open.


Credit-survival and issuer-basis cash now have a distinct full original-owner
`cash_basis_hedge_risk` route. See [cash basis hedge risk](CASH-BASIS-HEDGE-RISK.md)
for the complete security/mapping/factor contract, ICMA and rebuild controls,
actual mixed-book scope and remaining recovery/host/production qualification.

## Treasury bill quotations and removed long-bill proxies

The unused floating-point functions
`convex_analytics::yields::bond_equivalent_yield_simple` and
`convex_bonds::pricing::bond_equivalent_yield` are removed, including their
public reexports. Their long-bill formulas were approximate and their signatures
could not carry original issue dates or return a checked failure. There is no
compatibility alias. Repository callers did not use these exports.

The retained Result-returning `convex_analytics::yields::bond_equivalent_yield`
calculates fixed365 simple yield **in percent**, for1..182 remaining days and
positive price/face. Long terms return InvalidInput with a dated-API migration
message; Decimal overflow returns CalculationFailed. Discount/par/premium prices
retain their original short-term values. The shared `zero`/`bill_conversions`
operation keeps its original182-day/face-redemption/domain guards, result fields
and percent units. It is an explicit fixed365 conversion, not an issue-based
Treasury investment-rate convention.

Use `convex::treasury_bill::quote_treasury_bill`, or shared `zero`/`treasury_bill`,
with **original issue date, settlement, maturity, currency, face/redemption and
the supplied quote**. When migrating a bank-discount rate to the dated route,
convert percent to decimal once. Keep absolute cash separate from per100 price.
The calculation selects its365/366 basis from the original issue interval and
returns decimal investment/bank-discount rates, inverse residual and signed risk.
Never infer an issue date from remaining days or silently retain fixed365 for
a leap-year Treasury investment-rate quote. See
[the full contract and primary sources](TREASURY-BILL-QUOTES.md).

Both `ShortDateCalculator` types now return `Result<Decimal>` from
`money_market_yield` and `bond_equivalent_yield`. `ZeroCouponBond::discount_yield`
and `bond_equivalent_yield` also return `Result<Decimal>`. Update callers to
propagate `?` or handle the error; invalid cash, zero/negative days, inactive
dates, non-face redemption and arithmetic failure no longer produce a zero yield.
The zero-bond methods require their actual issue date to precede maturity and
settlement to be on or after issue and strictly before maturity. Its explicit
settlement is used directly, without an inferred issue date or settlement lag.

These six object methods delegate to one checked Decimal implementation in
`convex_bonds::pricing::bill`, also available as `convex::bill_yield`:

| Function | Rate meaning and units |
|---|---|
| `bank_discount_yield(price, face, days)` | Decimal rate on original face, explicit 360 basis. |
| `money_market_yield(price, face, days)` | Decimal investment rate on original price, explicit 360 basis. |
| `simple_bill_yield(price, face, days)` | Decimal price-basis simple rate on fixed 365, admitting 1 through 182 days inclusive. |
| `simple_bill_yield_from_money_market(rate, days)` | Convert a decimal price-basis 360 rate to fixed 365; require positive implied face/price and 1 through 182 days. |

Discount, par and premium cash remain valid in the positive cash domain.
Negative money-market rates remain valid inside the exact face/price domain;
the rate pole is checked using bounded integer arithmetic to avoid rounding away
a positive residual. Decimal overflow is an error. The remaining infallible
floating-point exports `convex_bonds::pricing::discount_yield` and
`convex_analytics::yields::discount_yield_simple` are removed with their reexports.
Use the checked Decimal functions; there is no floating-point compatibility alias.

The existing analytics `discount_yield`, `cd_equivalent_yield` and
`bond_equivalent_yield` retain their **percent** outputs and now delegate to the
same owner, multiplying by 100 once. The six object methods and four checked
kernel functions return **decimal** rates. Generic zero price/yield, compounding,
full dated Treasury quotation, curve/risk/hedge/lifecycle and shared JSON/C
entry points keep their distinct owners. This is a checked conversion migration,
not a market convention, model, host or production certificate.

### Checked zero-coupon pricing and compounding

`ZeroCouponBond::price_from_yield`, `yield_from_price` and `continuous_rate`,
plus `instruments::convert_yield` and `Compounding::periods_per_year`, now return
`BondResult`. Propagate `?` or handle the error. Continuous compounding has no
discrete period count: use `periods_per_year_opt` when that distinction is needed.
There is no zero-yield, face-price or panic compatibility fallback.

Object prices are **absolute cash amounts**, in the original face/redemption
units. Pricing and inversion use contractual **redemption**, including non-face
redemption. The shared quoted-price API retains its separate per-100 semantics.
The checked methods require positive original face/redemption, issue before
maturity, active settlement and a positive horizon of at most 150 years. They
use the existing checked context-free ACT/360, ACT/365F, ACT/ACT ISDA, 30/360 US
and 30E/360 year fractions. A convention needing missing reference or termination
context errors explicitly; no builder preset or issue date is inferred here.

The pure `convex::zero_yield` (`convex_bonds::pricing::zero`) cash/rate kernels
accept explicit redemption and year fraction. Decimal domain checks precede
binary64 projection. Small cash changes are applied before Decimal addition;
small price differences are retained before logarithms. Positive residuals near
periodic rate boundaries survive projection. Results beyond numerical/Decimal
representation, including nonzero underflow, return errors. Same-compounding
conversion validates its domain and returns the original Decimal exactly.

Run `cargo run --locked -p convex --example zero_methods` for the independent
price/inverse/conversion, dates/redemption, purity and typed failure controls.
This migration does not qualify the remaining convention/model/host scope.

### Lossless direct zero-coupon terms

Direct `ZeroCouponBond` serialization now encodes `issue_price`, `face_value`
and `redemption_value` as exact plain Decimal **strings**. Optional issue price
may be null. Use the same strings when decoding; JSON numbers, exponent notation,
rounding, overflow and more than 28 fractional digits are rejected. This uses
the existing `ExactDecimal` convention already used by the shared `ZeroInput`.

The direct decoder accepts the ten canonical serialized day-count enum names and
rejects unknown conventions. It also rejects extra outer fields. Serialized
`BondIdentifiers` containers reject unknown keys across bond types; connectors
must map provider data into the declared fields before decoding. Economic
pricing validation remains in the checked pricing/terms owners. Constructors,
pricing formulas, calendar resolution and the shared request schema are unchanged.

Run `cargo run --locked -p convex --example zero_terms` for all field roundtrips,
unknown/missing/duplicate/type rejections and exact Decimal ingress boundaries.

## Exact supplied term decimals

Bond face and redemption multipliers, dated profile values, principal prices and
partial amounts, exercise prices and fixing rates decode through the existing
strict `ExactDecimal` owner. JSON values must be exact decimal strings, including compact fractional forms
such as `.03` and `-.0500`; raw numbers,
exponents, overflow and excessive precision are rejected. Native fields remain
`Decimal`, with their original arithmetic and checked economic admission.

The optional principal amount retains `null` and the existing omitted-field
meaning. Present values use the same strict decoder. IRS/OIS profiles retain the
original sequence bound and now use the shared dated-value decoder. The duplicate
wire struct and rounding parser are removed. Generated field schemas expose the
same exact-string syntax and size constraints as native ingress.

`cargo run --locked -p convex --example term_decimals` exercises the independent
metadata controls. Existing valid string-valued terms and financial results retain
their contracts. This does not supply the remaining stochastic optional models or
independent production qualification.

## Retired callable WASM convenience

The fixed-bond browser convenience no longer accepts `call_schedule` or
`volatility`, including null or empty values. `BondParams` rejects unknown payoff
fields rather than silently treating them as a straight fixed bond. The separate
`CallScheduleEntry` wire type and callable-only OAS/effective-risk output fields
are removed, along with their demo controls and placeholder risk displays.

The old wrapper assigned American exercise to a finite date list, assumed 1%
volatility, and returned Z-spread as OAS after a failed option calculation. These
results did not represent the supplied optional contract. Use `library()` with
complete `BondTerms.exercise`, explicit model assumptions or calibration, and the
`optional_bond` operation. Its supported European/Bermudan and aligned term-FRN
model scopes, full diagnostics and remaining native model requirements are
unchanged. American and broader joint/cap/floor models still require their own
implementation and qualification; this removal does not waive them.

Other fixed-bond convenience parsers, quotation units and model support remain
separately scoped and unqualified. The full supplied-term shared API remains the
available route for exact cash strings, calendars, history and model provenance.

JavaScript-object callers use the same bounded own-field check before every fixed
bond convenience. The JS serde backend visits declared struct fields only, so the
JSON unknown-field annotation alone is insufficient. Retired/unknown/symbol keys
and nonplain objects fail; inherited fields cannot supply economics. Invalid
`get_cash_flows` inputs return an error object; valid inputs retain their cash
array and units. Other fixed-parser, numeric, convention and unit qualification
remains separate work.

The retained fixed WASM helpers now reject unknown enums and nonfinite/out-of-range
numeric values. Finite JSNumber inputs use their shortest exact decimal spelling;
unsupported Decimal scale/range fails without rounding or zero substitution.
Percentage conversion shifts the coefficient exactly. Explicit quote/settlement
settings apply after a registered preset, and nonfixed payoff classes fail.
Prices/risk/accrued are per100 original face; projected cash is absolute currency,
with redemption explicitly equal to the supplied face. The unused third curve
argument on `price_from_yield` and its browser caller are removed. Curve-bearing
operations reject unknown point fields instead of discarding them. All original
broader Street/ICMA/ex-coupon/stub/tax/workout and quote-consistent risk qualification
remains required: these unchanged legacy engines and preset labels are not a
complete convention certificate. Use full supplied-term/contextual library routes
for admitted complete economics. No native requirement or authority is waived.
