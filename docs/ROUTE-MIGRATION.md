# Removed government spread conveniences

## Exact quote wire values and generated RPC schemas

Textual marks and tagged price/yield/spread inputs preserve admitted Decimal
values exactly. Unrepresentable digits, rounded percentage conversion and
nonzero erasure are errors. Decimal constructor and risk/hedge-report fields
accept bounded exact strings as well as established numeric inputs. Outputs
remain numbers when the shortest binary64 wire number decodes to the same
Decimal; otherwise they are exact strings. Consumers must support both forms.
Use exact strings when supplying every original decimal digit.
Object descriptions use this same policy for supplied coupon, face and spread
values; the browser-facing description no longer discards admitted digits.

All convenience request/response schemas now derive from the actual native
DTOs. Response schemas describe serialized fields, including omission rules.
The FFI adds registered-name alternatives at the same fields its dispatcher
resolves; native DTO handles remain integers. Returned schemas are self-contained.
Structural schema validation does not replace checked financial construction,
Decimal representation, history, domain or workload validation.

Tagged native bond/curve construction rejects ignored and duplicate fields.
The C boundary delegates that policy and retains its bounded raw JSON admission.
Convenience RPC input objects reject unknown economic fields, including `null`.
Native discount queries propagate checked underlying curve errors and reject
nonfinite, nonpositive and underflowed factors. Negative rates may give a valid
factor above one. Conversion to the core Decimal curve interface rejects a
nonzero factor that Decimal cannot represent. Broader curve/hedge/model and
client qualification remains part of CVX-063.

## Checked constructors and removed inferred liquidation estimates

C bond/curve constructors use the shared bounded JSON parser. Raw nonzero
numbers cannot underflow silently to zero. Decimal fields use one checked
number-or-string decoder, including optional cap, floor and redemption inputs;
rounding, overflow and nonzero Decimal erasure fail. Numeric inputs keep their
binary64 wire interpretation. Use exact decimal strings when every original
decimal digit matters. Admitted defaults remain; the exact wire policy below applies to Decimal fields.

Unsupported fields fail for the selected constructor, including fields supplied
as `null`. Fixed, callable, floating, zero and sinking bonds cannot silently
consume another shape's payoff or reset terms. Curves cannot silently discard
supplied provider or projection context. `BondSpec` and `CurveSpec` schemas now
come from their native DTOs. The schemas describe structure; checked ingress and
construction still enforce work, representation and economic invariants. The
convenience RPC descriptors are generated from actual DTOs as described below.

The native `estimate_days_to_liquidate` function, `DaysToLiquidate` type and
`POST /api/v1/liquidity/days-to-liquidate` route are removed. A liquidity score
does not supply daily trading volume. The old calculation also returned negative
days for shorts and zero gross execution work for offsetting positions. No
replacement estimate is implied. Execution-capacity work needs actual supplied
volume, units, dates, source lineage and explicit execution assumptions.

`POST /api/v1/liquidity/analysis` returns supplied descriptive metrics and bucket
distribution. It has no `days_to_liquidate` field. Its retired
`max_participation_rate` input produces a structured 422 `INVALID_PORTFOLIO_INPUT`
response. Remove that parameter when requesting descriptive analysis; callers
needing execution estimates must treat that capability as unavailable.
The removed route remains absent from routing and authorization registration.

The unused demo `types/api.ts` file is removed. `PortfolioAnalyticsMessage`
matches the actual flattened portfolio WebSocket payload: monetary and Decimal
risk values are strings; factor availability remains nullable; settlement,
coverage and the source timestamp are retained. Native integer timestamps stay
numeric through stream state. These changes do not certify the remaining bond,
ETF, transport or execution client contracts.

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


