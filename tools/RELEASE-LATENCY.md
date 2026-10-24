# Release-build core update measurements

Build and measure the existing mixed-book workflow:

```sh
export CARGO_INCREMENTAL=0 RUSTFLAGS=-Dwarnings RUSTDOCFLAGS=-Dwarnings
cargo build --locked --release -p convex-server --bins
cargo build --locked --release -p convex --example book_capacity_native
cargo build --locked --release -p convex-engine --example core_snapshot_io
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_update_latency.py \
  --binary-dir target/release --output /tmp/convex-release-updates.json
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_snapshot_io.py \
  --binary-dir target/release --output /tmp/convex-release-snapshot-io.json
```

If `CARGO_TARGET_DIR` is set, use its `release` directory in `--binary-dir`.
The repository release profile uses optimization level 3, full link-time
optimization, one code generation unit, stripped symbols and aborting panics.
Do not replace those settings with a faster build profile when comparing results.

On macOS/Linux ARM, the existing `sha2` dependency enables its accelerated
backend with CPU feature detection and its software fallback. Other targets
retain their original dependency features. This adds the locked `sha2-asm`
package, built with the existing C/assembly toolchain. It changes no hash domain,
canonical bytes, snapshot/result identity, financial formula or stored schema.
No integrity check, physical read, authorization or freshness check is skipped.

The unchanged [update harness](UPDATE-LATENCY.md) measures 36 trades with 12
receipts per bond and 60 trades with no additional coupon receipts. Both books
contain bond, CDS, IRS, OIS and overnight/term-futures lots. Each sample alternates
the actual supplied discount curve and requires a changed live IRS valuation.
The timer runs from publication on the existing feed to durable worker readiness,
including pricing/risk, P&L, hedge proposals, file/redb storage and HTTP transport.
The local observation proxy is also inside the timed path.

Native financial comparisons run outside the timed interval. Every saved output
must survive server and worker restart byte-for-byte, recover from the existing
result port, and match forced recomputation using the saved inputs. The separate
[snapshot diagnostic](SNAPSHOT-IO.md) retains exact file bytes, identities and
individual I/O, JSON and integrity stage samples. Stage medians are not additive.

These are sequential synthetic historical/demo runs on warm local storage.
Report the median and observed range; the samples do not establish p99, an SLA,
live freshness, authenticated or concurrent ingress, remote-storage or colocation
performance. Failed work is never counted as successful update latency.

The broader [capacity workflow](BOOK-CAPACITY.md) exercises partial cash receipts,
compact checkpoints, loss of a committed hedge acknowledgment and restart. Those
correctness checks remain necessary alongside the timing measurements.
