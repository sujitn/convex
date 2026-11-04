# Exponential UFR calibration to product prices

`CalibrationMode::Ufr(Box<ProductUfrSettings>)` fits an exponential
forward-convergence tail (to an ultimate forward rate, UFR) to full rate quotes or
Government/Repo quotes. It uses the rate and cash builders and the shared
`calibration` operation. The base curve and the last liquid point (LLP) stay
fixed. There is no sampled grid, no zero-observation substitute and no new service
or integration layer.

"Checked" below means the function validates its input and returns an error
instead of guessing.

The full `Helpers` recipe keeps calendars, cash steps, eligible history, quote
units and revisions, and external product dependencies. Trial curves use the
FlatForward or NS/Svensson model and its analytic **left** forward at the LLP. A
completed product-parametric fit is a valid base even though its recipe says
Helpers, because the check uses its validated query owner. Nodes, stepped or
derived sources and MonotoneConvex are not supported as bases in this route.

The settings are `base`, `last_liquid_point`, `initial`, `lower`, `upper`,
`parameter_scales`, `quote_scales`, `weights`, `max_iterations` and
`objective_tolerance`. The parameter objects have exactly `ufr` and `alpha`. UFR
is a continuous annualized decimal forward. Alpha is a positive speed per year of
curve time. The order is UFR, then alpha. Solver coordinates are the native
parameter divided by its scale, and the graph's Jacobian step and measured
conditioning refer to those coordinates. Starts and closed bounds are required.
UFR stays inside the [-10,10] input bound, and no parameter is inferred.

The recipe's initial extrapolation must equal the initial settings. Its
dependencies must include the base version and every external product
dependency. The role, currency, reference date, time basis, continuous-zero
representation and interpolation must match the base. The LLP must be at least
its minimum and inside its maximum. Source extension cannot supply the join.
Government/Repo fitting keeps the ACT365F contract. Issuer residual basis and
recovery are a separate model.

At least two positive-weight quotes are required, and at most 128. That is
necessary but does not guarantee the parameters are identified. Duplicate
exposures, quotes that all fall before the LLP, and a flat tail whose UFR equals
its boundary forward cannot identify both parameters. The undamped SVD
rank/conditioning check is still required. The solver policy, 1..256 attempts and
the 16,384 product valuation ceiling are unchanged. Every native residual must
pass after fitting, including zero-weight quotes. These cases return no curve and
leave the caller's last-good graph untouched:

- Missing history or resources.
- A bad base or domain.
- Underidentification.
- Stagnation.
- Exhausted work.

For `T=LLP`, the kernel keeps the integrated discount anchor:

```text
f(t) = ufr + (f(T)-ufr) exp(-alpha (t-T))
t z(t) = T z(T) + ufr (t-T) + (f(T)-ufr) (1-exp(-alpha (t-T)))/alpha
P(t) = exp(-t z(t))
```

The output keeps the input recipe and the helper valuations. The UFR and speed,
the join, and every residual, rank, conditioning, work and bound outcome are
explicit diagnostics. `domain().extrapolation` reports the fitted policy, and
equivalent or zero-spread views inherit it. The cash `annualized_parameters` grid
field is empty, because alpha is never labelled as a zero rate. The fitted tail
has no interpolation grid.

`UfrJoinedForward { point, side }` queries the joined curve and requires a
Left/Right side.

- Below the LLP it uses the base interval and model.
- At the LLP, Left uses the base's ending interval (`extrapolated:false`) and
  Right uses the continuous tail limit (`true`). The base's continuing right
  interval is not the tail boundary.
- Beyond the LLP both sides use the analytic tail. A left origin and source
  extension fail.

`UfrForward` is tail-only and rejects points before the LLP. Finite query times
are limited to 150 years. The engine's scalar instantaneous-forward bridge picks
the right interval, so callers that need a side should use the native or shared
joined query.

Quote-risk scenarios rebuild the full products with fixed history and the base
dependencies. Annual-rate bumps are decimal: `0.0001` means 1bp even when a helper
is quoted in percent or basis points. The key converts the move to native units
once. Tail-quote risk returns all five signed PVs with half-step stability checks.
Base quote moves refit the base and then the dependent tail. A move can leave the
set of solutions the bounded tail allows. It then fails, without widening bounds,
changing the objective, freezing a dependent curve or returning partial risk. The
fixture has one larger move that fails this way and a smaller one that is
accepted. A uniform 1bp bump is not guaranteed to work, and global convergence is
not claimed.

Strict shared JSON, schema and C, MCP and the native-compiled WASM all use the
same code. [Schema3 capture and replay](CALIBRATION-SNAPSHOT-REPLAY.md) keeps the
input, the solved tail, the full products and the diagnostics. Reopening a file,
quote risk and a runtime or recovery rebuild all compare the full output.
Schema1/2 and non-UFR schema3 identities are unchanged. Runtime assembly must
request every captured curve by name, including the base. Replay installs the
validated graph in dependency order, and curve names do not decide which
dependencies are available.

This fits the exponential tail described above. EIOPA's
[technical documentation §§9.1,9.4,9.5,9.15](https://www.eiopa.europa.eu/system/files/2023-12/EIOPA-BoS-23-433-RFR%20Technical%20Documentation.pdf)
describes a different model: a Smith-Wilson cashflow-matrix fit with
convergence-based alpha selection. This API does not implement that regulatory
procedure. It also does not fit zero or forward observations, the base model, the
LLP or a separately fixed UFR. Those would each need their own parameter,
objective and validation rules.

## Running the checks

```sh
python3 tests/scripts/product_ufr_reference.py --check
python3 tests/scripts/product_ufr_admitted_reference.py --check
cargo run --locked -p convex --example product_ufr
cargo run --locked -p convex --example product_ufr --no-default-features
cargo run --locked -p convex --example product_ufr --features schemars
cargo test --locked -p convex -p convex-analytics -p convex-engine -p convex-ffi -p convex-mcp -p convex-wasm --all-features --test product_ufr -- --test-threads=1
```

The independent Decimal60 controls declare 18 graphs, 36 product quotes and 126
analytic points, including signed rates, coupon, notional and principal steps,
following dates and LLP sides. A second set of controls with explicit origin
checks keeps the first set and all expected answers and tolerances. FRA, IRS,
OIS, futures and basis quotes in mixed callbacks are generated by the Rust code.
They test that the pieces compose and are not independent financial references.

Not yet covered: Windows/.NET/Excel/Java/JNI and wasm32/browser execution, some
market, model and operation combinations, and independent validation. Providers,
storage, MQ, clocks and order handling stay outside the analytics. No new package,
dependency, route or C export is needed. This work is partial and not certified.