The fixed WASM convenience no longer returns `asw_spread`. The v1 YAS response
and curated schema no longer expose `asw_spread_bps` or the unused `oas_bps`.
Their browser/Excel grid consumers are removed. The old price-only YAS ASW
formula binds no complete swap role and does not qualify as asset-swap valuation;
the retained legacy native diagnostics remain unqualified. Use complete supplied-
term ASW and optional-model/OAS library operations with actual curve/model roles.
These native requirements remain required; retirement does not waive them.

The browser risk panel displays supplied native measures, without substituting
zero before calculation. DV01 loss per100face scales to the explicitly supplied
invoice face. Estimated benchmark/risk/proceeds hedges and the inert DV10/DV25
selector are retired. Full-factor supplied instrument hedge APIs retain their
original risk/model/quantity/eligibility contracts. Windows/Excel execution and
broader quote-consistent convention/risk qualification remain separate owners.


Fixed WASM cash entries now expose `coupon_amount`, `principal_amount` and
`fee_amount` in absolute bond currency, alongside the original amount/date/type.
Classification follows actual native `CashFlowType`; combined fixed terminal cash
uses actual fixed-bond redemption and checked Decimal subtraction. The restricted
fixed adapter cannot substitute for full supplied-term stepped, floating, optional,
amortizing or lifecycle cash. Every original native requirement remains.

The browser cash table displays native components and totals. Its5%fallback,
semiannual/row-index discounting,100principal inference and PV/cumulative-PV
columns are retired. No cash projection is a recorded payment or accepted model
PV. Invalid non-array cash results clear the dependent display and show the native
error. Actual native price/risk/P&L routes retain their own full curve roles and
convention qualification; this cleanup creates no market/model certificate.


### Checked dated-cash WASM quote basis

`analyze_bond` and `price_from_yield` share native
`convex_bonds::pricing::dated_cash::dated_cash_yield_risk`. Its absolute PV and
annual-rate derivatives use the same entitled cash, settlement-to-payment basis
and selected compounding. The fixed wrapper normalizes prices/risk per100 face.
Yield must reprice within1e-8per100 before risk is returned. The result identifies
`Dated cash`; it does not qualify a named Street/ISMA/tax market convention.
Missing ICMA reference or ex-dividend calendar/entitlement context fails.

Unrelated legacy YAS risk/money-market yield and the unused YAS result/date
conversions are removed from this convenience. Full supplied-term native YAS,
ICMA, money-market, quote-risk/hedge and model APIs remain separate. A missing
Z-spread is `None` with `z_spread_error`, which the browser displays. No zero
substitute or erase of valid dated-yield risk. The convenience's distinct legacy
yield/Z curve representation and broader market mechanics retain their original
qualification owners; this increment is not a production/model certificate.


## Checked yield scalar reporting

`YieldResult::yield_percent/yield_decimal`, native/analytics simple-current f64
helpers, native Decimal-current wrappers and their from-bond variants now return
`Result`. Propagate with `?`; invalid input and nonzero underflow cannot return a
successful zero. Analytics reexports the native `YieldEngineResult`. Existing
fallible Decimal percentage APIs retain their units and now check arithmetic.
From-bond per100 current yields no longer scale with original face. See
[YIELD-SCALARS.md](YIELD-SCALARS.md). Raw solver and dated-convention qualification
remain separate; no approximation is promoted to full instrument support.


Direct supplied-cash yield dirty/clean prices and both YieldEngine price methods
now return Result; propagate errors with `?`. Analytics StandardYieldEngine is
the native type with an analytics error adapter. Explicit supplied-cash pricing
and unique nonnegative-cash inverses use `cash_yield`; unsupported convention
enums cannot silently use a periodic approximation. See [supplied-cash yield
pricing](YIELD-PRICING.md). Contextual date/ICMA/accrued/calendar methods still
require their genuine complete migration and qualification.


## Checked accrued-interest migration

