# Convex server configuration

The [issuer-default workflow](../../tools/ISSUER-EVENTS.md) reuses this server's
reference, curve, fixing, snapshot and result ports for bonds/CDS, risk, hedges
and P&L, including atomic book updates and restart recovery.

The server supports an explicit demo profile and loopback-only historical replay with an immutable plan and canonical quote file. Production is the default and fails before opening storage because production provider composition is not certified. Demo responses contain demonstration inputs; they do not establish production readiness. The [runtime guide](../convex-engine/RUNTIME.md) covers replay, readiness, canonical results and joined recovery.

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
`--pnl-output /tmp/core-pnl.json` to retain the complete native P&L envelope.

The six supplied trades are a cash bond, stepped/amortizing IRS, seasoned OIS,
single-name CDS, overnight rate future and term rate future. The example selects
complete terms, calendars, history, curves, positions, FX and cash events from
the existing synthetic causal-book fixture. It sends every financial calculation
through the server: native prices; discount, term-projection, overnight-projection,
hazard and recovery risk; a constrained bond/IRS/CDS/futures hedge proposal; and
causal P&L split into market, FX, position and event changes. It also removes a
required discount curve and checks the native `missing_input` error.

Output includes the input fingerprint, quantity/reference units, ordered factor
keys, before/after risk, proposed trade quantities and native P&L reconciliation
residuals. Futures monetary quote levels and currency present values retain
separate reporting groups. A feasible hedge can retain residual exposure. The
example's eligibility, liquidity limits and zero costs are explicit synthetic
paper assumptions; its proposal does not execute orders or post cash.

### HTTP contract

`POST /api/v1/analytics` accepts the existing
[`LibraryRequest`](../convex/src/library/requests.rs) JSON and returns the
unchanged `convex::library::execute_json` envelope. Product construction, curve
validation/calibration, pricing, risk, hedging and P&L stay in their existing pure
owners. The request supplies immutable inputs; this endpoint performs no provider
or repository lookup and saves no calculation state.

Native success is `{"ok":"true","result":...}`. Native calculation or input
failure is `{"ok":"false","error":...}` at HTTP 200; always inspect `ok`.
Transport/authentication failures use HTTP error statuses. The existing HTTP
boundary limits requests to 2 MiB and results to 4 MiB; an oversized result
returns HTTP 413 with `ANALYTICS_RESPONSE_TOO_LARGE`. Work uses the blocking
pool with immediate admission bounded by the engine's `max_concurrent_calcs`
setting (default 4, maximum 64) per router. Exhausted capacity returns HTTP 429.
Cancellation retains calculation and access admissions until the worker exits.

The default router denies access. An authenticated deployment requires the
`Analyst` calculate role, all four `Reference`, `Market`, `Books`, `Results` data
scopes, and explicit `*` resource grants for instruments, curves, sources and
books. Named-only grants cannot use this complete supplied-input endpoint;
per-operation entitlement discovery is not implemented. Current grants are
checked before dispatch and again before returning the result. Existing origin,
identity and request quotas also apply. Only the isolated demo command selects
`local_development` access.

This extends the supplied-input HTTP workflow. The managed replay runtime still
has its documented fixed-bullet scope, and the production executable still
requires a completed provider composition. The example uses rate futures; full
delivery-basket/CTD operations remain available through the native library
contract and this same endpoint. Live provider admission, distributed ownership,
measured latency and production release qualification remain separate work.

## Core snapshots, input updates and restart recovery

