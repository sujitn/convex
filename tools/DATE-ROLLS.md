# Valuation-date rolls and scheduled cash

Run the local end-to-end example from the repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_roll_workflow.py
```

It builds the existing server and importer, uses temporary storage and owned
loopback processes, and stops them on exit. No external service is required.
The inputs are explicitly synthetic historical observations and contracts.

The six-position book contains a stepped bond, IRS, a short seasoned OIS, CDS,
and overnight and term rate futures. It rolls from 7 to 8, 9 and 13 October 2026:

| Point | Native economic effect | Cash actually recorded |
|---|---|---|
| 8 October | USD 3 coupon and USD 400 principal claims; remaining principal 600 | 0 |
| 9 October | Final USD 0.06 coupon and USD 600 redemption claims; bond matures | 0 |
| 13 October | Outstanding claims remain due at nominal under the supplied policy | 0 |
| Partial receipt | The first principal claim retains USD 200 outstanding | −200 |
| Completing receipts | All four claims settle; the bond closes | −1,003.06 total |

The bond is short, so these cash amounts are payables. Original face stays
−1,000 in the position input: the contract's amortization profile and native
lifecycle effect own the change to −600 outstanding principal. The lifecycle
`remaining_fraction` tracks termination of the contract; it is 1 after ordinary
amortization and 0 after maturity. Scaling face again would count amortization
twice.

Each release recomputes native pricing, signed current and pending risk,
constrained hedge proposals and complete causal P&L. Outstanding claims retain
collateral discount risk until due or paid. The matured security is excluded
from fresh candidates by security identity, including candidate aliases and
other reference revisions. Other instruments remain eligible.

The example preserves the original solver settings. Its very short bond makes
the first two hedge matrices ill conditioned, so those results contain a
withheld-proposal diagnostic. After maturity removes that candidate, the four
remaining stages produce feasible proposals. No solver guard or financial
comparison bound is widened to obtain a proposal.

## Supplying a roll

The adapter accepts the same complete native book structure as event admission,
with one additional pin to the last successfully captured base snapshot:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/book_rolls.py roll.json \
  --url http://127.0.0.1:8080/api/v1 --prepare-only
PYTHONDONTWRITEBYTECODE=1 python3 tools/book_rolls.py roll.json \
  --url http://127.0.0.1:8080/api/v1
```

Use the URL of your existing server. The batch has exactly `schema_version: 1`,
`id`, `base`, `base_snapshot`, `definition` and `workflow`. The example's `roll`
and `append_phase` functions show the complete supplied after-point construction.
They are synthetic producers, not a vendor mapping or financial implementation.

Publish fresh curve recipes and actual eligible fixing revisions through the
existing curve and fixing ports. A roll preserves every earlier point, market
selection and immutable fact, appends explicit fixing/time/market phases, and
advances the definition with the existing reference compare-and-swap transaction.
Cancel pending exposure with an explicit position phase before maturity. Missing
scheduled boundaries, invented payments, changed prior history, inconsistent
risk bindings, stale sources and unavailable fixings block release.

The native lifecycle owners calculate coupons, principal steps and maturity from
the full contracts and calendars. Sources supply ordered `contract_step` and
`contractual_cash` events with exact revisions and publication knowledge. A time
roll with no due cash needs no invented event. Actual cash receipts use the
separate existing `book_events.py` payment route; a date roll does not post cash.
No background scheduler or cash ledger is introduced.

The stream subscription may opt into `follow_fixing_dates: true` alongside its
existing book selection. New dates are then accepted only for the explicitly
subscribed index/source pairs. Complete reconnect images must retain all initial
fixing keys and every fixing key selected by the active book. A released
workflow selects exact additional keys; observation
values still come from the fixing port. The worker retains book/version
high-water marks across restart. Its opt-in historical replay clock can advance
without changing the boot configuration; live/replay modes remain separate.

Historical P&L curves retain their original recipes and source cutoffs, including
frozen curves used later for carry. Freshness is checked at their first selection;
availability is checked at every point. Current risk must use fresh followed
curves at the rolled plan cutoff. A new curve reference date requires an explicit
date roll to that date; other model/time/interpolation policies remain fixed.
Fixings are partitioned by each point's publication **and** receipt cutoff, so a
newly received observation cannot enter an earlier P&L information set.

## Recovery and bounds

Retain the exact batch after any uncertain response and retry it. The existing
reference idempotency receipt, immutable snapshots/results and stream book image
own recovery. A competing or changed batch conflicts. Publish the returned book
release only after native admission succeeds.

The runnable check exercises the adapter CLI, a missing newly due fixing, an
unrefreshed current curve, idempotent retry, an actual kill after hedge commit
but before its HTTP acknowledgement, server and worker restart, old result byte
conservation and saved-input recomputation. Native controls separately cover
bond, IRS and CDS maturity and the original freshness guards. The existing OIS
coupon owner is reused.

All existing input, event, causal-path and result limits remain: at most 16
causes, the native JSON work bounds and the server's 4 MiB saved-result bound.
The example reuses unchanged immutable holding bundles and supplies a shorter
OIS so its full daily diagnostics fit those bounds. Long-running books need an
explicit future checkpoint/opening-state increment; this adapter does not
silently discard history. Scheduled rate-futures final settlement, physical
futures delivery, automatic calendar-event discovery, live provider mapping,
authoritative cash posting and multi-server scheduling remain separate work.
This is local engineering qualification, not independent model, platform,
production or latency certification.