The four native constant-coupon calculator methods and both yield-engine accrued
traits now return Result. Full coupon context corrects ISDA and ACT/365L accrual;
ex-coupon requests consume an actual supplied calendar and revision. The strict
shared `accrued_interest` route retains original face cash units and delegates
to the same native owner. Unsupported entitlement/calendar context fails.
Use `Act365Fixed` and `Thirty360US`; their ambiguous short aliases are removed.
Read [the accrued-interest contract and migration](ACCRUED-METHODS.md).
Remaining general dated yield/reference and calendar methods, full product/model/
calibration/claim/physical/mixed-book integration and mandatory mappings remain
required. This increment supplies no independent approval or Windows/Excel check.


## Checked cash projection and short-date policy

The ambiguous flat-model money-market yield helpers are now checked
`constant_rate_return` adapters. Shared `reinvestment` dispatch retains complete
native terms, original cash units and explicit separate rates. Short-date policy
construction/queries now return `Result` and consume the enabled flag. The
unqualified YAS money-market row and fabricated calculator true yield are retired.
Read [the supplied-cash projection and migration](REINVESTMENT.md).


## Stable periodic growth and retired supplied-value analysis

The native reinvestment owner evaluates logarithmic growth before exponentiation
and rejects material nonzero Decimal conversion loss. Existing supplied terms
and units remain. The unused `YasAnalysis` / `YasAnalysisBuilder` facade, which
manufactured zero for missing measurements, is removed with its reexports. Use
actual supported `YASCalculator` results and genuine absence in caller records.
See [the projection migration](REINVESTMENT.md).

## Explicit dated discount context

The dated yield solvers no longer infer nominal periods or stub status from
cash order, round a month interval, clamp off-cycle timing or silently fall back
to a date-only convention. Bind the actual `DiscountTimeBasis` using
`StandardYieldEngine::with_discount_basis` or `YieldSolver::with_discount_basis`.
Missing ICMA, ACT365L or termination context now returns a typed error.
The standard engine uses the supplied discount day count independently of accrual;
callers must remove the workaround that overwrote the accrual role.
Use [dated cash timing](DATED-YIELD.md) and shared `cash_yield/dated_price`,
`dated_solve`, `dated_risk` for explicitly supplied entitled dated cash. Full
contract schedules, entitlement, quotation policy and paid-claim state retain
their actual instrument/workflow owners.

## Strictly future cash with zero discount time

An actual payment after settlement can have zero discount years under 30/360.
`PreparedDatedCash::new` validates the original dates and full discount context;
its price retains that cash at a discount factor of one. Dated analytical risk
keeps its PV with zero first and second yield derivatives. A zero discount time
does not mean a payment has settled or an outstanding claim has disappeared.

`YieldSolver::solve_dated_cash` retains constant cash and reprices the complete
original cash before accepting a variable-cash yield. Constant price has no
identified yield: a matching target returns `BondError::YieldNonUnique`, and
other targets return `BondError::YieldNoRoot`. Mixed nonnegative cash also has
no root at or below its constant-cash floor. Rust callers with exhaustive
`BondError` matches must handle these two added variants. Shared diagnostics
retain `invalid_input` with the distinct native kind. Raw supplied-time price
and inverse methods retain their positive-time contract.

The uncalled crate-private flat-reference discount-margin helper is removed.
It had no public reexport or callers. Existing fixing-aware FRN and complete
curve/model workflows retain their original requirements and owners.


### Native instrument discount-time context

`Bond::yield_cash_flows` now returns `PreparedDatedCash`. Its slice view and
iteration retain original per100cash units; mutation and construction from raw
numerical times are unavailable. Custom implementations must return cash checked
against actual future dates and a complete `DiscountTimeBasis`. Fixed-bond ICMA
time uses the original schedule anchor, stub and roll through every actual
payment, including the next nominal period after maturity. Coupon entitlement
and accrued amounts retain their original owners. Other fixed-bond quotation
conventions retain their existing semantics and their remaining qualification.

