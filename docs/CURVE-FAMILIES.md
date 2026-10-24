# Complete supplied parametric, segmented and delegated rate curves

`convex::curve_contracts` extends the original checked `CurveRecipe`,
`build_curve_set`, immutable `BuiltCurve` and `CurveQuery`. Financial values use
the original `NelsonSiegel`, `Svensson`, `SegmentedCurve`, `DelegatedCurve` and
bounded least-squares implementations. No sampled zero grid replaces a model.
All values, identities, revisions, domains and numerical settings are supplied.
No provider, storage, MQ, clock or authority is consulted.

## Original parametric models

`CurveSource::Parametric` supplies `parameters`, a finite explicit `horizon`
and every original dated `observations` record. `ParametricModel` is a strict
`kind = nelson_siegel` or `svensson` object with every original beta and decay.
Betas are annual decimal zero-rate loadings; decays are years of the recipe's
actual time axis. The matching `interpolation` tag is `NelsonSiegel` or
`Svensson`. Representation is `ZeroRates { compounding: Continuous }` under
an explicit rate role/currency/reference/time basis. This model has no node grid.

The [BIS technical documentation](https://www.bis.org/publ/bppdf/bispap25.pdf)
describes the spot and instantaneous forward equations from these parameters.
This route preserves the existing native equations. Supplied zero observations
are different economics from coupon-bond prices or par swap rates; those still
require their original full product helpers and pricers.

Raw zeros, DF/ratios and finite-period zero/forward quotations use the original
checked query/conversion functions. `InstantaneousForward` uses the original
model's analytic forward, including its finite origin limit, within
the declared horizon. It does not infer derivatives for a composed/grid curve
or extend a model forward through a raw flat-zero boundary.

Direct mode reuses the original finite beta/positive finite decay constructor
guards, positive finite horizon through 150 years, and up to 1,024 observations.
Each observation retains unique ID, independent exact quote revision, actual
positive date within the horizon, decimal/percent/basis-point continuous-zero
unit and a positive original absolute residual tolerance in that unit. Every
supplied observation must pass. Empty observations mean supplied parameters;
they imply no fit, objective, rank or market calibration claim.

`None` rejects positive-time queries beyond the horizon. `Flat` holds the raw
zero at that horizon; the DF still applies that zero over the actual query
period. Extending the parametric formula requires explicitly supplying a longer
model horizon; no empirical fit coverage or hidden model extrapolation is claimed.
Other tail selections fail, and every DF must be positive and finite.

## Bounded native zero fitting

`fit_parametric_curve(&recipe, &settings)` fits the actual four/six parameters
to all original dated zero observations. `ParametricFitSettings` supplies exact
lower/upper vectors in native parameter order, one native scale and weight per
original observation, bounded attempts/objective and original finite-difference/
undamped-SVD controls. Beta/decay bounds retain the direct model domains.
Observation order is retained. Scales change the objective, never quotation units.

The original solver requires full weighted undamped identification, the scaled
RMS bound and every original native residual, including zero-weight inputs.
Dimensions are four/six parameters and 1..128 observations; original attempts
are bounded by 10,000, with bounded SVD and finite-difference work. Rank, condition,
actual evaluations/iterations, objective, native residuals and active bounds remain
visible. Invalid, unidentified, stalled, iteration or numerical failures return
diagnostics without a curve. No relaxed retry or best-effort accepted fit exists.

Output preserves `input_recipe` and `settings` separately from the actual solved
curve recipe/parameters. The latter can be reconstructed directly and rechecked
against the same original observations; it does not lose the starting conditions.
This is a local bounded zero-observation fit. Complete
[product-price parametric fitting and quote-risk rebuilding](PRODUCT-PARAMETRIC-CALIBRATION.md)
have a separate owner in the existing full rate/cash calibrators. Global optimum
and complete market convention qualification remain required work.

## Exact segmentation and delegation

`CurveSource::Segmented { segments, join_tolerance }` has the `Segmented`
construction tag. Each segment supplies actual start/end points, an exact full
`base` curve version and original `fallback`. The graph contains that base's
full source/representation/method/extension contract. Sources must match role,
currency, reference, time basis and representation exactly; an explicit equivalent
view can change representation before composition. No anonymous replacement,
sorting, gap, overlap, unbounded tail or interpolation substitution is allowed.

One to 128 ordered intervals cover `[0,last_end]` contiguously. Internal ends are
exclusive; the right source owns each join, and the final endpoint uses the final
source value. Every join retains actual left/right versions/values, signed raw
residual and the caller's unchanged absolute C0 tolerance. Failed joins return no
curve. A DF composition retains exact P(0)=1. C0 does not assert continuous
derivatives or instantaneous forwards. Outer tails are `None` or raw `Flat`.

`CurveSource::Delegated { base, range, fallback }` has the `Delegated` tag.
The original native policies are explicit:

| Policy | Actual contract |
| --- | --- |
| `Strict` | Override lies wholly in the original source domain; `None` rejects raw points outside the override. |
| `Clamp`, `FlatExtrapolation` | `Flat` clamps the raw query to the explicit override endpoints. |
| `Trust` | Keeps the exact source domain and extension policy; it cannot discard a narrower supplied bound. |

Segment fallbacks apply against each dependency's own actual domain. Clamp/flat
boundaries must be actually obtainable from the source. Raw flat DF and raw flat
zero have different financial meanings. Responses retain dependency domains and
flag both outer and consumed inner extensions, including extension hidden inside
an otherwise in-range segment or derived view. Discount P(0)=1 remains the
mathematical definition; a raw query outside a strict interval still fails.
Raw clamp queries at the origin disclose the consumed extension. Discount and
survival origin identities consume no source value and report no extension.

Exact dependency version admission, atomic graph construction, 256-curve capacity
and 32-level derivation bounds remain. Native wrappers evaluate each selected
dependency once; nested delegation does not duplicate pricing work. These are
rate DF/zero contracts. Credit/basis segmented hazard/integration, arbitrary
derivative queries and complete curve-risk/helper combinations remain separate
requirements and fail explicitly where unsupported.

## Shared API and reproduction

Shared `curves`, `curve_query`, `curve_time` and `roundtrip` consume the complete
new recipes. `parametric_fit` supplies original `recipe` and `settings` and emits
the native solved curve plus solver diagnostics. Existing generic JSON/C/Java/
Excel/MCP/WASM clients delegate unchanged. New source/policy/parameter fields are
strict: missing or unknown data and sampled-grid aliases fail.

```sh
python3 tests/scripts/curve_family_reference.py check
python3 tests/scripts/library_curve_family_inputs.py
cargo test --locked -p convex-curves --all-features --test curve_families
cargo run --locked -p convex --example library_curve_families --features schemars
```

The supplied corpus selects exact original examples such as
`curve.model.fit.ns.positive`, `curve.segment.describe` and
`curve.delegate.Strict.3`. Independent Decimal(60) economics were frozen before
candidate valuation; generated native parity envelopes are transport expectations.
CVX-063 remains partial, all original mandatory records remain Planned, and actual
Windows/Excel and wasm32/browser execution remains unverified. No model, market,
library, production or latency certificate is inferred.

Remaining grid methods are documented in [explicit curve methods](CURVE-METHODS.md). Full product fitting uses the separate [parametric calibration](PRODUCT-PARAMETRIC-CALIBRATION.md) contract and original product callbacks.

An [explicit UFR tail](CURVE-UFR.md) can join the original parametric model
inside its declared native horizon. It retains the exact source and analytic
left forward, rather than resampling its parameters into a node grid.

The engine now retains and reconstructs complete original supplied recipes
through [schema-2 snapshot replay](CURVE-SNAPSHOT-REPLAY.md), including native
source/model/date/unit/role/time/method/tail and exact dependency versions.
Full product-calibration snapshots and quote-risk graphs remain separate work.
