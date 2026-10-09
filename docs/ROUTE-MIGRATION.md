# Removed government spread conveniences

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
| WASM `price_from_g_spread`, `price_from_benchmark_spread` | These exports are absent. Rebuild the WASM package and consuming application together. A complete government-spread price inverse is still pending; no replacement inverse is implied. |
| WASM `AnalysisResult.g_spread`, `benchmark_spread`, `benchmark_tenor` | These result fields are absent. Use `library(request_json)` for actual-security government-spread calculation. |
| Browser G-spread editor and synthetic benchmark-bond panel | Removed with their callbacks. The remaining spread panel displays actual Z/ASW/OAS results; unavailable OAS is not replaced by Z-spread. |
| Browser mock analytics after engine-load failure | Removed. Failure to load the engine displays an error and does not mount the analytics application. |

The [full supplied-input API](LIBRARY-API.md) calculates government spreads from
complete cash securities, actual quote/calendar/reference revisions, a common
explicit dated-cash yield convention and exact or adjacent contractual maturity
dates. It preserves fractional basis points. It does not assert Street/workout
convention coverage or implement a government-spread price inverse.

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