Fixed-bond price and inverse methods delegate to the prepared native cash owner.
Strictly future zero-time cash stays in PV. Matching constant-price targets return
`YieldNonUnique`; other positive targets return `YieldNoRoot`. Ordinary positive
time follows the same original solver and native zero-coupon kernel.

`ZeroCouponBond::{price_from_yield_with_basis,yield_from_price_with_basis,
continuous_rate_with_basis}` accepts an explicit matching discount context.
ICMA requires its actual nominal mesh; ACT365L requires actual determination
periods; 30E/360 ISDA requires the original termination matching maturity. The
context-free methods keep their existing convention boundary. These methods
use absolute original Decimal redemption and price, independently of face. At
zero time, a valid yield prices the exact redemption and inverse methods return
a structural root error. Public raw time-only zero-coupon helpers still require
positive years. This API does not choose a market convention or supply a model
or production certificate.

## Checked native calendars and floating fixing dates

`FloatingCouponBond::fixing_date(reset_date)` now returns `BondResult<Date>`.
Propagate its error with `?`. It resolves the actual configured calendar and
uses the existing bounded business-day offset. UK, Japan and joint calendars
retain their actual rules; unsupported identifiers fail through the checked
resolver. The reset date and signed original reset lag remain explicit.

Use `Calendar::try_is_business_day` at input admission. Built-in bitmap-backed
calendars validate their actual stored 1970–2100 range; checked searches,
offsets, adjustment and direct native calibration/futures/CDS/lifecycle queries
propagate coverage failures. Custom calendar implementations can declare their
own coverage with `Calendar::validate_date`. Their callbacks must terminate.
Zero offsets and Unadjusted dates retain their date-only contracts.

`DynamicCalendar` retains the exact supplied holiday set across construction,
updates, merge and chronological export, including supplied dates outside the
built-in bitmap range. Its JSON shape and supplied weekend rule stay unchanged.
The existing shared 4096-holiday admission bound stays. Data completeness and
reference provenance remain the caller's responsibility; this supplies no market
calendar or model approval.

The old first-settlement shortcut and fallback resolver remain pending their
separate retirement and protected-test owner review. Existing raw Calendar
helpers, loader/static-merge/generator ranges and date-only ARRC calculations
retain their separate required migration/qualification. These changes do not
waive any mandatory convention, model, operation, host or certificate gate.
# Checked native settlement invoice helpers

`SettlementInvoiceBuilder::build` now returns `SettlementInvoiceResult` and
requires `settlement_date`, `accrued_days`, `clean_price`, `accrued_interest`
and `face_value`. Supply actual metadata; omitted dates and days are errors.
The existing YAS calculator already supplies those fields. All nine serialized
invoice fields and ordinary amount calculations retain their existing units.

`calculate_proceeds` and `calculate_accrued_amount` return checked results.
Builder and proceeds share the same principal/accrued/net component calculation.
Dirty price and all reported amounts must be representable as native Decimals;
overflow and nonzero underflow fail without a partial accepted invoice. Signed
positions, negative accrued interest and explicitly supplied accrued-day counts
remain valid. Currency and ledger rounding belong to the caller; the scalar
invoice display no longer supplies a dollar symbol. Ordinary product order is
retained. Failed per100 products can use an exact two-place operand scale shift;
failed accrued products can try the other two bounded multiplication groupings.
These operations do not resolve a bond convention or authorize cash posting.

`calculate_settlement_date(trade_date, settlement_days, &calendar)` now requires
the actual supplied `Calendar` and returns a checked result. It delegates the
existing 366-business-day offset and 3660-calendar-day search limits, propagates
unavailable holiday coverage, and requires financial dates in years 1–9999.
Zero offset preserves the valid date without querying holiday data. Custom
calendar callbacks must terminate. The old unbounded Boolean-callback loop is
removed.

