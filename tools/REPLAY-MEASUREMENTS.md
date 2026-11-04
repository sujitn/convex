# Measuring the core workflow

Run the supplied mixed-book benchmark from the repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_replay_benchmark.py --output measurements.json
```

It builds one measurement executable with the existing optimized `ci` profile,
uses temporary local redb/file adapters through the existing ports, and removes
the temporary stores afterwards. There is no server, network connection, market
provider, execution permission or persistent telemetry. `--quick` selects four
common cases; `--samples 7 --warmup 1` are the defaults. `--binary PATH` reuses an
explicit executable. `--workload-directory NEW_DIR` retains exact per-case inputs
and raw results; otherwise the report retains their hashes and the pinned generator
and fixture reproduce them. Each case runs in a separate process, sequentially.

The report includes raw monotonic nanosecond samples, median/min/max, input and
output sizes, executable/input hashes and host/compiler context. It does not
derive p99 estimates from a handful of samples or enforce a CI timing threshold.
Supplied binaries need their own build provenance; the report does not infer a
profile from an arbitrary path.

## Workloads and checks

The retained six-instrument book supplies full native bond, CDS, stepped IRS,
seasoned OIS, overnight future and term future economics, curves, fixings and
publication rules. The benchmark creates 6/12/24/30/36 distinct legal trade lots
from these templates, with 0/12/120 split coupon receipts per bond. A USD1m face
bond holding preserves a real matured coupon claim while actual signed USD1
receipts settle it in parts. Separate 512/1024-receipt cases stress history growth.
Large split-receipt counts are explicitly stress cases. These are synthetic lots
over six economic templates, not a live desk inventory or measured production
distribution. Other currencies, long OIS daily-fixing histories, issuer defaults,
physical-delivery CTD and deployment/network contention need their own workloads.

Every measured case must pass native/managed pricing, risk and P&L parity. Full
and compact checkpoint windows must produce identical financial output bytes, and
reopened stores must return and recompute the original result. Receipt identities,
revisions, signs, publication rules and original financial controls remain supplied.
Invalid financial inputs abort measurement. Capacity rejections are reported with
their phase and diagnostic, rather than counted as fast successful calculations.
Existing event, JSON/node, definition, calculation-count and result bounds remain.

The stages separate:

- bounded native decode, typed execution and full native JSON execution;
- fixed-bond lifecycle replay with the original shared resources and a diagnostic
  request omitting its unused publication rules, with exact financial parity when
  both requests are admitted;
- first snapshot publication, repeated assembly, adapter read and application
  validation, plus in-memory snapshot verification and shared-reference decoding;
- P&L recomputation and cached acknowledgments, the full sequential risk book,
  native risk computation, adapter/result reads and in-memory result verification;
- full versus compact checkpoint sizes/recomputation and store reopening.

Typed lifecycle execution includes resource/contract construction, native replay
and result-Value construction. JSON execution additionally includes ingress and
serialization. It is not an isolated transition-instruction benchmark. Timings
include drops of temporary objects; output hashes/parity checks run outside the
timed intervals. Store reopening uses warm OS caches and is not a cold-disk or
machine-reboot test. The application timings exclude HTTP/auth/network overhead.
Recomputation forces native calculation on the same immutable snapshot; repeated
result persistence is idempotent and can avoid a new disk write. It does not
measure a new market tick's ingestion or full streaming/hedge-solving latency.

## Findings and decisions

The common short-history cases spend substantially more time validating,
materializing and serializing snapshots/results than replaying the fixed bond.
The shared publication schedule alone has 2,002 entries. Every named calculation
previously decoded that same reference object again during snapshot validation.
The implementation now decodes it once per validation call and gives each
calculation its own owned inputs. All calculation, freshness, coverage, digest,
checkpoint and native guards still run. Nothing is cached across requests.

On the measured arm64 macOS host with Rust 1.97.1 and the optimized `ci` profile,
seven samples after one warmup gave the following medians. Each row uses identical
inputs before and after the shared-reference change. Sizes are compact UTF-8 JSON
in decimal kB; P&L size is one complete native output, not every stored result.

| Trades | Receipts/bond | Snapshot kB | P&L kB | Cached P&L ms, before → after | Full risk book ms, before → after |
|---:|---:|---:|---:|---:|---:|
| 6 | 0 | 342.0 | 520.9 | 12.52 → 9.94 | 74.09 → 58.32 |
| 6 | 12 | 354.3 | 546.1 | 12.84 → 10.75 | 74.81 → 57.49 |
| 6 | 120 | 466.2 | 774.7 | 16.55 → 13.85 | 85.12 → 70.42 |
| 12 | 0 | 512.1 | 820.6 | 20.83 → 16.49 | 209.06 → 151.77 |
| 12 | 12 | 536.9 | 871.1 | 21.32 → 16.42 | 211.60 → 155.58 |
| 12 | 120 | 760.5 | 1328.1 | 29.20 → 23.93 | 259.55 → 194.57 |
| 24 | 0 | 1003.8 | 1420.1 | 39.59 → 28.93 | 734.26 → 490.73 |
| 24 | 12 | 1053.4 | 1521.0 | 41.33 → 30.47 | 752.96 → 507.34 |
| 6 | 512 | 873.9 | 1608.9 | 30.72 → 27.47 | 141.84 → 123.66 |

All 117 risk/P&L output digests across the nine accepted cases stayed identical;
checkpoint and reopened-store checks also passed. A separate four-case repeat
confirmed the common-case improvement. Timings are observations, with raw samples
and build provenance retained in qualification evidence; they are not SLA bounds.
Other stages still have allocation, hashing, serialization and full-validation
costs. Each named risk run still validates the whole definition, so larger books
repeat work; this increment does not introduce a batch execution API or cache.

Eight of the 17 cases hit existing limits. The 24-trade/120-receipt case, every
30-trade case and the 6-trade/1024-receipt case exceed bounded definition capacity
(65,536 JSON values and/or 1 MiB). The 36-trade cases exceed 32 named calculations.
The full-resource 1024-receipt bond request also exceeds native JSON capacity.
These limits remain explicit; none of these cases counts as a successful core run.

The six-trade cases isolate the following fixed-bond replay medians. The two
typed columns include native replay/result construction; the diagnostic JSON
column adds bounded ingress and serialization. The diagnostic omits only unused
publication rules, preserving output bytes wherever the full request is admitted.

| Bond events | Shared-resource typed ms | Diagnostic typed ms | Diagnostic JSON ms |
|---:|---:|---:|---:|
| 4 | 0.179 | 0.029 | 0.098 |
| 16 | 0.256 | 0.100 | 0.275 |
| 124 | 1.740 | 1.577 | 2.723 |
| 516 | 19.012 | 17.823 | 22.535 |
| 1028 | Capacity rejected | 68.192 | 75.753 |

The last row is a standalone diagnostic, not an admitted managed mixed book.

Long receipt histories do make immutable native state replay more expensive, but
the definition and native ingress limits also constrain these books. Storage
sharing reduces snapshot bytes; it does not remove native replay and can increase
materialization time. For example, the 512-receipt checkpoint snapshot shrank from
876.2 kB to 608.4 kB, while its full P&L acknowledgment took longer with sharing.
Keep it opt-in and measure its cost for the actual book.

Do not raise limits, drop audit history, remove result verification or introduce
computed-state restoration on the strength of this microbenchmark. Larger desks
need explicitly bounded books and coherent aggregation/update requirements; this
increment does not silently partition a portfolio. Product-specific latency,
throughput, concurrent update and recovery targets remain to be agreed and tested
on the intended deployment. Local warm-cache timings do not certify colocation
or production latency. The broader library catalogue is not a prerequisite for
these measurements.
