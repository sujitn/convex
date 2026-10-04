# File market data and quote replay

`FileQuoteReplay` delivers normalized bond quote events in recorded JSONL order. `MarketDataAssembler` in `convex-engine` validates and reconstructs them without reading a clock or starting a server. This is an offline ingestion contract; live provider certification and immutable cross-feed pricing snapshots are subsequent work.

## Run a deterministic trace

From the repository root:

```sh
cargo run --locked -p convex-engine --example quote_replay -- crates/convex-ext-file/tests/fixtures/sequenced-quotes.jsonl
```

Each output line contains the source/instrument key, session, sequence, admission outcome and resulting data state. The [expected trace](tests/fixtures/sequenced-quotes.expected.json) declares all 28 outcomes independently of the implementation. Repeating the command produces identical output. Optional arguments after the file are maximum retained keys and field freshness age in milliseconds:

```sh
cargo run --locked -p convex-engine --example quote_replay -- crates/convex-ext-file/tests/fixtures/sequenced-quotes.jsonl 1 5000
```

That second run refuses the second supplier with `capacity_exceeded` and marks retained data `gap`. The cursor reads one event per call with no background producer or timing sleeps. A production arrival-rate or latency benchmark must supply its own open-loop schedule; this example does not measure latency or certify trading.

## Quote event schema and recovery

Schema 1 is defined in [quote_event.rs](../convex-ports/src/quote_event.rs). Sequences start at 1 and are scoped to `(source, instrument, session)`. An adapter translating a partition-wide sequence must reconstruct per-key sequencing or introduce a separately declared partition contract. Reusing a partition sequence across sparsely updated instruments would report false gaps.

An envelope records the internal instrument ID, source, session, sequence, original source time and receipt time as UTC Unix milliseconds, delivery quality, entitlement state/tags, and an optional permitted hash/reference to raw evidence. These transient events do not provide a durable event journal, checkpoint, authenticated licensing decision or exactly-once delivery.

An `image` contains `set` instructions for every supplied field; omitted fields are absent. A `delta` changes only supplied fields. An omitted field or `unchanged` retains its value, source time, receipt time and supplying sequence. `clear` removes just that field. The value wire is always an exact decimal **string**, paired with a unit. Schema 1 admits bond clean prices per 100, yield decimals, spreads in basis points and face-amount sizes. It never computes a mid, yield or spread from a different field. Futures ticks/lots and other product quotations need an explicit contract extension.

```json
{
  "schema_version": 1,
  "key": {"instrument_id": "BOND-001", "source": "provider-a"},
  "session": "session-a",
  "sequence": 2,
  "source_time_ms": 1791086400100,
  "receive_time_ms": 1791086400102,
  "quality": "live",
  "entitled": true,
  "entitlement_tags": ["internal-analytics"],
  "evidence_reference": "fixture:permitted-observation",
  "event": {
    "kind": "delta",
    "fields": [
      {"field": "bid_price", "update": {"action": "set", "value": {"value": "99.125", "unit": "price_per100"}}},
      {"field": "ask_price", "update": {"action": "clear"}}
    ]
  }
}
```

| Input | Outcome and retained state |
|---|---|
| First delta, skipped sequence, explicit gap or transport lag | Require an image; retained fields stay available with Gap status |
| Complete image at or above the recovery watermark | Validate units, time and assembled prices, then replace the complete image |
| Older sequence | Ignore with OutOfOrder; never roll values backwards |
| Latest identical data retry, with a different receipt time permitted | Duplicate; never refresh original field ages |
| Changed latest sequence payload, crossed prices, invalid unit/time/schema | Invalid; retain last good fields, return the rejected decision, require an image |
| New session without Reset | Gap; explicit Reset then image is required |
| Reset with sequence 0 | AwaitingImage; prior display value stays unavailable for fresh use |
| Heartbeat | Liveness only; does not refresh observations or repair a gap |
| Revoked entitlement | Unauthorized; restoration requires an entitled image |
| Closed or Error | Closed or Invalid; no fresh data until an image |
| New key at capacity | CapacityExceeded; no new key retained, existing keys marked Gap |

The assembler exposes Live, Delayed, Indicative, Manual, Stale, Gap, Invalid, Closed, Unauthorized, AwaitingImage and Incomplete. Freshness uses every retained field's original source/receipt time. The default policy is 10,000 retained keys, maximum age 5,000 ms inclusive and allowed source-clock lead 100 ms. Bounds are configurable through `IngestionPolicy`; trading SLOs require product-specific approval. An empty or size-only quote is Incomplete. There is no buffered delta catch-up: recovery is by validated image. Session history and older-payload deduplication are not retained.

