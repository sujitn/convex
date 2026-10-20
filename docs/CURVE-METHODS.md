# Explicit step-zero and flat-forward methods

`convex::curve_contracts` now supports complete `CurveSource::PiecewiseConstant`
and `CurveSource::FlatForward` supplied grids through the existing shared
`curves`, `curve_query` and `roundtrip` operations. Full recipes retain role,
currency, immutable curve version, reference date, actual dated/model-time
pillars, native quote units, time basis, continuous compounding and explicit
interpolation/extrapolation choices. The source and method must agree.

These are direct continuous-zero rate sources for Discount, Projection,
Government and Repo roles. They accept decimal or percent quotes, normalize
unsorted input without changing the retained recipe, and reject duplicate or
numerically inseparable pillars. A Discount role's collateral currency must
match the curve currency. There are 2–1,024 nodes; model times lie in [0,150]
and separation is at least 1e-10. No origin is inserted. If supplied on a
flat-forward grid, its zero must equal the first positive interval's zero.
`P(0)=1` remains the exact discount identity and consumes no source.

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

Supply matching `interpolation: "FlatForward"`, representation
`{"ZeroRates":{"compounding":"Continuous"}}`, the original reference date
and `time_basis: "Act365Fixed"`. Extrapolation is required. The full request
and runnable examples are in [the supplied corpus](../tests/fixtures/library/curve-methods.json).

`PiecewiseConstant` holds the preceding zero over `[t_i,t_{i+1})`, takes the
new value at each exact knot, and retains the final supplied zero at the final
endpoint. It replaces the older native `PiecewiseConstant` tag's incorrect
linear interpolator. The pure math kernel works on arbitrary finite bounded
grids, including actual hazard steps, but the new checked source admits only
continuous-zero rate roles. Step zeros can create discount jumps: they do not
represent integrated constant forwards or a smooth no-arbitrage curve. A
derivative at a jump fails; `FlatForwardRate` on step zeros is Unsupported.
Discount-ratio and explicitly dated period-forward queries still calculate
their actual finite-period values. Hazard integration/calibration retains its
separate complete credit contract.

`FlatForward` reuses the original native forward kernel. Each interval has
`f_i=(t_{i+1}z_{i+1}-t_i z_i)/(t_{i+1}-t_i)` and integrated zero
`z(t)=(t_i z_i+f_i(t-t_i))/t`, so its log-discount is linear on that interval.
The [QuantLib curve presentation](https://www.quantlib.org/slides/rate-curves.pdf)
describes this log-discount/flat-forward relationship on pages5–7. Financial
expected values here are calculated independently with Decimal(60), rather
than captured from a native or external engine. Negative zeros and forwards
are valid; finite discount factors above one are allowed.

Use an explicit side for an instantaneous forward:

```json
{"kind":"FlatForwardRate","point":{"Tenor":2.0},"side":"Left"}
```

`Left` uses the interval ending at an exact knot; `Right` uses the starting
interval. Away from knots both return the same original segment forward.
There is no negative-time side at zero. The final right side needs exact
FlatForward extension. Actual rate units are continuous annualized decimal
per year of the recipe's time basis. Dates use that same original time basis;
ordinary finite-period forwards retain their own explicit accrual/compounding
inputs. Parametric `InstantaneousForward` and this grid query have separate
native owners; derived views require their own forward contract.

`None` rejects positive-time queries outside the supplied grid. `Flat` holds
the raw zero at the boundary; `Linear` extends the original boundary raw
zero secant. These policies are distinct from forward extension and cannot
silently supply an out-of-grid `FlatForwardRate`. On the FlatForward source,
`FlatForward` extrapolation extends the last original forward by integrating
from the final supplied log-discount. Before a positive first pillar it holds
the first original zero, giving a constant forward there. A missing side at
that first pillar or the final pillar is explicitly reported as extrapolated
when the declared exact extension supplies it. Raw/forward extension at zero
is disclosed; the discount origin identity is not extrapolated.

The original checked `Nodes` route retains its method constraints. Use the
new explicit source for these models. The older raw `DiscreteCurve` FlatForward
tail no longer substitutes a flat raw value: checked queries reject it and
its infallible compatibility query returns NaN. See [route migration](ROUTE-MIGRATION.md).
The older engine `CurveBuilder` now declares its actual `Flat` zero-tail default.
It rejects a `FlatForward` selection before provider access or cache publication.
`BuiltCurve::from_snapshot` rejects `ZeroExtrapolation::FlatForward` rather than
relabeling the supplied policy. Full checked flat-forward recipes are available
through the pure shared library and [complete schema-2 snapshot replay](CURVE-SNAPSHOT-REPLAY.md). Existing flat-zero snapshots retain their policy.
DF/survival/issuer-basis/periodic-zero reinterpretations, UFR, product/helper
fitting and quote-risk rebuilding are unsupported on these new sources.
Existing method owners remain required for complete library coverage.

Run the standalone consumer and independent controls:

```sh
python3 tests/scripts/curve_method_reference.py --check
python3 tests/scripts/library_curve_method_inputs.py --check
cargo test --locked -p convex-curves --all-features --test curve_methods
cargo run --locked -p convex --example library_curve_methods --features schemars
```

Native/shared JSON/C/Java/MCP and native compiled WASM checks are engineering
reachability. Actual Windows/Excel and wasm32/browser execution remains
unverified. Every mandatory convention/model/operation record remains Planned;
CVX-063, independent financial approval and library/production/latency
certificates remain open.
