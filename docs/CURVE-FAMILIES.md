# Parametric, segmented and delegated curves

`convex::curve_contracts` extends `CurveRecipe`, `build_curve_set`, `BuiltCurve`
and `CurveQuery` with Nelson-Siegel, Svensson, segmented and delegated curves.
They use the existing `NelsonSiegel`, `Svensson`, `SegmentedCurve` and
`DelegatedCurve` types and a bounded least-squares fitter. No sampled zero grid
replaces a model.

The caller passes in all values, identities, revisions, domains and numerical
settings. Nothing is looked up from a provider, storage, a message queue or a clock.
Each function validates its input and returns an error instead of guessing.

## Parametric models

`CurveSource::Parametric` supplies `parameters`, a finite explicit `horizon`
and the dated `observations` records. `ParametricModel` is a strict
`kind = nelson_siegel` or `svensson` object with every beta and decay.
Betas are annual decimal zero-rate loadings; decays are years of the recipe's
actual time axis. The matching `interpolation` tag is `NelsonSiegel` or
`Svensson`. Representation is `ZeroRates { compounding: Continuous }` under
an explicit rate role/currency/reference/time basis. This model has no node grid.

The [BIS technical documentation](https://www.bis.org/publ/bppdf/bispap25.pdf)
describes the spot and instantaneous forward equations from these parameters.
This route keeps the existing Rust equations. Zero observations
are different from coupon-bond prices or par swap rates; those still
require the full product helpers and pricers.

Raw zeros, DF/ratios and finite-period zero/forward quotations use the existing
query and conversion functions. `InstantaneousForward` uses the model's analytic
forward, including its finite limit at the origin, within the declared horizon.
It does not infer derivatives for a composed or grid curve. It does not extend a
model forward through a raw flat-zero boundary.

Direct mode reuses the existing constructor checks: finite betas, positive finite
decays, a positive finite horizon of at most 150 years, and up to 1,024
observations. Each observation has:

- a unique ID and its own quote revision;
- a positive date within the horizon;
- a continuous-zero unit (decimal, percent or basis point);
- a positive absolute residual tolerance in that unit.

Every observation must pass its tolerance. With no observations, the parameters
are used as given and no fit is implied.

`None` rejects positive-time queries beyond the horizon. `Flat` holds the raw
zero at that horizon; the DF still applies that zero over the actual query
period. To extend the parametric formula, supply a longer model horizon. The
library does not extrapolate the model on its own.
Other tail selections fail, and every DF must be positive and finite.

## Bounded zero fitting

`fit_parametric_curve(&recipe, &settings)` fits the four (Nelson-Siegel) or six
(Svensson) parameters to all dated zero observations. `ParametricFitSettings`
supplies:

- lower and upper bound vectors in parameter order;
- one scale and one weight per observation;
- limits on attempts and objective evaluations;
- finite-difference and undamped-SVD controls.

Beta and decay bounds keep the direct-model domains. Observation order is
retained. Scales change the objective, never quotation units.

The solver requires full weighted undamped identification, the scaled RMS
bound and every residual, including zero-weight inputs. Dimensions are four or
six parameters and 1..128 observations. Attempts are bounded by 10,000, with
bounded SVD and finite-difference work. Rank, condition, evaluations, iterations,
objective, residuals and active bounds are all reported. Invalid, unidentified, stalled, iteration or numerical failures return
diagnostics without a curve. There is no relaxed retry and no best-effort fit.

The output keeps `input_recipe` and `settings` separate from the solved curve
recipe and parameters. The solved curve can be rebuilt directly and rechecked
against the same observations, and the starting conditions are not lost.

This is a local, bounded fit to zero observations. Fitting to product prices
and rebuilding quote risk is handled by the rate and cash calibrators; see
[product-price parametric fitting and quote-risk rebuilding](PRODUCT-PARAMETRIC-CALIBRATION.md).
Not yet covered: a global optimum and market-convention validation.

## Exact segmentation and delegation

`CurveSource::Segmented { segments, join_tolerance }` has the `Segmented`
construction tag. Each segment supplies start and end points, an exact `base` curve version and a
`fallback`. The graph contains that base's source, representation, method and
extension settings. Sources must match role,
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
The policies are explicit:

| Policy | Contract |
| --- | --- |
| `Strict` | Override lies wholly in the source domain; `None` rejects raw points outside the override. |
| `Clamp`, `FlatExtrapolation` | `Flat` clamps the raw query to the explicit override endpoints. |
| `Trust` | Keeps the exact source domain and extension policy; it cannot discard a narrower supplied bound. |

Segment fallbacks apply against each dependency's own domain. Clamp and flat
boundaries must be obtainable from the source. Raw flat DF and raw flat
zero have different financial meanings. Responses retain dependency domains and
flag both outer and consumed inner extensions, including extension hidden inside
an otherwise in-range segment or derived view. Discount P(0)=1 remains the
mathematical definition; a raw query outside a strict interval still fails.
Raw clamp queries at the origin disclose the consumed extension. Discount and
survival origin identities consume no source value and report no extension.

Dependency versions must match exactly, the graph is built atomically, and the
limits of 256 curves and 32 derivation levels apply. Each selected dependency
is evaluated once, so nested delegation does not duplicate pricing work. These are
rate DF/zero contracts. Credit/basis segmented hazard/integration, arbitrary
derivative queries and complete curve-risk/helper combinations remain separate
requirements and fail explicitly where unsupported.

## Shared API and reproduction

The shared `curves`, `curve_query`, `curve_time` and `roundtrip` operations accept
the new recipes. `parametric_fit` takes `recipe` and `settings` and returns the
solved curve plus solver diagnostics. The existing JSON, C, Java, Excel, MCP and
WASM clients call these unchanged. New source/policy/parameter fields are
strict: missing or unknown data and sampled-grid aliases fail.

```sh
python3 tests/scripts/curve_family_reference.py check
python3 tests/scripts/library_curve_family_inputs.py
cargo test --locked -p convex-curves --all-features --test curve_families
cargo run --locked -p convex --example library_curve_families --features schemars
```

The input corpus includes cases such as `curve.model.fit.ns.positive`,
`curve.segment.describe` and `curve.delegate.Strict.3`. Independent Decimal(60)
expected values were fixed before the Rust results were generated. The generated
envelopes only check that every host returns the same text as Rust.

Not yet covered: execution under Windows/Excel and under wasm32/browser, and
validation against market data. This work is partial, and no model, market,
library, production or latency certification is implied.

Remaining grid methods are in [explicit curve methods](CURVE-METHODS.md). Full product fitting uses the separate [parametric calibration](PRODUCT-PARAMETRIC-CALIBRATION.md) contract.

An [explicit UFR tail](CURVE-UFR.md) can join the parametric model
inside its declared horizon. It keeps the exact source and analytic
left forward, rather than resampling its parameters into a node grid.

The engine stores and rebuilds complete recipes through
[schema-2 snapshot replay](CURVE-SNAPSHOT-REPLAY.md), including source, model,
date, unit, role, time basis, method, tail and exact dependency versions.
Full product-calibration snapshots and quote-risk graphs are handled separately.
