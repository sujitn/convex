# Snapshot read and JSON measurements

Build the existing server and snapshot diagnostic:

```sh
cargo build --locked -p convex-server --bins
cargo build --locked -p convex-engine --example core_snapshot_io
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_snapshot_io.py \
  --binary-dir target/debug --output /tmp/convex-snapshot-io.json
```

This reuses the existing importer and snapshot ports to capture the same
36-trade/12-receipt and 60-trade/0-receipt workload shapes as the
[update benchmark](UPDATE-LATENCY.md). The definitions contain the original
bond, CDS, IRS, OIS and overnight/term-future templates. All captured terms,
curves and fixings remain in the actual immutable snapshot.

Seven samples after one warmup separate bounded warm file reads, JSON decoding,
content copying, canonical content/envelope serialization, SHA-256, verification
and the complete file-adapter snapshot read. `--samples 3..31` and
`--warmup 1..3` select other bounded sample counts. Every output is checked
against the original captured bytes or typed value outside its timed interval.
Returned-value destruction is also outside the stage interval; work performed
inside verification or the adapter remains inside those total measurements.
The stages are diagnostics, and their medians are not additive.

The JSON records every sample, snapshot identity, exact file/definition digest,
input size and actual executable/harness hashes. Use the same build profile and
flags for comparisons. The default development profile has optimization level 1.
Warm local file reads do not measure cold disk, remote storage, concurrent load
or a production latency distribution.

Run the unchanged `core_update_latency.py` separately to measure complete
stream-update readiness and check native pricing, risk, hedges, P&L, retained
result bytes and cached/forced replay after server/worker restart. The broader
`core_capacity_workflow.py` checks partial cash receipts, checkpoints and loss of
a committed hedge acknowledgment. Native financial owners and bounds remain.

Verification borrows the snapshot content when calculating its identity. It
uses the same constructor checks, serialization and domain-separated SHA-256.
Every physical adapter read, requested identity, canonical file comparison,
schema/plan/size check and engine validation remains. File corruption, duplicate
keys, alternate encodings, missing data, symlinks and non-regular files still
reject. This requires no new port, cache, configuration, backend or provider.
