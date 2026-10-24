# Reference master contract

The authoritative `ReferenceRepository` retains instrument terms, entity attributes,
identifier assertions, calendar definitions and convention packs. Memory and redb
implement the same port. Domain validation and history selection live in
`convex-ports`; file decoding belongs to `convex-ext-file`; the engine resolves
resources before calling the existing pure bond analytics. No vendor SDK,
database handle or application clock enters a pricing formula.

## Identity and history

Internal instrument/entity IDs remain stable when a provider changes its symbol.
`ExternalIdentifier` supports ISIN, CUSIP, LEI and a provider value with an explicit
`SourceId` namespace. Values are bounded, case sensitive opaque identifiers.
Format, check digits, licensing and provider certification are separate work.

Every bond and master resource has immutable positive versions and an exact UTC
`recorded_at`. Its economic interval is half open: `[from, until)`. A historical
`ReferenceLookup` supplies both `as_of` and `known_at`. Later-recorded corrections
cannot appear before their information cutoff; no repository substitutes the
current clock. Exact version reads return the requested retained version.

An identifier mapping is one assertion identified by a `ReferenceDataId`. To
remap, close the old assertion with an expected-version update and create a new
assertion targeting the same internal ID, in one command. A correction replaces
the full assertion interval as known at that time. Outside that corrected interval
the superseded assertion does not reappear. Setting `target: null` withdraws an
assertion. Multiple applicable assertions, including duplicates to the same target,
produce `AmbiguousIdentifier`; there is no arbitrary supplier preference.

`resolve_at` returns the exact mapping version, stable target and target version.
This resolves identity; production valuation additionally uses `for_valuation`,
which resolves and checks the bond's pinned issuer, calendar and convention.
Entity parent relationships use exact version pins; missing parents, cycles and
chains beyond 128 entities are rejected.

The existing `resolve`/GET ISIN/CUSIP operations search supplied aliases on current
bond records. They are current projection queries, without historical mapping
semantics. Use `resolve_at` for ingestion, provider mappings and replay. Both
operations use the single `ExternalIdentifier` type; the former identifier type
has been removed.

## Contractual terms and admission

New writes require `BondReferenceData.contractual_terms`. The object supplies an
effective interval, dated/accrual-start date, explicit stub policy and payment lag,
redemption amount, issuer/calendar/convention version pins, matching calendar
dispatch and convention settings, and bounded source/evidence provenance. Existing
issue/maturity/first-coupon dates, frequency, face, currency and coupon remain on
the bond record. Redemption is an amount in the same face units as `face_value`;
coupon is a decimal rate. All reference decimals use exact plain decimal strings.

The current service path supports fixed bullets with regular periods or an explicit
short first coupon boundary. It rejects undeclared irregular periods, long first
periods, short last/long stubs, double stubs, nonzero payment lag and unsupported
frequencies. At most 1,200 regular coupon intervals are admitted. Unknown day-count
labels and conflicts with the typed convention fail. No day-count or frequency
fallback is used. The existing builder consumes dated/first coupon dates,
redemption, currency, calendar, business-day adjustment, EOM, settlement lag and
calendar-day ex-coupon settings, and preflights its schedule before lazy generation.

A fixed callable (`FixedCallable`, `is_callable`) is valued only when its
contractual terms carry `call_terms`: `style` (`american`, `european` or
`bermudan`) and `notice_days` (at most 366). The entries of `call_schedule` carry
only a date and price, so the exercise style is never inferred from them. With
`call_terms` present the schedule must be non-empty, strictly increasing, inside
`(dated_date, maturity_date]`, with positive prices; make-whole entries are refused
as unsupported. `call_terms` is optional and omitted when absent, so stored records
without it keep their exact bytes; a callable without it still records but is never
valued. Puts, sinking funds and floating or inflation terms remain unsupported.

