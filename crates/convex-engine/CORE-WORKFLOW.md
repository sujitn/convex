# Persisted core calculation workflow

`CoreWorkflow` resolves existing port records into an immutable input snapshot,
executes the existing native library, and retains its exact JSON result. It adds
no financial formulas, scheduler, broker, database dependency in pricing, or
provider-specific instrument representation.

Run `PYTHONDONTWRITEBYTECODE=1 python3 tools/core_snapshot_workflow.py` from the
repository root. Python 3.10+ and the repository Rust toolchain suffice. The script
builds the existing server and the offline input importer, starts isolated demo
servers, verifies updates and recovery, and stops its processes. Use
`--binary-dir /absolute/path/to/debug` to reuse both executables.

## Inputs and ownership

The existing `ReferenceRepository` retains a `calculation` master resource using
its existing compare-and-set versions, effective/known times, immutable history
and idempotency receipts. `CoreDefinition` contains named full native requests and
one shared native reference-input bundle: calendars, publication rules and other
explicit reference/calculation settings. It stores that shared data once.
Curve and fixing values must be absent from the definition's resource slots.
The initial managed operations are `GraphHedgeRisk`, `BookPnl` and `Hedge`; the
complete supplied-input API continues to expose the other native operations.

`CoreSnapshotPlan` chooses the exact definition revision, economic and knowledge
cutoffs, named sets of exact curve-record revisions, and explicit index/date/source
fixing keys. There is no implicit current reference, latest curve, source failover
or missing-history forecast. The existing `CurveStore` retains whole
`SnapshotCurveRecipe` values and metadata, including native dependencies and
construction policies. Numeric source-record versions are separate from the
native curve keys within their complete market sets; both are saved unchanged.
The `FixingRepository` returns all eligible revisions, retaining actual source,
publication, receipt, preliminary/final status and entitlement. Native
LatestKnown/FirstPublished policies select the appropriate observations.

Assembly rejects missing exact records, duplicate/ambiguous selections, incorrect
adapter identities, future-known reference/curve/fixing records, invalid/stale
curve quality and exhausted source age policies. Recipe ages are evaluated at
the explicit plan cutoffs; P&L market records must also have been available at
their original points. No snapshot is published until the entire selection has
resolved. Native request shapes are checked; unsupported managed operations fail.
The native calculation still owns financial completeness/model/numerical checks.
Its missing/unsupported/numerical failure envelopes remain failures and are
stored without substituting zero or accepting a partial financial report.

Snapshots contain the actual complete reference record, recipes and fixing
revisions. Their canonical content identity includes those values, cutoffs and
source revisions. All graph-risk calculations in the book bind that real shared
snapshot identity, so vectors from changed input sets cannot silently aggregate
as the same hedge book. Causal P&L retains its original full before/after point
identities and ownership history. Existing delivery baskets and CTD behavior stay
in their original native owners.

## Durability and recovery

The existing `SnapshotRepository` publishes a distinct core schema with atomic,
no-clobber local file publication and file/directory synchronization. Core files
are `<digest>.core.json`; fixed-bullet snapshots retain their existing schema.
The existing `RuntimeStore` retains immutable native results. The result key
includes the complete snapshot identity, calculation name and host build identity.
The executable derives that build identity from its actual binary bytes, including
local builds. Embedded hosts must supply an equally precise build identity.

A run acknowledges only after its result commits. A disconnect keeps calculation
and access admission until the owned work, including persistence, completes.
Reusing a durable result performs no numerical calculation. Recompute reconstructs
the native request entirely from the saved snapshot, even when current input ports
are empty or changed. A different output under an existing result identity conflicts;
the original is retained. Corrupt identity/content fails instead of resetting data.

Unknown snapshot/result acknowledgements return `UNKNOWN_OUTCOME` with the actual
`snapshot_id` or `result_id`. Read that identity to reconcile before changing inputs
or retrying. Identical retries are idempotent. If the process stopped before a result
committed, rerun the named calculation on the saved snapshot. The receipt identifies
the calculation, snapshot, build and result and says whether a durable result was
reused; inspect the native envelope's `ok` for financial success.

Reference, recipe and fixing publications are independent immutable port commits.
The offline importer can stop between them; retry the identical captured import.
There is no distributed transaction or cross-backend partial-snapshot acknowledgement.
Snapshot publication follows complete resolution. Redb fixing batches themselves
commit atomically, including conflict/chronology rejection.

The additive redb tables are `curve_recipes_v1`, `published_fixings_v1` and
`core_results_v1`; the existing application schema and tables remain intact.
Back up the stopped database and core snapshot directory together. Recovery tests
reopen the actual database/files and repeat saved calculations with empty alternative
input ports. Local redb remains one process with exclusive file ownership. Another
database, fixing source or snapshot backend implements the same existing traits;
new methods default to explicit Unsupported for older adapters.

## Evidence and scope

The runnable example preserves the prior full synthetic terms, financial controls
and tolerances. It compares saved pricing/risk/P&L with the unchanged native HTTP
endpoint; publishes a complete captured curve alternative; selects the corresponding
captured reference revision; corrects a published SOFR fixing; computes a native
paper hedge; and checks exact stored output and saved-input recomputation after
restart. It checks P&L's own zero reconciliation residuals and retains separate
currency present-value and futures quote-level reporting groups.

Captured reference revisions are imported with their explicit historical knowledge
times. Current HTTP reference writes keep their actual server commit time. The
example's later hedge definition uses that returned commit time. No backdated HTTP
reference-write behavior is introduced.

This is scoped engineering evidence with synthetic data and paper hedge authority.
Provider composition, licensed-data admission, distributed ownership, production
qualification and measured product-specific latency remain required. The default
production executable remains closed. The default durable filesystem adapter is
available on local Unix; unavailable Windows/Excel execution remains a separate gate.
