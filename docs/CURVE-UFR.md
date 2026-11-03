# Explicit UFR convergence

`convex::curve_contracts` supports `CurveSource::UfrConvergence`, which extends
a `FlatForward` or Nelson–Siegel/Svensson continuous-zero curve beyond its last
liquid point (LLP) so that the forward rate converges to an ultimate forward rate
(UFR). The shared `curves`, `curve_query` and `roundtrip` operations keep both full
recipes. The calculation needs no provider, clock, storage, message queue or server.

The source declares its base version and its LLP:

```json
{"UfrConvergence":{"base":{"id":"base.positive","version":"source.v7"},
  "last_liquid_point":{"Tenor":20.0}}}
```

Pass the base recipe in the input set, list the same key in `dependencies`, and
set the output recipe's extrapolation explicitly:

```json
{"UfrConvergence":{"ufr":0.042,"alpha":0.1}}
```

`ufr` is a continuous annualized decimal forward per year of the recipe's
time basis; `alpha` is a positive convergence speed per such year. Neither is
inferred from the currency, a regulatory table or a market-data provider.

The output role, currency, reference date, time basis, continuous-zero
representation and interpolation must match the base exactly. Discount,
Projection, Government and Repo roles are supported. The base's quotes,
parameters, calendar dates and versions are kept unchanged.

The LLP can be a date or a model time. It must come after the base's minimum and
lie within its maximum. At a FlatForward knot, the interval ending at that knot
supplies the boundary forward, and an LLP inside an interval uses that same
interval. The parametric sources supply their analytic forward. Source
extrapolation cannot provide the boundary. Queries before the source minimum fail
even if that source allows a short-end tail.

For `T=LLP`, the tail keeps the boundary zero `z_T` and the left instantaneous
forward `f_T` of the base curve:

```text
f(t) = ufr + (f_T - ufr) exp(-alpha (t-T))
t z(t) = T z_T + ufr (t-T) + (f_T-ufr) (1-exp(-alpha(t-T)))/alpha
P(t) = exp(-t z(t))
```

The integral uses the stable `expm1` expression and its zero-product limit,
including tiny positive alpha or time changes. Negative zeros, forwards and UFRs
are valid, and finite positive discount factors may exceed one.

Alpha and the UFR must be finite, alpha must be positive, and the UFR must lie
within the decimal rate input bound [-10,10]. Query times keep the [0,150] ceiling
and the declared time basis. Derived views keep the existing depth bound.

Raw and discount queries up to the LLP are answered by the base curve. Beyond the
LLP they use the tail above. The query domain reports the base minimum and the LLP
as the interval, with the required UFR extension policy; it does not return an
interpolation grid. The `ufr` diagnostics report the full source key, the LLP
date/time, the boundary tenor, zero and left forward, the UFR and alpha. Solver
iterations are zero and residuals are empty, because this is a model input and not
an instrument calibration.

Use the tail-specific analytic forward query at or after the LLP:

```json
{"kind":"UfrForward","point":{"Date":"2054-02-28"}}
```

This query returns a continuous decimal rate, the time basis and whether the tail
was used (`extrapolated: true`), including its right limit at the join. Raw and
discount queries at the join use the base curve and are not extrapolated.
`P(0)=1` is the exact discount identity and does not need an origin point.
Ordinary dated zero, discount-ratio and period-forward queries keep their output
compounding and accrual basis. `UfrForward` before the LLP, or on another source,
fails. The parametric and FlatForward forward queries are unchanged.

This is a simple exponential forward-convergence model. EIOPA's
[December 2023 technical documentation](https://www.eiopa.europa.eu/system/files/2023-12/EIOPA-BoS-23-433-RFR%20Technical%20Documentation.pdf)
describes a Smith–Wilson cashflow-matrix construction. This tail does not
implement that regulatory methodology. The test controls use Decimal(60)
equations and the exact submitted binary query coordinates. They have not been
compared against an external engine or a regulatory calibration.

The generic `Nodes` route still rejects UFR policy selections. Other source
families would need their own analytic join; numerical derivatives, slope
overrides and sampled-node substitutes are rejected.

The MonotoneConvex source and [recipe snapshot replay](CURVE-SNAPSHOT-REPLAY.md)
are implemented. Fitting a UFR tail to rate, Government and Repo product prices,
quote risk, and schema-3 capture and replay are described in the
[product UFR calibration contract](PRODUCT-UFR-CALIBRATION.md). Calibration of
native zero or forward curves and regulatory calibration are not covered. See
[route migration](internal/ROUTE-MIGRATION.md).

Run the example and the reference checks:

```sh
python3 tests/scripts/ufr_reference.py --check
python3 tests/scripts/ufr_math_fixture.py --check
python3 tests/scripts/library_ufr_inputs.py --check
cargo test --locked -p convex-math --test ufr_tail -p convex-curves --test ufr_contracts
cargo run --locked -p convex --example library_ufr --features schemars
```

The shared JSON, C, Java and MCP routes and the WASM build are exercised by tests.
Windows, .NET, Excel and browser execution have not been tested.

Tails fitted to full products, and the explicit Left/Right `UfrJoinedForward`, are
covered in the [product UFR contract](PRODUCT-UFR-CALIBRATION.md). The boundary
forward comes from the Nelson–Siegel/Svensson curve itself, including curves
fitted earlier from products; it is never taken from a recipe label or a sampled
derivative.