The unused `SettlementInvoice::calculate_settlement` recomputation and
`Bond::years_to_maturity` default are removed. Read the checked invoice's stored
`settlement_amount`. For maturity calculations, use actual contractual maturity
and the existing checked discount-time owner with an explicit convention/context;
no ACT/365 proxy is inferred. `ZeroCouponBond` retains its separate checked
instrument timing method. Full dated security quotation/invoice, lifecycle,
ownership, actual receipts and cash posting remain their existing owners.

## Native YAS quoted yield and risk

`YASCalculator::analyze` uses the existing canonical bond yield and risk owners,
including the original dated day count, coupon/principal normalization and the
configured periodic quote frequency. Supported counts are 1, 2, 4 and 12; other
counts return an error. The default remains semiannual. Yield is reported in
percent, duration in years, convexity in years squared and DV01 per 100 or actual
face. Checked native cash/accrual failures and unrepresentable measures propagate.

This correction is scoped to quoted yield and its risk. Remaining current/simple
yield, government/benchmark proxies, Z/ASW and batch cardinality work retains its
native owner. Those diagnostics require separate qualification. Full supplied
security/curve/model routes remain documented above; no vendor equivalence or
market/model/production certificate is implied.

## Complete-cash native yield risk

`BondRiskCalculator::from_bond` now takes the actual `Frequency` quotation basis
and prepares the bond's original normalized dated cash, including ICMA references.
Pass that same frequency through pricing and risk. For explicit Simple, continuous
or other compounding with supplied dated context, use `from_prepared_cash`.
`from_cash_flows` remains an explicit numerical-time route; it infers no dates.

The calculator, scalar duration/convexity helpers and dated cash share the checked
native derivative owner. Simple-interest modified duration consumes every cash
flow's denominator. `modified_from_macaulay` now returns `Result` and rejects
Simple interest because an aggregate time cannot determine its derivative; use
`modified_duration` with the original complete cash instead. Nonfinite inputs,
invalid domains, unrepresentable measures and nonzero arithmetic underflow fail.
Signed cash retains its actual positive net PV; supplied signed/zero face scales
position DV01 explicitly. Genuine future zero-time payments retain PV with zero
yield derivatives and require validated dated context.


## Checked risk arithmetic and MCP request admission

Native bond analytical risk now consumes the same immutable full-cash derivative
owner as the raw/prepared calculator. Native effective risk validates the actual
yield bump before repricing. Supplied-price duration/spread/key-rate stencils,
convexity, DV01 and price-change approximations share the native
`convex_bonds::pricing::risk_math` owner. These arithmetic stencils do not establish
model h/h2 convergence. Original effective/spread1e-12 bump minima and price1e-10
admission remain; native/key-rate methods retain smaller finite nonzero bumps.

`BondAnalytics::dv01_notional` consumes a per100 dirty price and scales actual
signed notional by100, independently of instrument face. For an instrument face
of200, the previous face denominator incorrectly halved the position result.
Analytical risk and every derived measure reject nonfinite or unrepresentable
arithmetic. Genuine zero positions and zero effects remain zero.

The following raw analytics now return `AnalyticsResult`: `price_change_from_duration`,
`price_change_with_convexity`, `dv01_from_duration`, `dv01_from_prices`,
`dv01_per_100_face`, `notional_from_dv01`, `BondRiskMetrics::estimate_price_change`,
`BondRiskMetrics::convexity_adjustment`, `EffectiveDurationCalculator::new`,
`KeyRateDurationCalculator::with_tenors`, `KeyRateDurationCalculator::with_bump_bps`
and `KeyRateDurations::total_duration`. Propagate `?` at actual callers; no zero
fallback or infallible compatibility route is supplied. Default10bp effective
and1bp key-rate bumps and standard tenors are unchanged. The key-rate calculator
requires the complete finite, positive, unique declared tenor set (at most4096),
accepts supplied row permutations and emits declaration order. Missing/duplicate/
unexpected factors fail. An undefined notional inverse fails explicitly.

