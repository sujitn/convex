# Core lifecycle risk, cash claims and recovery

Run this from the repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_lifecycle_workflow.py
```

It builds the existing server and importer, starts a local feed and analytics
worker, and stops its processes on completion. `--binary-dir /absolute/path/to/debug`
reuses an existing build. No provider account or Python packages are needed.

The six-trade bond/CDS/IRS/OIS/rate-futures book receives a supplied 50% OIS
termination with an agreed USD1,000 cash claim, an actual USD400 receipt and the
USD600 completion. The native library retains the earned scheduled claims,
remaining exposure, signed quantities, pending orders and original cash owners.
Five-factor hedge proposals and full causal P&L update after each released batch.

The example compares every stored result with the direct native calculation,
checks the original P&L reconciliation and hedge constraints, kills the worker
after the changed hedge result commits but before acknowledgement, and restarts
both worker and server. Stored bytes, result IDs, caches and saved-input
recomputation must agree. Its 32 outputs are **synthetic historical replay**:
exact prepared inputs are imported offline at captured cutoffs. This is local
engineering evidence, not live-provider or production qualification.

## Exposure and cash ownership

The existing `graph_hedge_risk` output remains the risk of a fresh instrument at
its original reference size. A new `lifecycle_hedge_risk` operation adds:

- `lifecycle.current`: the complete actual holding's remaining economic exposure
  plus its outstanding owned cash claims.
- `lifecycle.pending`: hypothetical pending exposure at that same remaining
  fraction, excluding claims already earned by the current holding.
- `lifecycle_mark`: the original native P&L mark, including state, claims and
  actual current/pending amounts.

The original quantity is not replaced with an invented fill when a termination
changes the remaining fraction. Agreed termination cash is a whole-position
amount; it is never multiplied by notional again. Scheduled earned claims keep
their original owners. Native P&L removes the corresponding embedded product
cash before adding outstanding claims, preventing double counting. Actual
receipts reduce their original claims and enter cash P&L; a proposal or cash due
date cannot create a receipt.

Each factor rebuilds the original graph at the original positive/negative full
and half bump. The original replayed legal state and actual receipts stay fixed.
Native product pricing, cash discounting, recovery binding and signed-risk
normalization supply the results. The unchanged explicit native cash-carrying
policy discounts future claims and carries due unpaid cash at nominal value.

The hedge solver accepts these complete state sensitivities exactly once via
`action: {"kind":"solve_lifecycle","states":[...]}`. It still uses the original
fresh reference vectors for new hedge trades, with the original quantities,
eligibility, FX, credit scope, costs, bounds and numerical controls. State risk
uses one complete state copy as its reporting unit; it is not a tradable notional.

## Bind a complete book

Keep using the existing [event adapter](BOOK-EVENTS.md), reference transactions,
curve/fixing ports, [book releases](BOOK-UPDATES.md) and streaming worker. There
is no new database, queue, server route or event reducer.

For each affected cash product, replace its named risk operation with
`lifecycle_hedge_risk`, retaining its existing `graph`, `product` and `input`, and
add these fields:

- `lifecycle`: the exact full native lifecycle of that P&L closing holding.
- `history`: the complete native causal quantity/fill/FX declaration, including
  opening and ordered points, available standalone points and genuine reasons
  for unavailable counterfactuals. Earlier phases are not fabricated or dropped.
- `max_state_evaluations`: an explicit reservation of at least five per factor,
  at most 80. Fresh instrument risk retains its separate original work policy.

`tools/lifecycle_risk.py::bind_requests` assembles those fields from the complete
existing book definition without financial arithmetic. See
`core_lifecycle_workflow.py` for actual native requests and event batches.
The managed server resolves resources through the same ports and binds the
captured snapshot ID. Historical dates/knowledge cannot exceed that snapshot.

Before returning a priced event release, the adapter runs native P&L and lifecycle
risk on the same snapshot. The worker repeats the checks on its selected input.
Positions, manifests, dates, quantities, full state, claim ownership and local
amounts must match exactly. Missing proof or mismatched history blocks the hedge.
An ordinary product-risk operation cannot admit a terminated or partial holding.
An unresolved [CDS](CDS-EVENTS.md) or [bond](BOND-EVENTS.md) default instead selects a native unpriced audit;
the worker withdraws readiness until actual recovery completes its valuation.

## Supported scope and recovery

This composition supports standard bond, IRS/OIS and CDS states admitted by the
original native owners. Partial termination currently requires an IRS/OIS joint
fixed/floating accrual boundary. [Single-name CDS default and recovery](CDS-EVENTS.md)
retain unknown recovery as explicitly unpriced and value actual known claims.
The [bond default workflow](BOND-EVENTS.md) adds actual principal recovery and retained earned cash.
Partial bonds/CDS, exercise, physical delivery and multi-cohort
ownership remain rejected here. Ordinary rate futures
retain their existing risk and settlement owners. The separate delivery-futures
and CTD workflows are unchanged; this example does not qualify their lifecycle
risk. Existing financial admission is not relaxed.

Retry the identical batch after uncertainty. The original reference receipt,
immutable snapshot/results, stream complete image and one local worker receipt
own recovery. A failed committed candidate stays auditable and unreleased.
Worker restart requires a new complete image and fresh validation; saved
readiness is never trusted. Existing gap, stale-data, revision and liveness checks
continue to apply. One local worker owner is supported. Hedge results remain
proposals with no execution authority.

The independent `lifecycle-risk-contract.json` control uses Decimal80 to declare
a USD1,000 claim discounted for 14 Actual/365 days at 3% continuous, with full and
half 1bp moves. Its predeclared absolute USD/risk bound is 1e-7. Native comparisons
check the claim increment while pending and fresh-candidate risk stay identical;
this analytic control does not replace independently approved market/model packs.
