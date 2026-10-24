# Convex local redb storage

This adapter provides recoverable storage for one process on a local filesystem. References, configuration history, curve snapshots and audit identities use real redb transactions. It supports an embedded analytics service and test fixtures; a shared deployment needs the separately planned shared database adapter.

Create the stores once with `create_redb_storage(path)` and share the resulting `StorageAdapter`. The engine and API use its authoritative reference repository. The [configuration lifecycle](../convex-engine/CONFIGURATION.md) adds engine validation, immutable staging, atomic active selection, expiry and retained rollback through `StorageAdapter.configuration`. Authorization and shared deployment remain separate work.

## Transactions and capabilities

| Operation | Local contract |
|---|---|
| Reference commit | Bond terms, entity/calendar/convention definitions, identifier assertions, their immutable histories and the original command/result for retry recovery commit together. Expected versions and idempotency keys are required. |
| Pricing configuration save | Current content and immutable history commit together. A new ID starts at version 1; an update supplies exactly current version + 1. |
| Configuration retry | Identical content at a retained ID/version succeeds without changing current content. Different content at that identity, or a skipped version, returns `TraitError::Conflict`. |
| Configuration reads | `get_version` returns only the requested retained version. `list_versions` reports retained versions in ascending order. Actor and change reason are absent because the existing save port does not supply them. |
| Configuration deletion | Returns `Unsupported`. `ConfigStore` can retain another version with `active: false`; managed configuration documents require active content and do not silently deactivate a running selection. |
| Application configuration stage | Complete immutable document, actor/reason/time, staging request and exact pricing history commit together. Staging leaves the active pointer unchanged. |
| Application activation | Active pointer CAS, immutable activation/audit record and original request receipt commit together. Hot mode rejects restart-required changes inside the same transaction. |
| Application selection/rollback | Pointer, original command/digest and exact staged content are read coherently. Rollback selects a retained activation's content and appends a new sequence; its content version can decrease. |
| Curve configuration | A mutable local projection with deterministic ID ordering; this port does not promise configuration history or model validation. |
| Curve snapshot | Immutable by curve ID and timestamp. An identical retry succeeds; changed content at the same identity conflicts. Time ranges are inclusive and latest lookup stays within the selected curve. Explicit retention deletion removes timestamps strictly before its cutoff. |
| Market and core snapshot | Immutable by content digest, one transaction per publication. An identical retry succeeds; different bytes under a retained digest conflict. Reads re-verify identity and canonical bytes, so tampered content fails rather than reads as absent. |
| Runtime result archive | Immutable by `(runtime_id, revision)`, one transaction per result. An identical redelivery succeeds; different content under a retained key conflicts. The server archives replay results only; live results are not stored. |
| Audit append | Immutable by caller-supplied ID. Identical retries succeed; different content at the same ID conflicts. This append is a separate transaction from other stores. |
| Audit query | All supplied event, entity, actor and inclusive time constraints are applied before paging. Items and the filtered total come from one read snapshot, ordered by audit ID. |
| Policy exceptions | Scoped approved freshness exceptions belong to immutable configuration content. Expiry is evaluated by engine use cases at explicit cutoffs; retained actor/reason/approval/expiry history is never mutated away. |
| Shared writers and financial ledger | Unsupported by this adapter. Separate trait calls cannot form one event/projection/audit/outbox transaction. |

Reference text search uses the existing `BondFilter.text_search` contract: case-insensitive containment in description, issuer name, internal ID, ISIN and CUSIP, together with every supplied structured filter. The reference list's `total` counts matching active records. There is no generic full-text search service, relevance ranking or query language. Audit text fields use exact matching.

