# Convex server configuration

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
