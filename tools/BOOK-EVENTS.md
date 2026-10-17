# Core fills, lifecycle and cash events

Run the complete local example from the repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_event_workflow.py
```

It builds the existing server/importer, starts a local feed and the existing
analytics worker, then stops all owned processes. `--binary-dir /absolute/path/to/debug`
reuses a build. No Python packages or provider account are needed.

The same six-trade bond/CDS/IRS/OIS/rate-futures book receives an actual supplied
IRS fill and pending-position change, an official daily futures settlement, then
two separately identified partial/completing cash receipts. The native library
calculates positions' economic consistency, prices, five-factor risk, constrained
hedge proposals and causal P&L. The example checks proposed versus received cash,
complete reconciliation, exact direct-native parity, idempotent retries, a killed
worker after a durable result but before acknowledgement, and server/worker restart.
Saved result bytes, caches and saved-input recomputation must agree.
Clearing and the first receipt share one ordered event phase; both original
events remain in the native replay, and its closing state is explicitly partial.
This keeps the full report within the unchanged4MiB native result bound.

The financial example is **synthetic historical replay**. Its exact prepared
reference commands are imported offline, with the server stopped, at the original
captured cutoff. The adapter then retries those same commands through HTTP and
validates their native P&L. A separate actual-UTC HTTP reference write exercises
process death after commit, lost acknowledgement, retry, CAS and content conflicts;
that later revision stays unreleased. This does not certify a live provider.

## Ingest an event batch

`book_events.py` is a small provider-neutral adapter over the existing
`ReferenceRepository`, curve/fixing snapshot ports and native result port. The
provider maps its records to existing native `PnlFill` and `LifecycleStep` inputs
and supplies complete closing points and risk requests. There is no second
position reducer, cash ledger, pricing formula, event database or queue.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/book_events.py /absolute/path/to/batch.json \
  --url https://your-server/api/v1
```

`CONVEX_TOKEN` supplies authentication; `--token-env NAME` selects another
environment variable. Only HTTPS or loopback HTTP is accepted. Token headers
are never included in the batch or diagnostics. Existing reference-write,
calculate, market/book/result access checks still apply. `--timeout` is bounded
to120seconds. Default production startup remains subject to its existing gates.

The bounded JSON document has exactly these fields:

```json
{"schema_version":1,"id":"external-batch-42","base":{},"definition":{},"workflow":{}}
```

- `base`: the full previously released [book record payload](BOOK-UPDATES.md),
  including its exact reference commit, definition version and closing step.
- `definition`: the complete next existing `CoreDefinition`. Extend its native
  `book_pnl` ordered path with actual position, event or time phases and explicit
  standalone counterfactuals. Preserve the opening information set, every prior
  phase and counterfactual, full lifecycle prefixes, fills, receipt identities,
  resources, curve roles and numerical policies. Update the named risk requests
  to match the same closing products, identities and reference units.
- `workflow`: the same complete worker policy/configuration with only its exact
  definition version advanced by one. Market updates continue through the existing
  stream; reference/policy changes use the existing complete-book release path.

The example's `batch()`, `clearing_step()`, `cash_step()` and
`lifecycle_definition()` assemble working requests. They retain full native terms,
contexts, signed quantities, consideration, fees, source and knowledge timestamps,
expected lifecycle revisions, original obligations and separately recorded cash.
Pending quantities are explicit order facts; a fill does not automatically cancel
an order. Never infer a fill, payment, clearing mark or physical receipt from a
position difference, price change or hedge proposal.

`--prepare-only` validates the structural prefix and exact bindings and prints
`{schema_version,command,release}` without writing. It is useful for inspection
and historical offline import; it is **not** native financial admission.

## Commit, validate, then release

The adapter resolves the exact base reference receipt and checks its complete
closing binding. It preserves full information sets even when local holding
pool indices change. Actual event/fill/receipt identities are immutable within
their native kind/trade scope; the extended ordered path must contain new facts.
Counterfactuals do not create extra executions or receipts.

One atomic reference transaction uses the base definition's expected version.
Its stable key derives from book and batch IDs; the stored provenance also binds
the hash of the entire batch, including workflow and base. Reusing the same ID
with different contents conflicts. A competing stale base cannot overwrite
another committed batch. Successful retries retain the original server timestamp
and version, including when the first acknowledgement was lost.

After commit, the adapter assembles the exact managed snapshot from the existing
reference, curve and fixing ports, then runs native P&L. Invalid quantities,
overpayments, missing predecessors, future-known events, changed causal history,
ineligible fixings and unsupported native economics cannot produce a release.
All numerical tolerances and native financial decisions remain with their owners.

The current product-risk route covers ordinary active exposures. For holdings
changed by this batch, the adapter additionally withholds release for ended or
partial exposure, exercise notices, physical delivery, or newly affected future
cash claims needing lifecycle-specific risk. Unchanged holdings retain their
existing native risk owners. Due cash under the explicit nominal carrying policy
and normal daily futures variation/receipts are supported. This is a conservative
integration boundary; standalone lifecycle support does not by itself imply that
its full risk can enter the automatic hedge workflow. Full cohort/physical-delivery,
default, exercise and lifecycle-risk integration remains required work.

On success, stdout contains **one existing normalized `book` record**. Publish it
on the existing ordered feed; retain it in subsequent complete images. The
existing worker rechecks the exact committed definition and native closing
positions, then persists current pricing/risk, hedge and P&L results from the
same selected snapshot. Publishing the reference record alone does not activate it.

## Retry and recovery

Keep each original batch until its release has been published and retained in
the producer's full image. After uncertain delivery or process death, rerun the
**identical** command/batch. The durable reference receipt and result cache own
recovery; the adapter has no additional local state file to reconcile. After a
lost output, it regenerates the same release. Replaying that release is safe.

The worker's existing receipt, version floors, stream-gap handling, full-image
recovery, freshness checks and snapshot/result persistence continue unchanged.
Its restart never trusts saved readiness. A skipped or lower book revision
cannot replace the admitted state. Consumers enforce `valid_until` and worker
output liveness; all proposals retain no execution authority.

A committed candidate that fails native admission remains auditable and
**unreleased**. Do not silently fall back, edit its immutable facts, or advance
the same batch ID. Use the existing explicit reference/book correction process
to establish the intended new base before submitting further event batches.
This deliberately small adapter does not automate correction agreements or
cross-source conflict resolution.

Limits remain:2MiB/65,536nodes/depth64 per batch; native core definitions1MiB/
32calculations;16ordered P&L phases;33points; existing native product-work ceilings;
at most4,096 distinct scoped fact identities in an adapter batch's full path.
Longer windows need an explicit new full book with conserved history/ownership.
One serial book publisher and one existing worker are the supported local setup;
multi-server ownership, live execution and measured low latency are separate gates.
