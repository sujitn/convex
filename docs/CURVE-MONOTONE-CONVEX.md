# Monotone convex curves

`convex::curve_contracts::CurveSource::MonotoneConvex { nodes }` builds an
immutable continuous-zero rate curve with the nonnegative monotone convex
interpolator in `convex_math::interpolation::MonotoneConvex`. The shared
`curves`, `roundtrip` and `curve_query` operations keep the full recipe through
`convex::library`, JSON, `convex_library`, Java, Excel, MCP and WASM exports.
Windows/Excel and wasm32/browser execution has not been verified.

Use `interpolation: "MonotoneConvex"`, `calibration: "Direct"` and continuous
`ZeroRates`. Discount, projection, government and repo roles are supported.
Discount collateral currency must match the rate currency.

Supply 2 to 1,024 pillars at positive times, with DecimalRate or PercentRate
quotes. Dates use the ACT/365F, ACT/360 or ACT/ACT ISDA time basis you give and
an immutable reference date. The quote units and order survive a roundtrip.
Construction normalizes and sorts the model times, and rejects duplicates and the
existing minimum separation. No currency or index preset is inferred.

All interval forward means, including the interval from zero to the first pillar,
must be nonnegative. Zero intervals are valid. These inputs fail:

- negative means;
- an explicit pillar at time zero;
- representations other than rates (discount factors, hazards, basis);
- unavailable compounding;
- product or helper calibration.

Other models support their own signed-rate domains, and this model has no signed
forward extension. Generic `Nodes` with this method is unsupported.

## Financial convention

The implementation follows the nonnegative, unameliorated Hagan-West model.
Adjacent interval means determine weighted node estimates. The endpoint
estimates are computed before the simultaneous positivity bounds are applied.
Four local polynomial regimes share an analytic primitive.

On the exact axis where the endpoint deviation is zero, the rational pieces are
singular. The implementation uses the zero-area quadratic there. That convention
preserves node prices and continuous forwards. It is reported in the model
diagnostics as `hagan_west_2006_nonnegative_exact_axis_quadratic`. It can differ
from the limit of a curve approaching the axis, and continuity in the model
inputs at that degenerate point is not promised.
[Hagan and West (2006), equations 30-62 and 81](https://bank.uni-hohenheim.de/uploads/media/Hagan_and_West__2006__-_Interpolation_Methods_for_Curve_Construction.pdf).

Every continuous-zero pillar reprices exactly. The discount factor and zero rate
use the same integral as the analytic forward. Query-time clipping is removed.
The forward is continuous at pillars but its derivative can jump. Zero rates are
C1 at positive times. `Interpolator::derivative` uses analytic equations with a
stable limit at the origin. It samples no adjacent point and needs no extension
to query the terminal left derivative.

The domain is `[0, last pillar]`, with the analytic origin rate derived by the
model and `P(0)=1`. The positive input grid is returned without an invented
origin quote. The `monotone` diagnostics report the discrete forward means and
the origin-plus-node instantaneous forwards, together with the model convention.
They are interpolation diagnostics, not instrument calibration residuals.

## Queries and tails

`CurveQuery::MonotoneConvexForward { point, side }` returns a decimal continuous
forward per year of the curve's time basis. Both sides agree at interior pillars.
`Left` at zero fails, because there is no interval before time zero. A final
`Right` uses the declared extension even at the last pillar and reports
`extrapolated: true`. Final `Left` is the model's own forward and reports false.
Other model families and derived views reject this method-specific query.

| Declared extension | Zero-rate economics beyond the last pillar | Terminal right forward |
|---|---|---|
| `None` | Fails | Fails |
| `Flat` | Last zero rate | Last zero rate, possibly different from the left forward |
| `FlatForward` | Integrates the terminal instantaneous forward | Terminal instantaneous forward |

`Linear` and UFR tails fail on this source. A UFR join over this model needs its
own boundary contract. Queries at positive times are bounded by 150 curve-time
years. Construction work is bounded by the node count. Queries use an indexed
interval search and a fixed amount of local polynomial work. No source lookup,
clock, provider, database, message queue or runtime is involved.

`Raw`, `DiscountFactor`, `DiscountRatio`, equivalent `ZeroRate` and finite-period
`ForwardRate` keep their existing units, compounding and date-period
conventions. Equivalent zero-rate queries at zero remain undefined. The raw
origin value is a model convention you supply explicitly. Rate and discount
views use the model and its tail directly instead of resampling a grid.

## Migration and checks

The previous implementation integrated an unclipped polynomial, clipped only its
forward and sampled zero derivatives. Its first-node estimates left out the true
origin interval, and its flat-zero and flat-forward tails disagreed. Those
equations are replaced in place. No legacy implementation or approximate alias is
kept.

- `with_extrapolation()` now integrates the terminal forward.
- Queries at negative time fail, and a negative first interval also fails.
- Existing snapshot records that select MonotoneConvex with a negative interval
  are rejected.
- A signed-rate curve needs an explicitly chosen model that allows it, such as
  Linear. Reconstruction never relabels the original policy.
- The protected native test sections are unchanged.
- Older raw-value `DiscreteCurve` and convenience construction still requires its
  declared financial representation. A method enum alone is not validation.

Run the pure example and the frozen independent checks:

```sh
python3 tests/scripts/monotone_reference.py --check
python3 tests/scripts/monotone_math_fixture.py --check
python3 tests/scripts/library_monotone_inputs.py --check
cargo test --locked -p convex-math --test monotone_complete -p convex-curves --test monotone_contracts -p convex --test library_monotone --all-features
cargo run --locked -p convex --example library_monotone
```

The independent Decimal(60) set covers all regimes, exact and near axes,
positivity limits, zero slices, uneven nodes, origin and terminal derivatives,
tails, and three date bases. Freshly captured outputs validate transport only;
they are not the financial reference. No external MonotoneConvex engine is
compared, and the numerics have not had an independent review.

Not yet covered: product and helper calibration, quote-risk rebuilding,
product-calibration snapshot graphs, other models, and host support.