Calendar dispatch is restricted to the actually implemented `weekends_only`,
`sifma` and `target2` paths with rule version `convex-calendar-v1`. Definitions pin
that rule version and a declared inclusive date coverage. Custom/joint calendars,
dated holiday revisions and certification of those compiled rules remain separate
work. Bump the rule version when their semantics change; do not replay an older pin
through a changed implementation.

Resource IDs, versions, issuer identity, calendar code, date coverage and convention
settings must agree. Resources must exist, be known by the information cutoff and
be effective at the economic instant. Calendar settings are never inferred from
currency. The service refuses optional, floating, inflation-linked and other
expanded product families until their complete models are implemented. Native
standalone analytics APIs remain available under their own contracts.

This is reference admission, not a production or library certificate. The
[immutable snapshot application](../convex-engine/SNAPSHOTS.md) now captures these
resolved reference values together with exact configuration, quote continuity,
published fixings and supplied curves. Generation-safe graph publication, real
curve calibration/configuration, complete product workflow admission, corporate-action
lifecycles and provider sessions remain separate.

The closed `book` master definition holds a position snapshot owned by an external
book management system: an owning system, positions (resolved instrument, exact
quantity, `face` or `units`, optional pinned instrument version) and an effective
interval whose bounded end retires the book. At most `MAX_BOOK_POSITIONS` (1,000)
rows fit, because the commit stores a book four times inside one transaction.
`ReferenceRepository::list_books` pages current books by ID; a backend without it
returns `Unsupported`. Older binaries reject the new kind. Conformance:
`books_page_in_order_and_version_with_compare_and_set`. HTTP surface: see the
server README, "Books and portfolios".

CVX-022 adds closed `futures`, `basket` and `cash_universe` master definitions
and a futures mapping target. Exact reciprocal contract/basket revisions,
published factors, actual security versions, calendar coverage and source
availability are checked atomically. Both backends reuse these tables and
immutable history. Run the additional [futures conformance suite](tests/support/futures_conformance.rs)
for another backend. The [futures guide](../../docs/FUTURES-REFERENCE.md) describes
disjoint bond/rate terms, source qualification, cash prerequisites and the
indicative-only advisor. `for_valuation` remains a bond-specific operation;
futures pricing/input resolution remains subsequent work. Older binaries reject
the new master kinds: upgrade all readers before importing them and retain the
isolated backup/restore rollback procedure below.

## Transactions and bounds

`ReferenceCommand` contains an idempotency key, bond `changes` and optional
`master_changes`. Each master change is a full definition and either
`expected_version: null` for a never-used identity or the exact current version.
One command contains 1–256 combined operations. Duplicate aggregate keys fail.
Recording time cannot move backwards within an aggregate. Bond records, master
records, their histories and the original command/receipt commit atomically.

An identical retry returns the original receipt, even after later updates. A changed
payload under the same key conflicts. `UnknownOutcome` requires receipt lookup and
the same key/payload when recovering. No prefix of a failed command is committed.
Memory is volatile; redb uses immediate durable transactions in one local process.

Local adapters permit 100,000 combined bond/master identities, including tombstones,
1,000,000 combined immutable versions and 100,000 command receipts. Identifier
history selection is bounded but scans retained assertions; it is an administration
and replay path, not a measured low-latency lookup. Cache resolved immutable inputs
at the application boundary using the immutable snapshot contract. redb
and file calls are synchronous and should run away from latency-sensitive loops.

## File imports and runnable example

`load_reference_import` strictly decodes a regular file up to 8 MiB:

```json
{
  "schema_version":"2.0",
  "command":{
    "idempotency_key":"entity-001","changes":[],
    "master_changes":[{"expected_version":null,"definition":{
      "kind":"entity","definition":{
        "entity_id":"ENTITY-001","legal_name":"Fictional issuer","parent":null,"country":"GB",
        "effective":{"from":"2000-01-01T00:00:00Z","until":null},
        "provenance":{"source":"file-master","evidence_reference":"fixture/entity-001"}
      }
    }}]
  }
}
```

