# Market-data adapter

For sequenced HTTP streaming, gap detection and image-based reconnect/restart,
use the [stream adapter](MARKET-DATA-STREAM.md). It shares this adapter's payload
validation and existing port publication path.

Run the complete synthetic ingestion, analytics and recovery example from the
repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_ingestion_workflow.py
```

Python 3.10+ and the repository Rust toolchain suffice. The command builds the
existing server and reference importer, starts isolated local demo servers and
stops its processes. `--binary-dir /absolute/path/to/debug` reuses both binaries.
It retains the existing full bond/CDS/IRS/OIS/rate-futures book, native five-factor
risk, constrained paper hedge and causal P&L checks. Every market input enters
through the adapter while the server runs; only reference terms use the existing
offline historical import. The example also covers duplicate delivery, a committed
prefix followed by rejection, a conflicting fixing batch, and adapter/server
restart with unchanged snapshot identities, saved results and recomputation.

## Publish a capture

For an already configured server:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/market_data_adapter.py market.jsonl \
  --url http://127.0.0.1:8080/api/v1
```

Authenticated deployments use HTTPS and an environment variable supplied by the
operator's secret mechanism:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/market_data_adapter.py market.jsonl \
  --url https://pricing.example/api/v1 --token-env MARKET_DATA_TOKEN
```

The adapter requires the existing `ReferenceWriter` role, market data scope and
curve/source wildcard grants. It grants no calculation, book or execution
authority. HTTP is accepted only on loopback. Redirects and inherited proxies
are disabled so input and credentials go only to the explicit destination.
Tokens are never command-line values or diagnostics. Network/server errors
produce a safe JSON failure on stderr and a nonzero exit; success is JSON on
stdout after every publication has returned HTTP 204. Per-record timeout is
10 seconds by default; `--timeout` accepts 0.1–120 seconds.

## Normalized input

Each JSONL line has exactly these keys:

```json
{"schema_version":1,"kind":"curve","payload":{}}
{"schema_version":1,"kind":"fixings","payload":[]}
```

These lines illustrate the envelope only. Supply a complete
[`SnapshotCurveRecipe`](../crates/convex-ports/src/snapshot.rs) for `curve`, or a
nonempty array of full [`PublishedFixing`](../crates/convex-ports/src/fixing.rs)
revisions for `fixings`. Empty payloads are not usable market data. The runnable
example generates valid complete records from the existing synthetic book.

The adapter forwards those payloads to `POST /api/v1/analytics/curves` and
`POST /api/v1/analytics/fixings`. Those handlers already call the existing
`CurveStore` and `FixingRepository`; all server validation, permissions, immutable
conflicts and durable acknowledgements apply. Snapshot assembly and calculations
continue through the existing core workflow endpoints. No new server route,
database schema, queue, cursor store or pricing dependency is introduced.

Provider-specific connectors normalize their feed outside analytics. They must
resolve internal instrument/index/curve identities and retain exact native curve
keys, complete construction/helper economics, units, source-record versions,
source/receipt/publication times, quality, preliminary/final status and
entitlement. Financial decimal fields stay decimal strings. The adapter makes
no date/unit conversion, identifier mapping, source selection, version allocation,
quality promotion or missing-data substitution. The server/native owners retain
semantic validation. Complete curve recipes may contain the original explicit
calibration helper quotes; the adapter does not flatten or rebuild them.

This adapter consumes complete curve records and published fixing revisions.
Sequenced quote images/deltas retain their existing `QuoteEventSource` and
`MarketDataAssembler` contracts. An SDK, socket or broker connector can publish
the same normalized resource payloads at the existing boundary after handling
its own subscription, sequence, entitlement and reconnect requirements. File
replay does not establish live-feed continuity or measured low-latency behavior.

## Recovery and bounds

The adapter freezes and preflights the entire bounded file before network I/O.
It rejects malformed/duplicate-key/nonfinite JSON, unknown envelope fields,
unsupported kinds and excessive nesting. Maximums are 32 MiB per file, 10,000
records, 2 MiB per record/payload and 65,536 values/depth 64 per payload. The
server's existing native/request limits still apply. It does not tail a growing
file or run a hidden background producer.

Publication is ordered and stops at the first rejected or unconfirmed record.
Earlier acknowledged records remain committed. The failure reports its line,
HTTP status when known and the count acknowledged before the failure. A lost
response can follow a real commit; that record is deliberately unconfirmed.
Resolve the cause and replay the identical capture from the beginning. Existing
immutable identities make exact retries idempotent and conflicting retries fail.
There is no local checkpoint to lose or get ahead of the server. Corrections
use new original source-record revisions; never rewrite committed history.

Each fixing batch is atomic. A whole capture is a sequence of independent port
commits. Assemble the explicit snapshot only after the required publications
have acknowledged; missing inputs prevent a partial snapshot from being accepted.
The example verifies that rejected batches do not alter the accepted snapshot.

The executable still uses the documented local redb/Unix snapshot composition.
Live vendor admission, production startup, distributed ownership, licensed-data
integration and product-specific latency retain their existing release gates.
