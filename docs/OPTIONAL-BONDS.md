# Callable and putable bond models

`convex::optional_bond` prices callable and putable bonds with a Hull-White
one-factor (HW1F) lattice. It covers model construction and calibration, optional
price, OAS inversion and fixed-model factor risk. It takes a validated
[BondContract](BOND-CONTRACTS.md) and immutable [curve inputs](CURVE-CONTRACTS.md)
that you pass in. No provider, database, broker, engine, server or system clock
is involved.

The combinations listed below are supported. Not yet covered: market data packs,
independent review of the numerics, host schemas and dispatch, and lifecycle. The
existing compatibility OAS and tree APIs keep their behavior and their
quantitative artifacts, and they do not use this contract.

"Validated" here means the function checks its input and returns an error
instead of guessing.

## Supported combinations

| Input/model | Supported behavior | Limits |
|---|---|---|
| Constant HW1F parameters | Named, versioned `OptionalBondModel::assumed`, with a nonempty rationale | Mean reversion 0..10 per year; sigma 0..1 decimal rate per square-root year. Zero is an explicit deterministic assumption. Accepting a parameter in its domain does not guarantee a finite numerical solution for extreme inputs. |
| Swaption sigma calibration | `calibrate_sigma(SigmaCalibrationInputs)`, fixed exogenous mean reversion, single-curve European swaptions | Fixed-leg schedule from expiry start, with stubs, day count and the calendar you pass in; payment at the accrual end; nonnegative fixed strike. Multicurve swaption calibration, negative-strike decomposition, cash-settled annuity conventions and piecewise sigma are unsupported. |
| Fixed/stepped/amortizing cash | `prepare_fixed`, fixed-price European/Bermudan issuer calls and holder puts | Notice, contractual exercise, settlement, principal and coupon dates. No make-whole, triggered or continuous American model. Ex-coupon optional ownership is unsupported. |
| Term callable/putable FRN | `prepare_floating`, one common HW1F rate state with an explicit deterministic multiplicative projection basis | One reset per coupon; zero fixing offset; payment at accrual end; linear unrounded coupon; negative coupons allowed; constant notional, gearing, coupon and spread inside each future reset. Principal reductions and payments, and zero-notice and zero-delay exercise, coincide with coupon and reset boundaries. |
| Overnight/path-dependent options | Explicit unsupported result | No deterministic projected coupon, lookback or lockout approximation, cap/floor clipping, or off-cycle accrued-coupon substitute. Plain overnight projection stays in BondContract. |
| OAS/factor risk | Unrounded bounded bracket inversion; discount, projection, joint curve or OAS finite differences | Parameters and eligible fixings are frozen. Price and sensitivity refinement are checked separately, and undersampled option-risk bumps fail. Quote bump and recalibration, and a general credit interpretation, are separate operations. |

Unsupported structures keep their terms and fail. American terms do not become a
daily or coupon-date Bermudan. A missing volatility, projection role, eligible
fixing or notice state cannot become zero, par or a plain PV. Fixed coupon JSON
now rejects unknown cap, floor and payoff fields, while the public
`BondCoupon::Fixed` variant and valid serialization stay compatible.

The calendar and unit handling does not depend on currency. The fixtures are
declared synthetic contracts. They do not validate USD, EUR or GBP market
conventions.

## Construction and inputs

`OptionalBondContext` contains the settlement and model root, payment-date
inclusion and an explicit `AliveUnexercised` state. Accepted historical notices
and exercises are application and lifecycle inputs, and this route does not guess
them. An unexpired exercise whose last notice date precedes the root returns
Unsupported.

`OptionalBondInputs` supplies:

- a currency and collateral discount `BuiltCurve`;
- for FRNs, an index projection curve;
- a validated model;
- the exercise calendar.

Term reset slices reuse the calendars kept at BondContract construction.
Calendar identities, versions and the external source lineage are the caller's
responsibility as part of the immutable input set.

An assumed model records its key, parameters and rationale. A calibrated model
keeps the calibration curve and version, the root, the full quote inputs, the
fixed mean reversion, residuals in each quote's unit and the solver diagnostics.
Initial pricing rejects a different calibration curve, version or root. Scenario
risk changes curve factors explicitly while freezing this base model. It does not
silently recalibrate parameters or invent new curve identities.

The facade and the direct crate APIs expose the same functions. Input terms and
settings round-trip through strict Serde. Resolved resources borrowed by the
functions, and the model outputs, are Rust objects.

