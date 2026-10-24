# Full-product exponential UFR calibration

`CalibrationMode::Ufr(Box<ProductUfrSettings>)` fits the existing exponential
forward-convergence tail to complete original rate or Government/Repo quotes.
It uses the existing pure rate/cash builders and shared `calibration` operation.
The immutable base curve and actual last liquid point (LLP) stay fixed. There is
no sampled grid, zero-observation substitute or new service/integration layer.

The full original `Helpers` recipe retains calendars, full cash steps, eligible
history, actual quote units/revisions and external product dependencies. Trial
curves use the exact native FlatForward or NS/Svensson model and its original
analytic **left** forward at the LLP. A previous complete product-parametric fit
is an admissible base even though its retained recipe says Helpers. Admission
depends on its actual validated query owner. Nodes, stepped/derived sources and
MonotoneConvex remain unsupported bases in this route.

Required settings are `base`, `last_liquid_point`, `initial`, `lower`, `upper`,
`parameter_scales`, `quote_scales`, `weights`, `max_iterations` and
`objective_tolerance`. Parameter objects have exactly `ufr` and `alpha`: UFR is
a continuous annualized decimal forward, alpha positive speed per curve-time
year. The numerical order is UFR then alpha. Solver coordinates are native
parameter/scale; the graph's explicit Jacobian step and measured conditioning
refer to those coordinates. Starts and closed bounds are required, UFR remains
inside the original [-10,10] ingress bound, and no parameter is inferred.

The recipe's initial extrapolation must exactly equal the complete initial
settings. Its dependencies must include the exact base version plus every
actual external product dependency. The actual role, currency, reference date,
time basis, continuous-zero representation and interpolation must match the
base; LLP must follow its native minimum and remain inside its native maximum.
Source extension cannot supply the join. Government/Repo fitting retains the
original ACT365F contract; issuer residual basis/recovery is a separate model.

At least two positive-weight original quotes are required, at most128. This is
necessary but does not establish identification: duplicate exposures, quotes
entirely before the LLP or a flat tail whose UFR equals its boundary forward
cannot identify both parameters. The original undamped SVD rank/conditioning
guard remains mandatory. Original solver policy, 1..256 attempts and16,384
actual product valuations are unchanged. Every original native residual must
pass after fitting, including zero-weight quotes. Missing history/resources,
bad base/domain, underidentification, stagnation and exhausted work return no
accepted curve and preserve the complete caller last-good graph.

For `T=LLP`, the existing kernel retains the exact integrated discount anchor:

```text
f(t) = ufr + (f(T)-ufr) exp(-alpha (t-T))
t z(t) = T z(T) + ufr (t-T) + (f(T)-ufr) (1-exp(-alpha (t-T)))/alpha
P(t) = exp(-t z(t))
```

The complete output retains the original input recipe and full helper valuations.
The actual UFR/speed, join and every solver residual/rank/conditioning/work/bound
outcome are explicit diagnostics. `domain().extrapolation` reports the actual
fitted policy; equivalent/zero-spread views inherit that actual policy. The cash
`annualized_parameters` grid field is empty: alpha is never labelled as a zero
rate. No interpolation grid exists for the fitted tail.

`UfrJoinedForward { point, side }` queries the original joined curve with a
required Left/Right side. Below LLP it uses the original base interval/model.
At LLP Left uses the base's ending interval (`extrapolated:false`), Right the
exact continuous tail limit (`true`). The base's continuing right interval is
not the tail boundary. Beyond LLP both sides use the analytic tail; left origin
and source extension fail. Existing `UfrForward` remains tail-only and rejects
pre-LLP points. All finite query times retain the original150-year ceiling.
The engine's scalar instantaneous-forward bridge selects the right interval;
side-sensitive callers use the native/shared joined query.

Original quote-risk scenarios rebuild full original products, with fixed
history and exact base dependencies. Canonical annual-rate bumps are decimal:
`0.0001` means1bp even when a helper is quoted in percent or basis points. The
original key converts that move to native units once. Tail-quote risk returns
all five signed PVs with original half-step stability. Base quote moves refit
the base and then the dependent tail. A move can leave the bounded tail's
admissible solution set; it fails without widening bounds, changing objective,
freezing a dependent curve or returning partial risk. The fixture retains one
such actual larger-move failure and a separately explicit admissible smaller
scenario. No uniform1bp or global-convergence guarantee is implied.

Strict shared JSON/schema/C, existing MCP and native-compiled WASM delegate to
these same owners. [Schema3 capture/replay](CALIBRATION-SNAPSHOT-REPLAY.md)
retains the complete input, solved tail, full products and native diagnostics;
actual file reopen, original quote-risk and runtime/recovery rebuild and compare
the complete output. Existing schema1/2 and non-UFR schema3 identities remain.
Runtime assembly must explicitly request every exact captured curve, including
the immutable base. Replay installs the complete validated graph in dependency
order; curve names do not determine which dependencies are available.

This fits the existing heuristic exponential tail. EIOPA's
[technical documentation §§9.1,9.4,9.5,9.15](https://www.eiopa.europa.eu/system/files/2023-12/EIOPA-BoS-23-433-RFR%20Technical%20Documentation.pdf)
describes a different Smith–Wilson cashflow-matrix model and convergence-based
alpha selection. This API does not implement that regulatory procedure or fit
native-zero/forward observations, the base model, LLP or an independently fixed
UFR. Those require their own explicit parameter/objective/admission contracts.

Run the supplied fictional consumer and independent controls:

```sh
python3 tests/scripts/product_ufr_reference.py --check
python3 tests/scripts/product_ufr_admitted_reference.py --check
cargo run --locked -p convex --example product_ufr
cargo run --locked -p convex --example product_ufr --no-default-features
cargo run --locked -p convex --example product_ufr --features schemars
cargo test --locked -p convex -p convex-analytics -p convex-engine -p convex-ffi -p convex-mcp -p convex-wasm --all-features --test product_ufr -- --test-threads=1
```

Independent Decimal60 controls declare18 graphs,36 complete product quotes and
126 analytic points, including signed rates, full coupon/notional/principal
steps, actual following dates and LLP sides. Separate explicit origin admission
preserves the first controls and all original financial answers/tolerances.
Mixed FRA/IRS/OIS/futures/basis callback quotes are native-generated integration
controls, not independent financial answers. CVX-063 remains partial/unaccepted/
uncertified; actual Windows/.NET/Excel/Java/JNI and wasm32/browser qualification,
all mandatory market/model/operation coverage and independent certificates stay
open. Providers, storage, MQ, clocks and execution authority stay outside pure
analytics. No new package, dependency, route or C export is required.
