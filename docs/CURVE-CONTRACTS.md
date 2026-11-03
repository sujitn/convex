# Curve recipes and queries

This document covers building curves from recipes and querying them. The code
is in `convex::curve_contracts` (directly `convex_curves::contracts`) and
`convex::curve_inputs` (directly `convex_analytics::curve_inputs`). You pass in
the values, dates, calendars and fixing histories. No engine, handle registry,
runtime, repository, vendor, network connection or system clock is needed.

[Bounded fitting and explicit transformations](CURVE-NUMERICS.md) cover dated
money-market global fitting, exact discount-equivalent views, declared
sticky-zero and sticky-forward rolling, and stable short-period conversions.
The v1 Global builder route still returns Unsupported, so use the named fitting
API. Full product calibration is not covered here.

Run the [standalone consumer](../crates/convex/examples/curve_contracts.rs):

```sh
cargo run --locked -p convex --example curve_contracts
cargo run --locked -p convex --example curve_contracts --no-default-features
cargo run --locked -p convex --example curve_contracts --features schemars
```

It builds a dated deposit/FRA discount curve, adds a continuous zero spread
over that exact version, builds a survival curve, and queries discount,
forward and hazard values and date conversion. It checks that the public API
is reachable and that the analytic results hold. It is not a validation of the
library or its models. [The coverage inventory](internal/LIBRARY-COVERAGE.md)
tracks what remains.

## Construction

`CurveRecipe` names:

- an exact `CurveKey { id, version }`, the currency and the reference date;
- the role: collateral discount, index projection, government, credit, issuer
  basis, repo or volatility;
- the representation, model time basis, interpolation and extrapolation;
- the dependency versions consumed, the source and the mode.

Credit roles keep the entity, seniority and the recovery you supply. A
projection role keeps its index. A government benchmark cannot implicitly
select collateral discounting.

Versions are immutable input versions assigned by the caller. Change the
version whenever the market, calendar, reference or model inputs change. The
library does not generate content hashes or check these assignments.

`build_curve_set(recipes, supplied_values)` returns a complete immutable
`CurveSet` and a deterministic dependency-first build order. The supplied set
contains validated curves, and lookup needs an exact version. Another version
cannot fill a missing dependency. These fail:

- duplicate result identities;
- replacing a supplied immutable curve;
- unused or mismatched dependency sets;
- missing values;
- cycles.

A failed build returns no partial set and does not touch the caller's last good
curves. Independent recipes are ordered by identity. In the supported routes,
node and helper order does not change the financial solution.

Supported construction:

| Source and output | Construction | Boundary |
|---|---|---|
| Discount factors you supply | Direct positive finite nodes, explicit P(0)=1 | Linear, LogLinear or CubicSpline; positive output checked on every query |
| Zero rates you supply | Direct rates with explicit compounding/time basis | Linear or CubicSpline; valid simple/periodic compounding bases required |
| Survival probabilities you supply | Direct positive nonincreasing nodes, explicit Q(0)=1 | Credit role; Linear/LogLinear; None/Flat extension; no CDS calibration |
| Dated deposits/FRAs | Exact simple-accrual discount-factor chain; final repricing of every input quote | Piecewise DF with local Linear/LogLinear; each start is the origin or an exact previously solved pillar; FRA fixing must be strictly after reference date |
| Continuous zero spread | P_spread(t)=P_base(t)*exp(-spread*t) over the exact base | Same currency, reference/time basis, inherited interpolation/extension; explicit annualized spread unit |

For the chain, P(end)=P(start)/(1+r*tau), using the helper's own start, end and
accrual basis. It is an analytic construction, reports zero solver iterations,
and checks every implied quote after the final build. It does not substitute a
deposit quote for a continuously compounded zero.

Two cases are not handled:

- A start inside an unsolved or interpolated interval is Unsupported. A more
  general bootstrap needs a product calibration implementation.
- Same-day or seasoned FRA fixing eligibility needs the full fixing-aware
  product route. The builder cannot infer publication state from a date alone.

Direct nodes report their source and method and the sorted pillars. They have
no instrument residuals. `QuoteRepricing` reports:

- the helper identity and pillar;
- the input quote and the model quote in the same unit;
- the signed model-minus-input residual;
- the absolute tolerance you supplied.

A residual above the tolerance produces `RepricingFailure`. Tolerances are not
changed.

Rate recipes accept only None, raw-value Flat and raw-value Linear
extrapolation. Flat discount factor and flat zero rate mean different things,
and the response states which extension applies. Survival allows None or
raw-value Flat, and unrestricted linear probability tails are rejected. Cubic
rate interpolation can overshoot, and an invalid discount or compounding result
fails at query time. There is no global arbitrage or convergence check.

### Other curve families and methods

[Parametric, segmented and delegated rate curves](CURVE-FAMILIES.md) are built
from their complete sources with dated zero fitting and domain, join and
dependency diagnostics. A method tag on a node grid cannot construct them.

The `Nodes` route rejects PiecewiseConstant, FlatForward, MonotoneConvex and
UFR method selections. Instead:

