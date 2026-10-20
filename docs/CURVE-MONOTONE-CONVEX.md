# Complete nonnegative MonotoneConvex curves

`convex::curve_contracts::CurveSource::MonotoneConvex { nodes }` constructs a
complete immutable continuous-zero rate curve using the original native
`convex_math::interpolation::MonotoneConvex` owner. Shared `curves`, `roundtrip`
and `curve_query` operations preserve its complete recipe through
`convex::library`, JSON, `convex_library`, Java, Excel, MCP and WASM exports.
Actual Windows/Excel and wasm32/browser execution remains unqualified.

Use `interpolation: "MonotoneConvex"`, `calibration: "Direct"` and continuous
`ZeroRates`. Discount, projection, government and repo roles are supported.
Discount collateral currency must match the rate currency. Supply 2–1,024
actual positive-time pillars with original DecimalRate or PercentRate quotes.
Dates use the supplied ACT/365F, ACT/360 or ACT/ACT ISDA time basis and immutable
reference date. Original quote units/order survive roundtrip; construction
normalizes and sorts the actual model times, rejecting duplicates and the
existing minimum separation. There is no inferred currency/index preset.

All interval forward means, including the interval from zero to the first
pillar, must be nonnegative. Zero intervals are valid. Negative means, an
explicit zero-time input pillar, non-rate/DF/hazard/basis representations,
unavailable compounding and product/helper calibration fail. Other existing
models support their own signed-rate domains; this model supplies no signed
forward extension. Generic `Nodes` with this method remains unsupported.

## Native financial convention

The corrected owner follows the nonnegative, unameliorated Hagan–West model:
adjacent interval means determine weighted node estimates; the original
endpoint estimates are computed before simultaneous positivity bounds. Four
local polynomial regimes share an analytic primitive. At an exact zero
endpoint-deviation axis, the rational pieces are singular; this implementation
explicitly retains the original zero-area quadratic. That convention preserves
node prices and continuous forwards. It is disclosed as
`hagan_west_2006_nonnegative_exact_axis_quadratic` in model diagnostics.
It can differ from a limiting near-axis curve; no continuity in model inputs at
that degeneracy is promised. [Hagan and West (2006), equations 30–62 and 81](https://bank.uni-hohenheim.de/uploads/media/Hagan_and_West__2006__-_Interpolation_Methods_for_Curve_Construction.pdf).

Every original continuous-zero pillar reprices exactly. The discount factor
and zero rate consume the same integral as the analytic forward. Query-time
clipping is removed. The forward is continuous at native pillars; its derivative
can jump. Zero rates are C1 at positive times. `Interpolator::derivative`
uses analytic equations, with a stable origin limit; it samples no adjacent
point and needs no extension to query the terminal left derivative.

The native domain is `[0, last actual pillar]`, with the analytic origin rate
derived by the model and `P(0)=1`. The original positive input grid is returned
without an invented origin quote. `monotone` diagnostics report original
discrete forward means and origin-plus-node instantaneous forwards, together
with the exact model convention. They are interpolation diagnostics, not
instrument calibration residuals.

## Query and tail contracts

`CurveQuery::MonotoneConvexForward { point, side }` returns a decimal continuous
forward per year of the actual curve time basis. Both sides agree at interior
pillars. `Left` at zero fails because no negative-time interval exists. A final
`Right` consumes the declared extension even at the last pillar and reports
`extrapolated: true`. Final `Left` is the original native forward and reports
false. Other model families and derived views reject this method-specific query.

| Declared extension | Zero-rate economics beyond the last pillar | Terminal right forward |
|---|---|---|
| `None` | Fails | Fails |
| `Flat` | Original last zero rate | Original last zero rate, possibly different from the native left forward |
| `FlatForward` | Integrates the original terminal instantaneous forward | Original terminal instantaneous forward |

`Linear` and UFR tails fail on this source. UFR joins over this model require
their own boundary contract. Positive-time queries remain bounded by 150
curve-time years. Construction has bounded node work; queries use an indexed
interval search and fixed local polynomial work. No source lookup, clock,
provider, DB, MQ, runtime or authority enters this owner.

`Raw`, `DiscountFactor`, `DiscountRatio`, equivalent `ZeroRate` and finite-period
`ForwardRate` retain existing native units, compounding and actual date-period
conventions. Equivalent zero-rate queries at zero remain undefined; the native
raw origin is an explicitly supplied model convention. Exact rate/discount
views consume the original model and tail rather than resampling a grid.

## Migration and checks

The old owner integrated an unclipped polynomial, clipped only its forward and
sampled zero derivatives. Its first-node estimates omitted the true origin
interval and its flat-zero/flat-forward tails disagreed. Those equations are
replaced in the existing owner; no legacy implementation or approximate alias
is retained. `with_extrapolation()` now integrates the terminal forward.
Queries at negative time fail, and a negative first interval also fails.
Existing snapshot records selecting MonotoneConvex with a negative interval
are rejected. A signed-rate curve requires an explicitly chosen admissible
model, such as Linear; reconstruction never relabels the original policy.
Original protected native test sections remain unchanged. Older raw-value
`DiscreteCurve`/convenience construction still requires its declared financial
representation and host qualification; a method enum alone is not validation.

Run the pure consumer and frozen independent controls:

```sh
python3 tests/scripts/monotone_reference.py --check
python3 tests/scripts/monotone_math_fixture.py --check
python3 tests/scripts/library_monotone_inputs.py --check
cargo test --locked -p convex-math --test monotone_complete -p convex-curves --test monotone_contracts -p convex --test library_monotone --all-features
cargo run --locked -p convex --example library_monotone
```

The independent Decimal(60) set covers all regimes, exact and near axes,
positivity limits, zero slices, uneven nodes, origin/terminal derivatives,
tails and three actual date bases. Fresh native envelopes validate transport;
they do not supply the financial oracle. No external MonotoneConvex engine or
independent human quantitative approval is claimed. Product/helper calibration,
original quote-risk rebuilding, complete product-calibration snapshot graphs, other models,
host and library/production/latency certificates remain required. CVX-063 stays
partial and every mandatory coverage record stays Planned.
