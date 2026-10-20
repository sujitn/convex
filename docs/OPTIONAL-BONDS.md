# Supplied optional-bond models

CVX-059 adds pure `convex::optional_bond` construction/calibration, optional price,
OAS inversion and fixed-model factor risk. It consumes the checked
[BondContract](BOND-CONTRACTS.md) and actual immutable
[curve inputs](CURVE-CONTRACTS.md). No provider, database, broker, engine,
server, system clock or new dependency enters these calculations.

This is an engineering candidate for the combinations below. Full market packs,
independent quantitative approval, host schemas/dispatch, lifecycle and LB0/LB1
certificates remain pending. Existing compatibility OAS/tree APIs retain their
behavior and quantitative artifacts; they do not acquire this contract.

## Supported combinations

| Input/model | Supported native behavior | Explicit limits |
|---|---|---|
| Constant HW1F parameters | Named/versioned `OptionalBondModel::assumed`, with nonempty rationale | Mean reversion 0..10 per year; sigma 0..1 decimal rate per square-root year. Zero is an explicit deterministic assumption. Domain admission does not guarantee all extreme inputs have finite numerical solutions. |
| Swaption sigma calibration | `calibrate_sigma(SigmaCalibrationInputs)`, fixed exogenous mean reversion, physical single-curve European swaptions | Actual expiry-start fixed-leg schedule, stubs/day count and supplied calendar; payment at actual accrual end; nonnegative fixed strike. Multicurve swaption calibration, negative-strike decomposition, cash-settled annuity conventions and piecewise sigma are unsupported. |
| Fixed/stepped/amortizing cash | `prepare_fixed`, fixed-price European/Bermudan issuer calls and holder puts | Actual notice, contractual exercise, settlement, principal and coupon dates; no make-whole, triggered or continuous American model. Ex-coupon optional ownership is unsupported. |
| Term callable/putable FRN | `prepare_floating`, one common HW1F rate state with explicit deterministic multiplicative projection basis | One reset per coupon; zero fixing offset; payment at accrual end; linear unrounded coupon; negative coupons allowed; constant notional/gearing/coupon/spread inside each future reset. Principal reductions/payments and zero-notice/zero-delay exercise coincide with coupon/reset boundaries. |
| Overnight/path-dependent options | Explicit unsupported result | No deterministic projected coupon, lookback/lockout approximation, cap/floor clipping or off-cycle accrued-coupon substitute. Plain supported overnight projection remains in BondContract. |
| OAS/factor risk | Unrounded bounded bracket inversion; discount, projection, joint curve or OAS finite differences | Parameters and eligible fixings frozen. Price and sensitivity refinement checked separately; undersampled option-risk bumps fail. Quote bump/recalibration and a general credit interpretation are separate operations. |

Unsupported structures keep their original terms and fail. American terms do
not become a daily or coupon-date Bermudan. Missing volatility, projection
role, eligible fixing or notice state cannot become zero, par or plain PV.
Fixed coupon JSON now rejects unknown cap/floor/payoff fields even though the
public `BondCoupon::Fixed` variant and valid serialization remain compatible.

The native supplied-calendar/units behavior is currency neutral. The fixtures
are declared synthetic contracts, not USD/EUR/GBP market-convention admission.

## Construction and inputs

`OptionalBondContext` contains settlement/model root, payment-date inclusion
and an explicit `AliveUnexercised` state. Historical accepted notices/exercises
are application/lifecycle inputs; this route does not guess them. An unexpired
exercise whose last notice date precedes the root returns Unsupported.

`OptionalBondInputs` supplies an exact currency/collateral discount `BuiltCurve`,
an explicit index projection curve for FRNs, a checked model and the actual
exercise calendar. Term reset slices reuse the calendars retained at checked
BondContract construction. Calendar identities/versions and external source
lineage remain the caller's immutable input-set responsibility.

An assumed model records its supplied key, parameters and rationale. A calibrated
model retains the exact calibration curve/version, root, original full quote
inputs, fixed mean reversion, native residuals and actual solver diagnostics.
Initial pricing rejects a different calibration curve/version or root. Scenario
risk explicitly changes curve factors while freezing this base model; it does
not silently recalibrate parameters or invent new curve identities.

The facade and direct crate APIs expose the same functions. Input terms/settings
have strict native Serde roundtrip. Borrowed resolved resources and model outputs
are native objects. This does not add a BondSpec variant, generated JSON schema,
C ABI export or v1 server/Java/Excel/MCP/WASM dispatch; those remain CVX-063/033.

## Volatility quotation and calibration

