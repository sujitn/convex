# Numerical curve behavior and transformations

CVX-060 adds checked operations to `convex::curve_contracts` and a bounded
setup/calibration kernel in `convex_math::optimization`. Dates, curves, quotes,
settings and versions are caller-supplied. No engine, server, DB, MQ, vendor or
evaluation clock is needed.

Run the [standalone consumer](../crates/convex/examples/curve_numerics.rs):

```sh
cargo run --locked -p convex --example curve_numerics
cargo run --locked -p convex --example curve_numerics --no-default-features
cargo run --locked -p convex --example curve_numerics --features schemars
```

It fits full dated negative-rate deposits in decimal/bp units, checks rank and
native repricing, creates a daily-zero view, rolls with an explicit policy, and
checks a failed one-step fit. Feature runs establish native reachability. The
schema build adds no JSON Schema/C ABI dispatch. Certificates remain separate.

## Domains, methods and precision

The [curve contract](CURVE-CONTRACTS.md) defines explicit units, exact dependencies
and method selection. Queries return finite values in their stated domains.
Positive discounts may exceed one; rate curves/forwards need not be nonnegative.
Survival curves separately require positive nonincreasing probabilities.

| Path | Actual policy |
|---|---|
| Direct nodes | Sort associated points/values; reject duplicate or less than `1e-10`-year-separated pillars; explicit P(0)/Q(0)=1 |
| Linear, LogLinear, CubicSpline | Reuse existing kernels; LogLinear requires positive DF/survival; CubicSpline rate/DF queries may fail between valid nodes |
| Extension | Explicit None, raw-value Flat or raw-value Linear; no invalid-output clipping; survival has None/Flat only |
| Other discrete/parametric/segmented methods | Existing separate APIs retain constraints; checked recipe ingress returns Unsupported without substitution |
| Dated deposit/FRA chain | Sorted unique IDs/maturities; starts at origin/solved pillars; positive simple accumulation; all native quote residuals meet supplied tolerances |
| Dated global fit | Explicit parameter dates, continuous zeros, supported rate interpolation/extension; full helper dates/units preserved |
| Rate conversion | Explicit Simple/Continuous/Annual/SemiAnnual/Quarterly/Monthly/Daily, actual period fraction and positive compounding bases |
| Date/tenor | ACT/360, ACT/365F, ACT/ACT ISDA axes; different output basis requires dates; context-dependent bases remain Unsupported |

`discount_from_rate`, `rate_from_discount`, and `convert_rate` use `ln_1p` and
`exp_m1` for simple/periodic accumulation. Rate-to-rate conversion works directly
in log accumulation, preserving a tiny period before its discount rounds to one.
Rates over zero-length periods are undefined. P(0)=1 is mathematical; periodic
bases are validated even at the origin.

These are f64 operations. A supplied discount rounded to exactly one has no
recoverable nonzero rate. Tiny differences, very short periods and extreme
discounts need product-specific precision studies. Overflow/underflow to an
invalid discount, invalid fractions/bases and nonfinite arithmetic fail. Existing
infallible `ValueConverter` and older calibration kernels are unchanged.

New analytic fixtures use `abs(error) <= 1e-11 + 1e-11 * abs(expected)` in stated
DF/rate units; short-period tests use tighter explicit bounds. The DF reference
uses independent Python Decimal exponentials at 50-digit precision. All previous
quantitative artifacts, expectations, tolerances and defaults remain unchanged.
Supplied-contract engineering evidence does not award independent quantitative
approval, an admitted market pack or a product certificate.

## Global fitting and stopping

Call `fit_money_market_curve(recipe, parameter_dates, controls)`, then add a
successful curve with `CurveSet::with_curve`. The recipe supplies Global settings
and full MoneyMarket helpers; output is continuous zero rates. Starts correspond
to supplied parameter dates; scales/weights correspond to supplied helpers.
Pairs sort together deterministically. The origin zero is tied to the first
parameter, avoiding an unidentified extra variable.

Global helpers may share a maturity: all are retained, with their original
native thresholds. Conflicting observations fail when those thresholds cannot
all be met. Parameter dates remain distinct. Piecewise-chain duplicate
maturities are rejected, as defined by that construction mode.

`bounded_least_squares` also accepts independent native residual callbacks;
parameter/observation counts may differ. Callbacks must terminate and not panic.
Original typed failures propagate, including missing fixing lists, without a
zero residual substitute.

