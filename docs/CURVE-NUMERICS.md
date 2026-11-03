# Curve fitting, conversions and rolling

This document covers the numerical operations added to `convex::curve_contracts`
and the bounded least-squares fitting code in `convex_math::optimization`. They
cover money-market curve fitting, rate conversion, discount-equivalent views
and curve rolling. You pass in dates, curves, quotes, settings and versions. No
engine, server, database, message queue, vendor or evaluation clock is needed.

Run the [standalone consumer](../crates/convex/examples/curve_numerics.rs):

```sh
cargo run --locked -p convex --example curve_numerics
cargo run --locked -p convex --example curve_numerics --no-default-features
cargo run --locked -p convex --example curve_numerics --features schemars
```

It fits dated negative-rate deposits in decimal and bp units, checks rank and
repricing, creates a daily-zero view, rolls with an explicit policy, and checks
a failed one-step fit. The feature runs show that the Rust API is reachable.
The schema build adds no JSON Schema or C ABI dispatch.

## Domains, methods and precision

The [curve contracts](CURVE-CONTRACTS.md) define units, dependencies and method
selection. Queries return finite values in their stated domains. Positive
discounts may exceed one, and rate curves and forwards need not be nonnegative.
Survival curves separately need positive nonincreasing probabilities.

| Path | Policy |
|---|---|
| Direct nodes | Sort associated points/values; reject duplicate or less than `1e-10`-year-separated pillars; explicit P(0)/Q(0)=1 |
| Linear, LogLinear, CubicSpline | Reuse existing kernels; LogLinear requires positive DF/survival; CubicSpline rate/DF queries may fail between valid nodes |
| Extension | Explicit None, raw-value Flat or raw-value Linear; no clipping of invalid output; survival has None/Flat only |
| Other discrete/parametric/segmented methods | Existing separate APIs keep their constraints; recipe input returns Unsupported without substitution |
| Dated deposit/FRA chain | Sorted unique IDs/maturities; starts at origin/solved pillars; positive simple accumulation; every quote residual meets the supplied tolerance |
| Dated global fit | Explicit parameter dates, continuous zeros, supported rate interpolation/extension; helper dates/units preserved |
| Rate conversion | Explicit Simple/Continuous/Annual/SemiAnnual/Quarterly/Monthly/Daily, actual period fraction and positive compounding bases |
| Date/tenor | ACT/360, ACT/365F, ACT/ACT ISDA axes; a different output basis needs dates; context-dependent bases remain Unsupported |

`discount_from_rate`, `rate_from_discount` and `convert_rate` use `ln_1p` and
`exp_m1` for simple and periodic accumulation. Rate-to-rate conversion works
directly in log accumulation. This keeps a tiny period from being lost when its
discount rounds to one. Rates over zero-length periods are undefined. P(0)=1
holds by definition, but periodic bases are still validated at the origin.

These are f64 operations. If a discount you pass in rounds to exactly one, no
nonzero rate can be recovered from it. Tiny differences, very short periods and
extreme discounts need product-specific precision studies. These fail:

- overflow or underflow to an invalid discount;
- invalid fractions or bases;
- nonfinite arithmetic.

The infallible `ValueConverter` and the older calibration kernels are
unchanged.

The new analytic fixtures use `abs(error) <= 1e-11 + 1e-11 * abs(expected)` in
the stated DF or rate units. Short-period tests use tighter explicit bounds.
The DF reference uses independent Python Decimal exponentials at 50-digit
precision. Earlier quantitative artifacts, expectations, tolerances and
defaults are unchanged. These tests have not been independently reviewed.

## Global fitting and stopping

Call `fit_money_market_curve(recipe, parameter_dates, controls)`, then add a
successful curve with `CurveSet::with_curve`. The recipe supplies Global
settings and full MoneyMarket helpers, and the output is continuous zero rates.
Starts correspond to the parameter dates you pass in, and scales and weights
correspond to the helpers. Pairs sort together deterministically. The origin
zero is tied to the first parameter, so there is no unidentified extra
variable.

Global helpers may share a maturity. All are kept, each with its own tolerance.
Conflicting observations fail when those tolerances cannot all be met.
Parameter dates must stay distinct. Piecewise-chain construction rejects
duplicate maturities.

`bounded_least_squares` also accepts independent residual callbacks, and the
parameter and observation counts may differ. Callbacks must terminate and not
panic. Typed failures propagate, including missing-fixing lists, and no zero
residual is substituted.