The `library()` operation `optional_bond` ([LIBRARY-API](LIBRARY-API.md))
dispatches these functions on every adapter, WASM included. It does not add a
BondSpec variant, a generated JSON schema, a C ABI export, or v1
server/Java/Excel/MCP dispatch of the Rust functions.

## Volatility quotation and calibration

`SwaptionVolatility::Normal { annualized_decimal }` is absolute rate diffusion:
0.007 means 70 normal-vol basis points. `ShiftedLognormal` is relative annual
diffusion (0.20 means 20%) with a separately supplied decimal-rate shift. Both
the shifted forward and the shifted strike must be positive. Unknown conventions,
missing shifts, shifts given with a normal vol, and out-of-domain magnitudes
fail. This boundary never converts percentages or basis points. A model's
short-rate sigma is not a swaption volatility quotation.

The fixed schedule comes from the common schedule primitive, with actual dates
and irregular reference periods. Annuity and forward use the payment DFs and
day-count accruals. The model uses a dated-payment analytic Jamshidian
decomposition. The reference formulation separates mean reversion, volatility and
initial-curve fitting.
[QuantLib HW model source, v1.40](https://raw.githubusercontent.com/lballabio/QuantLib/v1.40/ql/models/shortrate/onefactormodels/hullwhite.cpp).

The existing bounded scaled least-squares kernel fits sigma within the bounds and
start you supply. Quotes are ordered by unique instrument ID so the fit is
deterministic. Each residual is model-implied minus input volatility, in the
*same decimal convention*. Scale and weight affect the objective, and a zero
weight never removes the residual check.

The diagnostics keep:

- market and model premium per unit notional;
- forward, annuity and tolerance;
- attempts and evaluations;
- weighted RMS;
- undamped rank and conditioning;
- bounds.

A failed fit returns diagnostics without a model.

Mean reversion is deliberately an input. Parameter correlation and a full
volatility surface cannot be identified just by calling a fitter. ORE documents
explicit model parametrization and calibration choices, not an inferred universal
model. [ORE user guide](https://opensourcerisk.org/content/uploads/2026/04/userguide.pdf).

## Cash, exercise and floating state

Prices are per **100 original face**, including for amortizing bonds. PV is in
absolute contract currency at settlement. Dirty minus accrued on the original
face equals clean. OAS is a continuous decimal-rate discount shift, and basis
points keep fractional precision. OAS does not change projected index rates or
known coupons.

For fixed cash, decisions are made at the last notice date. The exercise branch
includes:

- scheduled coupon and principal cash through contractual exercise;
- delayed cash that is already owed;
- retirement of the outstanding principal after the boundary;
- the accrued convention in the contract.

Scheduled cash due on a decision date is paid before exercise. Accrual stops at
contractual exercise, and retirement and stub accrued are paid at the separate
adjusted settlement date. Fully accrued delayed coupons are kept at their
original cash date, so they are not counted twice as a stub. `IncludedInPrice`
adds no partial accrued coupon, and `PayAccrued` does.

At notice, backward induction compares continuation with the conditional value of
the whole exercise branch. It does not simply cap the value at the future
redemption price, because advance notice changes the option economics.
[Waterloo notice-period analysis](https://cs.uwaterloo.ca/~paforsyt/numcall.pdf).

Calls minimize and puts maximize the holder's value. `ExercisePriority` orders
simultaneous rights explicitly, including crossed call and put prices. Node
diagnostics report each right's strict value-changing decisions in this order.
They are not exercise probabilities, a record of realized exercise, or a default
model.

FRN history resolution uses publication and known-at eligibility, and checks the
whole history before any forecast. A reset at or before the model root must be
eligible published history. Known amounts stay fixed under all curve scenarios.
Results separate the eligible `fixings` from `future_reset_requirements`. There is
no single forecast coupon that stands for the stochastic option cash.

For a future aligned reset at s, with payment at e, the conditional index bond is

```text
P_index(s,e) = P_discount(s,e) *
    [D_index(0,e)/D_index(0,s)] / [D_discount(0,e)/D_discount(0,s)]
```

This is a common rate state with a deterministic multiplicative basis, not a
stochastic basis or correlation calibration. The rate coupon PV at reset is
`N * gearing * coupon_accrual/index_accrual * (1/P_index(s,e)-1)` plus the
deterministic component, discounted to payment at the declared OAS. Future
coupons are not evaluated on a deterministic forecast at the exercise layer.

The aligned payment and reset restrictions on exercise let already earned coupons
be separated from continuation without an extra remembered rate state. Callable
overnight or off-cycle FRNs would need that additional state.

## Numerical and risk contract

Model time is elapsed ACT/365F from settlement. Curve queries keep the curve's
own ACT/360, ACT/365F or ACT/ACT ISDA basis, including fractional calendar-day
mapping at intermediate grid points. Every mandatory date is an exact grid
anchor. No redemption or coupon is bucketed into another date.

The lattice matches the exact OU conditional moments and computes nonnegative,
conserving trinomial probabilities. It fits Arrow-Debreu state prices to the
root-normalized DFs at every grid point. Negative rates and DFs above one remain
valid. Invalid probabilities, non-finite exponentials, bounds or initial
curve-fit failures return an error.
[OU moment tree reference, QuantLib v1.40](https://raw.githubusercontent.com/lballabio/QuantLib/v1.40/ql/methods/lattices/trinomialtree.cpp).

`GridSettings` requires at least two doublings (three grids). Both final
successive price differences must satisfy absolute plus relative bounds per
original face. The prices, interval and node counts, minimum probability and
curve-fit residuals are all kept. A difference test is a numerical check on the
result. It is not proof against an independent reference and not a universal
error estimate.

The synthetic fixtures use 128/256/512/1024 desired intervals and 0.004 absolute
+ 1e-8 relative price bounds, with independent analytic European and notice
references. Existing artifacts and tolerances are unchanged.

OAS solves only within the closed bracket you supply. Success needs a dirty-price
residual and all price refinements. Endpoint roots report zero bisections, and
later attempts are bounded. There is no bracket expansion, whole-bp rounding,
last iterate accepted as a success, or zero-residual replacement. A price plateau
or no crossing is an error. A crossing does not prove uniqueness for arbitrary
signed cash contracts.

Risk freezes a/sigma and OAS for curve risk. Spread risk freezes both input
curves and changes only OAS. Discount-only, projection-only and joint factor axes
are different operations. Shifts are continuous zeros on ACT/365F model time,
reported separately from the base curve identity. Known fixings never move.

`risk` needs independent `RiskSettings` for the final successive duration and
convexity refinement. A bump below two finest-grid short-rate state cells at a
live non-root decision returns `RiskResolution`. This screen catches undersampled
movement of the option boundary, but it is not a general accuracy guarantee.
Sensitivity convergence and independent references are still required. A failed
sensitivity refinement returns the measured grids without a successful risk
result.

The independent callable-zero fixture checks each declared 50, 100 and 200bp
finite scenario against an analytic bump and reprice, and captures negative
convexity. It rejects an undersampled 1bp request. These finite scenario widths
can legitimately have different durations because they change the exercise
regions, so equal durations across them are not an invariant. Small-bump risk
needs finer numerical methods and settings, and this is a limitation of the
current implementation. No low-latency or general option-risk guarantee is made.

| Resource | Ceiling |
|---|---|
| Time/events/tree | 150 ACT/365F years; 2,048 events/actual intervals; 2,000,000 total nodes per tree; 8,193 nodes per row |
| Contract price | 128 live dated rights; 512 future FRN coupons; 32,000,000 conservative node visits per grid evaluation |
| Calibration | 128 quotes; 512 actual payments per quote; 128 Jamshidian root attempts; 64 implied-vol bisections per evaluation; LM and each SVD 1..10,000 attempts |
| OAS | Closed decimal spread bracket within [-1,1], 1..10,000 bisections; price residual required |

An event-rich or near-coincident grid can fail before these nominal settings are
reached. The bounds limit setup and work. They say nothing about capacity, memory
or latency. Caller calendars and publication callbacks must terminate without
panicking.

## Example and tests

The [example](../crates/convex/examples/optional_bonds.rs) builds a curve,
labels a synthetic calibration quote, calibrates sigma, constructs a callable
contract, solves a fractional-bp OAS and computes fixed-model risk:

```sh
cargo run --locked -p convex --example optional_bonds
cargo run --locked -p convex --example optional_bonds --no-default-features
cargo run --locked -p convex --example optional_bonds --features schemars
cargo test --locked -p convex-bonds --test hw1f
cargo test --locked -p convex-analytics --test optional_bonds -- --nocapture
```

The [analytic bond-option fixtures](../crates/convex-bonds/tests/fixtures/hw1f-options.json)
and the [dated model, notice, FRN and risk fixtures](../crates/convex-analytics/tests/fixtures/optional-bonds.json)
use independent Python `math.erfc` formulas, declared dates and scoped
tolerances. The documented approximation error of the library's CDF is included
in the analytic option bounds. This is an independent calculation, not a human
review of the numerics, and not a calibration to live market data.

See [assurance](internal/ASSURANCE.md) and [coverage](internal/LIBRARY-COVERAGE.md)
for requirement traceability and the support that remains.
