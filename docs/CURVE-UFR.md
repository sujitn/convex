# Explicit UFR convergence

`convex::curve_contracts` supports a supplied `CurveSource::UfrConvergence`
over an exact immutable `FlatForward` or Nelson–Siegel/Svensson continuous-zero
source. The existing shared `curves`, `curve_query` and `roundtrip` routes
preserve both full recipes. No provider, clock, storage, MQ or server is needed.

The source declares its base version and actual last liquid point (LLP):

```json
{"UfrConvergence":{"base":{"id":"base.positive","version":"source.v7"},
  "last_liquid_point":{"Tenor":20.0}}}
```

Supply the original base recipe in the immutable input set, list that same
key in `dependencies`, and set the output recipe's extrapolation explicitly:

```json
{"UfrConvergence":{"ufr":0.042,"alpha":0.1}}
```

`ufr` is a continuous annualized decimal forward per year of the recipe's
time basis; `alpha` is positive convergence speed per such year. Neither is
inferred from currency, a regulatory table or a market-data provider. The
output role, currency, reference date, time basis, continuous-zero representation
and interpolation must match its base exactly. Discount, Projection,
Government and Repo roles are supported. The base's full native-unit quotes,
parameters, calendar dates and versions remain intact.

The LLP can be a date or model time. It must follow the base's native minimum
and lie within its native maximum. At a FlatForward knot, the actual interval
ending at that knot supplies the boundary forward. An LLP inside an interval
uses that same original interval. The parametric sources supply their original
analytic forward. Source extrapolation cannot provide the boundary. Queries
before the source minimum fail even if that source allows a short-end tail.

For `T=LLP`, the native tail retains the original boundary zero `z_T` and left
instantaneous forward `f_T`:

```text
f(t) = ufr + (f_T - ufr) exp(-alpha (t-T))
t z(t) = T z_T + ufr (t-T) + (f_T-ufr) (1-exp(-alpha(t-T)))/alpha
P(t) = exp(-t z(t))
```

The integral uses the stable `expm1` expression and its zero-product limit,
including tiny positive alpha/time changes. Negative zeros/forwards/UFRs are
valid; finite positive discount factors may exceed one. Alpha and UFR must be
finite, alpha positive and the supplied UFR within the existing decimal rate
ingress bound [-10,10]. All query times retain the original [0,150] ceiling
and exact declared time basis. Derived views retain the existing depth bound.

Raw and discount queries through the LLP delegate to the original source.
Beyond the LLP they use the exact native tail. Its domain reports the base
minimum and explicit LLP as the original interval, with the required UFR
extension policy. It does not return a fabricated interpolation grid. The
`ufr` diagnostics report the full source key, original LLP date/time, actual
boundary tenor, zero, analytic left forward, UFR and alpha. These are supplied
model diagnostics; zero solver iterations and empty residuals do not claim an
instrument calibration.

Use the tail-specific analytic forward query at or after the LLP:

```json
{"kind":"UfrForward","point":{"Date":"2054-02-28"}}
```

This query returns continuous decimal rate, original time basis and actual
tail consumption (`extrapolated: true`), including its right limit at the join.
Raw/discount at the join consume the original source and are not extrapolated.
`P(0)=1` is the exact discount identity and does not consume an unavailable
origin. Ordinary dated zero/discount-ratio/period-forward queries retain their
explicit output compounding and accrual basis. `UfrForward` before the LLP or
on another source fails; existing parametric and FlatForward forward queries
retain their separate native ownership.

This is the existing heuristic exponential forward-convergence model.
EIOPA's [December 2023 technical documentation](https://www.eiopa.europa.eu/system/files/2023-12/EIOPA-BoS-23-433-RFR%20Technical%20Documentation.pdf)
describes a Smith–Wilson cashflow-matrix construction. This tail does not
implement that regulatory methodology. Independent controls use Decimal(60)
equations and exact submitted binary query coordinates; no external engine
execution or regulatory/market calibration is claimed.

The original generic `Nodes` route still rejects UFR policy selections.
Other native source families require their own complete analytic join contracts;
numerical derivatives, supplied slope overrides and sampled-node aliases are
rejected. The complete MonotoneConvex source and [full supplied recipe snapshot replay](CURVE-SNAPSHOT-REPLAY.md) are implemented. Parameter/product-price fitting, original quote-risk rebuilds and complete product-calibration snapshot graphs remain required work. See [route migration](ROUTE-MIGRATION.md).

Run the standalone consumer and frozen controls:

```sh
python3 tests/scripts/ufr_reference.py --check
python3 tests/scripts/ufr_math_fixture.py --check
python3 tests/scripts/library_ufr_inputs.py --check
cargo test --locked -p convex-math --test ufr_tail -p convex-curves --test ufr_contracts
cargo run --locked -p convex --example library_ufr --features schemars
```

Native/shared JSON/C/Java/MCP and native compiled WASM checks establish scoped
engineering reachability. Actual Windows/.NET/Excel and wasm32/browser remain
unverified. CVX-063 is partial; all mandatory records and independent financial,
library, production and latency certificates remain open.
