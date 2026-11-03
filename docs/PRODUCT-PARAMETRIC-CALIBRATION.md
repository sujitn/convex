# Nelson-Siegel and Svensson calibration to product prices

The rate and cash calibrators accept
`CalibrationMode::Parametric(Box<ProductParametricSettings>)`. The source stays
`CurveSource::Helpers`, with every full product in source order. Each trial uses
the analytic Nelson-Siegel (NS) or Svensson continuous-zero model and the full
product pricer. Bond prices, loan rates, futures price points and swap, FRA and
basis rates drive the objective. Zero observations and sampled interpolation
grids do not replace those products.

"Checked" below means the function validates its input and returns an error
instead of guessing.

The [BIS technical documentation](https://www.bis.org/publications/paper-25-zero-coupon-yield-curves-technical-documentation.pdf)
describes these parametric families and product-price fitting. Convex keeps its
formulas. The calibration composes them with the full product callbacks and the
bounded least-squares solver.

## Inputs and ownership

`ProductParametricSettings` requires all of the fields below. No numerical choice
is inferred from currency, provider, instrument name or model family.

| Field | Meaning |
| --- | --- |
| `initial` | Complete `ParametricModel`, with loadings and decay years |
| `lower`, `upper` | Finite closed bounds in native units that contain every start |
| `parameter_scales` | Positive native parameter scales, in model order |
| `quote_scales` | Positive native quote scales, in helper order |
| `weights` | Nonnegative helper weights |
| `max_iterations` | 1-256 Levenberg-Marquardt (LM) attempts, within the work bounds of the family |
| `objective_tolerance` | Positive weighted scaled RMS target, at most one |

NS order is `[beta0, beta1, beta2, tau]`. Svensson order is
`[beta0, beta1, beta2, beta3, tau1, tau2]`. Loadings are decimal continuous
rates. Decays are positive years on the curve's time axis.

The solver works in coordinates `native_parameter / parameter_scale`. The graph's
Jacobian step and its measured condition estimate use those coordinates. Returned
model parameters are always restored to native units. An endpoint clamp removes
multiplication rounding at a supplied closed bound. It does not widen the bound.

The graph controls are the only finite-difference, SVD and rank policy. The
solver, damping policy and defaults are unchanged. Scaling can change the effect
of absolute damping even when it preserves native finite differences. Small quote
residuals do not show that the parameters are accurate or globally unique. The
undamped local rank and condition diagnostics describe the coordinate policy you
supplied. There is one local solve, with no automatic retries, start selection,
work expansion or hidden change to the objective.

A `RateCurveRecipe` or `CashCurveRecipe` still needs:

- Dated terms, quote units and revisions, and calendars.
- Output and dependency pins.
- Settlement and cash steps.
- Publication and history.
- A role and model policy.

`bootstrap` must be absent. The method is `NelsonSiegel` or `Svensson`. The tail
policy is `None` or an explicit raw-zero `Flat`. Coverage dates are strictly
increasing future dates, and the last one is the model horizon, not a grid
parameter. Every helper query and delayed payment must fit inside that horizon.

## Supported products

The rate owner supports deposits, FRAs, IRS and OIS, exchange rate futures and
basis swaps with complete dated terms. Collateral discount, index projection and
the explicit overnight zero-basis views keep their bindings. Publication and
due-history checks and resource checks run before any trial. Dependencies are
built in order, one after another; this is not a simultaneous cyclic solver.

The cash owner supports plain fixed, stepped, amortizing and zero government
securities, and dated collateral repo loans, on ACT365F continuous zeros. Clean
and dirty price, cashflow-yield, bank-discount and par-coupon quote conversions
are unchanged. Two kinds of model have a different financial meaning and are
rejected in this zero-model mode: signed issuer residual basis, and credit
recovery. Floating and optional cash helpers are not supported. When one of the
cash quotation kinds is used, its quote inverse conventions, work limits and
tolerances still apply.

At least four NS or six Svensson positive-weight observations that carry
information are required, with at most 128 helpers. Every native quote tolerance
must still hold, including for zero-weight quotes. The 16,384 helper valuation
ceiling counts trial, preflight and final work. These cases return typed errors
with whatever diagnostics are available, and no curve:

- Rank failure.
- Inconsistent quotes.
- Missing history or pins.
- Domain or horizon errors.
- Work or convergence failures.

Supplied snapshots and last-good curves are not modified.

## Output, risk and snapshot replay

The fitted `BuiltCurve` keeps the full Helpers recipe. Its query owner holds the
solved model, the horizon and the analytic forward formula. `interpolation_grid()`
rejects this family. Rate and cash solver diagnostics return:

- The native model.
- The attempt and evaluation counts.
- The scaled objective.
- Every residual.
- The undamped rank and conditioning.
- The active bounds.

The shared JSON output also keeps the product valuations in source order. The
cash `annualized_parameters` grid field is empty for this mixed-unit mode, so
decay years are never labelled as annualized rates.

Use the shared `calibration` operation for builds, prices, scenarios, product
quote risk and dependent rebuilding. The separate `parametric_fit` operation
still fits zero observations. This mode adds no server route, C symbol, package,
provider, storage or MQ integration, and no order handling. The C JSON, MCP and
native WASM exports share one strict decoder and one dispatch. Schema validation
checks the payload shape. The financial code still checks the economics and the
numerical postconditions.

[Schema3 capture and replay](CALIBRATION-SNAPSHOT-REPLAY.md) keeps the parametric
product input, the numeric capture revisions you supply, the model, and the
fitted, helper and solver diagnostics. Reopening a file and a runtime or recovery
rebuild both rebuild through the same code and compare the full output before
use. Quote risk rebuilds the full product graph for each of the five PVs, with
the half-step stability rules. Tampered parameters, helper outputs, quote
revisions or capture identity are rejected as a whole. Schema1, schema2 and
nonparametric schema3 identities are unchanged.

## Running the checks

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

The fixtures contain 12 fictional graphs, covering NS and Svensson, positive and
negative rates, and deposit, government and repo products. They also contain 60
independent Decimal(60) dated quote and model controls. Government cash includes
coupon and notional steps, redemption premiums and the following payment dates.
The expected answers and tolerances were fixed before the Rust code was run.

Separate declarations keep three earlier attempts: the first invalid short-stub
input, an unscaled or looser-objective input, and an input that exhausted its
work. The final input supplies a stricter objective and coordinates, and two
closer, nonzero local Svensson bond starts. These are choices made for the test.
They are not defaults and do not promise global convergence. None of the answer
bounds was loosened. Mixed-rate quotes generated by the Rust code check only that
the pieces compose. They are not an independent financial reference.

Not yet covered: Windows/.NET/Excel and wasm32/browser execution, some
instrument, market and method combinations, optional bond models, workout
conventions and constrained cross-instrument hedging.