The MCP `library` wrapper keeps its exact opaque `request_json` string and now
rejects extra wrapper fields, matching its generated schema. The native inner
request/diagnostic contract and library operations stay with their existing owner.
Legacy convenience-tool DTOs, registries and their qualifications remain separate.
`Duration`, `Convexity` and `DV01` now implement `TryFrom<f64>`; propagate the
result. Nonfinite values, Decimal overflow and nonzero underflow fail. Decimal
construction and serialization remain available. The infallible accessor uses
the locked Decimal implementation's total `as_f64` conversion, without a zero
fallback. The five protected test adapters have explicit separate owner approval.

The numeric `aggregate_portfolio_risk` helper returns `AnalyticsResult` and its
`weighted_duration` is optional. Signed value and DV01 survive a zero net value;
the undefined duration ratio is `None`. All supplied positions share the caller's
currency/unit; the helper does not price instruments or infer FX. The existing
absolute net-value guard stays `1e-10`. Key-rate lookup requires exact finite
positive tenor identity; nearby tenors are separate factors.

Scalar parametric VaR uses an actual inverse normal CDF under the explicit
zero-mean Gaussian and IID square-root-of-time assumptions. Historical VaR uses
the nearest-rank inverse empirical CDF of signed losses. Neither method clamps a
negative loss quantile to zero. See [the scalar contracts](RISK-CONTRACTS.md) for
units, exact confidence/rank semantics, bounds and remaining portfolio scope.

The four weekend-only Date methods `next_weekday`, `prev_weekday`,
`add_business_days` and `business_days_between` now return `ConvexResult`.
Propagate `?`; the twelve existing test adapters have separate owner approval.
They retain ordinary date/count semantics and use complete weeks plus at most
six daily steps. Calendar exhaustion returns an error, including signed-minimum
offsets. This does not replace a supplied holiday calendar.

The unused `Bond::first_settlement_date` method and `CalendarId::to_calendar`
fallback are removed, including their implementations and five approved mock
methods. Actual settlement uses `convex_core::cashflows::settlement_date` with
explicit trade date, lag, convention and calendar. Identifier resolution uses
`CalendarId::try_to_calendar`; unknown identifiers cannot become weekend calendars.

The MCP HTTP transport and CORS now share one explicit Origin list. By default it
contains the actual listener's own HTTP origin. Repeat `--allow-origin` to supply
an exact replacement list. Invalid, duplicate or excessive configuration fails
at startup; an untrusted Origin receives 403. Remote authentication/TLS, session
and frame limits and legacy convenience tools retain their separate requirements.

The older portfolio key-rate HTTP adapter now requires each actual market price,
rejects an invalid position as a whole and no longer invents an ISIN or drops a
failed conversion. Its broader Holding aggregation, factor coverage, weighting
and missing-result presentation still require their own migration.


## Supplied portfolio key-rate exposure

The existing key-rate endpoint now preserves missing factors and undefined ratios as
null, reports known currency DV01 separately from complete factor exposure, and
requires the supplied full dirty market_price basis. Migrate nullable client fields
and inspect per-factor coverage; never send a clean quote as a full dirty price.
The native optional views delegate the checked aggregate_key_rate_exposure owner.
Synthetic demo factor allocations, fallback risk and NAV-multiple VaR are removed.
See [the contract](PORTFOLIO-KEY-RATE-RISK.md) for units, bounds and remaining scope.


## Typed risk-decimal decoding

Duration, Convexity and DV01 share one checked Decimal decoder. Healthy native
Decimal values and serialized representation stay unchanged. Finite numeric
inputs keep their established shortest-decimal interpretation; strings admit
only exactly representable Decimal mantissas and supported exponents, without
rounding nonzero digits or overflow. Decimal text is bounded to8,192bytes.
This differs from native TryFromf64, which explicitly converts binary model bits.