The assembler retains at most 12 fields and one accepted data envelope per key, plus a bounded supplier count index. Rejections return one decision to the caller; they do not accumulate an internal quarantine or trace queue. A caller requiring durable quarantine must persist the permitted evidence reference and decision in its chosen journal. Explicit unsubscribe/removal releases the key; reintroduction requires an image.

The listener sends only admitted Live price observations into the current calculation graph, bounds cache reads by the captured monotonic freshness deadline and removes expired entries on a 100 ms maintenance sweep, evicts suspended quote/bond caches and guards timer calculations against lost/replaced quote revisions. It refuses two suppliers for one instrument because the current graph has no source-selection policy. Shared-channel lag, closure or key-capacity overload halts all automatic calculations: quote images can reconstruct local observations, but unknown auxiliary dependencies require a new engine and full reload. There is no hot-resume method that assumes reconciliation. Complete runtime consolidation, exact dependency generations, source-selection policy and snapshot-consistent publication remain separate tasks. The present server still fails the enforced Empty-success and synthetic-curve probes; this ingestion work does not complete a production release gate.

## Resource and delivery limits

Replay accepts regular files up to 256 MiB, at most 1,000,000 records and at most 64 KiB per record including its newline. It also checks bytes consumed if the file grows. Blank/malformed/unknown-field/numeric-decimal records produce `StreamError::Failure`; the cursor then closes rather than skipping evidence. Unsupported schema numbers decode into events and receive the assembler's Invalid decision. EOF is `StreamError::Closed`, including an empty file. Files must remain immutable during replay; no concurrent-file consistency or cryptographic authentication is claimed.

Streaming ports now share `StreamReceiver<T>::recv() -> Result<T, StreamError>`; `Lagged { missed }`, `Closed` and adapter `Failure` are distinct. Quote subscriptions deliver `QuoteEvent`. The old per-type receiver wrappers and unsequenced quote update type are removed without aliases. Existing snapshot DTOs, REST v1 economic units, pure pricing and language-client calls retain their contract. Rust adapter callers migrate their subscribe signatures and handle every delivery result. Listener construction now takes graph, throttle manager, update receiver and shutdown receiver; the unused curve-builder argument is removed. Configure `with_policy` before the first event. [Pinned Tokio broadcast documentation](https://docs.rs/tokio/1.50.0/tokio/sync/broadcast/index.html) defines the underlying transient loss/closure behavior.

## Snapshot file migration

All three snapshot sources read regular files up to 16 MiB, reject unknown record fields and replace the complete cache only after successful validation. Missing files fail explicitly. A failed reload keeps the prior data with its prior timestamps; a successful reload removes keys absent from the replacement. Quote/fixing snapshots hold at most 100,000 records. Curve snapshots have at most 4,096 inputs per curve and 100,000 inputs total; curve tenors are positive, ASCII and bounded. Required instrument types are preserved rather than inferred as swaps. Calibration validity belongs to the curve construction layer.

Quote CSV requires these headers and a recorded timestamp/source. Blank optional prices mean absent:

```csv
instrument_id,bid_price,ask_price,mid_price,last_price,timestamp,source
BOND-001,99.125,99.25,99.1875,,1791086400100,file-eod
```

Curve JSON uses exact rate strings and explicit instrument types/timestamps:

```json
[{"curve_id":"USD.TEST","inputs":[{"tenor":"2Y","rate":"0.04","instrument_type":"Swap","timestamp":1791086400100,"source":"file-eod"}]}]
```

Fixing CSV is implemented rather than returning an empty placeholder:

```csv
index,date,rate,timestamp,source
SOFR,2026-10-02,0.04,1791086400100,file-eod
EURIBOR-3M,2026-10-02,0.025,1791086400100,file-eod
```

Index names SOFR, ESTR, SONIA, EURIBOR-tenor and TERM-SOFR-tenor map to native index types; other bounded names retain their explicit Other identity. Fixing ranges are inclusive and date-ordered; duplicate index/date records fail. This file snapshot does not provide fixing revision history or known-at filtering.

Convert previous curve numeric rates to exact strings and supply authoritative instrument types, timestamps and source names. Previous quote files also need recorded timestamps and sources. There is no fallback that invents their provenance. An intentionally empty snapshot uses a header-only CSV or `[]` curve JSON; demo/test composition can explicitly use the existing empty sources.