For native `r_i=model_i-input_i`, use `e_i=sqrt(weight_i)*r_i/quote_scale_i`.
The objective is `sqrt(sum(e_i^2)/sum(weight_i))`. Scales are positive native-unit
quantities; weights are nonnegative with sufficient positive observations to
identify parameters. Zero weight never waives a quote's native threshold.
Changing decimal/percent/bp units requires scaling quotes, scales and tolerances
together.

Success requires the supplied scaled RMS target **and every original native
tolerance**, plus full undamped weighted-Jacobian rank even at an exact start.
Finite differences stay inside closed bounds and become one-sided at a bound.
Bounded SVD checks rank/conditioning and solves the damped augmented system
`[J; sqrt(lambda) I]`, without an inverse or `J'J`. Trial parameters project onto
supplied bounds. Damping starts at `1e-3`, varies by 10, and stays in
`[1e-12,1e12]`. It cannot relax residual thresholds. No stochastic start,
last-good substitution or hidden rate clamp.

Diagnostics report actual attempts/evaluations, last native residuals, scaled
RMS, rank/condition estimate and active bounds. Invalid input, callback failure,
nonfinite arithmetic, ill conditioning, SVD/LM limits and stagnation are distinct
typed errors. Failures contain no fitted parameters or curve.

Limits: 128 parameters/observations, 1–10,000 LM attempts, and 1–10,000 iterations
per SVD. Default difference step is `1e-6`, singular ratio `1e-10`, SVD budget
1,000. Callback count is at most
`1 + max_iterations + 2 * parameters * (max_iterations + 1)`.
These are setup ceilings, not latency guarantees or bounds on arbitrary callbacks.

The v1 `build_curve_set` Global route retains Unsupported. The named fitting
function is the checked dated deposit/FRA global route. Older
`GlobalFitter`/`PiecewiseBootstrapper` retain their fixed defaults/bounds and
residual definitions, with negative-rate/failure compatibility tests.
`SequentialBootstrapper` remains diagnostic only. Product-backed OIS/IRS/basis,
CDS, government/issuer/repo and option calibration remains CVX-021/057/058/059.

## Exact views and rolling

`CurveSource::DiscountEquivalent` expresses the immutable base discount function
as DF or explicitly compounded zeros. Currency, role, reference/time basis and
source methods match. The base is evaluated directly; nodes are not converted
and reinterpolated. The inherited interpolation label describes the source
function, not a fresh interpolation of output zeros.

`CurveSource::Rolled` requires Direct mode, DF output, an exact base version and
`RollPolicy`. The new reference moves forward inside the original domain with a
nonempty remaining horizon, even when a tail is enabled.

| Policy | Meaning |
|---|---|
| StickyForward | `P_new(t)=P_old(offset+t)/P_old(offset)`; offset is the actual old/new reference fraction; remaining horizon shrinks by offset |
| StickyZero | `P_new(t)=P_old(t)`; preserve remaining-tenor zero structure/source domain; fixed-calendar discounts generally change |

On supported additive time axes, StickyForward also equals
`P_old(date)/P_old(new_reference)` for dates, including ACT/ACT ISDA leap years.
Queries retain actual requested rate conventions. Original domain/tail failures
propagate; extension is disclosed. The source's 150-year query ceiling still
applies to shifted time. Rolling/rebasing is a declared scenario, not realized
cash, a new market calibration or market prediction. Credit/volatility rolls,
backward rolls, quote rebuild and implicit method changes remain unsupported.

At most 32 nested equivalent/roll/spread views can be built, including chains
across separate calls. Failed fits/builds leave supplied sets immutable. Exact
dependencies, duplicate IDs and set size are checked.

New native source variants require new arms in exhaustive Rust enum matches.
Old serialized recipes and client/host dispatch remain unchanged. Numerical
controls reject unknown fields. No DB migration or automatic version replacement.

QuantLib's [implied term-structure source](https://github.com/lballabio/QuantLib/blob/master/ql/termstructures/yield/impliedtermstructure.hpp)
illustrates explicit future-reference structures, and its
[LM interface](https://github.com/lballabio/QuantLib/blob/master/ql/math/optimization/levenbergmarquardt.hpp)
separates objective/stopping criteria. Convex's rank/native-residual/immutable-view
policies are engineering choices. Installed pinned nalgebra SVD source confirms
positive `try_new` iteration limits and None/solve failures. No external model
binary or new numerical dependency is used.
