# Automatic core analytics from market streams

Run the complete local example from the repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_live_workflow.py
```

This builds the existing server/importer and starts a synthetic NDJSON feed and
one analytics worker. The book contains a stepped bond, CDS, IRS, seasoned OIS,
overnight future and term future with five explicit hedge factors. The example
checks automatic pricing/risk, constrained paper hedge proposals and full causal
P&L, then stops its processes. `--binary-dir /absolute/path/to/debug` reuses builds.
The existing financial examples, native owners and numerical fixtures are reused.

The checks cover a changed discount curve, a fixing correction, stale input, a
sequence gap, disconnect/reimage, an actual process kill after a result commits
but before its acknowledgement, and worker/server restart. Saved outputs and
recomputed risk/P&L/hedge results must match exactly. LatestKnown OIS consumes
the correction while the FirstPublished future retains its original observation.

## Run the worker

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/streaming_analytics.py \
  --feed-url https://feed.example/market --stream-id desk.usd \
  --url https://pricing.example/api/v1 --subscription subscription.json \
  --workflow workflow.json --state /absolute/path/to/desk-usd-state.json \
  --feed-token-env FEED_TOKEN --server-token-env ANALYTICS_TOKEN
```

Use the existing [stream subscription and normalized wire contract](MARKET-DATA-STREAM.md).
The worker reuses that consumer's publication, sequencing, gap and reconnect
logic. Its callback runs only after every record in an image/update acknowledges
through the existing curve/fixing ports. Invalid or unconfirmed frames cannot
start calculations. Duplicate frames and heartbeats do not recalculate or renew
market timestamps. A frame's individual port commits remain independent.

The state file's parent must exist. One Unix worker owns that receipt through an
OS advisory lock; retain the lock file alongside it. Do not start workers for the
same book using different state paths. This is local ownership, not distributed
leadership. Back up the server's redb and `.core-snapshots` directory together
with the server stopped, retaining the workflow/configuration receipt as well.

The server token needs existing market publication, analyst, reference-write and
reference-commit read grants for the selected book/data. Native calculation
admission, access scopes, entitlements, immutable identities and resource bounds
remain enforced by the existing server. Feed and server credentials are separate;
redirects/proxies are disabled and HTTP is limited to loopback. The production
server profile retains its existing release gate.

## Workflow configuration

`workflow.json` is bounded to 2 MiB, 65,536 JSON nodes and depth64. It contains
exactly these fields; `configuration()` in the runnable example constructs a
complete working instance from the retained core book and existing paper policy.

| Field | Meaning |
|---|---|
| `schema_version` | Integer1. |
| `id` | Explicit workflow/policy identity. |
| `plan` | Existing `CoreSnapshotPlan`: exact native definition revision, source/receipt cutoffs, named complete curve sets and fixing keys. |
| `bindings` | One binding per plan curve slot: `set`, `curve_id`, `source`, exact native `recipe_key`, `follow`, `max_age_ms`. |
| `risks` | Map from every native holding ID to its named `graph_hedge_risk` calculation, at most30. Native outputs include the original product price/PV and every factor calculation. |
| `pnl` | Named existing `book_pnl` calculation with full supplied causal positions, lifecycle/cash/security claims and market bindings. |
| `hedge` | Full supplied native `hedge` solve request/policy, except each holding/candidate's `risk` is a holding ID and its separate `fx` is the explicit native FX pin. These become actual returned vectors and same-snapshot FX pins before native validation/solving. |

For a current binding, `follow:true` selects the highest confirmed immutable
source-record revision for its curve/source pair. Its native recipe key must
still match the configured product/factor roles. Source-record revisions and
native curve-key revisions are different identities. A native key change blocks
the workflow until a new, explicitly rebound definition/configuration is supplied.
No automatic financial field mapping, calibration convention or policy is inferred.

