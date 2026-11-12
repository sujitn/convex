# Convex server configuration

`convex-server` (`crates/convex-server`, binary `convex-server`) is the Convex pricing server: an HTTP and WebSocket service over the library, the engine and local redb storage. This page covers how to run and configure it and what its routes do. The crate is not published to crates.io; build it from this repository.

The [issuer-default workflow](../../tools/ISSUER-EVENTS.md) reuses this server's
reference, curve, fixing, snapshot and result ports for bonds/CDS, risk, hedges
and P&L, including atomic book updates and restart recovery.

The server has an explicit demo profile and a loopback-only historical replay mode, which takes an immutable plan and a canonical quote file. Production is the default profile. It runs the Live runtime of a stored, activated configuration. It starts only with access control configured (policy file and introspection) behind the `proxy` TLS boundary. Otherwise it names the first missing setting and fails before opening storage. Live valuations are display-only; the official durable results are the core-workflow snapshot runs. Demo responses contain demonstration inputs and are not production data. The [runtime guide](../convex-engine/RUNTIME.md) covers replay, readiness, canonical results and joined recovery.

Authentication defaults to required verification. The [access guide](ACCESS.md) documents authenticated replay, exact role/book/data grants, identity/secret rotation, TLS proxy boundaries, quotas and distinct health states. For a local demonstration, run from the repository root:

```sh
mkdir -p data
CONVEX_PROFILE=demo CONVEX_HOST=127.0.0.1 CONVEX_ACCESS_MODE=local_development cargo run --locked -p convex-server
```

To select a TOML file, supply its path after the binary name:

```sh
cargo run --locked -p convex-server -- /path/to/settings.toml
```

For example:

```toml
profile = "demo"
host = "127.0.0.1"
port = 8080
websocket_enabled = true
storage_path = "./data/convex.redb"

[access]
mode = "local_development"
```

## One core book from pricing to P&L

From the repository root, with Python 3.10+ and the repository's Rust toolchain:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_workflow.py
```

This command builds the existing server, starts it on a temporary loopback port,
runs the book, stops it, restarts against the same temporary storage, checks an
identical same-snapshot P&L response, and stops it again. It needs no Python
packages, external feed, account or manually started service. Use
`--binary /absolute/path/to/convex-server` to reuse a build, or
`--pnl-output /tmp/core-pnl.json` to retain the complete P&L envelope.

The six trades are a cash bond, stepped/amortizing IRS, seasoned OIS,
single-name CDS, overnight rate future and term rate future. The example selects
complete terms, calendars, history, curves, positions, FX and cash events from
the synthetic causal-book fixture. It sends every financial calculation
through the server: prices; discount, term-projection, overnight-projection,
hazard and recovery risk; a constrained bond/IRS/CDS/futures hedge proposal; and
causal P&L split into market, FX, position and event changes. It also removes a
required discount curve and checks the `missing_input` error.

Output includes the input fingerprint, quantity/reference units, ordered factor
keys, before/after risk, proposed trade quantities and P&L reconciliation
residuals. Futures monetary quote levels and currency present values retain
separate reporting groups. A feasible hedge can retain residual exposure. The
example's eligibility, liquidity limits and zero costs are explicit synthetic
paper assumptions; its proposal does not execute orders or post cash.

### HTTP contract

`POST /api/v1/analytics` accepts the existing
[`LibraryRequest`](../convex/src/library/requests.rs) JSON and returns the
unchanged `convex::library::execute_json` envelope. Product construction, curve
validation/calibration, pricing, risk, hedging and P&L stay in their existing pure
owners. The request carries all of its inputs; this endpoint performs no provider
or repository lookup and saves no calculation state.

Native success is `{"ok":"true","result":...}`. Native calculation or input
failure is `{"ok":"false","error":...}` at HTTP 200; always inspect `ok`.
Transport/authentication failures use HTTP error statuses. The existing HTTP
boundary limits requests to 2 MiB and results to 4 MiB; an oversized result
returns HTTP 413 with `ANALYTICS_RESPONSE_TOO_LARGE`. Work runs on the blocking
pool, and a request is rejected at once when the engine's `max_concurrent_calcs`
limit (default 4, maximum 64 per router) is reached; that returns HTTP 429.
A cancelled request keeps its calculation and access slots until the worker exits.

The default router denies access. An authenticated deployment requires the
`Analyst` calculate role, all four `Reference`, `Market`, `Books`, `Results` data
scopes, and explicit `*` resource grants for instruments, curves, sources and
books. Named-only grants cannot use this endpoint, because a request can touch any
instrument, curve, source or book; per-operation entitlement discovery is not implemented. Current grants are
checked before dispatch and again before returning the result. Existing origin,
identity and request quotas also apply. Only the isolated demo command selects
`local_development` access.

The managed replay runtime still covers fixed-bullet bonds only, and the production
profile runs only a stored, activated configuration behind required access. The
example uses rate futures; delivery-basket and cheapest-to-deliver operations are
available through the same endpoint. Not yet covered: live provider feeds,
distributed ownership and measured latency.

## Core snapshots, input updates and restart recovery

Run the persisted version of the same six-trade workflow:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_snapshot_workflow.py
```

It imports captured synthetic reference revisions, whole curve recipes and fixing
history through the existing ports, then uses the server to assemble immutable
snapshots and persist 20 results. It verifies that they match the results from the
`/api/v1/analytics` endpoint, selects updated reference/curve revisions, and publishes
a SOFR correction. OIS consumes the eligible correction; the overnight future
retains its FirstPublished revision. After stopping and restarting the
server, it reads every stored result and recomputes pricing, P&L and the hedge
from saved inputs with identical outputs. Both processes stop automatically.

The [core persistence contract](../convex-engine/CORE-WORKFLOW.md) describes the
input definitions, endpoint shapes, backend responsibilities and recovery rules.
`core_workflow.py` remains the smaller example.

| Endpoint | Behavior |
|---|---|
| POST /api/v1/analytics/curves | Store one immutable complete `SnapshotCurveRecipe` |
| POST /api/v1/analytics/fixings | Atomically store a batch of immutable `PublishedFixing` revisions |
| POST /api/v1/analytics/snapshots | Resolve a `CoreSnapshotPlan` through reference/curve/fixing ports and publish the complete captured record |
| GET /api/v1/analytics/snapshots/{snapshot_id} | Read the saved definition, full recipes, history and lineage |
| POST /api/v1/analytics/snapshots/{snapshot_id}/run/{calculation} | Run a saved named calculation; `{}` permits reuse, `{"recompute":true}` uses saved inputs again |
| GET /api/v1/analytics/results/{result_id} | Return the exact stored result envelope without recalculation |

Reference definitions use the existing reference transaction/history contract's
`calculation` master resource. Input publishers need `ReferenceWriter`, market
scope and explicit curve/source wildcard grants. Calculation definitions and
their reference receipts also require the core book/data grants. Core calculations
retain the earlier Analyst/full-grant requirement; saved reads require Reader or
Analyst with all those data/resource grants. Source publication and calculation
are separate permissions. Current grants are rechecked before returning results.

The executable keeps references, curves, fixings, core and market snapshots, and
replay results in the one redb database at `storage_path`, on Linux and Windows.
Back up that file with the server stopped; `convex-redb backup` and `restore` run on
Unix only. Startup refuses `<storage_path>.core-snapshots`, `<storage_path>.snapshots`
or `<storage_path>.results.jsonl` left by an earlier build. Remove them, or keep
running that build against the database. Other hosts can supply implementations of the same existing
ports. Unsupported adapter methods fail explicitly. The offline
`convex-core-inputs DATABASE CAPTURED_INPUTS.json` importer also requires a stopped
server; interrupted imports recover by retrying the identical captured records.

