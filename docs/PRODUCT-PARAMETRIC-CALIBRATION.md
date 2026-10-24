# Full product-price Nelson–Siegel and Svensson calibration

The existing pure rate and cash calibrators accept
`CalibrationMode::Parametric(Box<ProductParametricSettings>)`. The source remains
`CurveSource::Helpers` with every original full product in source order. Each
trial uses the original analytic NS/Svensson continuous-zero model and the
original full product pricer. Actual bond prices, loan rates, futures price
points and swap/FRA/basis rates drive the objective. Neither supplied zero
observations nor a sampled interpolation grid replace those products.

The [BIS technical documentation](https://www.bis.org/publications/paper-25-zero-coupon-yield-curves-technical-documentation.pdf)
describes these parametric families and product-price fitting. Convex retains
its existing native formulas; the new code composes those formulas with existing
full product callbacks and the existing bounded least-squares solver.

## Inputs and ownership

`ProductParametricSettings` requires all of the following; no numerical choice
is inferred from currency, provider, instrument name or model family:

| Field | Meaning |
| --- | --- |
| `initial` | Complete `ParametricModel`, with original loadings and decay years |
| `lower`, `upper` | Finite closed native bounds containing every start |
| `parameter_scales` | Positive native parameter scales, in exact model order |
| `quote_scales` | Positive native quote scales, in original helper order |
| `weights` | Nonnegative original helper weights |
| `max_iterations` | 1–256 original LM attempts, within existing family work bounds |
| `objective_tolerance` | Positive weighted scaled RMS target, at most one |

NS order is `[beta0, beta1, beta2, tau]`; Svensson order is
`[beta0, beta1, beta2, beta3, tau1, tau2]`. Loadings are decimal continuous rates.
Decays are positive years of the curve's actual time axis. The solver works in
coordinates `native_parameter / parameter_scale`; the enclosing graph's
explicit Jacobian step and measured condition estimate use those coordinates.
Returned model parameters are always restored to their original native units.
An endpoint clamp removes multiplication rounding at a supplied closed bound;
it does not widen that bound.

The original graph controls remain the only finite-difference/SVD/rank policy.
The original solver, damping policy and defaults remain unchanged. Scaling can
change the effect of absolute damping even when it preserves native finite
differences. Quote residuals alone do not certify parameter accuracy or global
uniqueness; the undamped local rank/condition diagnostics describe the actual
supplied coordinate policy. There is one local solve, without automatic retries,
start selection, work expansion or hidden objective changes.

The complete `RateCurveRecipe` or `CashCurveRecipe` still requires actual dated
terms, quote units/revisions, calendars, exact output/dependency pins, settlement,
cash steps, publication/history and role/model policy. `bootstrap` must be absent.
The matching method is `NelsonSiegel` or `Svensson`; tail policy is `None` or
explicit raw-zero `Flat`. Coverage dates are strictly increasing future dates;
their last date is the exact native model horizon, not a grid parameter. Every
actual helper query and delayed payment must fit that horizon.

## Admitted product composition

The rate owner supports its existing complete dated deposits/FRAs, IRS/OIS,
exchange rate futures and basis swaps. Separate collateral discount, index
projection and explicit overnight zero-basis views retain their original exact
bindings. Full publication/due-history and resource admission precedes trials.
Dependency ordering remains atomic; this is not a simultaneous cyclic solver.

The cash owner supports its existing full plain fixed/stepped/amortizing/zero
government securities and dated collateral repo loans on ACT365F continuous
zeros. Existing clean/dirty, cashflow-yield, bank-discount and par-coupon quote
conversion remains native-owned. Signed issuer residual basis and credit recovery
models have different financial meanings and reject this zero-model mode.
Floating or optional cash calibration helpers remain outside the admitted owner.
The original cash quote inverse conventions and their own work/tolerances remain
required when those quotation kinds are used.

At least four NS or six Svensson positive-weight informative observations are
required, with at most 128 original helpers. All original native quote tolerances
remain mandatory, including zero-weight quotes. The existing 16,384 actual helper
valuation ceiling includes trial/preflight/final work. Rank failure, inconsistent
quotes, missing history/pins, domain/horizon errors and work/convergence failures
return typed errors with available original diagnostics and no accepted curve.
Supplied snapshots and last-good curves remain immutable.

## Output, risk and snapshot replay

The fitted `BuiltCurve` retains the complete original Helpers recipe while its
native query owner contains the actual solved model, exact horizon and analytic
forward formula. `interpolation_grid()` rejects this family. Rate and cash solver
diagnostics return the complete native model, attempt/evaluation counts, scaled
objective, every original residual, undamped rank/conditioning and active bounds.
Full source-order product valuations also survive shared JSON output. The old
cash `annualized_parameters` grid field is empty for this mixed-unit mode; decay
years are never labelled as annualized rates.

Use the existing shared `calibration` operation for builds, prices, scenarios,
original product quote risk and dependent rebuilding. The existing separate
`parametric_fit` operation remains the native-zero-observation owner. No new
server route, C symbol, package, provider/storage/MQ integration or order authority
is added. Actual C JSON, MCP and native WASM exports use the same strict decoder
and native dispatch. Schema admission checks payload shape; the native financial
owner still checks economics and numerical postconditions.

[Schema3 capture/replay](CALIBRATION-SNAPSHOT-REPLAY.md) preserves the complete
original parametric product input, separately supplied numeric capture revisions,
exact model and full fitted/helper/solver diagnostics. File reopen and actual
runtime/recovery rebuild through the native owner and compare the complete output
before use. Original quote risk rebuilds the full product graph for each of the
five PVs, with original half-step stability rules. Tampered parameters, helper
outputs, quote revisions or capture identity reject atomically. Existing schema1,
schema2 and nonparametric schema3 identities remain unchanged.

## Executable engineering controls and limits

```sh
cargo run --locked -p convex --example product_parametric
cargo run --locked -p convex --example product_parametric --no-default-features
cargo run --locked -p convex --example product_parametric --features schemars
cargo test --locked -p convex -p convex-analytics -p convex-engine --all-features --test product_parametric -- --test-threads=1
cargo test --locked -p convex-ffi -p convex-mcp -p convex-wasm --all-features --test product_parametric
python3 tests/scripts/product_parametric_reference.py --check
python3 tests/scripts/product_parametric_admitted_reference.py --check
python3 tests/scripts/product_parametric_scaled_reference.py --check
python3 tests/scripts/product_parametric_strict_reference.py --check
python3 tests/scripts/product_parametric_conditioned_reference.py --check
python3 tests/scripts/product_parametric_starts_reference.py --check
```

The fixtures contain 12 explicitly fictional NS/Svensson × positive/negative ×
deposit/government/repo graphs and 60 independent Decimal(60) dated quote/model
controls. Full government cash includes coupon and notional steps, redemption
premiums and actual following payment dates. Financial answer/tolerance controls
were frozen before native execution. Separate declarations preserve the first
invalid short-stub input, unscaled/looser-objective and exhausted-work attempts.
The final input explicitly supplies stricter objective/coordinates and two closer,
nonzero local Svensson bond starts. Those caller choices are not universal defaults
or a global convergence promise. None of the independent financial answer bounds
is loosened. Native-generated full mixed-rate quotes check composition only and
are not an independent financial oracle.

CVX-063 remains partial, unaccepted and uncertified. All mandatory instrument/
market/method records and independent financial/library/production/latency gates
remain open. Actual Windows/.NET/Excel and wasm32/browser execution is unverified
in this local slice. Remaining required curve methods, optional bond models,
workout conventions and constrained cross-instrument hedging are separate scope.
