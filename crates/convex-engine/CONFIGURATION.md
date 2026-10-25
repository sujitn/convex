# Configuration activation and operator workflow

CVX-010 adds immutable application settings, numerical validation, staging,
atomic activation, retained rollback and expiring policy exceptions to the
[owned pricing runtime](RUNTIME.md). The current supported calculation is a
reference-admitted fixed bullet priced from its observed mid, with explicitly
supplied curves and clean price, dirty price, accrued interest and mid yield.
This is local development capability. The production server profile remains
rejected; complete calibration, bond/CDS/IRS/futures analytics, shared ownership,
authenticated administration and measured latency have separate requirements.

The separation is small: provider-neutral DTOs and `ConfigurationRepository`
live in `convex-ports`; `ConfigurationUseCases` resolves dependencies and runs
the existing numerical path; the redb adapter owns transactions; the server
composes actual capabilities. No database, queue, vendor session or credential
handle enters a pricing context. An alternate backend implements the same
coherent read, CAS, immutable history and retry semantics.

## Start with a fictional workspace

The example creates a **new directory**, seeds fictional reference terms and
exports two explicit configurations, numerical fixtures, one canonical quote
image and activation/rollback commands. It creates no active pointer and does
not start a server. Use an unused directory; it refuses an existing destination.

```sh
cargo build --locked -p convex-server --bins
cargo run --locked -p convex-server --example configuration_fixture -- /tmp/convex-config-example
target/debug/convex-config schema > /tmp/convex-config-example/schema.json
target/debug/convex-config /tmp/convex-config-example/inputs.redb validate /tmp/convex-config-example/document1.json /tmp/convex-config-example/fixtures.json /tmp/convex-config-example/quotes.jsonl
target/debug/convex-config /tmp/convex-config-example/inputs.redb stage /tmp/convex-config-example/stage1.json /tmp/convex-config-example/fixtures.json /tmp/convex-config-example/quotes.jsonl
target/debug/convex-config /tmp/convex-config-example/inputs.redb activate /tmp/convex-config-example/activate1.json /tmp/convex-config-example/fixtures.json /tmp/convex-config-example/quotes.jsonl
target/debug/convex-config /tmp/convex-config-example/inputs.redb effective CONFIG-RESTORE 2026-10-02T15:00:00Z 2026-10-02T15:00:00Z
target/debug/convex-config /tmp/convex-config-example/inputs.redb export-plan CONFIG-RESTORE > /tmp/convex-config-example/plan.json
```

If `CARGO_TARGET_DIR` is set, use its `debug/convex-config` binary. `validate`
and `explain` run identical application checks and return a nonzero exit for
an invalid document. They resolve exact references, check every configured
curve role/provider/build/backend, build an unpublished snapshot and execute
actual numerical fixtures. They neither stage content nor publish a snapshot
nor alter the active pointer. Opening a new storage adapter can create empty
tables/directories; a dry run is not a filesystem-free operation.

The fixture expectation is explicitly 99.25 for the fictional observed clean
mid, at zero tolerance. Operator fixtures require explicit finite expectations,
an absolute tolerance in 0..=1e-8 and at least one assertion for every instrument.
The service never generates its own expected values. All requested measures
must finish Complete/Valid. The CLI checks that each fixture trace is an exact
prefix of the separately opened canonical replay file.

