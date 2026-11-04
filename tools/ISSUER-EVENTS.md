# Issuer-wide bond and CDS defaults

Run from the repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_issuer_workflow.py
```

This builds the existing server/input importer and runs an owned local feed and
worker. `--binary-dir /absolute/path/to/debug` reuses a build. No vendor account
or Python package is required. The example stops all its processes.

The eight-trade, five-factor book includes two distinct bonds and a CDS of ACME,
another legal issuer's bond, IRS, OIS and two rate futures. One actual issuer
notice affects all three ACME contracts. Unknown recovery withdraws pricing and
hedge readiness across restart. Actual bond35% and CDS40% recovery, separate
partial/completing receipts and earned cash update native risk, hedges and causal
P&L. The second bond has its own2400 reference face; full contract terms, actual
quantities and NearestEven2payment precision are explicit. The other issuer remains active
even though its entity reference identifies ACME as its parent.

The replay verifies40durable priced outputs, three immutable unpriced audits,
direct native parity, constrained proposals, actual post-commit/pre-ACK worker
death, server restart and saved-input recomputation. It retains raw native P&L
residuals and checks the difference/addition half-ULP reconstruction budget for
each monetary field. It does not clamp residuals or choose a financial tolerance.
This is synthetic historical replay, not live-provider or production certification.

## Existing reference port

Publish exact entity attributes and one new typed `issuer_scope` master resource
through the existing `/reference/transactions` port. A directory contains:

```json
{
  "issuers": [{"entity_id":"ACME","version":1,"default":null}],
  "instruments": [{"id":"internal.security","version":1,"entity_id":"ACME"}]
}
```

The resource also supplies `id`, `effective` and `provenance`, as other master
definitions do. Every entity version must resolve through the existing reference
repository. Every supplied bond/CDS holding and hedge candidate requires its
exact native security ID/version mapping. No ticker, currency, parent contagion,
vendor symbol or credit model is used to infer legal ownership.

Managed `CoreDefinition.reference_inputs` explicitly selects the resource:

```json
{"issuer_scope":{"id":"internal.issuer-scope","version":1}}
```

This is a reserved selection alongside the existing native reference inputs.
The engine captures the entire selected resource in the immutable snapshot and
removes only the reserved selection before native market-input decoding. Old
snapshots without this selection retain their original bytes. Existing snapshots
replay against their captured directory even when sources change or disappear.
New assembly rejects a directory pin superseded at its captured economic and
knowledge cutoff. Historical receipt/source cutoffs remain explicit.

An actual issuer default supplies `id`, `effective_date`, `known_at`, and exact
`reference_id`/`reference_version` for its source notice. Each affected contract
uses the notice ID as its native `credit_default.event_id`, scoped by its own
trade ID. Native bond/CDS owners still validate the actual seniority, document,
accrual, cash precision and full event history. The issuer notice never supplies
a common recovery assumption. Existing notices cannot be erased/rewritten, and
existing security IDs cannot be removed/reassigned on directory updates. New
securities and explicit forward reference revisions can be added.

## Atomic default admission and recovery

`tools/issuer_events.py BATCH --url SERVER/api/v1` accepts:

```json
{
  "schema_version":1,
  "id":"actual-default-batch",
  "base":{},
  "base_snapshot":"saved-priced-input-snapshot",
  "definition":{},
  "workflow":{},
  "issuer_scope_change":{
    "expected_version":1,
    "definition":{"kind":"issuer_scope","definition":{}}
  }
}
```

Fill `base`, `definition` and `workflow` with the complete existing
[event-adapter](BOOK-EVENTS.md) declarations. Select directory2 in the new
definition and supply its full resource in the change. `base_snapshot` must be a
saved priced snapshot of that exact base definition. This lets an uncertain
retry validate the original directory1 after directory2 has committed.
`--prepare-only` prints a reviewable command and release without a write.

The adapter reuses the original immutable-prefix/event/P&L admission and commits
the issuer resource and complete book definition in one reference CAS transaction.
It releases only the exact committed selection. Publish that returned book record
on the existing ordered stream and retain it in complete recovery images. If a
publisher dies after commit, retry the identical batch and publish the same
release. Conflicting CAS/content does not restore an older book. Direct master
writes alone do not constitute a stream release; retain the existing single
ordered publisher discipline. No new event store, cursor, MQ, database or route
is introduced. Provider connectors normalize actual IDs/notices into this port.

Unknown recovery produces native unpriced audits and `issuer_recovery_pending`.
No actual mark/risk/P&L or proposal is published for that information set. Use
the existing event adapter's schema2 recovery resolution for every affected
`(trade_id, default_event)` pair. A shared notice ID does not let one contract's
recovery stand in for another's. Earlier priced phases and all actual facts stay;
old unpriced snapshots remain unpriced. Later cash batches keep the same issuer
directory pin and use normal schema1. Actual separately identified receipts own
claim reduction; proposed cash is not received cash.

The worker filters all mapped securities of a defaulted legal issuer, including
distinct bonds not held in the defaulted position. The additive native
`solve_issuer_lifecycle` action independently checks complete mappings, exact
snapshot and native affected holding state, then delegates to the original
constrained solver. Surviving IRS/futures and other issuers remain eligible
subject to the original limits. An empty eligible universe stays unavailable.
Proposals never authorize an order.

## Scope

Supplied standard principal-only bond recovery and single-name CDS legal default
owners remain unchanged. Actual contract-specific recovery/cash precision is
required. Unrounded contractual claims require exact receipts; a payment rounding
convention is never invented. Estimated recovery, optional/physical settlement,
corporate-family contagion and multi-cohort ownership remain separate work.
Use explicit complete-book releases for date, universe, market subscription or
policy changes. This example uses one local worker. Live provider composition,
multi-server/colocation performance and independent model/platform/production
qualification remain open; earlier financial fixtures and tolerances are retained.
