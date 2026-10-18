# Durable core P&L windows

Run the synthetic end-to-end example from the repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_checkpoint_workflow.py
```

It builds the existing server/importer, starts owned loopback processes and uses
an isolated temporary database. To reuse an existing build:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_checkpoint_workflow.py --binary-dir /path/to/debug
```

The six-position book retains bond, CDS, IRS, OIS and two rate futures with five
risk factors. It runs 26 actual ordered phases in three native P&L windows of
9, 8 and 9 causes. Two explicit checkpoints retain original signed quantities,
pending hedges, matured bond principal/coupon claims and three separately supplied
partial coupon receipts and an actual IRS fill with explicit consideration/fees. The example checks native parity, feasible hedge
constraints, a committed hedge whose acknowledgment is lost when the worker is
killed, fresh-image recovery, exact old output bytes, result reuse and saved-input
recomputation. Data and cash records are synthetic; no orders are submitted.

## Checkpoint contract

The producer supplies the next full bounded native window, its workflow selection
and an exact successful saved base. Invoke the thin adapter:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/book_checkpoints.py batch.json --url http://127.0.0.1:8080/api/v1
```

Optional flags are `--prepare-only`, `--token-env CONVEX_TOKEN` and `--timeout 120`.
A batch has exactly these fields:

```json
{
  "schema_version": 1,
  "id": "window.2",
  "base": { "id": "book-id", "version": 1, "reference_commit": "exact-commit", "workflow": {}, "position_step": 7 },
  "base_snapshot": "sha256:exact-saved-inputs",
  "definition": {},
  "workflow": {}
}
```

The placeholders represent the same full reference definition and workflow used
by the existing book/event adapters. See the runnable example's `rebase` helper
for concrete wire assembly. The next definition adds the reserved reference
selection (the server strips it before native input decoding):

```json
{
  "pnl_checkpoint": {
    "snapshot_id": "sha256:exact-saved-inputs",
    "calculation": "pnl",
    "build_id": "actual-receipt-build"
  }
}
```

Checkpoint admission retains the book identity, solver and provider/source
policies, economic and information cutoffs, fixing selection and exact source
revision. It increments the reference/book versions through the existing CAS
and idempotency receipt. It may discard unused old *window bindings* from the
new definition. Earlier immutable definitions, snapshots and results remain.
It does not silently trim a window when it reaches 16 causes.

The engine independently resolves that exact snapshot and successful saved
result. Expanded new opening inputs must equal the original closing point:
positions, native units/reference quantities, pending quantities, FX, calendars,
curves, eligible fixing revisions, publication rules, complete contracts and all
lifecycle events. Completed interval fills alone reset to empty; complete `opening_quantities` retain the original lifecycle lots while the new opening holds the actual post-fill quantity. Their IDs are
carried so they cannot be counted again. Changing a claim or receipt requires a
subsequent checked lifecycle event. Full lifecycle replay rebuilds economic state;
no JSON-decoded cached lifecycle state is trusted.

Freeze the opening market in its own set and use a separate followed current
market. This preserves the carry when later quotes update. Subsequent position,
event, issuer and date-roll increments continue through their existing adapters.
Checkpointing itself advances neither the valuation date nor a provider revision.

## Persistence and P&L meaning

The immutable snapshot captures the predecessor snapshot/result/build identity,
output digest, opening digest, consumed fill IDs, completed cause/window counts,
and cumulative native/reporting totals. It contains everything required for
recalculation of the current window without reading the predecessor, mutable
reference/curve/fixing providers or previous result store. Retain predecessor
records for the audit trail and detailed attribution.

Only successful checkpointed P&L results add `result.data.continuity`:

- `completed_windows` and `completed_causes`, including the current window;
- `local_totals` and `reporting_totals`, each retaining currency, valuation basis
  and accumulated marking, cash and current model P&L.

The original native `report` stays complete for its bounded window. Detailed
ordered attribution, counterfactual availability, CTD diagnostics and raw residuals
remain in each saved report. Cumulative totals add those reconciled interval
amounts in chronological order, retaining FX revaluation on completed cash and
futures consideration; they do not invent cross-window counterfactuals
or mix futures quote changes with discounted PV. Floating-point arithmetic
remains visible and finite; no financial tolerance is widened.

The native window already subtracts opening recorded cash from closing recorded
cash. Keeping the full lifecycle replay therefore preserves historical receipts
while counting only new receipts in the new window. Outstanding claims stay in
current risk; pending orders never acquire those historical claims.

The worker publishes/persists the same `pnl_continuity` in its ready bundle. A
saved receipt never restores readiness by itself: the usual complete source
image, version high-water checks, freshness checks, risk/P&L state equality and
native hedge admission must all pass again. Unknown default recovery remains
unpriced and cannot become a priced checkpoint.

Existing reference/snapshot/result ports own durability. No new database, queue,
scheduler, ledger, endpoint, dependency or execution permission is introduced.
A checkpoint remains bound to its source build; a host build change is rejected
until an explicit migration/requalification is supplied. Existing per-window
16-cause, 4096-event/receipt/fill, JSON, result-size and conditioning limits remain.
This extends causal-window lifetime, not unlimited economic history storage.
Live-provider, distributed operation, latency and independent model/platform/
production qualification remain separate.

## Sharing completed lifecycle history

Run the extension that shares completed history and checks updates/recovery:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_compaction_workflow.py
```

