# Streaming core market inputs

Run the synthetic core book with a real local NDJSON feed:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_streaming_workflow.py
```

The command builds the existing server/importer, starts its own feed and adapter,
checks the full bond/CDS/IRS/OIS/rate-futures workflow, then stops its processes.
`--binary-dir /absolute/path/to/debug` reuses the server/import executables.
It verifies updates, a skipped sequence that must not reach storage, a complete
image after reconnect, duplicate/heartbeat behavior, adapter restart, server
restart and identical saved risk/P&L/hedge results and recomputation.

## Run an adapter

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/market_data_stream.py \
  --feed-url https://feed.example/market \
  --url https://pricing.example/api/v1 \
  --stream-id desk.usd --subscription subscription.json \
  --feed-token-env FEED_TOKEN --server-token-env MARKET_DATA_TOKEN
```

The two optional token environment variables belong to separate destinations.
HTTPS is required except on loopback; redirects and inherited proxies are
disabled. Existing server publication roles/scopes still apply. All semantic
financial validation and immutable curve/fixing storage remain with the existing
ports. There are no additional server routes, backend tables or pricing formulas.

## Subscription and wire contract

The bounded local subscription selects exact source-separated resource keys:

```json
{"schema_version":1,"curves":[{"curve_id":"USD.OIS","source":"rates-feed"}],"fixings":[{"index_id":"USD.SOFR","observation_date":"2026-10-16","source":"official-fixings"}]}
```

At most 256 unique curve/source pairs and 1,024 unique fixing keys fit in a
256 KiB subscription file. The producer's configured feed URL/stream ID selects
that same universe. Every connection uses an HTTP GET with
`Accept: application/x-ndjson`, `X-Convex-Stream: <stream-id>` and
`X-Convex-Require-Image: true`. HTTP200 and the NDJSON content type are mandatory.
The first accepted data frame must be a complete image for the subscription.

Each newline-terminated frame contains exactly:

```json
{"schema_version":1,"stream_id":"desk.usd","session":"connection-A","sequence":1,"kind":"image","records":[]}
```

This illustrates the envelope; an image's records must cover the complete
subscription. Records use the existing [normalized curve/fixing envelope and
native payloads](MARKET-DATA-ADAPTER.md#normalized-input). Multiple immutable
revisions of a subscribed resource can be supplied, retaining its full history.
An `update` frame contains a nonempty subset of subscribed resource records and
advances the data sequence by exactly one. A `heartbeat` has empty records and
repeats the current data sequence. Neither heartbeat nor duplicate delivery
publishes inputs or refreshes their original source/receipt/publication times.

Sequence numbers are positive unsigned64 values, scoped to the declared stream
session. An exact repeat of the latest data frame is a duplicate. Different
content at that same sequence is a fatal conflict. A later complete image in the
same session can explicitly reestablish a complete source state at its watermark.
An update with a skipped/older sequence, a changed session, or a connection
starting with an update causes reconnection before that update can be published.
Unknown resource keys, incomplete images and malformed frames fail explicitly.
The adapter validates the entire frame before issuing any port write.

These are streams of complete curve records and published fixing revisions.
Existing `QuoteEventSource`/`MarketDataAssembler` retain ownership of quote
image/delta assembly. Vendor-specific identifiers, quote normalization,
subscriptions and entitlements stay outside analytics. Providers must supply
genuine versions, units, times, quality, full curve/helper economics and fixing
history; the adapter supplies none of those financial defaults.

## Continuity and recovery

The adapter publishes each image/update through the existing immutable
CurveStore/FixingRepository HTTP boundary. It advances its confirmed sequence
only after every record acknowledges. A partial/unconfirmed publication closes
the connection and requests a complete image; already committed records replay
idempotently. Permanent permission/input/immutable conflicts stop the adapter.
Fixing batches retain their existing atomicity; an entire frame is a sequence of
independent commits. Assemble a current snapshot only after image confirmation.

EOF, truncated delivery, idle/read errors and sequence gaps emit `recovering`
with `image_confirmed:false`. A reconnect always starts in `awaiting_image`.
There is no durable cursor to get ahead of stored inputs: every connection and
every process restart starts with a complete image. Saved historical snapshots
and results remain available during recovery; incomplete delivery does not
create a new accepted snapshot automatically.

Stdout contains bounded JSON state/acknowledgement records; stderr contains safe
failures without tokens or provider payloads. `image_confirmed` describes
transport continuity, not pricing freshness, analytics/strategy readiness or
execution authority. Native snapshot cutoffs/age/entitlement checks still apply.
Graceful termination, retry exhaustion and fatal errors report `stopped` with
confirmation false. Shutdown may leave an unconfirmed commit; replay is safe.

One pull loop applies backpressure while port writes complete. There is no
background data queue. Frames are at most 16 MiB, each native record at most
2 MiB, with at most 10,000 records/frame and one million wire frames/run. Default
read/publication timeout is 10 seconds, reconnect budget 10 and delay 0.25 seconds.
Options allow timeout0.1–120seconds, reconnects0–1000 and delay0.01–60seconds.
`--max-frames N` is an explicit finite demonstration/test stop after N confirmed
data frames. Resource ceilings are not measured capacity or latency targets.

The example uses synthetic data and the existing local Unix/redb composition.
Live vendor admission, distributed ownership, production release and measured
product-specific latency retain their existing qualification requirements.

To trigger native pricing/risk, hedge proposals and causal P&L after confirmed
frames, use the [automatic core worker](STREAMING-ANALYTICS.md).
`PYTHONDONTWRITEBYTECODE=1 python3 tools/core_live_workflow.py` checks automatic
updates, stale-data gating and interrupted-work/server restart recovery.

A subscription may also include one `book` identity. Its complete images then
require exactly one [book release](BOOK-UPDATES.md) along with market coverage.
Book records select an already committed reference revision for the analytics
worker; they do not issue market writes. The file adapter remains curve/fixing
only. Market-only subscriptions keep their existing wire and restart behavior.