Raw JSON input must first use convex_core::contracts::json::parse to reject
numeric tokens erased by an upstream binary64 parser. Generic Deserialize sees
the supplied parsed value and cannot recover a discarded original token. Keep
raw admission and typed Decimal admission separate. The supplied library and
key-rate REST paths already share that bounded raw owner. V1 and other
remaining decode/valuation/risk routes keep their explicit owning work.

## Incomplete REST stress routes removed

The `/api/v1/stress/test`, `/api/v1/stress/standard`, `/api/v1/stress/single` and
`/api/v1/stress/scenarios` routes are removed. Their request contracts could not
supply the required valuation context, compatible risk family, exact factor
coverage or foreign FX observations. Their converter used a hardcoded date and
could substitute prices, double count accrued interest or drop positions.
There is no calculation alias or fallback behind these paths.

Use the complete supplied-input library mixed-book scenario operations for full
model revaluation and attribution, including delivery baskets and CTD switches.
Direct Rust consumers can use the checked supplied-Holding Taylor estimate in
`convex_portfolio::stress` with an explicit `StressPolicy`. It is not a full model
scenario or realized P&L. A future transport must carry that complete declaration
and delegate to the same native owner.

The portfolio demo now displays its supplied sample positions and explicitly
face-based allocations. It does not invent market value, risk or scenario P&L.
The unrelated incomplete portfolio aggregate/quote conversion and remaining
provider integrations still require their own production-readiness work; removing
these stress paths does not qualify them.


## Supplied portfolio conversion and checked results

Portfolio, contribution, bucket and related supplied-input handlers propagate
checked calculation failures. The quote-to-Holding conversion uses the actual
clean mid price plus separate actual accrued per100; it requires one quote for
every requested position. Duplicate consumed quotes, missing prices/accrued,
invalid supplied identifiers/classifications and inconsistent dates/currencies
are errors. No par-price default, counterfeit ISIN or discarded builder failure
is admitted. Liquidity/credit/basket position converters also require supplied
prices and retain complete cardinality. Existing score-based ADV/liquidation
heuristics and other unqualified legacy approximations still require retirement
or complete explicit data/model contracts; this increment does not certify them.

The removed native incomplete-subset switch and engine minimum-coverage setter
have no supported replacement that labels a partial portfolio complete. Native
amount and aggregation callers must handle checked Results and explicit missing
values. Currency totals use exact strings; normalized output at a zero denominator
is unavailable or a typed failure where a legacy view requires a numeric ratio.

Batch portfolio analytics now returns one ordered `outcomes` array. Each entry
contains its zero-based `input_index`, original `portfolio_id`, and a `status`
of `success` with a native `result` or `error` with the actual diagnostic.
The former split success/error arrays and their error-only DTO retire; repeated
portfolio IDs and interleaved failures retain their original request positions.

## Checked native curve conversions

`ValueConverter` methods return `CurveResult`; handle rejected domains and
unrepresentable arithmetic explicitly. `convert_compounding(rate, time, from,
to)` uses the actual positive year fraction, including Simple compounding.
`instantaneous_forward(rate, derivative, time, compounding)` accepts the rate
and its derivative in the declared quotation and differentiates that same law.
Discount factors at time zero remain one. A bare discount at zero does not
define an annualized rate. `RateCurve` retains a supplied zero quotation at the
origin, or derives its limit from an actually supplied normalized discount and
analytic derivative. Simple quotations use their zero-time limit; periodic
quotations use horizon-independent conversion. Missing source information fails.

`RateCurve` zero and forward queries preserve checked source errors and declared
tails. An instantaneous forward requires a supplied analytic derivative; a
one-day approximation is no longer selected implicitly. `hazard_to_survival`
is explicitly constant-intensity arithmetic. `risky_discount_factor` describes
an explicitly supplied terminal recovery payoff, without a CDS or default-time
recovery assumption. Full dated IRS/OIS, CDS and issuer-credit pricing continue
through their complete supplied-term analytics owners. Broader native date,
par-grid and credit-wrapper semantics still require their own qualification.