Use the complete fictional [initial import](../convex-ext-file/tests/fixtures/instrument-master.json)
and [correction](../convex-ext-file/tests/fixtures/instrument-master-correction.json)
for the executable example. Semantic errors identify zero-based rows and fields;
structural decoding errors identify line/column. Unknown/duplicate JSON fields,
missing mandatory data, numeric decimals and bad pins fail. Conflict preflight
reports rows; the backend transaction still performs the definitive version check.
A race can therefore produce a transaction-level conflict. Diagnostics omit raw
provider values. An uncertain commit is never described as an uncommitted import.

Run from the repository root, using a new database path:

```sh
cargo run --locked -p convex-server --example reference_master -- validate crates/convex-ext-file/tests/fixtures/instrument-master.json
cargo run --locked -p convex-server --example reference_master -- import /tmp/reference-example.redb crates/convex-ext-file/tests/fixtures/instrument-master.json 2025-01-01T00:00:00Z
cargo run --locked -p convex-server --example reference_master -- import /tmp/reference-example.redb crates/convex-ext-file/tests/fixtures/instrument-master-correction.json 2026-02-01T00:00:00Z
cargo run --locked -p convex-server --example reference_master -- resolve /tmp/reference-example.redb '{"kind":"provider","namespace":"provider-a","value":"NEW.SYMBOL"}' 2026-01-01T00:00:00Z 2026-02-01T00:00:00Z
cargo run --locked -p convex-server --example reference_master -- version /tmp/reference-example.redb INSTRUMENT-001 1
```

This is an offline example, with caller-supplied recording time. It opens a local
redb database; a misspelled database path can create a new file. Keep imported files
immutable. It starts no server or live feed. The JSON array loader
`load_bond_references` remains a full-term bond loader; use the command importer to
establish resources and mappings atomically.

## HTTP and embedded use

`POST /api/v1/reference/transactions` accepts the command directly. The HTTP
`Idempotency-Key` must equal its body key. The application supplies `recorded_at` and
returns a receipt with both `records` and `master_records`. Existing bond CRUD can
operate after its resources are installed. `POST /api/v1/reference/resolve` accepts
`{"identifier":{...},"lookup":{"as_of":"...","known_at":"..."}}`. A missing
mapping returns 404; ambiguity returns 409 `AMBIGUOUS_IDENTIFIER`. Inline single
and batch pricing also resolve pinned resources before calculation/publication.

Embedded applications call `get_at`, `get_master_at`, `get_master_version`,
`resolve_at` and `for_valuation` on the port. Implement those methods for another
backend and run both [reference](tests/support/reference_conformance.rs) and
[master](tests/support/master_conformance.rs) conformance suites. Exact resource
versions are immutable, so a later commit cannot change an already resolved input.

## Stored data migration and recovery

This extends reference wire schema 2 with optional retained fields and two redb
tables, `reference_master_current_v2` and `reference_master_history_v2`. Opening a
complete earlier three-table reference repository adds those tables atomically.
Partial table sets fail. Existing record bytes, versions and receipts are preserved.
Records without contractual terms remain inspectable; valuation fails with an
explicit migration error. Previously committed commands can still recover their
original receipts. Supply verified complete terms and resources in a new expected-
version update; no missing schedule or provenance is invented.

Back up the closed database before upgrading. The application schema marker remains
1 for the existing configuration schema. Restoring the pre-upgrade backup into a
new isolated database is the rollback path; an older executable cannot read new
term fields or master commands. Do not erase history or remove tables to downgrade.
The [redb operator guide](../convex-ext-redb/README.md) covers exclusive locking,
offline compaction, integrity checks, logical reconciliation and no-clobber restore.

Portable conformance, migration, actual HTTP/WebSocket, separate-process CLI and
backup/restore tests cover this contract. They do not establish distributed
failover, external convention agreement, entitlement decisions, throughput or
colocation latency.