`follow:false` pins a historical P&L curve version. Every curve used by the
current risk calculations must have a freshness-checked `follow:true` binding.
The worker checks this against the resolved stored definition before running
risk. It never turns a historical P&L market into a current hedge mark.
Fixing keys must exactly match the subscription; the existing snapshot assembler
retains all eligible published revisions and the native publication rules select
their actual histories. Historical fixing observation dates do not become live
quote-age requirements.

Definitions retain complete supplied valuation dates, terms, calendars, models,
holdings, causal P&L points and hedge policy. Update those definitions explicitly
when the business date, positions, policy, instruments or native curve roles
change. This worker advances source/receipt cutoffs and outer source revisions;
it does not roll contracts, manufacture cash events, change historical point
cutoffs, auto-build missing curves or infer execution instructions.

## Freshness and completion

Normal operation uses UTC wall time. Each current curve's original source and
receipt times must be no later than that clock and younger than both the binding
age limit and the source record's `max_age_ms`. Limits are1–86,400,000ms. Invalid
or stale source quality, future fixing publications/receipts, future snapshot
cutoffs, clock regression and source rollback block completion. Native snapshot
cutoff, entitlement, history and quality checks still run independently.

After input admission the worker durably saves its selection, assembles a complete
snapshot and runs all named risk calculations and P&L through existing HTTP
routes. It then binds actual vectors into the explicit hedge policy, stores that
request as a content-addressed reference definition and persists the native hedge
result through the same snapshot/result routes. There is no new pricing engine,
server endpoint, database schema, dependency, queue or scheduler.

Stdout emits `unavailable` with `ready:false` while publishing/calculating,
recovering, stopped or blocked, and `ready` only after every result commits and
all build IDs agree. Completion includes native price measures, snapshot/result
receipts, hedge status and `valid_until`. Futures quote levels retain their native
basis and reference quantities. Financial values are not recalculated in Python.
Infeasible native proposals retain their status; completion never means a hedge
is feasible or executable. Errors remain unavailable, without substituted zeros.

Freshness is rechecked between calculations, before completion and on duplicate/
heartbeat delivery. Results which expire during calculation cannot become ready.
Consumers **must enforce `valid_until` themselves** and invalidate the worker's
state when its output stream closes or stalls. A prior stdout event or local
receipt is not a current readiness lease. Idle source reads time out using
`--timeout` (default10seconds); continuous heartbeats cannot refresh data age.
There is no execution authority, including when data is fresh.

The example uses `--replay-at` with a declared historical clock because its
synthetic financial terms and source times are historical. That override requires
loopback destinations and a server response carrying the `demo` profile; every
event identifies replay mode. It does not qualify historical data as live.

## Recovery and operational scope

The atomic, fsynced local receipt records the configuration digest, admitted
source-version/cutoff selection and last complete bundle. It is a recovery hint,
never a restored ready flag or a market-data cursor. A process restart acquires
the lock and requires a complete image. It rejects rollback below its saved
selection, assembles exact inputs again and reuses the existing build-bound
durable results. Partially completed work resumes through those same caches;
lost reference/result acknowledgements replay idempotently. Native failure
envelopes remain failures even when stored. Configuration mismatch or receipt
corruption stops startup; do not silently delete the high-water marks to recover.
For an intentional configuration migration, stop the old worker, retain its
receipt, and initialize a new explicitly reviewed selection/state path.

All frames execute serially with socket backpressure and bounded request/retry
budgets inherited from the stream consumer. The worker does not coalesce updates
or introduce a background calculation queue. A slow calculation delays the next
read; emitted results identify the processed frame and expire on original input
times. This increment establishes correctness/recovery on the existing local
composition. Concrete live-provider admission, multi-server ownership and
product-specific throughput/latency measurements remain further work.

[Core book releases](BOOK-UPDATES.md) add position and reference updates through
the existing reference commit port and this same worker. Opt in with a subscribed
book identity; a complete release binds the P&L closing positions, exact native
definition and hedge policy. Run `python3 tools/core_book_workflow.py` to verify
updates, consistency rejection and recovery without rewriting the boot config.