For an actual dataset, import complete references with the
[reference administration example](../convex-server/README.md#authoritative-bond-reference-data)
and supply independently reviewed numerical fixtures, explicit curves, source
capabilities, settlement, cutoffs and executable identity. The generated data
is an illustration, not a production template or vendor certification.

Start the fictional historical replay only after exporting the initial plan:

```sh
CONVEX_PROFILE=replay CONVEX_HOST=127.0.0.1 CONVEX_ACCESS_MODE=local_development \
CONVEX_STORAGE_PATH=/tmp/convex-config-example/inputs.redb \
CONVEX_RUNTIME_PLAN=/tmp/convex-config-example/plan.json \
CONVEX_QUOTES_FILE=/tmp/convex-config-example/quotes.jsonl \
cargo run --locked -p convex-server
```

Read `/health`, `/ready` and `/api/v1/runtime/results` as described in the
runtime guide. Replay readiness means valid historical analytics. Application
cutoffs must include the original activation time; selecting a configuration
recorded after `known_at` fails. `export-plan` retains the document's explicit
bootstrap cutoffs, so verify them before a first start. The actual build ID is
checked; a release artifact uses its compile-time `CONVEX_BUILD_ID`.

## Content, validation and inspection

`convex-config schema` and `GET /api/v1/configuration/schema` return the closed
Draft-2020-12 schema for the supported fixed-bullet document. Runtime checks
also enforce referential integrity, exact revision agreement, capabilities,
time order, finite numerical values and real fixtures. Unknown fields fail,
including a `price_override` setting. A redacted inspection is not an input
document to round-trip into staging.

| Section | Required meaning |
|---|---|
| `pricing` | Exact active `BondPricingConfig` identity/version, observed-mid method, filters and supplied curve roles; timestamps are UTC milliseconds. |
| `process` | Storage/result transport identities, explicit shared-writer requirement and numerical concurrency 1..=64. These are requirements checked against actual composition. |
| `providers` | Source identities and canonical quote/replay/live-image capabilities; optional credential **references**. |
| `runtime` | Runtime namespace, captured/replay/live interpretation, settlement, executable build, measures and existing compiled numerical policy. |
| `assembly` | Exact reference versions, selected quote source/fields, explicit supplied curve revisions and base freshness/skew requirements; bootstrap generation is 1. |
| `overrides` | Approved scoped freshness exceptions with creation/approval/expiry times, actor and reason. No model-price override representation is provided. |

Credentials accept `env://NAME` or `secret://namespace/key`. Inline credentials,
URLs and query strings fail. Inspection and safe errors redact locations as
well as values; no credential resolver or provider SDK is introduced here.
The local replay composition advertises canonical quotes and replay from the
actual opened trace, redb local transactions and acknowledged local JSONL.
It rejects live-provider and shared-writer requirements it cannot satisfy.

The process configuration is separate and retains the existing strict TOML and
environment precedence. Environment variables never change versioned model
terms. `convex-config process validate [TOML]` checks syntax and supported
startup; `process explain [TOML]` exports redacted settings and the explicit
`startup_supported` flag. A valid demo remains unready; production remains
unsupported. Neither command opens a market-data session or database.

## Compile a live configuration from books

`convex-config DB compile SELECTION` reads stored inputs at one cutoff and prints a
configuration document for `validate`, `stage` and `activate`. The selection names
everything the repository does not hold; nothing has a default:

```json
{
  "schema_version": 1,
  "pricing_config": "SHOWCASE-USD",
  "books": ["SHOWCASE-BOOK"],
  "source": "showcase-feed",
  "fields": ["mid_price"],
  "quote_max_age_ms": 600000,
  "max_source_skew_ms": 600000,
  "as_of": "2026-10-22T12:00:00Z",
  "known_at": "2026-10-22T12:00:00Z",
  "mode": "live",
  "settlement_date": "2026-10-22",
  "measures": ["clean_price_mid", "ytm_mid"],
  "numerical_settings": {"policy_id": "convex-bond-router-v1", "bump_bps": 1.0},
  "max_concurrent_calcs": 4
}
```

At the cutoff, compile:
- takes the universe from the live positions of 1 to 64 books (at most 256
  instruments) and pins each instrument's reference version;
- uses the latest version of the stored pricing configuration. It must be active,
  recorded by the cutoff, and apply to every instrument. Its id and version
  identify the configuration;
- pins the fresh recipe revisions of every configured curve role and their
  dependencies;
- requires one quote per instrument from `source`, which must be a provider the
  server composes for `mode`;
- sets the runtime id to `<pricing config>-v<version>`. Each compiled version is a
  separate runtime with its own checkpoint, and clients resynchronise on the new id.

Validation still needs explicit numerical fixtures. The document is checked against
the capabilities the server composes for its mode: a live document needs the
`display-only` result transport and a `live_images` provider. Fixture inputs and
results must be `Valid`. `convex-config --demo DB ...` checks against the demo
profile's composition instead, which also accepts `Demo` quality, the label of
synthetic demonstration data.

A staged version is immutable. To change the universe, store a new pricing
configuration version first, then compile, stage and activate again with the
server stopped.

## Stage, activate and roll back

A staging envelope contains `document`, `idempotency_key`, `actor`, `reason`
and `staged_at`. Validation and warming precede one transaction retaining this
immutable envelope, its request identity and the exact pricing version.
Staging a new version does not select it for the managed runtime. Existing
`ConfigStore` latest-version reads are a storage view; callers requiring managed
selection must use `ConfigurationRepository.active` or the captured runtime.

An activation command contains `id`, `configuration_version`,
`expected_revision`, `idempotency_key`, `actor`, `reason`, `activated_at`,
`mode` and `action`. Initial activation uses `expected_revision: 0` and
`mode: "restart_required"`. Action is `{"kind":"activate"}` or
`{"kind":"rollback","to_revision":1}`. The fixture files show exact JSON.

Activation revalidates/warm-checks content, then atomically compares the
active revision, updates the pointer, appends immutable audit and saves the
original retry receipt. Concurrent writers with the same expected revision
have one winner. An identical retry returns its original result without
rewinding a newer pointer. Reusing its key with another command conflicts.
An uncertain commit is reconciled through that original key and exact retained
content; an unconfirmed outcome remains `CONFIGURATION_UNKNOWN_OUTCOME`.

Content versions and activation sequences are different counters. For example,
v1 → v2 → rollback v1 produces activation sequences 1 → 2 → 3. Rollback selects
retained immutable content after revalidation; it neither deletes audit nor
creates a fictional new economic version. The command must name the retained
activation target and current expected pointer.

After stopping the fictional server and closing its database handle:

```sh
target/debug/convex-config diff /tmp/convex-config-example/document1.json /tmp/convex-config-example/document2.json
target/debug/convex-config /tmp/convex-config-example/inputs.redb stage /tmp/convex-config-example/stage2.json /tmp/convex-config-example/fixtures.json /tmp/convex-config-example/quotes.jsonl
target/debug/convex-config /tmp/convex-config-example/inputs.redb activate /tmp/convex-config-example/activate2.json /tmp/convex-config-example/fixtures.json /tmp/convex-config-example/quotes.jsonl
target/debug/convex-config /tmp/convex-config-example/inputs.redb rollback /tmp/convex-config-example/rollback1.json /tmp/convex-config-example/fixtures.json /tmp/convex-config-example/quotes.jsonl
target/debug/convex-config /tmp/convex-config-example/inputs.redb history CONFIG-RESTORE 0 100
target/debug/convex-config /tmp/convex-config-example/inputs.redb version CONFIG-RESTORE 2
```

Keep the original runtime plan when restarting after these hot policy changes.
Its digest anchors the durable namespace; the checkpoint and activation log
select exact accepted content. Export a new plan for initial startup or an
intentional restart footprint change, not for every hot activation.

| Change | Activation requirement |
|---|---|
| Description, priority, quote age limits, source skew, approved freshness overrides | Hot; current reference/model/provider/process footprint must match. |
| Backend, publisher, worker budget, providers or credentials | Restart. |
| Runtime namespace/mode, settlement, build, measures, numerical policy, model/filter/curve roles, reference/curve/fixing pins, selected quote sources/fields | Restart with compatible explicit plan and process. |

`PricingEngine::activate_configuration` separates durable commit from runtime
acceptance. A running runtime applies only a `hot` activation. A
`restart_required` activation still commits while the runtime runs; the runtime
keeps its accepted selection (or none, if it started without one), and later quotes
keep pricing under it; readiness targets that selection until the restart. The outcome
reports `restart_required: true`, and the next start applies the new selection. A
start whose plan does not fit the active selection is still refused. A hot
activation reports `restart_required: true` unless the runtime accepted that exact
revision. A `hot` activation of a restart-required change is
refused with `CONFIGURATION_RESTART_REQUIRED`. Its outcome includes the committed record and optional accepted
generation/activation revision. A full queue or application failure leaves a
committed selection for reconciliation; it does not guess a rollback. A later
concurrent activation can supersede the command before runtime acceptance.
Readiness closes while a notified desired revision has not been accepted.
New work freezes one coherent selection into its checkpoint before receipt;
previously admitted numerical work completes with its original immutable
snapshot/context. CPU workers never read a mutable configuration pointer.

The CLI is **offline** because redb requires exclusive ownership. Even a hot
CLI command requires a stopped server and applies when it restarts. Embedded
hosts can call the shared use cases and engine application method while running.
`GET /api/v1/configuration/:id` requires explicit `as_of` and `known_at` query
cutoffs and returns the same redacted effective-policy view as the CLI. The CLI's
actor/approval metadata is trusted local input.

A running server offers the same lifecycle over HTTP, behind
[required authentication, roles and quotas](../convex-server/ACCESS.md):

| Route | Body and headers |
|---|---|
| `POST /api/v1/configuration/validate` | `{document, fixtures}`; writes nothing |
| `POST /api/v1/configuration/diff` | `{previous, next}` |
| `POST /api/v1/configuration/:id/stage` | `{document, fixtures, reason, staged_at}`; `Idempotency-Key` |
| `POST /api/v1/configuration/:id/activate` | `{configuration_version, reason, activated_at, mode, fixtures}`; `Idempotency-Key`; `If-Match` with the expected active revision, `0` when none |
| `POST /api/v1/configuration/:id/rollback` | `{to_revision, reason, activated_at, mode, fixtures}`; same headers |
| `GET /api/v1/configuration/:id/history?offset=&limit=` | Activation audit |
| `GET /api/v1/configuration/:id/versions/:version` | One staged version, redacted |
| `GET /api/v1/configuration/:id/diff?from=&to=` | Changed fields between two staged versions, with their reload class; no values |

The HTTP routes validate under the capabilities a start composes for the document,
as the CLI does. The actor is the verified caller's audit fingerprint
(`access:<digest>`, or `local-development`), never a request field. Bodies are at
most 4 MiB, with one to eight fixtures. When the server runs a replay quotes file,
fixtures must prefix it, exactly as in the CLI; only its first 4,096 events, the
most a fixture trace can hold, are read. Otherwise nothing can check them:
the staged and activation records carry `fixture_evidence: "operator_supplied"`.
Replay-checked records omit the field, so earlier records keep their bytes.

## Override expiry and lineage

An exception scopes one selected instrument/source quote key and sets its
freshness limit. It requires a stable ID, actor, reason, creation time,
approval actor/time and finite expiry. Only one exception per scope is allowed.
It applies when approval is known and effective at the supplied cutoffs and
`as_of < expires_at`. At the exact expiry boundary the base policy applies
again; retained configuration and activation history still contain the
exception. Revocation/change requires another approved immutable content
version and activation.

Captured/replay time is supplied; historical processing does not reinterpret
expiry using today's clock. Live mode schedules a deadline at exception
expiry, including when quotes stop arriving, and assembles a new generation.
If the base policy makes inputs stale, the current result fails and the original
last-good timestamp/context remain displayable as Stale. A captured exception
also constrains execution-quality inspection at consumption time; no result
from this runtime grants strategy or execution authorization.

Snapshots retain activation sequence/document digest and only the exceptions
actually applied. Context input versions include configuration activation and
policy exception identities. A claimed policy that disagrees with retained
configuration fails engine admission. Historical accepted selections remain
verifiable after another activation. Credentials do not enter snapshots.

## Bounds, migration and recovery

One document is at most 2 MiB, with up to 256 instruments, one selected quote
per instrument, 64 providers/curves and 256 scoped exceptions. The current
runtime accepts no fixing-dependent instruments and at most four measures.
Validation admits one control operation per use-case instance, one to eight
fixtures, 4,096 aggregate trace events and 8,192 expectations; the warmed
valuation cache is bounded at 256. CLI JSON input/output is bounded and history
uses the existing page limit of 1,000.

The redb repository adds five `configuration_*_v1` tables and the independent
`convex_schema.configuration_activation=1` marker. Application schema stays 1.
Limits are 4,096 staged documents/request identities, 64 active IDs and 100,000
activation/request records. It promotes no older latest-pricing record into
an active selection. Stop and back up before upgrading; see the
[local storage guide](../convex-ext-redb/README.md).
Missing/partial/newer activation schemas fail instead of being recreated.
The explicit offline physical repair path requires exact business reconciliation
and another clean integrity check before backup; the backup guard remains enforced.

The unused mutable `PriceOverride`/`OverrideStore` API and adapter are removed.
Existing physical `overrides` data is preserved by backup and is not treated
as an approved new policy. Export/reconcile it offline before any explicit new
staging decision. Model-price overrides, market corrections and their risk/P&L
economics remain separate work. No retained old table is silently deleted.

Older snapshots/checkpoints omit the new optional capture and retain their
existing serialization/content hashes. New captured records require this
application version; older writers cannot safely process them. Recovery
verifies retained activation/content, restores accepted pending work and its
outbox before selecting a newer activation, and preserves original last-good
lineage. Footprint changes that alter a plan digest require an intentional new
runtime namespace; never reset an old checkpoint to force startup. Database
recovery is an offline backup/restore operation, distinct from content rollback.

This is one local process with threads sharing one handle. It does not provide
distributed leases, shared writers, replication or a mixed-version downgrade
protocol. redb I/O remains synchronous and the ten-second I/O budget is
cooperative; fixture calculations run bounded control work. None establishes
hard preemption, RSS, throughput, tail latency or colocation performance.

## Verification

```sh
cargo test --locked -p convex-engine --test configuration_activation -- --test-threads=1
cargo test --locked -p convex-ext-redb --test configuration_activation
cargo test --locked -p convex-server --test configuration_operator_tests
```

Tests exercise actual numerical work across activation, eight-writer CAS,
exact expiry and live quiet-feed timers, rollback/reopen, missing dependencies,
redaction, strict schema, committed/uncommitted failure recovery, false receipt
and policy rejection, and separate-process CLI operations. Existing runtime,
storage, snapshot and server suites remain required. v1 synthetic-curve behavior
remains the enforced G04 production failure. No production or library release
gate is complete.
