# Checked pure curve construction and queries

CVX-056 exposes `convex::curve_contracts` (directly
`convex_curves::contracts`) and `convex::curve_inputs` (directly
`convex_analytics::curve_inputs`). Actual values, dates, calendars and fixing
histories are supplied by the caller. No engine, handle registry, runtime,
repository, vendor, network connection or system clock is required.

CVX-060 adds [bounded fitting and explicit transformations](CURVE-NUMERICS.md):
dated money-market global fitting, exact discount-equivalent views, declared
sticky-zero/sticky-forward rolling and stable short-period conversions. The
original v1 Global builder route retains its Unsupported contract; use the named
fitting API. Full product calibration and library certificates remain pending.

Run the [standalone consumer](../crates/convex/examples/curve_contracts.rs):

```sh
cargo run --locked -p convex --example curve_contracts
cargo run --locked -p convex --example curve_contracts --no-default-features
cargo run --locked -p convex --example curve_contracts --features schemars
```

It constructs an exact dated deposit/FRA discount curve, adds a supplied
continuous zero spread over that exact version, constructs a supplied survival
curve and queries discount/forward/hazard/date conversion. It proves public
reachability and analytic postconditions. It is not a library/model certificate.
[The inventory](LIBRARY-COVERAGE.md) still retains all mandatory pending records.

## Construction and separation of concerns

`CurveRecipe` names an exact `CurveKey { id, version }`, currency, reference
date, role (collateral discount, index projection, government, credit,
issuer basis, repo or volatility), representation, model time basis,
interpolation, extrapolation, consumed dependency versions, source and mode.
Credit roles preserve entity, seniority and supplied recovery. A projection
role preserves its index. A government benchmark cannot implicitly select
collateral discounting. Versions are caller-assigned immutable input versions;
the caller changes them when market/calendar/reference/model inputs change.
This library does not mint content hashes or certify those assignments.

`build_curve_set(recipes, supplied_values)` returns a complete immutable
`CurveSet` and deterministic dependency-first build order. The supplied set
contains actual validated curves. Lookup requires an exact version; another
version cannot fill a missing dependency. Duplicate result identities,
replacement of an immutable supplied curve, unused/mismatched dependency
sets, missing values and cycles fail. A failed build returns no partial set
and cannot mutate the caller's last good curves. Independent recipes use
identity ordering. Node/helper order does not select a different financial
solution in the supported routes.

The checked native dispatch supports:

| Source and output | Actual supported construction | Explicit boundary |
|---|---|---|
| Supplied discount factors | Direct positive finite nodes, explicit P(0)=1 | Linear, LogLinear or CubicSpline; positive output checked on every query |
| Supplied zero rates | Direct rates with explicit compounding/time basis | Linear or CubicSpline; valid simple/periodic compounding bases required |
| Supplied survival probabilities | Direct positive nonincreasing nodes, explicit Q(0)=1 | Credit role; Linear/LogLinear; None/Flat extension; no CDS calibration claim |
| Dated deposits/FRAs | Exact simple-accrual discount-factor chain; final native quote repricing | Piecewise DF with local Linear/LogLinear; each start is the origin or an exact previously solved pillar; FRA fixing must be strictly after reference date |
| Continuous zero spread | P_spread(t)=P_base(t)*exp(-spread*t) over the exact supplied base | Same currency, reference/time basis, inherited interpolation/extension; explicit annualized spread unit |

For the chain, P(end)=P(start)/(1+r*tau) using the helper's actual start/end
and accrual basis. It is an analytic construction, reports zero solver
iterations, and checks every original implied quote after the final build.
It does not substitute a deposit quote for a continuously compounded zero.
Starts inside an unsolved/interpolated interval are explicitly Unsupported;
that more general bootstrap needs a product/calibration implementation.
Same-day or seasoned FRA fixing eligibility needs the full fixing-aware
product route. This builder cannot infer publication state from a date alone.

Direct nodes report their actual source/method and normalized sorted pillars;
they have no fictitious instrument residuals. `QuoteRepricing` reports helper
identity, actual pillar, original native input, model quote in that same unit,
signed model-minus-input residual and the supplied absolute native tolerance.
A residual above that tolerance produces `RepricingFailure`. Tolerances are
not changed, and a narrow calculation domain does not imply a market pack.

Only None, raw-value Flat and raw-value Linear extrapolation are accepted for
rate recipes. Flat discount factor and flat zero rate have different financial
meaning. The response explicitly discloses extension. Survival permits None
or raw-value Flat; unrestricted linear probability tails are rejected.
Cubic rate interpolation can overshoot; an invalid discount/compounding result
fails at query. No global arbitrage/convergence certificate is implied.

Complete original [parametric, segmented and delegated rate contracts](CURVE-FAMILIES.md)
now use their actual supplied sources and native mathematics, with dated native
zero fitting and exact domain/join/dependency diagnostics. A bare method tag on
a node grid cannot construct those families.