`SwaptionVolatility::Normal { annualized_decimal }` is absolute rate diffusion:
0.007 means 70 normal-vol basis points. `ShiftedLognormal` is relative annual
diffusion (0.20 means 20%) with a separately supplied decimal-rate shift. Both
shifted forward and shifted strike must be positive. Unknown conventions,
missing shifts, irrelevant normal-vol shifts and out-of-domain quotation
magnitudes fail. This boundary never converts percentages or basis points.
A model's short-rate sigma is not a swaption volatility quotation.

The fixed schedule is generated by the common schedule primitive, retaining
actual dates and irregular reference periods. Annuity and forward use actual
payment DFs and day-count accruals. The model uses a dated-payment analytic
Jamshidian decomposition. The reference formulation separates mean reversion,
volatility and initial-curve fitting. [QuantLib HW model source, v1.40](https://raw.githubusercontent.com/lballabio/QuantLib/v1.40/ql/models/shortrate/onefactormodels/hullwhite.cpp).

The existing bounded scaled least-squares kernel fits sigma within caller-supplied
bounds/start. Quotes are ordered by unique instrument ID for deterministic fitting.
Every residual is model-implied minus input volatility in the *same native decimal
convention*. Scale and weight affect the objective; zero weight never removes a
native residual postcondition. Diagnostics retain market/model premium per unit
notional, forward, annuity, tolerance, attempts/evaluations, weighted RMS, undamped
rank/conditioning and bounds. A failed fit returns diagnostics without a model.

Constant mean reversion is deliberately supplied. Parameter correlation and a
full volatility surface cannot be identified merely by calling a fitter.
ORE documents explicit model parametrization/calibration choices rather than
an inferred universal model. [ORE user guide](https://opensourcerisk.org/content/uploads/2026/04/userguide.pdf).

## Cash, exercise and floating state

Prices are per **100 original face**, including amortizing bonds. PV is absolute
contract currency at settlement. Dirty minus original-face accrued equals clean.
OAS is a continuous decimal-rate discount shift; basis points retain fractional
precision. OAS does not change projected index rates or known coupons.

For fixed cash, decisions occur at the last notice date. The exercise branch
includes scheduled coupon/principal cash through contractual exercise, already
owed delayed cash, retirement of AFTER-boundary outstanding principal and the
contracted accrued convention. Scheduled cash due on a decision date is paid
before exercise. Accrual stops at contractual exercise; retirement/stub accrued
is paid at the separate adjusted settlement date. Fully accrued delayed coupons
are retained at their original cash date, rather than double counted as a stub.
`IncludedInPrice` adds no partial accrued coupon; `PayAccrued` does.

At notice, backward induction compares continuation with the conditional value
of the complete exercise branch. It does not simply cap the value at the future
redemption price. Advance notice changes the option economics. [Waterloo notice-period analysis](https://cs.uwaterloo.ca/~paforsyt/numcall.pdf).

Calls minimize and puts maximize holder value. `ExercisePriority` explicitly
orders simultaneous rights, including crossed call/put prices. Node diagnostics
report each right's strict value-changing decisions in this order. They are not
exercise probabilities, a realized exercise record or a default model.

FRN history resolution uses publication/known-at eligibility and entire-history
preflight before forecasts. A reset at/before the model root must be eligible
published history. Known amounts stay fixed under all curve scenarios. Results
separate eligible `fixings` from `future_reset_requirements`; there is no single
forecast coupon claimed to represent the stochastic option cash.

For a future aligned reset at s, payment at e, the conditional index bond is

```text
P_index(s,e) = P_discount(s,e) *
    [D_index(0,e)/D_index(0,s)] / [D_discount(0,e)/D_discount(0,s)]
```

This is an explicit common rate state with deterministic multiplicative basis,
not a stochastic basis/correlation calibration. Rate coupon PV at reset uses
`N * gearing * coupon_accrual/index_accrual * (1/P_index(s,e)-1)` plus the actual
deterministic component, discounted to payment at the declared OAS. Future
coupons are not evaluated on a deterministic forecast at the exercise layer.
The aligned payment/reset exercise restrictions allow already earned coupons
to be separated from continuation without inventing an extra remembered rate
state. Callable overnight or off-cycle FRNs require that additional state.

## Numerical and risk contract

Model time is elapsed ACT/365F from settlement. Curve queries retain the curve's
own ACT/360, ACT/365F or ACT/ACT ISDA basis, including fractional calendar-day
mapping at intermediate grid points. Every mandatory date is an exact grid
anchor; no redemption or coupon is bucketed into another date.

The checked lattice matches exact OU conditional moments, computes nonnegative
conserving trinomial probabilities and fits Arrow–Debreu state prices to actual
supplied root-normalized DFs at every grid point. Negative rates/DFs above one
remain valid. Invalid probabilities, nonfinite exponentials, bounds or initial
curve-fit failures return an error. [OU moment tree reference, QuantLib v1.40](https://raw.githubusercontent.com/lballabio/QuantLib/v1.40/ql/methods/lattices/trinomialtree.cpp).

`GridSettings` requires at least two doublings (three grids). Both final successive
price differences must satisfy absolute plus relative per-original-face bounds.
All actual prices, interval/node counts, minimum probability and curve-fit residuals
are retained. A difference test is a numerical postcondition, not proof against
an independent oracle or a universal error estimate. New synthetic fixtures use
128/256/512/1024 desired intervals and 0.004 absolute + 1e-8 relative price bounds,
with independent analytic European and notice references. Existing artifacts and
tolerances are unchanged.

OAS solves only within an explicit supplied closed bracket. Success requires a
native dirty-price residual and all price refinements. Endpoint roots report zero
bisections; subsequent attempts are bounded. No bracket expansion, whole-bp
rounding, successful last iterate or zero-residual replacement is used. A price
plateau/no crossing is an error; a crossing does not prove global uniqueness for
arbitrary signed cash contracts.

Risk freezes a/sigma and OAS for curve risk; spread risk freezes both input curves
and changes only OAS. Discount-only, projection-only and joint factor axes are
different operations. Shifts are continuous zeros on ACT/365F model time,
reported separately from base curve identity. Known fixings never move.

`risk` requires independent `RiskSettings` for final successive duration and
convexity refinement. A bump below two finest-grid short-rate state cells at a
live non-root decision returns `RiskResolution`. This conservative screen catches
undersampled option-boundary movement; it is not a general accuracy guarantee.
Sensitivity convergence and independent references are still required. Failed
sensitivity refinement returns measured grids without a successful risk result.

The independent callable-zero fixture checks each declared 50/100/200bp finite
scenario against analytic bump/reprice and captures negative convexity. It rejects
an undersampled 1bp request. These finite scenario widths can legitimately have
different durations because they change exercise regions; equality across them
is not a mathematical invariant. Small-bump risk needs finer certified numerical
methods/settings and remains an explicit limitation of this engineering candidate.
No low-latency or universal option-risk certificate is claimed.

| Resource | Checked ceiling |
|---|---|
| Time/events/tree | 150 ACT/365F years; 2,048 events/actual intervals; 2,000,000 total nodes per tree; 8,193 nodes per row |
| Contract price | 128 live dated rights; 512 future FRN coupons; 32,000,000 conservative node visits per grid evaluation |
| Calibration | 128 quotes; 512 actual payments per quote; 128 Jamshidian root attempts; 64 implied-vol bisections per evaluation; LM and each SVD 1..10,000 attempts |
| OAS | Closed decimal spread bracket within [-1,1], 1..10,000 bisections; native price residual required |

An event-rich/near-coincident grid can fail before these nominal settings are
reached. Bounds are setup/work ceilings, not capacity, memory or latency promises.
Caller calendars/publication callbacks must terminate without panicking.

## Standalone workflow and evidence

The [standalone example](../crates/convex/examples/optional_bonds.rs) builds a real
supplied curve, labels a synthetic calibration quote, calibrates sigma, constructs
a callable contract, solves fractional-bp OAS and computes fixed-model risk:

```sh
cargo run --locked -p convex --example optional_bonds
cargo run --locked -p convex --example optional_bonds --no-default-features
cargo run --locked -p convex --example optional_bonds --features schemars
cargo test --locked -p convex-bonds --test hw1f
cargo test --locked -p convex-analytics --test optional_bonds -- --nocapture
```

New [analytic bond-option fixtures](../crates/convex-bonds/tests/fixtures/hw1f-options.json)
and [dated model/notice/FRN/risk fixtures](../crates/convex-analytics/tests/fixtures/optional-bonds.json)
use independent Python `math.erfc` formulas, actual declared dates and scoped
tolerances. The existing library CDF's documented approximation error is included
in the analytic option bounds. This is independent engineering calculation,
not independent human quantitative approval or live market calibration.

See [assurance](ASSURANCE.md), [coverage](LIBRARY-COVERAGE.md) and the private
[CVX-059 evidence](../.ai/production-readiness/evidence/CVX-059/optional-bonds.md)
for actual commands/results, requirement traceability and remaining support.
