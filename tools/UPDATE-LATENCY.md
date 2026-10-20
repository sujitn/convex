# Core update measurements

For optimized binaries, follow the [release-build measurements](RELEASE-LATENCY.md).

Build the existing executables, then run the local measurement:

```sh
cargo build --locked -p convex-server --bins
cargo build --locked -p convex --example book_capacity_native
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_update_latency.py \
  --binary-dir target/debug --output /tmp/convex-update-latency.json
```

The script reuses the [larger-book example](BOOK-CAPACITY.md): 36 trades with
12 receipts per bond and 60 trades with no additional coupon receipts. Both
contain bonds, CDS, IRS, OIS and overnight/term futures. They are distinct legal
lots over six synthetic economic templates. No provider or backend is selected
by the measurement code.

Each book has one warmup and three measured curve updates (`--samples 3..20`).
The timer starts before publication to the existing ordered feed and stops at
the worker's ready receipt, after durable pricing/risk, P&L and hedge results.
Alternating complete supplied discount curves must actually reprice the live
IRS. A local forwarding proxy measures the existing HTTP calls. Its overhead
is included in the complete update, and the same proxy is used for comparisons.
Stage times include server work and transport; their sum does not measure
exclusive CPU time. Native comparisons run outside the timed intervals.

The JSON retains every sample, HTTP status, request/response size, captured
snapshot identity, initial definition hash, native result digests and executable
hashes. It separately probes snapshot reads and cached/forced risk runs. A
restarted server and worker must recover every final durable receipt, preserve
all earlier result bytes and reproduce every final result from saved inputs.
Failed or rejected work cannot count as successful latency.

Run before and after builds with the same profile, flags and harness. Preserve
both executable sets. The default development profile uses optimization level
1; release results are a separate experiment. Three samples describe only this
local sequential experiment: use the median and observed range, not a claimed
p99 or SLA. The explicit historical replay clock, demo access, warm local disk,
single worker and synthetic books do not measure live freshness, authenticated
ingress overhead, concurrent load, production tails or colocation performance.

The broader [capacity workflow](BOOK-CAPACITY.md) retains partial-cash,
checkpoint, committed-hedge acknowledgment loss and restart checks. Existing
streaming tests cover stale data, gaps and reconnect. Those correctness checks
remain necessary independently of these timing samples.

The server remembers one successfully validated snapshot digest per workflow
instance. Each snapshot request still reads the adapter and verifies content
against its requested identity. A new digest runs all existing reference,
curve/fixing, history, checkpoint and named-calculation checks; failed validation
is never remembered. This avoids decoding every trade's contracts for every
other trade's risk request. It retains no inputs or financial results, needs no
configuration and starts empty on restart. Other books can replace the one
remembered identity safely. Live freshness and authorization remain separate
checks on every update/request. Native pricing and forced recomputation still
run through their original owners. Snapshot reading, hashing, JSON transport and
durable result checks remain costs to measure before further changes.