Run the persisted version of the same six-trade workflow:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_snapshot_workflow.py
```

It imports captured synthetic reference revisions, whole curve recipes and fixing
history through the existing ports, then uses the server to assemble immutable
snapshots and persist 20 native results. It verifies exact parity with the original
supplied-input endpoint, selects updated reference/curve revisions, and publishes
a SOFR correction. OIS consumes the eligible correction; the overnight future
retains its actual FirstPublished revision. After stopping and restarting the
server, it reads every original result and recomputes pricing, P&L and the hedge
from saved inputs with identical outputs. Both processes stop automatically.

The [core persistence contract](../convex-engine/CORE-WORKFLOW.md) describes the
input definitions, endpoint shapes, backend responsibilities and recovery rules.
The original `core_workflow.py` remains the smaller supplied-input example.

| Endpoint | Behavior |
|---|---|
| POST /api/v1/analytics/curves | Store one immutable complete `SnapshotCurveRecipe` |
| POST /api/v1/analytics/fixings | Atomically store a batch of immutable `PublishedFixing` revisions |
| POST /api/v1/analytics/snapshots | Resolve a `CoreSnapshotPlan` through reference/curve/fixing ports and publish the complete captured record |
| GET /api/v1/analytics/snapshots/{snapshot_id} | Read the saved native definition, full recipes, history and lineage |
| POST /api/v1/analytics/snapshots/{snapshot_id}/run/{calculation} | Run a saved named calculation; `{}` permits reuse, `{"recompute":true}` uses saved inputs again |
| GET /api/v1/analytics/results/{result_id} | Return the exact stored native envelope without recalculation |

Reference definitions use the existing reference transaction/history contract's
`calculation` master resource. Input publishers need `ReferenceWriter`, market
scope and explicit curve/source wildcard grants. Calculation definitions and
their reference receipts also require the core book/data grants. Core calculations
retain the earlier Analyst/full-grant requirement; saved reads require Reader or
Analyst with all those data/resource grants. Source publication and calculation
are separate permissions. Current grants are rechecked before returning results.

The executable composes local redb with the existing durable Unix filesystem
snapshot adapter in `<storage_path>.core-snapshots`. Back up both together with the
server stopped. Other hosts can supply implementations of the same existing
ports. Unsupported adapter methods fail explicitly. The offline
`convex-core-inputs DATABASE CAPTURED_INPUTS.json` importer also requires a stopped
server; interrupted imports recover by retrying the identical captured records.

This is a synthetic, persisted core workflow. Production startup, managed live
provider composition, distributed ownership and measured latency retain their
existing release requirements.

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
native pricing/risk, constrained hedge proposals and causal P&L on confirmed
stream frames. It uses exact configured input bindings and the existing ports,
with freshness gating and restart recovery. Run
`PYTHONDONTWRITEBYTECODE=1 python3 tools/core_live_workflow.py` for the complete
synthetic example, including stale input and interrupted native-result recovery.

[Complete book releases](../../tools/BOOK-UPDATES.md) add position and reference
updates through the same reference/snapshot/result ports and worker. They bind
current risk and hedge inputs to one complete native P&L closing point. Run
`PYTHONDONTWRITEBYTECODE=1 python3 tools/core_book_workflow.py` for fill/reference
updates, consistency rejection, commit faults and restart recovery.

The [event adapter](../../tools/BOOK-EVENTS.md) admits actual fills, lifecycle and
cash facts through those same ports and publishes the existing book record after
native P&L validation. Run
`PYTHONDONTWRITEBYTECODE=1 python3 tools/core_event_workflow.py` for an IRS fill,
official futures settlement, partial/completing cash receipts and restart recovery.

The [lifecycle risk binding](../../tools/LIFECYCLE-RISK.md) adds remaining
exposure and outstanding owned cash claims to those same native snapshots.
Run `PYTHONDONTWRITEBYTECODE=1 python3 tools/core_lifecycle_workflow.py` for a
50% OIS termination, partial/completing receipts, five-factor hedges, causal P&L
and recovery after an interrupted hedge result. New hedge trades retain fresh
instrument risk; actual current claims stay with their original owner.

Run `PYTHONDONTWRITEBYTECODE=1 python3 tools/core_cds_workflow.py` for
[CDS default and actual recovery](../../tools/CDS-EVENTS.md). It verifies unpriced
default blocking across restart, recovery claim risk, defaulted-entity candidate
exclusion, cash/P&L reconciliation and interrupted-result recovery.

## Loading and validation

Configuration precedence is compiled defaults, selected TOML file, then environment overrides. With no explicit path, the server checks `config/convex.toml`; an absent default file permits environment-only startup. A missing explicit path, unreadable file, invalid setting, unknown TOML key or unknown `CONVEX_` environment variable fails startup. Each supplied file must itself be valid before overrides apply. TOML input is limited to 64 KiB.

These process settings require a restart. Versioned quantitative/integration/policy settings use the separate [configuration lifecycle](../convex-engine/CONFIGURATION.md), including strict schema, actual numerical validation, staging, CAS activation, expiring freshness exceptions and retained rollback. No inline secret-valued setting is supported. Startup inspection redacts paths, and configuration errors omit submitted values and file contents.

| TOML key | Environment variable | Behavior |
|---|---|---|
| profile | CONVEX_PROFILE | `production` by default; explicit `demo` or loopback `replay` can start |
| host | CONVEX_HOST | IPv4/IPv6 literal; default `0.0.0.0` must be changed to loopback for the current HTTP executable; hostnames fail |
| port | CONVEX_PORT | Integer 0–65535; default 8080; zero selects an ephemeral port |
| websocket_enabled | CONVEX_WEBSOCKET_ENABLED | Boolean; environment accepts true/false or 1/0, case insensitive; default true |
| websocket_port | CONVEX_WEBSOCKET_PORT | Optional; accepted only when WebSockets are enabled and it equals the HTTP port |
| storage_path | CONVEX_STORAGE_PATH | Nonempty local redb path; default `./data/convex.redb`; parent directory must exist |
| runtime_plan | CONVEX_RUNTIME_PLAN | Required explicit plan for replay; fixed-bullet mode, exact build, no seed events |
| quotes_file | CONVEX_QUOTES_FILE | Required canonical JSONL replay in replay mode; unsupported in other modes |
| curves_file | CONVEX_CURVES_FILE | Rejected: file provider composition is not implemented |
| fixings_file | CONVEX_FIXINGS_FILE | Rejected: file provider composition is not implemented |
| bonds_file | CONVEX_BONDS_FILE | Rejected: file provider composition is not implemented |
| access.mode | CONVEX_ACCESS_MODE | Required by default; explicit local_development only for isolated loopback |
| access.policy_file | CONVEX_ACCESS_POLICY_FILE | Bounded versioned grants, atomically reloaded each second |
| access.tls_boundary | CONVEX_TLS_BOUNDARY | local_only or proxy; executable upstream stays loopback |
| access.cors_origins | CONVEX_CORS_ORIGINS | JSON array of exact origins; empty denies cross-origin browser requests |
| access.introspection | TOML only | Explicit endpoint/issuer/API audience/client and secret reference; see the access guide |

Startup logs the effective redacted configuration and, after a successful bind, the actual listener address. A disabled WebSocket setting removes `/ws`, `/api/v1/ws` and `/api/v1/ws/status`.

## Demo output labels

Demo HTTP responses, including WebSocket upgrades, carry `x-convex-profile: demo` and `x-convex-data-quality: demo`. These headers are exposed to browser clients through CORS. Every demo WebSocket JSON message additionally carries `profile: "demo"` and `data_quality: "demo"`; existing economic fields retain their semantics. The numeric `quality` field is not a certification label.

Router construction uses the engine’s configured reference repository. Provider capability validation occurs on the server startup path. The replay runtime calculates actual fixed-bullet results with per-measure quality and immutable lineage; complete products, live certification and production release gates remain open.

## Migrating existing startup code

ServerConfig::from_env and apply_env_overrides now return Result. ServerConfig::from_file returns ConfigError, and Server::start returns StartupError. Callers must handle errors. Add an explicit demo profile and local_development access for isolated loopback demonstrations; otherwise compose the required verifier and policy. Previously ignored invalid values, unsupported settings and hostnames must be corrected; they no longer fall back silently.

The startup configuration changes preserve pricing conventions. The reference repository schema and write requirements are described below.

`convex_server::compose(config)` is the single composition root. The executable and `tests/application_path.rs` both call it. It validates startup settings, opens storage and builds the engine and server. It does not start or bind anything. Call `composition.engine.start()`, then `composition.server.start()` or `serve_until`. Adapter failures while opening storage, snapshots, the result publisher, the quote replay or the build identity return `StartupError::Composition { stage, .. }`. Configuration, access, I/O and engine-build failures keep their own `StartupError` variants. Dropping a stopped composition releases the redb file, so calling `compose` again reopens the same state. The Fly showcase image is described in [deploy/showcase](../../deploy/showcase/README.md).

## Authoritative bond reference data

Bond CRUD and pricing reads use the same `ReferenceRepository` configured in the engine's `StorageAdapter`. The executable uses redb. Creating another router for the same engine shares its references; it does not allocate another bond store. `MemoryReferenceRepository` is available for volatile fixtures and explicit embedded demonstrations.

Each mutation atomically retains current state, immutable versions and a command/receipt for retry recovery. Versions start at 1. Updates and deletion increment the version; deletion retains a tombstone. A deleted internal identity cannot be recreated with a reset version. Exact historical lookup returns the requested version or absence. The receipt records the reference change and its commit time; authenticated actor audit, financial events and outbox transport remain separate work.

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

## Reference schema and Rust migration

The redb reference tables are `reference_current_v2`, `reference_history_v2`, `reference_commits_v2`, `reference_master_current_v2` and `reference_master_history_v2`. A complete earlier three-table repository adds the master tables atomically while retaining exact prior content and receipts. Terms without `contractual_terms` require an explicit verified update before valuation. Back up before upgrade and restore into isolation to roll back. A nonempty previous unversioned `bonds` table fails startup with an explicit export/import requirement. Preserve its database and export full terms with a compatible reader before importing them into a new versioned repository; do not erase the existing table to bypass validation. Unversioned data needs explicit export/import; missing prior history is not reconstructed.

Implement `ReferenceRepository` to slot in another backend and run both shared reference conformance suites. Required historical/master methods are described in the [port guide](../convex-ports/REFERENCE_MASTER.md#http-and-embedded-use). `ReferenceDataProvider` covers auxiliary issuer, rating and ETF sources. Default router helpers deny protected access. Supply `create_router_with_access` or `create_router_with_access_and_portfolios` with the verified boundary; the access guide describes embedding migration.

`create_test_storage` names the file-backed redb test helper accurately. Prefer `create_redb_storage` with an owned temporary directory when deterministic file cleanup is required. The validated analytics risk wrapper is now `ValidatedRiskResponse`; the former name has no compatibility alias. Pure pricing formulas, units and signs are unchanged.

The [local redb storage guide](../convex-ext-redb/README.md) documents immutable configuration history, atomic active selection, schema migration and the `convex-redb` offline backup/restore commands. Pricing saves require the next version, deletion explicitly fails and audit queries apply their filters. Managed rollback selects retained content through a new activation. Shared persistence and distributed ownership remain separate work.

## Market replay and file inputs

[The file adapter contract](../convex-ext-file/README.md) documents sequenced quote replay, image/delta recovery, exact units/timestamps, field clears, bounds and snapshot-file migration. Run its standalone trace example before wiring an ingestion adapter. File snapshots require recorded timestamps and sources; missing files or malformed records fail explicitly. The owned runtime uses snapshot-consistent pricing; existing v1 synthetic-curve behavior remains the enforced G04 production gap.

## Configuration operator

Build `cargo build --locked -p convex-server --bins` to obtain `convex-config` alongside the server. Its [runnable configuration guide](../convex-engine/CONFIGURATION.md) creates a new fictional workspace and shows schema, validate/explain, diff, stage, activate, rollback, exact-version/history/effective inspection and plan export. The CLI opens local redb exclusively, so stop the server for these operations. Embedded hosts can apply hot policies through the same engine use cases; the server adds only read-only `GET /api/v1/configuration/schema` and `GET /api/v1/configuration/:id?as_of=...&known_at=...`. There is no network configuration mutation route. These network inspection routes share [current access control and quotas](ACCESS.md). The offline CLI still relies on trusted local filesystem/operator access; authenticated shared administration remains subsequent work.

On-demand nominal fixed/zero quotes now use the [shared pure bond route](../../docs/BOND-QUOTES.md). Supply an observed price and exact optional discount/swap/government inputs; no demo curves, assumed volatility or model/par price are generated. Missing optional measures have structured errors. Full historical contexts, live admission and complete product/model certificates retain their separate gates.


The [bond default/recovery example](../../tools/BOND-EVENTS.md) reuses these ports
for principal-only recovery, separately earned cash, lifecycle risk, hedge
proposals, complete causal P&L and restart recovery. Run
`PYTHONDONTWRITEBYTECODE=1 python3 tools/core_bond_workflow.py`.
Unknown actual recovery withdraws readiness; actual recovery and receipts retain
the original native claim owners. No additional route or persistence schema is used.


The [valuation-date roll example](../../tools/DATE-ROLLS.md) reuses the existing
reference, curve, fixing, snapshot and result ports for scheduled coupons,
amortization, maturity, outstanding claim risk, hedge proposals and causal P&L.
Run `PYTHONDONTWRITEBYTECODE=1 python3 tools/core_roll_workflow.py`. Captured
fixings respect each historical point's knowledge cutoff; matured securities
leave fresh hedge candidates while actual payments remain separate events.
The example checks missing-input admission, source refresh, exact retry and
restart after a committed hedge loses its acknowledgement.
