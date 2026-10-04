# Convex local redb storage

This adapter provides recoverable storage for one process on a local filesystem. References, configuration history, curve snapshots and audit identities use real redb transactions. It supports an embedded analytics service and test fixtures; a shared deployment needs the separately planned shared database adapter.

Create the stores once with `create_redb_storage(path)` and share the resulting `StorageAdapter`. The engine and API already use its authoritative reference repository. The pricing configuration repository is persistent storage; configuration staging, authorization, activation and engine adoption belong to subsequent application tasks.

## Transactions and capabilities

| Operation | Local contract |
|---|---|
| Reference commit | Bond terms, entity/calendar/convention definitions, identifier assertions, their immutable histories and the original command/result for retry recovery commit together. Expected versions and idempotency keys are required. |
| Pricing configuration save | Current content and immutable history commit together. A new ID starts at version 1; an update supplies exactly current version + 1. |
| Configuration retry | Identical content at a retained ID/version succeeds without changing current content. Different content at that identity, or a skipped version, returns `TraitError::Conflict`. |
| Configuration reads | `get_version` returns only the requested retained version. `list_versions` reports retained versions in ascending order. Actor and change reason are absent because the existing save port does not supply them. |
| Configuration deletion | Returns `Unsupported`. Save a new version with `active: false` to deactivate; reactivate or roll back economic content through another new version. |
| Curve configuration | A mutable local projection with deterministic ID ordering; this port does not promise configuration history or model validation. |
| Curve snapshot | Immutable by curve ID and timestamp. An identical retry succeeds; changed content at the same identity conflicts. Time ranges are inclusive and latest lookup stays within the selected curve. Explicit retention deletion removes timestamps strictly before its cutoff. |
| Audit append | Immutable by caller-supplied ID. Identical retries succeed; different content at the same ID conflicts. This append is a separate transaction from other stores. |
| Audit query | All supplied event, entity, actor and inclusive time constraints are applied before paging. Items and the filtered total come from one read snapshot, ordered by audit ID. |
| Overrides | Mutable local projection, approved/non-expired selection and pending selection. `get_history` returns `Unsupported`; this port does not supply actor-aware deletion or an atomic approval audit. |
| Shared writers and financial ledger | Unsupported by this adapter. Separate trait calls cannot form one event/projection/audit/outbox transaction. |

Reference text search uses the existing `BondFilter.text_search` contract: case-insensitive containment in description, issuer name, internal ID, ISIN and CUSIP, together with every supplied structured filter. The reference list's `total` counts matching active records. There is no generic full-text search service, relevance ranking or query language. Audit text fields use exact matching.

Every write explicitly selects redb `Durability::Immediate`. A preparation failure aborts the transaction; a failed commit acknowledgment returns `UnknownOutcome`. Resolve a reference outcome by its idempotency key, a configuration outcome by exact ID/version/content, an audit outcome by ID/content, and a curve snapshot by curve ID/timestamp. Mutable projections have no general deduplication receipt; reconcile them before retrying an uncertain write. The locked redb 4.1.0 [transaction source](https://github.com/cberner/redb/blob/6ed1f981ba4deab0b2adbdd7bccb46ec409b2191/src/transactions.rs) defines the durability boundary. It depends on the operating system and storage honoring synchronization; it does not establish host, disk or site failure protection.

redb calls are synchronous even inside these async port methods. Run persistence work away from a latency-sensitive calculation loop. Sharing one handle across threads is supported; opening the file from another process fails while its exclusive lock is held. Keep the database on a local filesystem.

## Bounds and queries

Non-reference records are limited to 128 KiB serialized. There are at most 100,000 current configurations, curve configurations or overrides per table, and 1,000,000 configuration history entries, curve snapshots or audit entries per table. Curve snapshots contain 1–4,096 strictly increasing positive day tenors and finite rates; negative rates remain valid. These checks validate storage inputs, not calibration economics or supported pricing models.

Pages accept limits from 0 through 1,000. Limit zero returns only the filtered total. Oversized limits and inverted time ranges fail with `InvalidInput`; an offset beyond the result returns an empty page with the same total. Offsets are deterministic within one snapshot; separate page requests during writes do not provide a frozen cursor. Configurations and curve configurations have bounded complete-list operations rather than a fabricated pagination interface.

Pricing configuration JSON rejects unknown settings, including nested filters, pricing parameters, bid/ask settings and curve selections. Finite floating model values retain their existing units. A stored real-yield index ratio uses a decimal string to preserve full decimal precision independently of the workspace's `serde-float` feature; the native and existing public config wire types retain their conventions. Older retained numeric ratios remain readable at their recorded precision.

## Schema migration

The Convex application schema is version 1. Its `convex_schema` marker is independent of the redb physical file format and the reference records' schema version 2. `create_redb_storage` and `RedbConfigStore::new` validate or migrate it before accepting configuration work. The latter constructor now returns `Result`; propagate it with `?`.

For a database without a marker, one transaction imports each existing `pricing_configs` record into immutable history at its actual stored positive version and writes the marker. It preserves the current record's original bytes. If only version 7 remains, versions 1–6 return `None`; their lost content is not reconstructed. Invalid records, identity mismatches or orphaned history abort the whole migration. A newer marker, missing required tables, or disagreement between current configuration and its retained latest version fails startup.

The [reference master guide](../convex-ports/REFERENCE_MASTER.md#stored-data-migration-and-recovery) covers atomic adoption of the two master tables, readable earlier records, explicit complete-term updates and exact retained retries. Partial table sets fail startup. Reference and configuration schema versions are independent.

Nonempty unversioned `bonds` tables still require the explicit export/import process in the [reference migration guide](../convex-server/README.md#authoritative-bond-reference-data). Migration refuses them before writing the application marker. No unsupported aliases, approximate financial terms or invented historic timestamps are introduced.

Stop the service and take a backup before upgrading. A rollback restores that backup into a separate destination and runs the earlier compatible application against it after reconciliation. There is no lossy automatic schema downgrade or mixed-version writer protocol. Configuration content rollback uses a new version; database recovery and application activation are separate operations.

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

## Verification

```sh
cargo test --locked -p convex-ext-redb
cargo test --locked -p convex-ports --test storage_contracts
```

The [portable storage scenarios](../convex-ports/tests/support/storage_conformance.rs) cover exact versions, conflicting and concurrent writes, retry identity, decimal precision, audit filter/count/page agreement, snapshot ordering and explicit absent capabilities. The [restore seed](tests/fixtures/restore_seed.json) contains fictional configurations, references, curves, overrides and audit entries. Recovery tests start separate processes, exit after a committed write without dropping handles, reopen, reconcile, compact offline, verify integrity, reconcile again, back up, restore and reconcile all retained content and reference retry results. They also exercise the maintenance executable and reject invalid backups and existing restore targets. These tests do not certify power-loss behavior, distributed persistence or a production release gate.
