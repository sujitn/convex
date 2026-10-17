# Core bond default and recovery events

Run from the repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_bond_workflow.py
```

The example builds the existing server and input importer, runs an owned local
feed and worker, and stops its processes. `--binary-dir /absolute/path/to/debug`
reuses a build. No provider account or Python package is needed.

The six-trade, five-factor book receives an explicit bond order cancellation,
default, actual 35% principal recovery, partial recovery cash and final recovery
and earned-coupon receipts. It checks native pricing, actual claim risk, changed
hedges, complete causal P&L, immutable results and restart recovery, including a
worker killed after a hedge result commits but before acknowledgement.

This is synthetic historical replay. It uses the existing reference, curve and
fixing ports, snapshots, results, ordered book image and worker receipt. There is
no new route, database, queue, provider type or execution service.

## Event and claim ownership

Use the existing [event adapter](BOOK-EVENTS.md), [lifecycle risk](LIFECYCLE-RISK.md)
and [default-resolution envelope](CDS-EVENTS.md#resume-from-an-unpriced-default).
The same `credit_default`, `recovery` and `payment` actions apply to bonds.

The lifecycle bond carries an explicit `credit` mapping:

```json
{"entity":"BOND.ISSUER","seniority":"SeniorUnsecured","document":{"id":"principal-only-confirmation","version":1}}
```

It is an admitted outstanding-principal-only recovery agreement. Native replay
requires the exact entity, seniority and document on default, zero bond
`accrual_day_fraction`, and processed same-day principal/coupon boundaries.
Recovery is the signed remaining principal multiplied by the actual recovery
fraction. A bond recovery fraction is not a CDS loss-given-default fraction.
Coupon recovery, acceleration or estimated auction terms are not inferred.

Earned coupon and amortization claims remain separately owned. Original raw
position and event facts stay visible; economic remaining fraction becomes zero.
Pending orders require actual cancellation. No fictional trade closes the bond.

Unknown recovery produces a checked `lifecycle_unpriced` audit with
`actual_bond_recovery_unknown`, without actual PV, sensitivities or P&L mark.
The worker selects that revision, emits `bond_recovery_pending` and withdraws
readiness/proposals across restart. The old priced book cannot restore readiness.

Actual recovery uses schema 2 with `resolve_default:true` to complete only the
verified last unpriced default information set. All earlier priced phases and
immutable facts remain; the old unpriced audit is retained. Native full-path P&L
spans that interval without inventing an earlier default valuation.

Known cash has its original collateral discount risk. Actual recovery cannot be
changed by forecast credit or recovery shocks. Due unpaid cash uses the existing
nominal carrying policy. Actual separately identified payments reduce claims;
wrong direction/currency, early receipts, duplicates and overpayment fail.

## Hedge scope and recovery

Actual `lifecycle.default_entity` comes from the admitted bond mapping. The worker
excludes fresh candidates for the same security identity across reference
revisions and trade identities, and candidates with that entity in their declared credit scope.
The native solver independently enforces both exclusions and complete affected
holding states, alongside the existing factor, FX, quantity and work controls.
An empty eligible universe remains unavailable under the original solver policy.

Plain bond reference vectors do not carry an issuer credit model. An unrelated
bond's issuer is never inferred from its name or rate curve. The supplied eligible
universe must therefore account for other unmapped securities of a defaulted
issuer; this increment does not claim issuer-wide discovery or contagion modeling.
Existing complete-book inputs own mapping, universe, date and policy changes.

Restart replays the full stream image and immutable selected inputs. Identical
event retry uses existing CAS/idempotence. The example checks stored bytes,
cached results and saved-input recomputation, including still-unpriced audits.
Hedge proposals carry no order authority.

## Verification and limits

Independent Decimal80 controls declare 0%, 35% and 100% principal recovery,
signed long/short cash, 14-day collateral price/risk and full/half 1bp bumps.
The amortizing case processes same-day 5000-to-3000 principal reduction first,
retaining the earned principal and coupon separately. The declared absolute
USD/risk bound is 1e-7 with no relative allowance. Existing financial evidence
and tolerances are unchanged.

The example reuses the existing native arithmetic checker: exact cash/model-P&L
reconciliation and visible marking residuals bounded by the derived half-ULP per
operation budget. Residuals are never clamped or assigned a chosen tolerance.

This increment covers supplied standard bond principal recovery and actual cash.
Optional/exercise, physical settlement, multi-cohort ownership, zero/bill core
integration and estimated recovery remain separate. One local worker is
supported. Live providers, multi-server/colocation performance and independent
market/model/platform/production qualification remain open.