Every write explicitly selects redb `Durability::Immediate`. A preparation failure aborts the transaction; a failed commit acknowledgment returns `UnknownOutcome`. Resolve a reference/activation outcome by its original idempotency key, staging by exact retained envelope, pricing save by exact ID/version/content, audit by ID/content, and a curve snapshot by curve ID/timestamp. Exact activation retries return their original receipt without changing a newer pointer. Mutable curve projections have no general deduplication receipt; reconcile them before retrying an uncertain write. The locked redb 4.1.0 [transaction source](https://github.com/cberner/redb/blob/6ed1f981ba4deab0b2adbdd7bccb46ec409b2191/src/transactions.rs) defines the durability boundary. It depends on the operating system and storage honoring synchronization; it does not establish host, disk or site failure protection.

redb calls are synchronous even inside these async port methods. Run persistence work away from a latency-sensitive calculation loop. Sharing one handle across threads is supported; opening the file from another process fails while its exclusive lock is held. Keep the database on a local filesystem.

## Bounds and queries

Pricing/curve/audit records are limited to 128 KiB serialized. There are at most 100,000 current pricing or curve configurations per table, and 1,000,000 pricing history entries, curve snapshots or audit entries per table. Managed documents are bounded to 2 MiB plus 16 KiB staging metadata, 4,096 documents/staging requests, 64 active identities and 100,000 activations/activation requests. Market and core snapshots are at most 16 MiB of canonical bytes, with at most 100,000 of each. The result archive holds at most 1,000,000 results of up to 512 KiB each. Curve snapshots contain 1–4,096 strictly increasing positive day tenors and finite rates; negative rates remain valid. These checks validate storage inputs, not calibration economics or supported pricing models.

Pages accept limits from 0 through 1,000. Limit zero returns only the filtered total. Oversized limits and inverted time ranges fail with `InvalidInput`; an offset beyond the result returns an empty page with the same total. Offsets are deterministic within one snapshot; separate page requests during writes do not provide a frozen cursor. Configurations and curve configurations have bounded complete-list operations rather than a fabricated pagination interface.

Pricing configuration JSON rejects unknown settings, including nested filters, pricing parameters, bid/ask settings and curve selections. Finite floating model values retain their existing units. A stored real-yield index ratio uses a decimal string to preserve full decimal precision independently of the workspace's `serde-float` feature; the native and existing public config wire types retain their conventions. Older retained numeric ratios remain readable at their recorded precision.

## Schema migration

The Convex application schema is version 1. Its `convex_schema` marker is independent of the redb physical file format and the reference records' schema version 2. `create_redb_storage` and `RedbConfigStore::new` validate or migrate it before accepting configuration work. The latter constructor returns `Result`; propagate it with `?`.

CVX-010 adds `configuration_documents_v1`, `configuration_active_v1`, `configuration_activations_v1`, `configuration_activation_requests_v1` and `configuration_stage_requests_v1` in one transaction with `convex_schema.configuration_activation=1`. A newer activation marker fails; no prior latest-pricing record becomes active automatically. The unused mutable override API/adapter is removed. Existing physical `overrides` tables remain untouched and included in physical backups; they are not promoted into approved policies. Reconcile/export retained data offline before an explicit new staging decision. Snapshot/checkpoint capture is optional for older content; new captured records require the new application and cannot be processed by older writers.
Reopening an adopted activation schema performs read-only table/capacity checks. Missing tables, content/receipt count disagreement and unmarked partial table sets fail; startup never recreates them to conceal lost history.

For a database without a marker, one transaction imports each existing `pricing_configs` record into immutable history at its actual stored positive version and writes the marker. It preserves the current record's original bytes. If only version 7 remains, versions 1–6 return `None`; their lost content is not reconstructed. Invalid records, identity mismatches or orphaned history abort the whole migration. A newer marker, missing required tables, or disagreement between current configuration and its retained latest version fails startup.

The [reference master guide](../convex-ports/REFERENCE_MASTER.md#stored-data-migration-and-recovery) covers atomic adoption of the two master tables, readable earlier records, explicit complete-term updates and exact retained retries. Partial table sets fail startup. Reference and configuration schema versions are independent.

Nonempty unversioned `bonds` tables still require the explicit export/import process in the [reference migration guide](../convex-server/README.md#authoritative-bond-reference-data). Migration refuses them before writing the application marker. No unsupported aliases, approximate financial terms or invented historic timestamps are introduced.

Stop the service and take a backup before upgrading. A database rollback restores that backup into a separate destination and runs the earlier compatible application against it after reconciliation. There is no lossy automatic schema downgrade or mixed-version writer protocol. Managed content rollback appends a new activation selecting an exact retained version; it is separate from database recovery. Snapshots and the result archive are tables in the same database, so one backup covers the owned runtime. Servers run on Linux and Windows; offline backup and restore remain Unix-only.

## Backup and restore

Build the maintenance executable alongside the server:

```sh
cargo build --locked -p convex-ext-redb --bin convex-redb
```

After closing every database handle, run these commands with paths selected for the deployment:

```sh
convex-redb backup /srv/convex/convex.redb /srv/convex/backups/before-upgrade
convex-redb migrate /srv/convex/convex.redb
convex-redb restore /srv/convex/backups/before-upgrade /srv/convex/restore-check.redb
```

`backup` requires a new directory. It opens the source under redb's exclusive lock, verifies integrity and the supported application marker, and copies the entire physical file while retaining that lock. The directory contains `data.redb` and a final `manifest.json` with format, application schema, byte count and SHA-256. All tables, reference retry records and exact stored bytes are included. A live database or an existing backup destination fails. If source integrity required repair, reconcile the source before making a backup. The expanded recovery fixture normalizes allocator/free-page state with `convex-redb compact DATABASE`, verifies integrity and then compares every retained business record and retry receipt before backup. Compaction opens the closed file under the exclusive lock and performs redb’s physical compaction; its JSON reports `business_records_reconciled: false`. Compare exact retained versions, commands, curves, configurations and audit records again after maintenance. Compaction success is not logical reconciliation. The backup repair guard remains fail closed; a false integrity result must be investigated and reconciled before retrying backup.

`restore` requires a new database filename. It checks the bounded manifest, streams and hashes into an isolated temporary directory, validates redb integrity and the application marker, then publishes with a same-filesystem hard link that cannot overwrite an existing destination. The backup remains untouched. Incomplete, corrupt, truncated or unsupported backups fail before publication. A synchronization/cleanup error after publication returns `UnknownOutcome`; inspect the destination before retrying. The supported recovery platform is Unix with local filesystem locking, hard links and directory synchronization. Other platforms explicitly fail this recovery capability.

The maximum backup file is 64 GiB, the manifest is at most 8 KiB, and copying uses a 64 KiB buffer. Backup directories have mode 0700 and new backup/restored files mode 0600 on Unix. SHA-256 detects corruption; it is not authentication or encryption. Deployment policies must supply encrypted storage, access control, retention and isolated restore exercises. This maintenance path does not provide online backups, replication, point-in-time recovery or a measured recovery-time guarantee.

If integrity requires repair, retain an offline physical copy of the closed source before maintenance. `convex-redb repair DATABASE` explicitly invokes redb's physical integrity check/repair and reports `integrity_was_clean` and `business_records_reconciled: false`. A false clean flag means repair occurred, not a clean original file. Compare exact retained business content/history/retry receipts after any repair; then require a clean integrity result before backup. The expanded local fixture observed one repaired allocator-state check after compaction, followed by clean checks; this is not an upstream defect diagnosis or universal recovery guarantee. Backup/restore still reject a file repaired in their own checks. The recovery suite now verifies staged documents, activation/rollback history, original receipts and coherent active content alongside the prior reference/configuration/curve/audit records after unclean exit, maintenance and isolated restore.

## Verification

```sh
cargo test --locked -p convex-ext-redb
cargo test --locked -p convex-ports --test storage_contracts
```

The [portable storage scenarios](../convex-ports/tests/support/storage_conformance.rs) cover exact versions, conflicting and concurrent writes, retry identity, decimal precision, audit filter/count/page agreement, snapshot ordering and explicit absent capabilities. The [activation scenarios](tests/configuration_activation.rs) cover eight-writer CAS, coherent selection, immutable staging, original retries, retained rollback and reopen. The [restore seed](tests/fixtures/restore_seed.json) contains fictional configurations, references, curves and audit entries. Recovery tests start separate processes, exit after a committed write without dropping handles, reopen, reconcile, compact offline, verify integrity, reconcile again, back up, restore and reconcile retained content and reference retry results. They also exercise the maintenance executable and reject invalid backups and existing restore targets. These tests do not certify power-loss behavior, distributed persistence or a production release gate.