The original `Nodes` route rejects PiecewiseConstant, FlatForward, MonotoneConvex
and UFR method selections. Complete [explicit step-zero and flat-forward sources](CURVE-METHODS.md)
now admit their own checked continuous-zero rate contracts, original segment
forwards and declared exact extension. The native PiecewiseConstant tag now
selects actual right-owned steps. The older raw FlatForward tail substitution
is removed: checked queries reject it and infallible compatibility queries
return NaN outside that unsupported tail. Checked UFR setup/evaluation retains
its separate native constraints. The complete [explicit UFR source](CURVE-UFR.md)
joins an exact original FlatForward or parametric model at a supplied LLP with
its analytic left forward. The complete [nonnegative MonotoneConvex source](CURVE-MONOTONE-CONVEX.md) now owns matching analytic forwards/integrals and explicit terminal sides. UFR joins over other native source families remain open.
Original numerical controls and ordinary supported query paths are preserved.

## Full economic helper descriptions

The pure analytics layer owns `CurveHelperInput` and `HelperCurveRecipe`.
Keeping product terms above `convex-curves` avoids a dependency from curve
solvers back into products that already consume curves. The current fixed
set of enums/functions introduces no plugin registry or pricing hierarchy.

`HelperCurveRecipe` preserves each native quote, tolerance, mandatory positive
weight, residual scale and explicit maturity/last-relevant/custom pillar.
Global settings preserve bounded iterations, scaled objective tolerance,
initial rates, rate bounds, native scales and weights. Global construction
is Unsupported here; these settings cannot trigger an approximate mode.
Weights/scales describe a future global objective; the supported analytic
chain reprices every input independently of them.

| Helper | Terms retained and validated | Actual dispatch |
|---|---|---|
| Deposit/FRA | Exact accrual start/end, fixing/index, settlement date/convention, accrual basis and native rate units | Exact checked chain through the same core contracts |
| Term/arithmetic/compounded rate future | Contract/version, actual window, last trade/final cash dates, distinct observation/settlement calendar versions, tick/point multiplier/tick cash, index, lookback/shift/lockout, supplied convexity or model/version, publication-aware fixing context/requirements | Description and shared observation/history validation; calibration/pricing pending CVX-022/023/057 |
| Fixed/stepped/amortizing bond | Security/version, issue/accrual/settlement dates, anchored stubs/reference periods, distinct date/ex rules, independent coupon/notional steps, quote face, redemption fraction, principal boundary, cash rounding, negative coupon policy, ex accrued/payment-date entitlement and clean/dirty/yield conventions | Actual shared schedule/profile/coupon validation; quote-to-price/helper calibration pending CVX-017/055/058 |
| Single-name CDS | Entity/document versions, seniority, trade/step-in/upfront cash dates, premium schedule/accrual, separate protection dates/inclusion, notional steps, running premium, recovery, accrual-on-default, protection cash timing, clean/dirty/par quote and sign, exact discount version | Description/shared schedule/domain validation; actual standard model/calibration pending CVX-020/021 |
| IRS/OIS and basis | Independent leg schedules/calendars, dated notional/coupon/spread/gearing, explicit term-index tenor or overnight observations, publication/cutoff requirements, separate projection/discount versions and quoted basis leg | Shared leg/profile/fixing validation; full product-backed calibration pending CVX-018/019/057 |

`describe_helper` requires actual `HelperResources`: exact calendar-version
bindings and a validated immutable `FixingSeries`. Calendar mismatch cannot
fall back to weekends. Expected observation IDs/dates must exactly match
those generated by the shared primitives. Missing required published history
returns the original complete typed missing-fixing list. There is no forecast
callback in description validation. Future observations remain explicit
projection requirements; describing them cannot certify a future coupon.
See [shared primitives](CASHFLOW-PRIMITIVES.md) for their semantics.

A schedule declares one calendar for its date rules. Different legs may use
different calendars; independent accrual/payment/fixing calendars within a
single leg are a pending contract extension. Floating/optional bonds and
volatility/model calibration inputs also require their owning product tasks.
Unrecognized extra economics fail deserialization. These descriptions do not
approximate optional, inflation-linked or cross-currency products as vanilla.