- [Step-zero and flat-forward sources](CURVE-METHODS.md) have their own checked
  continuous-zero rate contracts, segment forwards and declared exact
  extension. The PiecewiseConstant tag now selects right-owned steps.
- The older raw FlatForward tail substitution is removed. Checked queries
  reject it, and the infallible compatibility queries return NaN outside that
  tail.
- Checked UFR setup and evaluation keep their own constraints. The
  [UFR source](CURVE-UFR.md) joins a FlatForward or parametric model at a
  supplied LLP with its analytic left forward.
- The [nonnegative MonotoneConvex source](CURVE-MONOTONE-CONVEX.md) provides
  matching analytic forwards and integrals and explicit terminal sides.
- UFR joins over other source families are not covered.

Existing numerical controls and ordinary query paths are unchanged.

## Helper descriptions

The analytics layer owns `CurveHelperInput` and `HelperCurveRecipe`. Keeping
product terms above `convex-curves` avoids a dependency from the curve solvers
back into products that use curves. The fixed set of enums and functions adds
no plugin registry or pricing hierarchy.

`HelperCurveRecipe` keeps each quote, tolerance, mandatory positive weight,
residual scale and maturity, last-relevant or custom pillar. Global settings
keep bounded iterations, the scaled objective tolerance, initial rates, rate
bounds, scales and weights. Global construction is Unsupported here, and these
settings cannot trigger an approximate mode. Weights and scales describe a
future global objective. The supported analytic chain reprices every input
independently of them.

| Helper | Terms kept and validated | Dispatch |
|---|---|---|
| Deposit/FRA | Accrual start/end, fixing/index, settlement date/convention, accrual basis and rate units | Exact chain through the same core contracts |
| Term/arithmetic/compounded rate future | Contract/version, window, last trade/final cash dates, observation and settlement calendar versions, tick/point multiplier/tick cash, index, lookback/shift/lockout, convexity or model/version, publication-aware fixing requirements | Description and shared observation/history validation; calibration and pricing not yet implemented |
| Fixed/stepped/amortizing bond | Security/version, issue/accrual/settlement dates, anchored stubs/reference periods, date and ex rules, coupon/notional steps, quote face, redemption fraction, principal boundary, cash rounding, negative coupon policy, ex accrued/payment-date entitlement and clean/dirty/yield conventions | Shared schedule/profile/coupon validation; quote-to-price and helper calibration not yet implemented |
| Single-name CDS | Entity/document versions, seniority, trade/step-in/upfront cash dates, premium schedule/accrual, protection dates/inclusion, notional steps, running premium, recovery, accrual-on-default, protection cash timing, clean/dirty/par quote and sign, discount version | Description, shared schedule and domain validation; standard model and calibration not yet implemented |
| IRS/OIS and basis | Leg schedules/calendars, dated notional/coupon/spread/gearing, term-index tenor or overnight observations, publication/cutoff requirements, projection and discount versions, quoted basis leg | Shared leg/profile/fixing validation; product-backed calibration not yet implemented |

`describe_helper` needs real `HelperResources`: exact calendar-version bindings
and a validated immutable `FixingSeries`. A calendar mismatch cannot fall back
to weekends. Expected observation IDs and dates must match those generated by
the shared primitives. Missing published history returns the complete typed
missing-fixing list. Description validation has no forecast callback. Future
observations remain projection requirements, and describing them does not
validate a future coupon. See [shared primitives](CASHFLOW-PRIMITIVES.md).

A schedule declares one calendar for its date rules. Different legs can use
different calendars, but separate accrual, payment and fixing calendars within
one leg are not supported yet. Floating and optional bonds and
volatility/model calibration inputs are also not supported yet. Unrecognized
extra economics fail deserialization. These descriptions do not approximate
optional, inflation-linked or cross-currency products as vanilla.

