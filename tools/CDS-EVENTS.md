# Core CDS default and recovery events

Run from the repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_cds_workflow.py
```

The example builds the existing server/importer, starts an owned local feed and
worker, and stops its processes. `--binary-dir /absolute/path/to/debug` reuses a
build. No provider account or Python packages are required.

The six-trade/five-factor book receives an explicit pending-order cancellation,
CDS default, actual 35% recovery terms, a partial protection payment and completion
of protection/default-accrual cash. The distinct supplied seasoned CDS has no
initial upfront payment; its legal dates and native valuation context are explicit.
The original contract, rounding, Actual/360 premium accrual, signed notional and
native lifecycle owners calculate the obligations. A recovery announcement creates
a claim; only separately identified actual receipts create recorded cash.

The result includes 32 priced outputs and an unpriced default audit. The worker
stays unavailable across restart while recovery is unknown. Known recovery updates
actual claim risk, removes the defaulted entity from fresh hedge candidates and
updates hedge proposals and complete causal P&L. Actual post-commit/pre-ACK process
death, server/worker restart, stored bytes, caches, direct-native parity and saved
input recomputation are checked, including the unpriced audit.

This is synthetic historical replay, with exact prepared inputs imported offline
at captured cutoffs. It is local engineering evidence, not a live-provider,
market/model or production certificate.

## Native state and risk

Use the existing [event adapter](BOOK-EVENTS.md), [book selection](BOOK-UPDATES.md)
and [lifecycle risk binding](LIFECYCLE-RISK.md). No database, queue, server route,
vendor DTO, cash reducer or execution service is added.

`credit_default` requires the original entity, seniority, document, actual fraction
of the default day and explicit cash rounding. It ends protection and requires
actual pending orders to have been cancelled. Original raw quantity and immutable
facts remain; no fictional sale or fill closes the position.

While auction/recovery is unknown, `lifecycle_hedge_risk` returns a
`lifecycle_unpriced` audit with actual state, position, manifest, snapshot and
`actual_cds_recovery_unknown`. It supplies no actual current/pending sensitivity
or P&L mark. Its separate original reference vector is hypothetical fresh product
risk, not a value for the unknown claim or permission to trade.

The adapter may publish this exact input selection even though it is unpriced.
The worker latches its revision, emits `cds_recovery_pending`, and withdraws
readiness and hedge proposals. Restart requires a full image and repeats the
native check; it cannot reuse earlier readiness. Consumers must process the
unavailable signal as well as enforce `valid_until` and output liveness.

`recovery` names the exact default event and actual fractional result, immutable
source revision and cash due date. Native pricing values the resulting protection
and separate accrued-premium claims. Only collateral discount risk remains on
fixed future cash; market hazard/recovery shocks cannot change an actual auction
result. Due unpaid cash retains the existing nominal carrying policy. Pending
exposure is zero. Native cash receipts enforce direction, due date, original
obligation, uniqueness and no overpayment; early receipts remain rejected.

`lifecycle.default_entity` is present only on actual defaulted CDS states. The
worker excludes fresh candidates with that explicit credit entity. The native
solver independently rejects any such candidate that remains and requires every
affected credit-scoped holding to have matching default state. All original
factor, FX, quantity, cost, eligibility, solver and work controls remain.
If exclusion leaves no eligible configured hedges, the original solver withholds
the proposal. Supply an appropriate eligible hedge universe; no replacement
instrument or empty success is invented.

## Resume from an unpriced default

Normal event batches remain schema 1. To resolve the last unpriced default
information set, use the same adapter with this explicit envelope:

```json
{"schema_version":2,"resolve_default":true,"id":"actual-auction-42","base":{},"definition":{},"workflow":{}}
```

`base` is the exact selected pending-default book record. The adapter reruns its
durable native default audits before admitting this mode. `definition` supplies
the complete native after point with the original default and actual recovery.
Every earlier priced phase, opening point, counterfactual, fill and immutable
event/receipt identity must remain. Every pending default needs its actual
matching recovery. The original reference definition and unpriced audit stay
immutable and retrievable; they are not retroactively assigned a recovery value.

The new priced path spans the unpriced interval and may consolidate only its last
unpriced event information set. There is no claimed instantaneous P&L at the
earlier unknown-recovery point. Native complete-path and causal admission run
before any priced release. Unused local wire bundles are removed by the producer;
this changes no fact or earlier information set.

Both schemas preserve the existing workflow policy and shared resource bundles.
Use the existing explicit complete-book/reference update path for valuation-date,
market-bundle, subscription or policy changes; the adapter never infers a roll,
backdates knowledge, guesses missing fixings or rewrites a priced phase.

Retry the identical batch after uncertainty. Existing reference CAS/idempotence,
snapshots, results, full stream image and local worker receipt own recovery. A
committed candidate that fails admission remains auditable and unselected; use
the existing explicit correction process. Publishing a reference alone does not
activate it. Proposals carry no execution authority.

## Verification and limits

Independent Decimal80 controls declare 0%, 35% and 100% recovery cash, actual
17.5-day accrued premium and 14-day discounted claim prices/risk at the original
1bp/full-half bumps. Absolute USD/risk bound is 1e-7. The first financial answers,
units and bounds remain unchanged; initial invalid request contexts are retained
in private qualification evidence.

Cash and current model-P&L residuals are exactly zero in the example. Raw native
marking residuals remain visible, including a roughly EUR 9e-12 cancellation
residual. The checker reconstructs the native arithmetic and uses its derived
half-ULP-per-operation roundoff budget. It neither selects a financial tolerance
nor clamps a residual to zero. Original exact-zero examples and all prior
numerical evidence remain unchanged.

This increment covers native single-name CDS default/actual recovery and cash.
Bond default, physical settlement, multi-cohort ownership and default estimation
remain with their existing owners. One local worker is supported; live providers,
multi-server/colocation performance, platform and independent production/model
qualification remain open.
