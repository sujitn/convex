# Complete product calibration and quote-risk snapshot replay

`SnapshotAssembly.calibration` captures one complete original rate/CDS/cash
product graph. `convex::library::CalibrationReplay` reuses the existing native
resource resolution, checked full builders and original quote-risk operations.
`convex_engine::calibration_replay` owns capture-time admission, immutable output
comparison and application integration. Ports retain bounded JSON and metadata;
they do not depend on financial crates. The engine adds one dependency on the
existing pure facade; no external package, backend or provider enters pricing.

The record retains complete original `LibraryInputs`: actual calendar tables and
revisions, fixing revisions/publication decisions, independently supplied native
curves, full original helper/product/legal terms, quote units/revisions,
calibration controls and exact dependencies. Every supplied, fitted and view
curve has an explicit `CalibrationCurvePin` with its original opaque definition
version and a separately supplied positive numeric capture revision. Missing,
extra or mismatched pins fail. No numeric revision is parsed from a string.

Original native graph output now retains full rate, CDS and cash family fits.
The capture's `expected` contains actual fitted recipes/grids, complete original
helper valuations, native residuals, piecewise roots or global rank/conditioning/
work diagnostics, build order and consumed publications. Replay reconstructs the
original graph and reprices every source through its original builder before
comparing the entire canonical output. Stored fitted nodes and `Helpers` IDs
are never decoded as a product graph or installed as a replay cache.

An exact diagnostic mismatch fails; a platform/build that computes different
canonical diagnostics cannot silently bless the earlier capture. This strict
replay contract is engineering reproducibility evidence, with no cross-platform
bitwise, independent financial or latency certificate. New captures require
actual original inputs and explicitly supplied metadata. `capture_calibration`
only seals an empty `expected` object; it cannot replace an existing capture.

| Content | Schema | Hash domain |
|---|---|---|
| Original zero records | 1 | `convex.snapshot.v1\0` |
| Complete direct recipes | 2 | `convex.snapshot.v2\0` |
| Complete product-calibration capture | 3 | `convex.snapshot.v3\0` |

An omitted `calibration` field preserves both older canonical identities. Schema
3 requires a capture; other versions reject a populated capture. Migration
creates a new content-addressed snapshot with the full original graph. Historical
snapshots are never overwritten, relabelled or completed using current providers.
Separate direct recipe graphs can coexist only with disjoint identities; they
cannot supply omitted inputs to the embedded calibration graph. Its independent
dependencies remain inside its original inputs. Older zero records cannot supply
missing native calibration dependencies.

A standalone schema-3 product/risk graph can omit unrelated bond references and
market marks. Original schema-1/2 reference requirements remain unchanged. The
runtime's fixed-bullet pricing use case still requires its actual instrument
references and quotes. An original supplied-node risk graph retains its complete
independent curves and full product action even when no product fit is needed.

All original valuation dates must match the snapshot economic date; an original
rate calibration instant must match `as_of`. Native input publication/known-at
times cannot exceed the snapshot cutoff. Restated captures can retain later
known revisions while keeping the original native calculation's earlier fixing
cutoff. Replay preserves that cutoff and its revision-selection policy. Missing
due history, unknown/discarded product fields, incorrect resource versions,
unidentified fits, exhausted work or original residual failures return no accepted
curve set or risk result. Original financial tolerances and solver defaults remain.

The actual assembler validates before repository reads/publication. Configuration
dry-run, stage/activation, exact snapshot selection, runtime graph activation,
valuation and recovery consume the complete capture. Existing fixed-bullet
observed-mid pricing can use its checked rate curves without resampling or losing
their original time basis/roles. Result lineage includes the graph capture,
every native curve's numeric capture revision and explicitly supplied definition
pins. Full original quote/legal/calendar/history provenance remains inside the
captured inputs and native fit output. Other server product/job/risk endpoints
remain their owning work; the offline operation does not add a server route.

Native `Build`, full product `Price`, quote/node/recovery `Scenario`,
`ProductScenario`, `ProductRisk` and bucket reconciliation reuse the original
calibration action. Exact quote keys/units, frozen versus rebuilt response,
five-PV/half-step stability and original product/model restrictions still apply.
Risk/scenario/bucket actions must claim this exact snapshot ID. Their product
time/cutoffs receive the same capture-time checks. Original inputs and last-good
results remain unchanged when an action fails. Data quality and provenance are
returned with the result; the calculation grants no execution authorization.

The capture's source timestamp and age budget conservatively apply to every
retained curve/dependency. Runtime expiry uses that deadline and original quote/
override deadlines. It does not refresh inputs on replay. Source-specific graph
assembly and managed provider/fixing services remain application work; callers
must supply actual conservative source metadata.

Snapshots retain the shared 16 MiB, 64 total curve and 4,096 original input
node/helper/observation/segment limits. Inputs keep the original strict library
2 MiB/32,768 JSON value, 64-level, 4,096-entry, 8,192-byte text bounds. Full output
diagnostics admit 262,144 JSON values within the snapshot byte limit. Original
64-recipe/128-helper graph and family/grid/history/solver bounds remain. Numeric
result lineage keeps its original 256-record limit. This is bounded setup/batch
work, with normal allocations and no hot-loop or colocation claim.

Run the real file reopen and original full rate quote-risk consumer:

```sh
cargo run --locked -p convex-engine --example calibration_snapshot_replay -- demo /tmp/convex-calibration-snapshot-replay
cargo run --locked -p convex-engine --example calibration_snapshot_replay -- --emit-configuration
cargo run --locked -p convex-engine --example calibration_snapshot_replay -- --emit-schema
cargo test --locked -p convex --all-features --test calibration_replay -p convex-engine --test calibration_snapshot_replay --test calibration_snapshot_runtime -p convex-ports --test calibration_snapshots
python3 tests/scripts/calibration_replay_inputs.py --check
```

The demo prints the actual snapshot ID and its complete original `risk_action`.
Save that action as a JSON file, then execute the separately reopened snapshot:

```sh
cargo run --locked -p convex-engine --example snapshot_replay -- verify DIRECTORY SNAPSHOT_ID
cargo run --locked -p convex-engine --example snapshot_replay -- calibration DIRECTORY SNAPSHOT_ID ACTION_JSON
cargo run --locked -p convex-engine --example snapshot_replay -- query-curves DIRECTORY SNAPSHOT_ID QUERIES_JSON
```

The parent directory must already exist. Action ingress rejects duplicate or
discarded economic fields and numeric Decimal precision loss through the original
strict decoder. Any failure exits unsuccessfully with no successful stdout result.
The published operator schema describes the capture envelope; the original pure
request schema and native decoder own financial admission. Actual exported schemas
are exercised by `tests/scripts/calibration_replay_schema.py`.

Thirty-two unchanged original calibration/risk requests and two independent
Decimal(60) dated-deposit controls provide scoped evidence. They do not qualify
all market/method combinations. CVX-063 and all 2,154 mandatory library records
remain partial/Planned. Product-price parametric fits, other method/model/hedge/
workout operations, actual Windows/Excel/wasm32/browser execution and independent
financial/library/production/latency certificates remain open.