Full helper terms roundtrip exactly through serde. That includes Decimal step
and tick amounts, publication nanoseconds, units and binary f64 quotes. The
workspace uses serde_json's `float_roundtrip` feature to avoid a one-ULP parse
change that we observed. The [serde_json feature
documentation](https://docs.rs/crate/serde_json/1.0.150/features) describes it,
and the fixture tests demonstrate its effect.

The helper DTOs encode every Decimal profile value, quoted face, redemption
fraction and tick or multiplier amount as an exact decimal string. These fail:

- numeric JSON amounts;
- strings that need rounding;
- values outside Decimal's range;
- decimal strings over 64 bytes.

A small wire adapter reuses the core DatedValue type and keeps its scale
without changing its older serialization.
[Decimal::from_str_exact](https://docs.rs/rust_decimal/1.42.0/rust_decimal/struct.Decimal.html#method.from_str_exact)
does the checked parsing, and a 28-place regression test shows the scale is
preserved. DTO parsing is not an ingress limit. Clients that embed the library
must apply transport byte and depth limits before deserializing large objects.

## Query contracts

`BuiltCurve::query` takes `CurvePoint::Date` or `CurvePoint::Tenor`. It returns
a finite typed value, the curve version, the time basis, the requested rate
conventions and an extrapolation flag. Built state is private and cannot be
deserialized.

| Query | Meaning and conventions |
|---|---|
| Raw | Interpolated representation value, in its own unit |
| Discount factor | P(t); P(0)=1 is the mathematical definition, not a missing input |
| Discount ratio | P(end)/P(start), strictly ordered boundaries |
| Zero | Equivalent decimal annualized rate from P(t), explicit compounding and rate day count |
| Forward | Equivalent decimal rate from P(end)/P(start) over the period, explicit compounding/day count |
| Survival/default | Credit survival; unconditional Q(start)-Q(end) or explicitly conditional 1-Q(end)/Q(start) |
| Hazard | -Q'(t)/Q(t) per curve-time year; caller chooses Left/Right at a pillar |

Time axes support ACT/360, ACT/365 Fixed and ACT/ACT ISDA. Date rate periods
also support the context-free 30/360 US and 30E/360. ICMA and conventions that
depend on the termination date or period need product or reference context, so
they are Unsupported for these context-free queries.

Zero and forward output support Simple, Continuous and every existing periodic
compounding including Daily. For periodic n, rate = n*(DF_period^(-1/(n*tau))-1),
with the actual tau. A fixed one-year exponent is not substituted.

With Date points, rates use the year fraction between the requested start and
end. With tenor or mixed points, the output day count must equal the curve time
basis, and changing basis needs actual dates. A zero-length rate is undefined
and fails instead of returning zero. A survival curve cannot be queried as a
discount curve. A flat survival extension has zero intensity because Q is
constant, which is different from missing historical observations.

`date_to_tenor` computes actual day counts. `tenor_to_date` needs AtOrBefore,
AtOrAfter or Nearest (ties go earlier). Its bounded inverse handles leap years
and ACT/ACT ISDA. Model-time nodes need not fall on whole dates, so inverse
rounding is explicit, and dates beyond the canonical calendar fail.

Rolling and rebasing are not exposed here. The sticky-zero and sticky-forward
choices are in [CURVE-NUMERICS.md](CURVE-NUMERICS.md).

## Limits and tests

Set and query limits:

- At most 256 curves in a set (including supplied values), with at most 1,024
  nodes or 1,023 helpers plus the unit origin.
- Query time is finite within [0,150] years. Canonical dates are years 1 to
  9999, and date periods span at most 150*366 days.
- Inverse search uses at most 16 bisections.
- Sorted pillars must be separated by at least 1e-10 model years.
- Rate input is finite within +/-10 decimal. Other quote magnitudes are at most
  1e12, with stricter unit domains.
- Discount factors above one and supported negative rates are valid.
- Compounding bases and every result must be positive and finite where
  required, and overflow or underflow fails.
- Finite-hazard survival nodes are in (0,1]. Zero-survival or defaulted
  contracts need their own credit model or lifecycle route.

Schedules, profiles and fixing histories keep their existing 4,096 and 65,536
bounds. Custom calendar callbacks must terminate without panicking. There is
no latency claim for setup, allocation, dependency lookup or serde. Dependency
depth is bounded by the acyclic 256-curve set, and evaluating a derived value
follows those immutable dependencies.

The [curve tests](../crates/convex-curves/tests/curve_contracts.rs) cover:

- exact dates and units, and analytic exp/log/pow identities;
- 256 deterministic generated cases and negative rates;
- duplicate, invalid and expired inputs;
- missing, cyclic and incompatible versions;
- tolerance failures, domains and convention conversion.

The [helper tests](../crates/convex-analytics/tests/curve_inputs.rs) use a
pinned synthetic pack to check term conservation, schedules, observation
eligibility and explicit Unsupported dispatch. Analytic f64 checks use absolute
1e-11 plus relative 1e-11. Exact contractual data uses exact checks. Existing
quantitative pins and tolerances are unchanged. The 105 retained comparison
failures remain enforced.

This API split is our own design. It was informed by the explicit dates,
implied quotes and pillars in
[QuantLib's rate helpers](https://github.com/lballabio/QuantLib/blob/master/ql/termstructures/yield/ratehelpers.hpp),
and by the currency, discount dependency, representation, interpolation and
extrapolation settings in
[ORE's yield curve configuration](https://github.com/OpenSourceRisk/Engine/blob/master/OREData/ored/configuration/yieldcurveconfig.hpp).
It does not copy their class hierarchies or claim numerical equivalence. The
source pages were read on 5 October 2026, and branch pages can change.

## Snapshots and what is not covered

The engine stores and rebuilds the recipes through
[schema-2 snapshot replay](CURVE-SNAPSHOT-REPLAY.md), including source, model,
date, unit, role, time, method, tail and exact dependency versions. Full
product-calibration snapshots and quote-risk graphs are in
[CALIBRATION-SNAPSHOT-REPLAY.md](CALIBRATION-SNAPSHOT-REPLAY.md).

Not covered here: OIS and projection bootstrapping and global calibration,
credit calibration, cash/basis/repo curves, optional models, schemas, C ABI and
client parity, and independent validation of the library. No production, model,
external data or latency gate is complete.
