# Supplied floating workout cash and discount margin

`convex::workflow::{quote_floating_workout_price,quote_floating_workout_dm,
quote_floating_workout_paths}` compose actual original term/overnight cash under
an explicit legal exercise choice. `BondContract::floating_workouts` resolves
the full unpaid original projection once; its checked `PreparedFloatingWorkout`
then composes each path. The same original cash, fixing, profile, accrued,
rounding, discount and bounded inverse owners serve plain and workout analytics.
No service, provider, database, MQ, global valuation date or system clock enters.

Supply `FloatingWorkoutInputs`: the full original checked contract and independent
actual purchase/exercise calendars; original `WorkoutContext`; complete
`BondProjectionContext`; original histories/publications; exact index-projection
and collateral-discount curve revisions; and signed original-face `RiskPosition`.
Settlement/payment ownership must agree across contexts. Fixing valuation date
and both curve origins must equal the original information date. Publication and
known-at cutoffs remain independent supplied timestamps. All due history is
resolved before forecasting. Missing history, administrator decisions, calendars
or exact curve versions fail; a discount curve cannot replace index projection,
even if all observations happen to be known.

The strict shared `floating_workout` operation takes `FloatingWorkoutInput`,
the original context and one disjoint action:

| `action.kind` | Supplied controls | Result |
|---|---|---|
| `price` | Full `selection`, annual decimal `decimal_dm` | Conditional full cash, price and signed purchase consideration |
| `discount_margin` | Full `selection`, original clean/dirty `target`, `BondInverseSettings` | Same-cash inverse, original domain/residual/work and consideration |
| `paths` | Common original `target`, `WorkoutSolveSettings`, exact `shifted_prefixes` | Every eligible maturity/European/Bermudan/American path, including puts |

`selection` is `{"path":{"kind":"exercise","provision":0,
"date":"2024-09-29"},"weight_end":null}` or an original maturity path.
`weight_end` must always be present as an actual date or explicit null. A partial
shifted overnight `PayAccrued` path requires its declared original observation
weight cutoff. Unshifted/term, completed boundary, maturity and included-accrued
paths require null. Purchase accrued uses its own original context cutoffs.
Path enumeration needs exactly the eligible shifted partial choices, without
duplicates, omissions, null choices or unused dates. No universal calendar
mapping or default cutoff is inferred for a nonbusiness exercise.

Term partial cash sums actual reset/profile slices, including independent
principal, coupon, gearing and spread steps. Overnight partial cash uses the
original resolved daily product prefix and actual partial holiday weights.
Lookback and shifted weight windows remain distinct. Original lockout observations
are reused; a shorter exercise coupon never creates a new lockout. Partial cash
retains only the original consumed reset intervals or clipped observation weights
and their original known/forecast origins. Deterministic coupon/spread cash stays
outside index compounding under the existing supplied contract.

Early exercise retires remaining principal after same-day scheduled reductions.
Completed unpaid coupons and effective principal claims retain their original
cash dates, including dates after redemption. Later cash is removed. `PayAccrued`
pays the actual original partial coupon; `IncludedInPrice` adds none. Partial
coupon rounding precedes once-rounded combined redemption settlement cash, with
the rounding allocation in principal. Purchase accrued/rebate and the original
face denominator remain unchanged. Ambiguous ex-coupon ownership, overlapping
partial periods, unmodeled cap/floor, trigger and make-whole terms fail.

DM applies the ACT/365F continuous annual spread to settlement-normalized
original collateral discount factors while every projected coupon is frozen.
It is a conditional projected dated-cash measure, separate from contractual YTM,
Street quotation, model OAS and stochastic exercise value. Original closed
domains, identification, mixed-sign, work ceilings and final native price
postconditions remain mandatory. No root, nonunique and exhausted/convergence
outcomes contain no accepted quotation. Negative rates/cash are accepted where
the original contract permits them; mixed-sign inverses retain their rejection.

`paths` inspects every actual eligible calendar date, retaining protection and
notice rules, original provision/date order and every holder put. A supplied
timely known notice forces its exact one path. There is no selected minimum or
maximum DM and no invented market worst-DM meaning. The unchanged original
16,384-date/67,108,864-work ceilings and all-path failure rules apply. Before any
inverse, the reservation sums `(original cash count + 2) × path count ×
(max_iterations + 8)` and twice the original coupon/rate/observation entry count
per path. Their sum must fit the supplied `max_cash_evaluations`. This reserves
both path admission and composition; primitive-specific internal work bounds
still apply. It is not a count of CPU operations. Every legal path and supplied
prefix is admitted before any inverse. Projection freezes once before the batch.
These are bounded batch calculations without a latency certificate.

Reference, independent quote and quotation-policy revisions, all original calendar
pins and source terms/context/cash/dependencies survive output. Price consideration
uses computed original-face dirty price; inverse consideration uses the original
observed dirty target (or original clean plus purchase accrued), separately from
approximate final inverse repricing. Signed quantities scale once. This unrounded
model consideration is separate from fees, ledger posting, receipts or authority.

ISDA's [compounding supplement FAQ](https://www.isda.org/a/WlEgE/A44552375-v5.0-ISDA_FAQs-on-Compounding-Averaging-Supplement.pdf)
distinguishes lookback accrual weights, shifted observation weights and frozen
lockout observations. These distinctions inform reuse of the existing primitives;
the supplied contract determines its dates and lags. No ISDA contract or market
certificate is claimed. Independent Decimal60 fictional controls are declared
before candidate execution and remain separate from native transport envelopes.

```sh
python3 tests/scripts/floating_workout_reference.py check
python3 tests/scripts/floating_workout_admission.py check
python3 tests/scripts/floating_workout_qualified.py check
cargo test --locked -p convex --all-features --test floating_workout
cargo run --locked -p convex --example floating_workout --features schemars
```

Fifty successful requests contain 70 independently calculated full quotations;
39 requests exercise original typed failures. The preserved original two
overnight all-path inverses have no root within their supplied bounds and remain
failures. Separate admissible contracts carry their own declared controls. Native
envelopes cannot replace these financial references.

The existing generic C/Java/Excel/MCP/WASM dispatcher carries this additive request.
Actual Windows/.NET/Excel, Java/JNI and wasm32/browser execution and independent
financial/model/library/production certificates remain separate qualification.
See [workout ownership](WORKOUT-QUOTATIONS.md) and [migration](ROUTE-MIGRATION.md).