The existing checkpoint CLI accepts `--compact-lifecycle`. It stores repeated
opening event prefixes once in `reference_inputs.lifecycle_history`, keyed by
original trade ID. A managed lifecycle's `events` can then be
`{"prefix":"original.trade.id","tail":[]}`. New actual events extend the tail.
Original terms, units, opening fixings and cutoff remain explicit. At the next
checkpoint, prefixes are flattened into a self-contained archive; references to
older snapshots are unnecessary for recomputing the latest saved inputs.
The adapter shares history only when actual serialized inputs get smaller.

The engine expands only existing P&L holding and lifecycle-risk request locations.
Every archived prefix must match the new window's opening. Existing checkpoint
admission binds that whole opening to the exact successfully priced prior close.
Native replay then checks every original event, receipt ID, revision, cash claim,
quantity and fixing. Missing/changed prefixes, nested references, trade mismatches,
overlong histories and expansion beyond the native request node bound reject.
The existing event adapter accepts later fills/lifecycle/cash updates and retains
compact references in the committed definition. Immutable old snapshots/results
remain available for audit. No automatic deletion is performed.

This is lossless storage compaction, not a new financial state restoration API.
Full native replay and its cost remain. The managed wire limit is 4096 expanded
events per lifecycle; the lower-level native state's separate bound is 65,536
items. Neither limit is raised. The existing definition/snapshot/result limits
and financial tolerances also remain. The example retains original lots,
post-fill quantities, unpaid claims, three partial receipts, current/pending risk,
feasible hedge constraints and cumulative local/reporting P&L across two compact
checkpoints, an ordinary cash update, worker interruption and server restart.

## Review of remaining work

Repeated history in pricing/risk inputs is a concrete storage issue addressed by
this increment. A unique short history is kept as-is because archiving it adds
bytes. Durable P&L continuation beyond 16 total causes, recovery from saved inputs,
cash-claim ownership and current/pending risk already have implemented controls;
they do not require new ledger, queue, scheduler or database abstractions here.

A faster native checkpoint that restores computed state would require a new
validation contract across default, exercise, clearing and physical-delivery
owners. No measured replay bottleneck or agreed workload currently justifies
that addition. Measure replay time and retained input sizes before choosing it.
Do not treat the broader CVX-063 catalogue as a dependency of this core workflow.
Live-provider integration, deployment-specific multi-server/latency tests and
independent financial/platform qualification still need concrete environments
and acceptance targets; this local synthetic example does not complete them.