For residuals `r_i=model_i-input_i`, use `e_i=sqrt(weight_i)*r_i/quote_scale_i`.
The objective is `sqrt(sum(e_i^2)/sum(weight_i))`. Scales are positive
quantities in the quote's unit. Weights are nonnegative, with enough positive
observations to identify the parameters. A zero weight never waives a quote's
own tolerance. Changing between decimal, percent and bp units means scaling
quotes, scales and tolerances together.

Success needs:

- the scaled RMS target you supplied;
- every individual quote tolerance;
- full undamped weighted-Jacobian rank, even at an exact starting point.

Finite differences stay inside closed bounds and become one-sided at a bound.
A bounded SVD checks rank and conditioning and solves the damped augmented
system `[J; sqrt(lambda) I]`, without an inverse or `J'J`. Trial parameters
project onto the bounds. Damping starts at `1e-3`, varies by a factor of 10 and
stays in `[1e-12,1e12]`. It cannot relax the residual thresholds. There is no
stochastic start, no last-good substitution and no hidden rate clamp.

Diagnostics report attempts, evaluations, the last residuals, scaled RMS, the
rank and condition estimate, and active bounds. Invalid input, callback
failure, nonfinite arithmetic, ill conditioning, SVD and LM limits, and
stagnation are distinct typed errors. A failure returns no fitted parameters or
curve.

Limits:

- 128 parameters and observations;
- 1 to 10,000 LM attempts;
- 1 to 10,000 iterations per SVD;
- a default difference step of `1e-6`, singular ratio `1e-10` and SVD budget of
  1,000;
- a callback count of at most
  `1 + max_iterations + 2 * parameters * (max_iterations + 1)`.

These are setup ceilings. They are not latency guarantees or bounds on
arbitrary callbacks.

The v1 `build_curve_set` Global route is still Unsupported. The named fitting
function is the dated deposit/FRA global route. The older `GlobalFitter` and
`PiecewiseBootstrapper` keep their fixed defaults, bounds and residual
definitions, with compatibility tests for negative rates and failures.
`SequentialBootstrapper` is diagnostic only. OIS, IRS and basis, CDS,
government, issuer and repo, and option calibration are not covered.

## Exact views and rolling

`CurveSource::DiscountEquivalent` expresses the immutable base discount
function as DF or as explicitly compounded zeros. Currency, role,
reference/time basis and source methods match the base. The base is evaluated
directly, so nodes are not converted and reinterpolated. The inherited
interpolation label describes the source function, not a fresh interpolation of
output zeros.

`CurveSource::Rolled` needs Direct mode, DF output, an exact base version and a
`RollPolicy`. The new reference date moves forward inside the original domain,
leaving a nonempty horizon, even when a tail is enabled.

| Policy | Meaning |
|---|---|
| StickyForward | `P_new(t)=P_old(offset+t)/P_old(offset)`; offset is the old-to-new reference fraction; remaining horizon shrinks by offset |
| StickyZero | `P_new(t)=P_old(t)`; preserve remaining-tenor zero structure/source domain; fixed-calendar discounts generally change |

On supported additive time axes, StickyForward also equals
`P_old(date)/P_old(new_reference)` for dates, including ACT/ACT ISDA leap
years. Queries keep the rate conventions requested. Domain and tail failures
propagate, and any extension is disclosed. The source's 150-year query ceiling
still applies to shifted time.

Rolling is a declared scenario. It is not realized cash, a new market
calibration or a market prediction. Credit and volatility rolls, backward
rolls, quote rebuild and implicit method changes are not supported.

At most 32 nested equivalent, roll and spread views can be built, including
chains across separate calls. Failed fits and builds leave supplied sets
unchanged. Dependencies, duplicate IDs and set size are checked.

New source variants need new arms in exhaustive Rust enum matches. Old
serialized recipes and client and host dispatch are unchanged. The fitting
controls reject unknown fields. There is no database migration and no
automatic version replacement.

QuantLib's
[implied term structure](https://github.com/lballabio/QuantLib/blob/master/ql/termstructures/yield/impliedtermstructure.hpp)
illustrates explicit future-reference structures, and its
[LM interface](https://github.com/lballabio/QuantLib/blob/master/ql/math/optimization/levenbergmarquardt.hpp)
separates the objective from the stopping criteria. The rank, residual and
immutable-view policies here are our own design. The pinned nalgebra SVD source
confirms that `try_new` takes positive iteration limits and can return None or
solve failures. No external model binary or new numerical dependency is used.