This is a synthetic, persisted core workflow. Under the production profile its
snapshot runs are the official durable results, while live valuations stay
display-only. Not yet covered: vendor feeds, distributed ownership and measured
latency.

The [provider-neutral market-data adapter](../../tools/MARKET-DATA-ADAPTER.md)
publishes normalized JSONL curve/fixing captures through these existing write
ports. It needs no additional server configuration or backend. Run
`PYTHONDONTWRITEBYTECODE=1 python3 tools/core_ingestion_workflow.py` for the same
core book with adapter-delivered inputs, updates, duplicate/interrupted delivery
and adapter/server restart checks. Provider parsing stays outside analytics;
immutable resource replay supplies recovery without an additional checkpoint.

The [streaming adapter](../../tools/MARKET-DATA-STREAM.md) accepts sequenced
NDJSON images/updates through the same curve/fixing ports. A gap or disconnect
requires a complete subscribed image before confirmation resumes. Run
`PYTHONDONTWRITEBYTECODE=1 python3 tools/core_streaming_workflow.py` to verify
the core book with gaps, reconnects and adapter/server restart recovery.

The [automatic core worker](../../tools/STREAMING-ANALYTICS.md) triggers stored
pricing/risk, constrained hedge proposals and causal P&L on confirmed
stream frames. It uses exact configured input bindings and the existing ports,
with freshness gating and restart recovery. Run
`PYTHONDONTWRITEBYTECODE=1 python3 tools/core_live_workflow.py` for the complete
synthetic example, including stale input and interrupted native-result recovery.

[Complete book releases](../../tools/BOOK-UPDATES.md) add position and reference
updates through the same reference/snapshot/result ports and worker. They bind
current risk and hedge inputs to one complete native P&L closing point. Run
`PYTHONDONTWRITEBYTECODE=1 python3 tools/core_book_workflow.py` for fill/reference
updates, consistency rejection, commit faults and restart recovery.

The [event adapter](../../tools/BOOK-EVENTS.md) accepts fills, lifecycle and
cash facts through those same ports and publishes the existing book record after
P&L validation. Run
`PYTHONDONTWRITEBYTECODE=1 python3 tools/core_event_workflow.py` for an IRS fill,
official futures settlement, partial/completing cash receipts and restart recovery.

The [lifecycle risk binding](../../tools/LIFECYCLE-RISK.md) adds remaining
exposure and outstanding owned cash claims to those same snapshots.
Run `PYTHONDONTWRITEBYTECODE=1 python3 tools/core_lifecycle_workflow.py` for a
50% OIS termination, partial/completing receipts, five-factor hedges, causal P&L
and recovery after an interrupted hedge result. New hedge trades retain fresh
instrument risk; current claims stay with their owner.

Run `PYTHONDONTWRITEBYTECODE=1 python3 tools/core_cds_workflow.py` for
[CDS default and recovery](../../tools/CDS-EVENTS.md). It verifies unpriced
default blocking across restart, recovery claim risk, defaulted-entity candidate
exclusion, cash/P&L reconciliation and interrupted-result recovery.

## Loading and validation

Configuration precedence is compiled defaults, selected TOML file, then environment overrides. With no explicit path, the server checks `config/convex.toml`; an absent default file permits environment-only startup. A missing explicit path, unreadable file, invalid setting, unknown TOML key or unknown `CONVEX_` environment variable fails startup. Each supplied file must itself be valid before overrides apply. TOML input is limited to 64 KiB.

These process settings require a restart. Versioned quantitative/integration/policy settings use the separate [configuration lifecycle](../convex-engine/CONFIGURATION.md), including strict schema, numerical validation, staging, compare-and-swap activation, expiring freshness exceptions and retained rollback. No inline secret-valued setting is supported. Startup inspection redacts paths, and configuration errors omit submitted values and file contents.

