# Step-zero and flat-forward curve methods

This document covers `CurveSource::PiecewiseConstant` and
`CurveSource::FlatForward` in `convex::curve_contracts`. You pass in the grid,
and the shared `curves`, `curve_query` and `roundtrip` operations build and
query it. A recipe keeps the role, currency, immutable curve version, reference
date, dated or model-time pillars, quote units, time basis, continuous
compounding and the interpolation/extrapolation choices. The source and method
must agree.

These are direct continuous-zero rate sources for the Discount, Projection,
Government and Repo roles. They accept decimal or percent quotes, sort unsorted
input without changing the stored recipe, and reject duplicate or numerically
inseparable pillars. A Discount role's collateral currency must match the curve
currency. There are 2–1,024 nodes; model times lie in [0,150] and separation is
at least 1e-10. No origin is inserted. If you pass one on a flat-forward grid,
its zero must equal the first positive interval's zero. `P(0)=1` remains the
exact discount identity and uses no source.

For example, the source inside a complete rate recipe is:

```json
{
  "FlatForward": {
    "nodes": [
      {"pillar":{"Date":"2024-02-29"},"quote":{"value":0.02,"unit":"DecimalRate"}},
      {"pillar":{"Date":"2025-02-28"},"quote":{"value":2.0,"unit":"PercentRate"}},
      {"pillar":{"Date":"2026-02-28"},"quote":{"value":0.025,"unit":"DecimalRate"}}
    ]
  }
}
```

Pass matching `interpolation: "FlatForward"`, representation
`{"ZeroRates":{"compounding":"Continuous"}}`, the reference date and
`time_basis: "Act365Fixed"`. Extrapolation is required. The full request and
runnable examples are in [the fixture corpus](../tests/fixtures/library/curve-methods.json).

`PiecewiseConstant` holds the preceding zero over `[t_i,t_{i+1})`, takes the
new value at each exact knot, and keeps the final zero at the final endpoint.
It replaces the older `PiecewiseConstant` tag, which wrongly used a linear
interpolator.

The pure math kernel works on any finite bounded grid, including hazard steps,
but the checked source accepts only continuous-zero rate roles. Step zeros can
create discount jumps. They do not represent integrated constant forwards or a
smooth no-arbitrage curve. A derivative at a jump fails, and `FlatForwardRate`
on step zeros is Unsupported. Discount-ratio and dated period-forward queries
still compute their finite-period values. Hazard integration and calibration
are covered by the separate credit documents.

`FlatForward` reuses the existing forward kernel. Each interval has
`f_i=(t_{i+1}z_{i+1}-t_i z_i)/(t_{i+1}-t_i)` and integrated zero
`z(t)=(t_i z_i+f_i(t-t_i))/t`, so its log-discount is linear on that interval.
The [QuantLib curve presentation](https://www.quantlib.org/slides/rate-curves.pdf)
describes this log-discount/flat-forward relationship on pages 5–7. The
expected values here are calculated independently with Decimal(60), not captured
from the Rust code or an external engine. Negative zeros and forwards are valid;
finite discount factors above one are allowed.

Use an explicit side for an instantaneous forward:

```json
{"kind":"FlatForwardRate","point":{"Tenor":2.0},"side":"Left"}
```

`Left` uses the interval ending at an exact knot; `Right` uses the starting
interval. Away from knots both return the same segment forward. There is no
negative-time side at zero. The final right side needs exact FlatForward
extension. Rates are continuous annualized decimals per year of the recipe's time
basis, and dates use that same time basis. Ordinary finite-period forwards keep
their own accrual/compounding inputs. The parametric `InstantaneousForward` and
this grid query are separate implementations; derived views need their own
forward contract.

`None` rejects positive-time queries outside the grid. `Flat` holds the raw zero
at the boundary; `Linear` extends the boundary raw-zero secant. These policies
are distinct from forward extension and cannot silently supply an out-of-grid
`FlatForwardRate`.

On the FlatForward source, `FlatForward` extrapolation extends the last forward
by integrating from the final log-discount. Before a positive first pillar it
holds the first zero, which gives a constant forward there. A missing side at the
first or final pillar is reported as extrapolated when the declared extension
supplies it. Raw and forward extension at zero are disclosed; the discount
origin identity is not extrapolated.

The checked `Nodes` route keeps its method constraints. Use the explicit source
for these models. The older raw `DiscreteCurve` FlatForward tail no longer
substitutes a flat raw value: checked queries reject it and its infallible
compatibility query returns NaN. See [route migration](internal/ROUTE-MIGRATION.md).

The older engine `CurveBuilder` cache has been removed; curves are stored as
complete recipes in `CurveStore`. `BuiltCurve::from_snapshot` rejects
`ZeroExtrapolation::FlatForward` rather than relabeling the policy. Flat-forward
recipes are available through the shared library and
[schema-2 snapshot replay](CURVE-SNAPSHOT-REPLAY.md). Existing flat-zero
snapshots keep their policy.

Not supported on these sources: DF, survival, issuer-basis and periodic-zero
reinterpretations, UFR, product/helper fitting and quote-risk rebuilding.

Run the example and reference checks:

```sh
python3 tests/scripts/curve_method_reference.py --check
python3 tests/scripts/library_curve_method_inputs.py --check
cargo test --locked -p convex-curves --all-features --test curve_methods
cargo run --locked -p convex --example library_curve_methods --features schemars
```

The Rust, shared JSON, C, Java, MCP and compiled WASM paths are tested for
reachability only. Windows/Excel and wasm32/browser execution is not yet
verified, and the results have not been independently validated against a vendor
system. Every required convention, model and operation is still planned work,
and financial approval and library, production and latency certification are
still open.