Full helper terms serde roundtrip exactly, including Decimal step/tick amounts,
publication nanoseconds, units and original binary f64 quotes. The workspace
uses serde_json's `float_roundtrip` feature to avoid a demonstrated one-ULP
parse change. No package/version/lockfile change is required.
[The pinned serde_json feature documentation](https://docs.rs/crate/serde_json/1.0.150/features)
identifies that supported feature; fixture tests demonstrate its actual effect.
The new helper DTOs encode every Decimal profile value, quoted face, redemption
fraction and tick/multiplier amount as an exact decimal string. Numeric JSON
amounts and strings requiring rounding or outside Decimal's representable
domain fail; decimal strings are at most 64 bytes. A small wire adapter reuses
the same core DatedValue type and retains scale without changing its older
serialization. [Decimal::from_str_exact](https://docs.rs/rust_decimal/1.42.0/rust_decimal/struct.Decimal.html#method.from_str_exact)
supplies checked parsing; a 28-place regression demonstrates preservation.
Raw DTO parsing is not an external ingress limit: embedding clients still
apply transport byte/depth limits before deserializing large objects.

## Query contracts

`BuiltCurve::query` accepts `CurvePoint::Date` or `CurvePoint::Tenor`, returns a
finite typed value, exact curve version, time basis, requested rate conventions
and an extrapolation flag. Built state is private and cannot be deserialized.

| Query | Meaning and conventions |
|---|---|
| Raw | Actual interpolated representation value, with its actual unit |
| Discount factor | P(t); P(0)=1 is the mathematical definition, not a missing input |
| Discount ratio | P(end)/P(start), strict ordered boundaries |
| Zero | Equivalent decimal annualized rate from P(t), explicit compounding and rate day count |
| Forward | Equivalent decimal rate from P(end)/P(start) over the actual period, explicit compounding/day count |
| Survival/default | Credit survival; unconditional Q(start)-Q(end) or explicitly conditional 1-Q(end)/Q(start) |
| Hazard | -Q'(t)/Q(t) per curve-time year; caller chooses Left/Right at a pillar |

Time axes support ACT/360, ACT/365 Fixed and actual ACT/ACT ISDA. Date rate
periods additionally support context-free 30/360 US and 30E/360. ICMA and
termination/period-sensitive conventions need product/reference context and
are Unsupported for these context-free queries. Zero and forward output
support Simple, Continuous and every existing periodic compounding including
Daily. For periodic n, rate = n*(DF_period^(-1/(n*tau))-1), with the actual
tau; a fixed one-year exponent is not substituted.

With Date points, rates use the requested actual start/end year fraction.
With tenor or mixed points, output day count must equal the curve time basis;
changing basis requires actual dates. A zero-length rate is undefined and
fails; it does not return zero. Survival cannot be queried as a discount curve.
A flat survival extension has zero intensity by its declared constant-Q
mathematics, distinguished from missing historical observations.

`date_to_tenor` calculates actual day counts. `tenor_to_date` requires
AtOrBefore, AtOrAfter or Nearest (ties earlier). Its bounded inverse includes
leap years and ACT/ACT ISDA. Model-time nodes need not be representable whole
dates; inverse rounding is explicit and dates beyond the canonical calendar
fail. Rolling/rebasing is not exposed here: sticky-zero/forward choices and
numerical transformation certificates remain CVX-060.

## Limits, numerical evidence and remaining work

Maximum set size is 256 curves (including supplied values), with at most
1,024 nodes or 1,023 helpers plus the unit origin. Query time is finite within
[0,150] years; canonical dates are years 1–9999 and date periods span at most
150*366 days. Inverse search uses at most 16 bisections. Sorted pillars require
separation >=1e-10 model years. Rate ingress is finite within +/-10 decimal;
other quote magnitudes are <=1e12 with stricter unit domains. Discount factors
above one and supported negative rates are valid. Compounding bases and every
result must remain positive/finite where required; overflow/underflow fail.
Finite-hazard survival nodes are in (0,1]; zero-survival/defaulted contracts
need their explicit credit model/lifecycle route.

Shared schedules/profiles/fixing histories retain their existing 4,096/65,536
bounds. Custom calendar callbacks must terminate without panic. Setup,
allocation, dependency lookup and serde have no certified hot-path latency
claim. Curve dependency depth is bounded by the validated acyclic 256-curve
set; evaluating derived values follows those immutable dependencies.

The [curve tests](../crates/convex-curves/tests/curve_contracts.rs) include exact
dates/units, analytic exp/log/pow identities, 256 deterministic generated cases,
negative rates, duplicate/invalid/expired inputs, missing/cyclic/incompatible
versions, native tolerance failure, domains and convention conversion. The
[helper tests](../crates/convex-analytics/tests/curve_inputs.rs) use a pinned
synthetic economic pack to check term conservation, actual schedules,
observation eligibility and explicit Unsupported dispatch. Analytic f64
checks use absolute 1e-11 plus relative 1e-11; exact contractual data use exact
checks. Existing quantitative pins and tolerances are preserved.

This API separation is a Convex engineering decision informed by the explicit
dates, native implied quotes and pillars in [QuantLib's rate helpers](https://github.com/lballabio/QuantLib/blob/master/ql/termstructures/yield/ratehelpers.hpp),
and by the currency, discount dependency, representation/interpolation and
extrapolation settings in [ORE's yield curve configuration](https://github.com/OpenSourceRisk/Engine/blob/master/OREData/ored/configuration/yieldcurveconfig.hpp).
It does not copy their class hierarchy or establish numerical equivalence.
The source pages were inspected on 5 October 2026; branch pages are mutable
and are not admitted binary/reference captures.

CVX-060 owns numerical/transform certification; CVX-057 owns full OIS/projection
bootstrap/global calibration; CVX-021 credit calibration; CVX-058 cash/basis/repo;
CVX-059 optional models; CVX-063 schemas/C ABI/client parity; CVX-064/065 the
library certificates. The existing 105 retained comparison failures remain
enforced. No production, model, external data or latency gate is completed by
this native contract task.