| TOML key | Environment variable | Behavior |
|---|---|---|
| profile | CONVEX_PROFILE | `production` by default: needs `configuration_id`, `access.mode = required` with policy and introspection, and `access.tls_boundary = proxy`. Explicit `demo`, or loopback `replay` or `live`, start locally |
| host | CONVEX_HOST | IPv4/IPv6 literal; default `0.0.0.0` must be changed to loopback for the current HTTP executable; hostnames fail |
| port | CONVEX_PORT | Integer 0–65535; default 8080; zero selects an ephemeral port |
| websocket_enabled | CONVEX_WEBSOCKET_ENABLED | Boolean; environment accepts true/false or 1/0, case insensitive; default true |
| websocket_port | CONVEX_WEBSOCKET_PORT | Optional; accepted only when WebSockets are enabled and it equals the HTTP port |
| storage_path | CONVEX_STORAGE_PATH | Nonempty local redb path; default `./data/convex.redb`; parent directory must exist |
| runtime_plan | CONVEX_RUNTIME_PLAN | Required explicit plan for replay; for live, this or `configuration_id`; fixed-bullet mode, exact build, no seed events |
| configuration_id | CONVEX_CONFIGURATION_ID | Production (required), live (instead of `runtime_plan`) or demo: run the stored active configuration with this id live; under demo, Demo-quality inputs are valued and labelled |
| quotes_file | CONVEX_QUOTES_FILE | Required canonical JSONL replay in replay mode; unsupported in other modes |
| curves_file | CONVEX_CURVES_FILE | Rejected: file provider composition is not implemented |
| fixings_file | CONVEX_FIXINGS_FILE | Rejected: file provider composition is not implemented |
| bonds_file | CONVEX_BONDS_FILE | Rejected: file provider composition is not implemented |
| admin_ui_dir | CONVEX_ADMIN_UI_DIR | Optional built admin app served at `/admin`; must hold `index.html` |
| limits.clear_hold_seconds | CONVEX_LIMITS_CLEAR_HOLD_SECONDS | Integer 0–300; default 5. How long the [limit monitor](#the-monitor) waits before recording a change to a less severe status; 0 records every change at once |
| access.mode | CONVEX_ACCESS_MODE | Required by default; explicit local_development only for isolated loopback |
| access.policy_file | CONVEX_ACCESS_POLICY_FILE | Bounded versioned grants, atomically reloaded each second |
| access.tls_boundary | CONVEX_TLS_BOUNDARY | local_only or proxy; executable upstream stays loopback |
| access.cors_origins | CONVEX_CORS_ORIGINS | JSON array of exact origins; empty denies cross-origin browser requests |
| access.introspection | TOML only | Explicit endpoint/issuer/API audience/client and secret reference; see the access guide |

Startup logs the effective redacted configuration and, after a successful bind, the listener address. A disabled WebSocket setting removes `/ws`, `/api/v1/ws` and `/api/v1/ws/status`.

## Demo output labels

Demo HTTP responses, including WebSocket upgrades, carry `x-convex-profile: demo` and `x-convex-data-quality: demo`. These headers are exposed to browser clients through CORS. Every demo WebSocket JSON message additionally carries `profile: "demo"` and `data_quality: "demo"`; existing economic fields retain their semantics. The numeric `quality` field is not a data-quality label.

Router construction uses the engine’s configured reference repository. Provider capability validation occurs on the server startup path. The replay runtime calculates fixed-bullet results with per-measure quality and immutable lineage. Not yet covered: other products and live provider feeds.

## WebSocket streaming

`/ws` and `/api/v1/ws` stream canonical runtime results and recorded limit status
changes, and nothing else. Send
`{"type":"subscribe_valuations","instrument_ids":[...]}`. A session follows at most 256
instruments, as many as a runtime values; listing one it already follows does not count
again. The server first sends the current result of each listed instrument that has one,
as a `valuation` message with its `runtime_id`, `revision` and result envelope, then the
`subscribed` acknowledgement. So when the acknowledgement arrives, a listed instrument
with neither a `valuation` nor a `RESULT_TOO_LARGE` error (below) has no result yet. Each later result arrives as another `valuation`.
A result sent at subscription can be newer than one already on its way, so keep the newer
by `(runtime_id, revision)`. After `RESYNC_REQUIRED`, subscribe again to the same
instruments to receive their current results, or read `GET /api/v1/runtime/results`.
A message is at most 64 KiB. A result larger than that, at subscription or later, arrives
as a `RESULT_TOO_LARGE` error naming the instrument, `runtime_id` and `revision`; read
the result from `GET /api/v1/runtime/results`. The session stays open.

Send `{"type":"subscribe_limits","book_ids":[...]}` to receive a `limit_status` message
for each change the limit monitor records for those books (see
[book limits](#book-limits)). A session follows at most 64 books; listing one it already
follows does not count again, and `subscribed.count` is the distinct books listed. Nothing
is sent on subscribe: seed the display from `GET /api/v1/books/{book_id}/limits/status`
after the acknowledgement, and after `RESYNC_REQUIRED`, read it again. A limit's message
arrives only when its status changes, so its `value` is as of that change.
`unsubscribe_limits` takes the same shape. A session receives a book's changes only while
it could read that book's status (see [access](ACCESS.md)). A session that holds the book
but not every instrument it holds or a limit names receives each change as a `FORBIDDEN`
error naming the book, without the change; a session whose access to the book is revoked
is closed, as for valuations. REST calculators return their results in
the response and send nothing on the WebSocket. The bond, ETF and portfolio
subscriptions are retired and refused with `INVALID_MESSAGE`; see the
[route migration](../../docs/internal/ROUTE-MIGRATION.md#rest-triggered-websocket-messages-removed).

## Migrating existing startup code

ServerConfig::from_env and apply_env_overrides now return Result. ServerConfig::from_file returns ConfigError, and Server::start returns StartupError. Callers must handle errors. Add an explicit demo profile and local_development access for isolated loopback demonstrations; otherwise compose the required verifier and policy. Previously ignored invalid values, unsupported settings and hostnames must be corrected; they no longer fall back silently.

The startup configuration changes preserve pricing conventions. The reference repository schema and write requirements are described below.

`convex_server::compose(config)` is the single composition root. The executable and `tests/application_path.rs` both call it. It validates startup settings, opens storage and builds the engine and server. It does not start or bind anything. Call `composition.engine.start()`, then `composition.server.start()` or `serve_until`. Adapter failures while opening storage, snapshots, the result publisher, the quote replay or the build identity return `StartupError::Composition { stage, .. }`. Configuration, access, I/O and engine-build failures keep their own `StartupError` variants. Dropping a stopped composition releases the redb file, so calling `compose` again reopens the same state. The Fly showcase image is described in [deploy/showcase](../../deploy/showcase/README.md).

## Curves and price-by-ID

`CurveStore` is the single curve owner. It holds immutable, versioned native
recipes (`SnapshotCurveRecipe`) in redb; the server keeps no curve cache.

| Route | Behaviour |
|---|---|
| `POST /api/v1/curves` | Store one recipe revision (same write as `POST /api/v1/analytics/curves`). `204`; an identical retry is `204`; different content under a stored version is `409 CONFLICT`, or `400 INVALID_INPUT` when its `recorded_at` is more than 5 s after the server clock. |
| `GET /api/v1/curves?after=&limit=` | Each curve's highest stored revision, in identity order, `limit` 1..=256 (default 100). `next` names the cursor for the following page. |
| `GET /api/v1/curves/{id}?known_at=` | The highest revision recorded at or before `known_at`, with `x-convex-curve-version`. `404` when none. |
| `GET /api/v1/curves/{id}/zero/{t}`, `/discount/{t}`, `/forward/{t1}/{t2}` | Build that revision and its exact dependencies, then query. A missing or stale curve is `422 MISSING_INPUT`. |
| `POST /api/v1/curves/bootstrap` | Stateless calibration: returns points and residuals, stores nothing. Unknown or incomplete instruments are `400`. At most 128 instruments, the calibrators' bound. |

There is no `DELETE`: revisions are history. `known_at` defaults to the current
time; cutoff-aware curve reads and quote responses echo it in `x-convex-known-at`.

A revision's `recorded_at` is the writer's: when it became known, which is in the
past for captured history. A cutoff resolves the highest version recorded by it. As
for fixings, each new revision is checked against the curve's stored versions just
below and above it: one recorded before the lower or after the higher is
`409 CONFLICT`, and equal times are allowed. Revisions stored before this rule are not
re-checked, so recording times never decrease as the version rises only in a history
written wholly under it. On either curve `POST` route, a `recorded_at` more than
5 s after the server clock is `400 INVALID_INPUT`, except that an identical retry of
a stored revision is still `204`. The offline `convex-core-inputs` importer has no
clock bound; it keeps the order rule, which belongs to the store. A past cutoff is
still not fixed: a revision can be recorded later with a time between the curve's
latest revision and now. Each response names the revisions it priced.

A stored revision is market data. A compiled Live configuration names its curves
and reads the current revision of each one whenever it builds a snapshot. A
re-mark therefore reaches the stream at the next quote, with no compile,
activation or restart, and request-time pricing reads the same revision. Each
runtime result lists the revisions it consumed in `provenance.input_versions`. See
[RUNTIME.md](../convex-engine/RUNTIME.md).

`GET /api/v1/quotes/{id}?market_price=&settlement_date=&known_at=&configuration=`
prices a stored bond under its valuation context. The context is the active
selection of `configuration`, or of the configuration the runtime runs when the
request names none. Only an activation approves a configuration: a staged version
never prices, and a rollback prices the version it restores. A server without a
runtime has no default, and a request that names nothing is refused with
`422 NO_VALUATION_CONTEXT`, as is a named configuration that has no activation. The
configuration must apply to the bond (`applies_to`), else `422 NOT_COVERED`. It
names the discount curve and optional swap benchmark, and its bid/ask policy
applies.

The reference terms and curve revisions are read at `known_at`, and the curves
must be fresh at it by the same rule as core snapshot assembly. The configuration
is the one active now, whatever the cutoff, and the response names it. That is why
the activation is reported: a price at an earlier cutoff under a later rule is
auditable only because its activation is named. A missing
or stale curve, or one with the wrong role or currency, is refused with
`422 MISSING_INPUT`; the route never prices without curves.

Each product reads only its own curves, when it first needs them: a bullet its
discount curve and swap benchmark, a callable or FRN the curves listed below. A
curve the product does not read refuses nothing, so a stale swap benchmark refuses
bullets and still prices callables and FRNs. A bond the configuration does not cover
is `422 NOT_COVERED` before any curve is read.

Callables and FRNs are valued by the runtime, through the code the stream runs, so
the numbers match the stream's for the same mark, cutoff and settlement. Each request
builds a one-instrument snapshot that is never stored: the stored terms at `known_at`,
the configuration's curves that product reads (a callable's discount curve; a note's
discount and `frn_reference_curve`), the note's due fixings, and the mark as one quote
from source `request`. The runtime values it on one of its interactive slots, with the
runtime's measures for the product. The response has the bullets' shape, with:
- `source: "request"`;
- `measure_errors` for each measure the valuation could not produce, with the
  runtime's code;
- `snapshot`: `{"id", "published": false, "curves"}`. The id names the content but
  cannot be fetched.
`x-convex-curve-versions` lists the curves that valuation read. A server without a
runtime refuses callables and FRNs with `422 UNSUPPORTED_PRODUCT`. A runtime that is
not ready returns `503 UNAVAILABLE`, and `429 CAPACITY_EXCEEDED` when every slot is
taken; both are retryable.

The response reports what it consumed:
- `x-convex-reference-version`
- `x-convex-pricing-config` (`id@version`)
- `x-convex-activation` (the activation revision)
- `x-convex-curve-versions` (`id@version,...`, including dependencies)
- `x-convex-known-at`

The settlement date defaults to the `known_at` date. `market_price` is required; a
request without it is `422 MISSING_INPUT`. The quote is returned in the response
only; it is not sent on the WebSocket.

`POST /api/v1/quote` and `POST /api/v1/batch/price` take the same optional
`configuration` and `known_at` and resolve the same context. A batch resolves the
context once, and each product's curves at the first bond that reads them. A bond the
context cannot price, a curve it reads included, fails on its own in `errors`, and
the rest still price. Their bodies carry `context`:
- `{"kind": "configuration", "configuration", "activation", "known_at", "curves"}`
  for a configuration, with the headers above. In a batch, `curves` and
  `x-convex-curve-versions` list every curve some bond read, each once. A batch
  prices each curve on one revision, the one its first read found, even when a newer
  revision is recorded while it runs.
- `{"kind": "supplied"}` when `POST /api/v1/quote` supplies its own curves or
  volatility. That is a what-if, and naming a `configuration` with it is
  `422 INVALID_INPUT`.
- `{"kind": "none"}` when no configuration is named and no runtime runs one. The
  price is then yield-only, and curve measures report `MISSING_MARKET_DATA`.

On these routes a callable or FRN must carry the stored terms at `known_at`; other
terms are `422 INVALID_REFERENCE`. Without `market_price`, each of its measures reports
`MISSING_MARKET_DATA`. A batch values its callables and FRNs one at a time, so it never
takes every interactive slot. An item that finds every slot taken fails on its own with
`CAPACITY_EXCEEDED`. Supplied-curve what-ifs still go to the router.

Curve and quote errors share one body, `{"error", "code", "retryable"}`
(`crate::error::ApiError`); other routes migrate to it over time.

## Authoritative bond reference data

Bond CRUD and pricing reads use the same `ReferenceRepository` configured in the engine's `StorageAdapter`. The executable uses redb. Creating another router for the same engine shares its references; it does not allocate another bond store. `MemoryReferenceRepository` is available for volatile fixtures and explicit embedded demonstrations.

Each mutation atomically retains current state, immutable versions and a command/receipt for retry recovery. Versions start at 1. Updates and deletion increment the version; deletion retains a tombstone. A deleted internal identity cannot be recreated with a reset version. Exact historical lookup returns the requested version or absence. The receipt records the reference change and its commit time; authenticated actor audit, financial events and outbox transport are not covered.

All bond write requests require one `Idempotency-Key` header. PUT and DELETE additionally require `If-Match` containing the positive version returned by a read, for example `"1"`. Wildcards, weak ETags, duplicate headers and unspecified versions fail. The path identity must equal the supplied body identity.

An identical retry returns the original committed response, including its original version, even after subsequent changes. A changed payload with the same key returns HTTP 409. Comparison uses normalized typed JSON, preserves exact decimal scale and includes operation order and expected versions. Transport whitespace/key order and server commit time are excluded. The repository never replaces the supplied source `last_updated` with the server clock; its independent `recorded_at` records commit time.

Bond reference decimal fields are plain decimal **strings**, including coupon (decimal rate), face, call price, floating spread (bp), cap/floor, inflation base and amount outstanding. JSON numbers, scientific notation, rounding beyond supported precision and unknown fields fail reference ingress. These rules preserve exact persisted terms despite the workspace's numerical serde feature flags. New writes require explicit `contractual_terms` and exact issuer/calendar/convention pins. The [reference master contract](../convex-ports/REFERENCE_MASTER.md) explains supported schedules, rule versions, historical identity resolution and admission. The current service path admits supported fixed bullets; full product/convention certification remains separate.

| Endpoint | Behavior |
|---|---|
| POST /api/v1/bonds | Create complete terms; return 201 and committed reference version |
| GET /api/v1/bonds/{id} | Current active terms; 404 for a missing or deleted identity |
| PUT /api/v1/bonds/{id} | Full replacement by expected version; return 409 on conflict |
| DELETE /api/v1/bonds/{id} | Append a deletion version; return 204 after commit |
| POST /api/v1/bonds/batch | Atomic create of 1–256 complete terms; any conflict rejects the batch |
| GET /api/v1/bonds/{id}/versions/{version} | Full immutable record including version, commit time and possible tombstone |
| POST /api/v1/reference/transactions | Atomically commit bond and master changes; header/body idempotency keys must match |
| POST /api/v1/reference/resolve | Resolve an identifier with supplied as_of and known_at; return mapping and target versions |
| GET /api/v1/reference/commits/{key} | Original receipt for lost-reply/unknown-outcome recovery |
| GET /api/v1/bonds/isin/{value} or /cusip/{value} | Current mapping; ambiguity returns 409 |
| GET /api/v1/bonds | Deterministic internal-ID order; filtered total and page from one read snapshot |

Single reference reads/writes expose the version through `ETag` and `x-convex-reference-version`. Stored-instrument quote responses expose the reference version actually consumed. List responses retain `bonds`, `total`, `limit` and `offset`, and add the matching `versions`. A page limit is 1–1000. Invalid or unknown supplied filters fail rather than disappearing. Offset pagination is deterministic for unchanged state; snapshot cursors across concurrent mutations are not implemented.

HTTP 400 identifies invalid reference requests; 404 missing active records; 409 version/idempotency/mapping conflicts; 503 unavailable storage, capacity exhaustion or unsupported persisted schema. `UNKNOWN_OUTCOME` is not marked retryable: inspect `/api/v1/reference/commits/{key}` and retain the same key/payload when recovering. The tests inject a lost acknowledgment after a real redb commit and verify recovery and reopen.

For a local fictional fixture, after starting the demo server:

```sh
python3 -c 'import json; print(json.dumps(json.load(open("crates/convex-ext-file/tests/fixtures/instrument-master.json"))["command"]))' > /tmp/convex-reference-command.json
curl -i -X POST http://127.0.0.1:8080/api/v1/reference/transactions \
  -H 'Content-Type: application/json' \
  -H 'Idempotency-Key: master-example-initial' \
  --data-binary @/tmp/convex-reference-command.json
curl -i http://127.0.0.1:8080/api/v1/bonds/INSTRUMENT-001
curl -i http://127.0.0.1:8080/api/v1/reference/commits/master-example-initial
```

File imports use `convex_ext_file::import_reference_file` for a strict versioned command with resources and mappings in the same transaction. The full-term array loader `load_bond_references` remains available. Both reject missing files/mandatory terms and files above 8 MiB. See the [offline example and diagnostics](../convex-ports/REFERENCE_MASTER.md#file-imports-and-runnable-example). `bonds_file` startup composition remains unsupported.

The initial adapters bound retained identities including tombstones to 100,000, commits to 100,000 and immutable versions to 1,000,000. Capacity exhaustion rejects writes; history/deduplication is never silently discarded. Queries and identifier resolution scan bounded local state. These bounds are safeguards, not measured throughput or latency targets. redb provides single-process transactions; managed shared-writer persistence remains future work.

## Books and portfolios

A book is a durable, versioned position snapshot owned by an external book management
system. It is a `MasterKey::Book` resource in the same reference repository as bonds, so it
inherits compare-and-set versions, immutable history, idempotency receipts, atomic redb
commits and `convex-redb backup`. No other store exists.

| Route | Result |
| --- | --- |
| `PUT /api/v1/books/{book_id}` | Full snapshot. 201 on version 1, 200 on a replace, 422 with per-row errors, 409 on a version or key conflict, 413 over the bound |
| `GET /api/v1/books?limit=&after=` | Summaries by book ID, at most 500 per page; `next_after` resumes |
| `GET /api/v1/books/{book_id}` | Current version, pinned in `ETag` |
| `GET /api/v1/books/{book_id}/versions/{v}` | One immutable version |
| `GET /api/v1/books/{book_id}/history?from=&limit=` | Version summaries, oldest first, at most 500 per page |

`Idempotency-Key` is required on every write. On `PUT`, an absent `If-Match` creates the book
and conflicts if the ID was ever used; a present `If-Match` replaces exactly that version. A
retry with the same key and body returns the original receipt; the same key with another body
conflicts. A snapshot holds at most 1,000 positions (`MAX_BOOK_POSITIONS`): a book is
serialized four times in one redb transaction against its 8 MiB budget, and 1,000 rows is also
below the 1,024 distinct-identity ceiling of the HTTP entitlement scan.

Each row gives `instrument_id` or an external `identifier` (`isin`, `cusip` or a `provider`
namespace for BMS keys), a `quantity`, an optional `unit` (`face`, default, or `units`), and an
optional exact `reference_version`. External identifiers resolve through the identifier
mappings at the time of the request; unknown or ambiguous identifiers are rejected, never
guessed. Every row must resolve or nothing commits, and the 422 body lists each failed row
as `{index, code, reason}` (the first 100 are reported). The 2xx body adds `resolutions` with
the instrument and mapping versions consumed. Delta and fill ingest are not supported.

`/api/v1/portfolios*` is a one-release alias over books with the former request and response
shapes. Position edits read the current version and commit with it as the expected version
(a caller `If-Match` replaces it), so a concurrent writer gets 409. `DELETE` retires a book by
closing its effective interval; a retired id is treated as absent and can be created again.
The alias list reads one repository page (1,000 books) and refuses with 503 beyond that; use
`/api/v1/books` for larger estates. Alias positions are not checked against the instrument
master, as before; `/api/v1/books` checks every row.

`POST /api/v1/portfolio/analytics` also accepts `{"book_id": "...", "version": n}` instead of
inline positions. It requires a complete runtime result for every position, `Valid`, or `Demo`
under the demo profile, and otherwise refuses the whole request (422
`BOOK_RESULTS_UNAVAILABLE`, naming the missing instruments and the pinned book version); it
never returns a partial portfolio. The runtime results reach the analyzer as quotes valued for
the runtime plan's settlement date, in each priced instrument's currency. To cover the
analytics, the configuration must request `clean_price_mid`, `accrued_interest`, `ytm_mid`,
`modified_duration_mid`, `convexity_mid`, `dv01_mid` and `z_spread_mid`. Callables publish
OAS measures and floating rate notes publish discount-margin measures, not
yield-to-maturity ones, so a book that holds either is refused. It also refuses
with 422:

| Code | When |
|---|---|
| `BOOK_CURRENCY_REQUIRED` | The book names no reporting currency |
| `UNSUPPORTED_QUANTITY_UNIT` | A position is held in units; no unit-to-face conversion is defined |
| `BOOK_ANALYTICS_REFUSED` | The analyzer refuses the inputs, for example a missing measure or a foreign-currency position |

Other portfolio analytics routes take inline positions only.

### Book limits

A book's limits are one versioned limit set per book (`MasterKey::LimitSet`), in the same
reference repository, with the same compare-and-set versions, history, idempotency receipts
and backup as books. A set holds at most 64 limits; the measures, bounds, warning bands and
statuses are described in [the engine's limits guide](../convex-engine/LIMITS.md).

| Route | Result |
| --- | --- |
| `PUT /api/v1/books/{book_id}/limits` | Propose a complete set: 202 with the proposal. Under local development it commits at once: 201 on version 1, 200 on a replace |
| `POST /api/v1/books/{book_id}/limits/proposal/approve` | A second subject commits the open proposal: 201 or 200 with the set |
| `POST /api/v1/books/{book_id}/limits/proposal/withdraw` | Close the open proposal without committing it: 200 |
| `GET /api/v1/books/{book_id}/limits` | The current set, pinned in `ETag` |
| `GET /api/v1/books/{book_id}/limits/versions/{v}` | One immutable version |
| `GET /api/v1/books/{book_id}/limits/history?from=&limit=` | Version summaries, oldest first, at most 500 per page |
| `GET /api/v1/books/{book_id}/limits/proposal` | The latest proposal, open or closed, pinned in `ETag` |
| `GET /api/v1/books/{book_id}/limits/status` | Each limit evaluated now against the current Live results |
| `POST /api/v1/books/{book_id}/limits/check` | Each limit evaluated now as if proposed position changes were applied; nothing is stored ([the pre-trade check](#the-pre-trade-check)) |
| `GET /api/v1/books/{book_id}/limits/records?after=&limit=` | The book's recorded status changes after a record id, oldest first, at most 1,000 a page ([records](#records)) |
| `POST /api/v1/books/{book_id}/limits/acknowledgements` | Acknowledge one limit in breach or unknown, until an expiry: 201 ([acknowledgements](#acknowledgements)) |
| `POST /api/v1/books/{book_id}/limits/acknowledgements/withdraw` | End the acknowledgement in force for one limit: 200 |
| `GET /api/v1/books/{book_id}/limits/acknowledgements` | The acknowledgements in force now, with the current version in `ETag` |
| `GET /api/v1/books/{book_id}/limits/acknowledgements/versions/{v}` | One version as written |
| `GET /api/v1/books/{book_id}/limits/acknowledgements/history?from=&limit=` | Version summaries, oldest first, at most 500 per page |

Every write needs `Idempotency-Key`, and the `risk_manager` role (see [access](ACCESS.md)).
The pre-trade check writes nothing and needs neither.
On `PUT`, `If-Match` names the limit set version the change replaces, and is absent only for
a book's first set; the book must exist. The proposer and the approver each hold every
instrument the new limits and the current set name, since a change drops any limit it
leaves out. A new proposal replaces an open one, so the caller must also hold each
instrument the open one names, as for withdraw. The proposal's history keeps every
version. Approve and withdraw take the proposal version in
`If-Match`. Approval commits the set and closes the proposal in one transaction. The set
records who proposed and who approved it, as configuration records an actor: `access:` and
the credential's digest. Under `access.mode = local_development` requests carry no subject,
so a change commits in one step, recorded as `local-development`.

| Code | When |
|---|---|
| 403 `LIMIT_SELF_APPROVAL` | The approver is the proposer |
| 409 `VERSION_CONFLICT` | `If-Match` does not name the current set (on `PUT`) or the current proposal (on approve or withdraw), or the key was used with another body |
| 409 `LIMIT_PROPOSAL_CLOSED` | The proposal was already approved or withdrawn |
| 409 `LIMIT_SET_CHANGED` | The set moved after the proposal was made |
| 409 `LIMIT_EXCESS_EXPIRED` | An excess the proposal adds or changes has expired by its approval |
| 409 `LIMITED_BOOKS_EXCEEDED` | 64 other books already hold a non-empty set (the monitor's bound, D87); an empty set takes no place |
| 403 `LIMITS_ROUTE_REQUIRED` | A reference transaction or import names a limit set, proposal or acknowledgements, or a commit receipt read holds one: only these routes write and read them |
| 404 `LIMIT_NOT_FOUND` | An acknowledgement names a limit the current set does not hold |
| 404 `LIMIT_ACKNOWLEDGEMENT_NOT_FOUND` | A withdrawal names a limit with no acknowledgement in force |
| 409 `LIMIT_NOT_ACKNOWLEDGEABLE` | The limit, evaluated now, is `within` or `warning` |
| 422 `LIMIT_RECORD_MISMATCH` | An acknowledgement's `record_id` is not a recorded status change of this book's limit |
| 413 `CHECK_TOO_LARGE` | A pre-trade check has more than 256 rows |
| 413 `BOOK_TOO_LARGE` | A pre-trade check would leave the book with more than 1,000 positions, as committing it would |
| 422 `BOOK_ROWS_REJECTED` | A pre-trade check row cannot be applied; `rows` names each one, as for a snapshot `PUT` |

Status names the book and set versions, the time and the snapshots whose results it read.
Each item carries the limit's measure and the bounds it was judged against beside its
`status`, `value`, `member`, `utilisation` and, when `unknown`, its `reason`; a `breach` or
`unknown` item also carries the [acknowledgement](#acknowledgements) in force for it, if
there is one, and a limit with a [temporary excess](#temporary-excess) in force carries it
as `excess`. It reads accepted runtime results as
analytics by book do, but a missing input never refuses the request: a position without an
accepted result, or with one that cannot be read as an analyzer quote, makes each limit that
reads market data `unknown`, and a `face` limit still evaluates. With the configured
measures of analytics by book, plus `cs01_mid` and the `krd_*_mid` tenors that limits read,
status for a 256-position book with 64 limits over every measure took 4.1 ms per request
against 3.8 ms for analytics by book (median, `ci` profile, WSL2 loopback). A runtime values
at most 256 instruments, so a 1,000-position book has results for at most 256 of its
positions.

#### The monitor

While the server serves, a monitor watches every book with a non-empty limit set (at most
64). It evaluates them as status does, on its own thread, off the tick path:
- after each published tick, every watched book that holds a revalued instrument;
- when a watched book, or any book's limit set, commits;
- when a [temporary excess](#temporary-excess) on a watched limit expires;
- every watched book once, on the latest results, when it falls behind either stream. It
  never queues.

It records each status *change* in the audit store, then publishes it (D86): one audit
entry per limit whose status differs from its last recorded one, under the
[clear hold](#the-clear-hold), then one `limit_status` WebSocket message. One evaluation
of a book writes its entries in one transaction (D92), all or nothing, and publishes them
once it commits. An evaluation that changes nothing
records nothing. Each entry has `event_type` `limit_status`, `entity_type` `book`, the book
id as `entity_id`, the new status as `action`, Unix milliseconds as `timestamp`, and the
message's JSON as `details`. The message carries `record_id` (the entry's id), the book and
set versions, `evaluated_at`, `previous` (the last recorded status, or `unevaluated`), the
limit's measure and the bounds it was judged against, its evaluation (`limit_id`, `status`,
`value`, `member`, `utilisation`, `reason`, `excess`), the snapshot ids read and
`monitor_started_at`, and for a `breach`
or `unknown` change the [acknowledgement](#acknowledgements) in force when it was
evaluated. A failed write
records none of the book's changes and keeps their last recorded statuses, so a later
evaluation records them; a book with a failed write is evaluated again within a second,
unless the store was full (below). The [records route](#records) reads them.

The audit store holds at most 1,000,000 entries and keeps every one: nothing prunes it.
With no hold, a value that crosses a bound back and forth records a change each time, at
most once per evaluation per limit. Once the store is full, every change is
refused: none is recorded or published, `convex_limit_status_append_failures_total` counts
each change refused, and one error is logged until a write succeeds again. A book's write
that would pass the cap is refused whole. `convex_audit_entries` reports the entries held:
alert at 800,000, and on the failure counter. How long records must be kept is the
operator's policy; nothing here archives them yet (D98). The monitor is the audit store's
only writer today. A stop is seen between books, so shutdown waits for one book's commit at
most; changes not yet recorded are restated at the next start.

A restart records each limit's status again, as a change from `unevaluated`, with the new
`monitor_started_at`: a standing breach is restated, not reported as a new one. Each book's
entries are one database commit, so at the bounds (64 books with 64 limits each) the first
evaluation after a start records its 4,096 entries within about 2.2 s here (`ci` profile,
WSL2; 14 to 43 s when each entry was its own commit). Valuations are not slowed meanwhile,
but changes in that window are recorded late. An entry's `details` is about 600 bytes, so a
book's commit is about 40 KB.

At the bounds (64 books, each holding all 256 priced instruments with 64 limits over every
measure), one evaluation pass takes about 90 ms whether a tick revalues 1, 64 or 256
instruments, since each holds every instrument: about 1.4 ms a book. Above about 11 ticks a
second the monitor coalesces ticks. One instrument's valuation hop (quote in to WebSocket
frame) stays at about 1.4 ms with the monitor busy.

#### The clear hold

A value hovering at a bound would otherwise record, and publish, a change at every
crossing. So the monitor records a change to a *more* severe status (`within` < `warning` <
`unknown` < `breach`), or a limit's first status, at once, and a change to a *less* severe
one only once the limit has stayed less severe than its recorded status for the hold
(`limits.clear_hold_seconds`, 5 s by default, at most 300). It then records the status of
its first evaluation at or after the hold's end, which may be any less severe status the
limit reached meanwhile. Any evaluation at the recorded status, or one more severe, ends
the hold, and the next clearing waits a whole hold again. The monitor wakes for a hold's
end with no tick or commit, as it does for an excess's expiry; holds run on the monotonic
clock, so a wall clock that steps does not shorten them.

- Status and the pre-trade check read no record, so they show the status of the moment.
  The `limit_status` messages, the records, and the `convex_limit_breaches` and
  `convex_limit_unknown` gauges show a breach until it has stayed clear for the hold.
- Approving an [excess](#temporary-excess) on a breached limit is a clearing, so it is
  recorded once it has held. Its expiry is a breach, recorded at once.
- A restart records every limit at its first evaluation, as before: any status is a change
  from `unevaluated`.
- A hold of 0 records every change at once, as before P18.

Measured (`ci` profile, WSL2): a value crossing a bound about three times a second for 60 s
(178 ticks) recorded 179 changes with no hold, and 2 with the default 5 s (its first
status and the breach), since no return lasted the hold.

#### Records

`GET /api/v1/books/{book_id}/limits/records?after=&limit=` returns the book's recorded
changes with record ids above `after` (default 0), oldest first, up to `limit` (default
100, at most 1,000). Each record is the JSON of its `limit_status` message, as the monitor
stored it, without `type`:

```json
{"book_id": "BOOK-A", "records": [{"record_id": 41, "limit_id": "net", "previous": "within",
  "status": "breach", "...": "..."}], "withheld": 0, "next_after": 41, "more": false}
```

Pass `next_after` as the next `after`. A reader following new records keeps passing the
last id it has: the monitor assigns ids in order, so it never sees a record twice or misses
one. `more` says whether the book had records past the page when it was read. A page also
stops before a record that would take it past 3 MiB or 60,000 JSON values, with `more`
true, so that the access layer can check the response whole.

The route needs what status needs (see [access](ACCESS.md)): the book, every instrument it
holds and every instrument its current set names. A record can also name an instrument
the book no longer holds: a `face` limit's instrument (on each position, the one that set
the status) and the instruments its reason lists. A record naming one the caller cannot
read is left out and counted in `withheld`, and `next_after` still passes it. A book whose
set is empty, or was never committed, still has its records. Without a limit monitor the
route answers 503.

The audit store indexes each entry by book in the transaction that writes it, so a page
reads only that book's records. With 1,000,000 entries in the store (64 books, about
15,600 records each), a page of 100 from the middle of one book's records took 0.16 ms and
a page of 1,000 took 1.78 ms; `query` with the same filter, which reads every entry,
took 1.17 and 1.25 s. Through the route, under local development, the
pages took 0.74 and 5.5 ms (`ci` profile, WSL2). The first open of a
store written before the index indexes every entry once, 65,536 a transaction: 5.0 s for a
full store, in 16 transactions. Later opens find nothing to index.

#### Acknowledgements

A risk manager acknowledges a limit in breach or unknown: the acknowledgement records who
has seen it, until when and why, and changes no bound (D95). The limit stays in breach.

```json
{"limit_id": "net", "expires_at": "2026-11-12T17:00:00Z",
 "note": "desk reducing by Thursday", "record_id": 41}
```

- The limit, evaluated now as status evaluates it, must be `breach` or `unknown`, so
  nothing is acknowledged in advance.
- The expiry is after the acknowledgement and at most seven days after it. The note is
  1 to 1,024 bytes, not blank, with no control character but a line feed.
- `record_id` is optional evidence: it must name a [record](#records) of this book's
  limit, which needs the limit monitor (503 without one).
- One acknowledgement is in force per limit; a new one replaces it.
- It covers every `breach` or `unknown` status of the limit until it expires. It ends
  sooner if it is withdrawn, or if a set version changes or drops the limit: each
  acknowledgement keeps the limit as acknowledged, and is in force only while the current
  set holds it exactly so. A limit dropped and later restored unchanged is covered again.
- Status, the pre-trade check, [records](#records) and `limit_status` messages carry the
  acknowledgement in force on each `breach` or `unknown` limit, as `acknowledgement`
  (`acknowledged_by`, `acknowledged_at`, `expires_at`, `limit_set_version`, `note`,
  `record_id`). An acknowledgement records and publishes nothing of its own (D97): a
  screen sees it on its limit's next change, or by reading status.

A book's acknowledgements are one versioned record per book
(`MasterKey::LimitAcknowledgements`), in the reference repository as its limit set is
(D94). Each version changes one limit's entry and says which, and who: `change` is
`{"action": "acknowledged", "limit_id"}` or `{"action": "withdrawn", "limit_id", "by"}`.
Entries no longer in force are dropped at the next version; the versions keep them.
`GET .../acknowledgements` shows those in force now, with `in_force_at`. A book with none
has an empty list and no version.

Acknowledging and withdrawing need `Idempotency-Key` but no `If-Match`: each changes only
its own limit's entry, so two risk managers acknowledging different limits do not refuse
each other. An exact retry returns the version it wrote, even after the set has changed or
dropped the limit. Either needs what status needs, and `LimitWrite` (the `risk_manager` role) on
the book and on each instrument the limit names; any such subject may withdraw any
acknowledgement. Under local development both are recorded as `local-development`. Reads
need what status needs, and a version's read also each instrument its entries' limits
name.

With 64 limits over every measure on a 256-position priced book, each in breach and
acknowledged with a 1,024-byte note, status took 3.79 ms per request against 3.70 ms for
the same positions with none acknowledged, in the same rounds (median, `ci` profile, WSL2
loopback). The response grew from 15 KB to 91 KB.

#### Temporary excess

A limit may carry an excess: wider bounds until an expiry, with a reason.

```json
{"limit_id":"dv01","measure":{"kind":"dv01"},"lower":"-50000","upper":"50000",
 "excess":{"upper":"65000","expires_at":"2026-12-01T00:00:00Z","reason":"index rebalance"}}
```

It is part of the limit, so it is changed through the set's proposal and takes two subjects,
as widening a limit should.

- Each bound it names widens one the limit has, strictly and on its own side.
- An excess the proposal adds or changes must expire after the proposal and at most thirty
  days after it, or the proposal is refused (400). If it has expired by the approval, the
  approval is refused (409 `LIMIT_EXCESS_EXPIRED`).
- An excess carried forward exactly as the current set holds it is not checked, even if it
  has expired, because a set is replaced whole and a change to one limit carries the others.
- Approving one changes the limit's definition, so it ends the limit's
  [acknowledgement](#acknowledgements). Its expiry does not.

Strictly before its expiry, the limit is judged against the excess bounds, with its warning
band on them. A bound the excess does not name stays the limit's own. Status, the pre-trade
check, records and `limit_status` messages then show the bounds judged as `lower` and
`upper`, so they always sit beside the value they were compared with, and carry `excess`:
`base_lower`, `base_upper` (the limit's own bounds), `expires_at` and `reason`. From its
expiry the limit is judged against its own bounds and carries no `excess`. An expired excess
stays in the set, inert, until a later version removes it.

The status stays one of the four. Approving an excess on a breached limit is a status change
(`breach` to `within` or `warning`), recorded and published with the excess as evidence, and
its expiry is the change back: the monitor wakes for it, with no tick or commit. A wall clock
that steps back would otherwise apply an expired excess again, so the monitor treats one it
has seen expire as expired for the rest of its run.

#### The pre-trade check

`POST /api/v1/books/{book_id}/limits/check` answers what status would say if proposed
changes were committed (D88). It stores nothing: no book version, audit entry or
`limit_status` message, and no `Idempotency-Key` or `If-Match`.

```json
{"changes": [
  {"instrument_id": "BOND-001", "quantity": "-250000"},
  {"identifier": {"kind": "isin", "value": "US0000000001"}, "quantity": "1000000", "rating": "A"}
]}
```

A row takes the grammar of a book snapshot row (`instrument_id` or `identifier`,
`quantity`, and optional `unit`, `reference_version`, `sector`, `rating`), at most 256 of
them, each instrument once. Its `quantity` is signed and added to the held position: it is
a trade, not a new holding (D89).
- A row for a held instrument changes only its quantity. A `unit`, `reference_version`,
  `sector` or `rating` that differs from the held position's is refused (`INVALID_ROW`); the
  same value may be restated.
- A position a row brings to exactly zero is dropped.
- A new instrument is added after the held positions, in row order, with the row's fields
  (`unit` defaults to `face`). It must be stored, as for a snapshot. One without a Live
  result makes the limits that read market data `unknown`, and a `face` limit still
  evaluates.

The response is the status route's, as if the changes were applied (D90): the book and set
versions it read, `evaluated_at`, the snapshots read, and each limit. An empty `changes`
gives the current status. Status and a check read the same accepted results, so a check of
changes equals the status of the book once they are committed, while no quote arrives in
between. The caller needs what status needs, plus `BookRead` on each instrument a row names,
including one reached through an identifier (D91; see [access](ACCESS.md)).

## Reference schema and Rust migration

The redb reference tables are `reference_current_v2`, `reference_history_v2`, `reference_commits_v2`, `reference_master_current_v2` and `reference_master_history_v2`. A complete earlier three-table repository adds the master tables atomically while retaining exact prior content and receipts. Terms without `contractual_terms` require an explicit verified update before valuation. Back up before upgrade and restore into isolation to roll back. A nonempty previous unversioned `bonds` table fails startup with an explicit export/import requirement. Preserve its database and export full terms with a compatible reader before importing them into a new versioned repository; do not erase the existing table to bypass validation. Unversioned data needs explicit export/import; missing prior history is not reconstructed.

Implement `ReferenceRepository` to slot in another backend and run both shared reference conformance suites. Required historical/master methods are described in the [port guide](../convex-ports/REFERENCE_MASTER.md#http-and-embedded-use). Default router helpers deny protected access. Supply `create_router_with_access` with the verified boundary; the access guide describes embedding migration.

`create_test_storage` names the file-backed redb test helper accurately. Prefer `create_redb_storage` with an owned temporary directory when deterministic file cleanup is required. The validated analytics risk wrapper is now `ValidatedRiskResponse`; the former name has no compatibility alias. Pure pricing formulas, units and signs are unchanged.

The [local redb storage guide](../convex-ext-redb/README.md) documents immutable configuration history, atomic active selection, schema migration and the `convex-redb` offline backup/restore commands. Pricing saves require the next version, deletion explicitly fails and audit queries apply their filters. Managed rollback selects retained content through a new activation. Shared persistence and distributed ownership are not covered.

## Market replay and file inputs

[The file adapter contract](../convex-ext-file/README.md) documents sequenced quote replay, image/delta recovery, exact units/timestamps, field clears and bounds. Run its standalone trace example before wiring an ingestion adapter. The owned runtime uses snapshot-consistent pricing; the v1 synthetic-curve behavior is still a gap for production use.

## Configuration operator

Build `cargo build --locked -p convex-server --bins` to obtain `convex-config` alongside the server. Its [runnable configuration guide](../convex-engine/CONFIGURATION.md) creates a new fictional workspace and shows schema, validate/explain, diff, stage, activate, rollback, exact-version/history/effective inspection and plan export. The CLI opens local redb exclusively, so stop the server for these operations. A running server offers the same validate, diff, stage, activate, rollback, history and version operations over HTTP for the `operator` role (see the [configuration guide](../convex-engine/CONFIGURATION.md)), beside read-only `GET /api/v1/configuration/schema` and `GET /api/v1/configuration/:id?as_of=...&known_at=...`. A restart-required activation commits and applies at the next start; the server never restarts itself. These routes share [current access control and quotas](ACCESS.md). The offline CLI still relies on trusted local filesystem/operator access.

## Management routes and the admin app

- `POST /api/v1/reference/import` takes a reference import file as the request body: the strict `{schema_version, command}` envelope, at most 8 MiB. It commits the one atomic transaction like `/api/v1/reference/transactions`. The file's idempotency key makes a re-upload return the retained receipt. A refusal carries the importer's `diagnostics` (section, row, field, code, line and column), never source values.
- `GET /api/v1/reference/masters/:kind?after=&limit=` lists current master resources of one kind (`entity`, `calendar`, `convention`, `rate_index`, `identifier`, ...) in identity order. `limit` defaults to 100, `next` is the cursor for the following page, and books and their limits keep `/api/v1/books`.
- `admin_ui_dir` (`CONVEX_ADMIN_UI_DIR`) serves a built admin app at `/admin`, falling back to its `index.html`. The repository's app is [`admin/`](../../admin/README.md); build it with `npm run build` and point the setting at `admin/dist`. List the page's own origin (for example `https://convex.example.com`, or `http://127.0.0.1:8080` locally) in `access.cors_origins`: browsers send it on the page's script and on every write, and the origin guard admits listed origins only. Startup refuses `admin_ui_dir` with no listed origin. Startup refuses a directory without one. The files are public static assets outside the API access layer. They are sent with a policy that loads scripts only from the server and forbids framing, because the page holds a pasted bearer token in memory. Unset, nothing is served there.

## Metrics

`GET /metrics` renders stage latency histograms and the limit monitor's counts in the Prometheus text format. It needs the operator role and the operations grant, like `/api/v1/runtime`, so a scraper sends a bearer token. The server installs one recorder per process at composition, with a thread that drains recorded observations every 5 s so an unscraped process stays bounded; the exporter opens no listener of its own. Every histogram is in seconds, with buckets from 1 µs to 10 s, and no label carries an instrument, curve or request id:

| Histogram | One observation |
|---|---|
| `convex_ingress_seconds` | `POST /api/v1/market/quotes`: one parsed batch through ingress validation and queue admission, the whole batch or nothing. It starts after JSON extraction and excludes the runtime's work. |
| `convex_snapshot_assemble_seconds` | One tick's snapshot, including the stored-curve read for an unpinned Live document |
| `convex_curve_build_seconds{path}` | One curve graph built from recipes: `request`, `stream` (the Live build check), `graph` (the one build per snapshot at activation, shared by every instrument it values) or `valuation` (a single valuation that missed the cache, such as a supplied context) |
| `convex_valuation_seconds` | One instrument valued in the runtime |
| `convex_calculate_seconds` | Every instrument of one snapshot, including graph commits |
| `convex_publish_seconds` | One result record handed to the publisher |
| `convex_request_context_seconds` | One request-time valuation context: the activation read. Each product reads its curves later, at first use, and their build is `convex_curve_build_seconds{path="request"}` |
| `convex_quote_seconds` | One `POST /api/v1/quote` |
| `convex_limit_monitor_seconds` | One limit monitor pass: evaluating the books a trigger names, then recording and publishing their changes |

The limit monitor also keeps these. The two book gauges are labelled `book_id`, one series
per watched book, so a scraper sees the ids of watched books; a book no longer watched
reads 0.

| Metric | Meaning |
|---|---|
| `convex_limit_breaches{book_id}` | Limits whose last recorded status is `breach` |
| `convex_limit_unknown{book_id}` | Limits whose last recorded status is `unknown` |
| `convex_limit_status_changes_total` | Status changes recorded |
| `convex_limit_status_append_failures_total` | Status changes that could not be recorded, and so were not published |
| `convex_audit_entries` | Entries the audit store holds, read when the monitor starts and after each write; every write is refused at 1,000,000 |

On-demand fixed and zero-coupon quotes use the [shared bond quote route](../../docs/BOND-QUOTES.md). Supply an observed price; the curves come from the activated configuration, or from discount, swap and government inputs you pass in for a what-if. No demo curves, assumed volatility or model/par price are generated. Missing optional measures return structured errors. Not yet covered: full historical contexts and live provider feeds.


The [bond default/recovery example](../../tools/BOND-EVENTS.md) reuses these ports
for principal-only recovery, separately earned cash, lifecycle risk, hedge
proposals, complete causal P&L and restart recovery. Run
`PYTHONDONTWRITEBYTECODE=1 python3 tools/core_bond_workflow.py`.
Unknown recovery withdraws readiness; recovery and receipts stay
with the owners of the original claims. No additional route or persistence schema is used.


The [valuation-date roll example](../../tools/DATE-ROLLS.md) reuses the existing
reference, curve, fixing, snapshot and result ports for scheduled coupons,
amortization, maturity, outstanding claim risk, hedge proposals and causal P&L.
Run `PYTHONDONTWRITEBYTECODE=1 python3 tools/core_roll_workflow.py`. Captured
fixings respect each historical point's knowledge cutoff; matured securities
leave fresh hedge candidates while payments remain separate events.
The example checks missing-input handling, source refresh, exact retry and
restart after a committed hedge loses its acknowledgement.
