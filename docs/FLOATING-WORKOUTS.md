# Floating-rate workout price and discount margin

`convex::workflow::{quote_floating_workout_price,quote_floating_workout_dm,
quote_floating_workout_paths}` price a floating-rate bond (term or overnight
index) under an explicit legal exercise choice, and solve for its discount margin
(DM). `BondContract::floating_workouts` resolves the full unpaid projection once.
The resulting `PreparedFloatingWorkout` then composes each path. Plain and
workout analytics share the same cash, fixing, profile, accrued, rounding,
discount and inverse-solver code.

No service, provider, database, message queue, global valuation date or system
clock is involved. Each function validates its input and returns an error
instead of guessing.

Pass in `FloatingWorkoutInputs`:

- the full validated contract and separate purchase and exercise calendars;
- a `WorkoutContext` and a complete `BondProjectionContext`;
- the fixing histories and publications;
- exact revisions of the index-projection curve and the collateral-discount curve;
- a signed `RiskPosition` in units of face.

Settlement and payment ownership must agree across contexts. The fixing valuation
date and both curve origins must equal the information date. Publication and
known-at cutoffs are separate timestamps that you supply. All due history is
resolved before forecasting. Missing history, administrator decisions, calendars
or exact curve versions fail. A discount curve cannot replace index projection,
even if all observations happen to be known.

The shared `floating_workout` operation takes `FloatingWorkoutInput`, the
context and one action:

| `action.kind` | Controls you pass in | Result |
|---|---|---|
| `price` | Full `selection`, annual decimal `decimal_dm` | Conditional full cash, price and signed purchase consideration |
| `discount_margin` | Full `selection`, clean/dirty `target`, `BondInverseSettings` | Same-cash inverse, domain/residual/work and consideration |
| `paths` | Common `target`, `WorkoutSolveSettings`, exact `shifted_prefixes` | Every eligible maturity/European/Bermudan/American path, including puts |

`selection` is `{"path":{"kind":"exercise","provision":0,
"date":"2024-09-29"},"weight_end":null}` or an original maturity path.

`weight_end` must always be present, either as a date or as an explicit null.

- A partial shifted overnight `PayAccrued` path requires its declared observation
  weight cutoff.
- Unshifted, term, completed-boundary, maturity and included-accrued paths
  require null.

Purchase accrued uses its own context cutoffs. Path enumeration needs exactly the
eligible shifted partial choices, with no duplicates, omissions, null choices or
unused dates. No calendar mapping or default cutoff is inferred for a
non-business-day exercise.

Term partial cash sums actual reset/profile slices, including independent
principal, coupon, gearing and spread steps. Overnight partial cash uses the resolved daily product prefix and the actual
partial holiday weights. Lookback and shifted weight windows remain distinct.
Existing lockout observations are reused, and a shorter exercise coupon never
creates a new lockout. Partial cash keeps only the reset intervals or clipped
observation weights it consumed, with their known or forecast origins.
Deterministic coupon and spread cash stays outside index compounding.

Early exercise retires remaining principal after same-day scheduled reductions.
Completed unpaid coupons and effective principal claims keep their cash dates,
including dates after redemption. Later cash is removed. `PayAccrued` pays the
partial coupon; `IncludedInPrice` adds none. Partial coupon rounding comes first,
then the combined redemption settlement cash is rounded once, with the rounding
allocation in principal. Purchase accrued, rebate and the original face denominator are
unchanged. Ambiguous ex-coupon ownership, overlapping
partial periods, unmodeled cap/floor, trigger and make-whole terms fail.

DM applies the ACT/365F continuous annual spread to the settlement-normalized
collateral discount factors while every projected coupon is frozen. It is a
conditional measure on projected dated cash. It is separate from contractual
YTM, Street quotation, model OAS and stochastic exercise value.

The closed domains, identification checks, mixed-sign rejection, work ceilings
and final price check all stay mandatory. A no-root, non-unique or
non-converged result carries no quotation. Negative rates and cash are accepted
where the contract permits them.

`paths` inspects every eligible calendar date. It keeps protection and notice
rules, provision and date order, and every holder put. A timely known notice
forces its one exact path. There is no minimum or maximum DM, and no market
worst-DM is invented. The 16,384-date and 67,108,864-work ceilings and the
all-path failure rules apply.

Before any inverse runs, the work reservation is the sum of two terms:
`(cash count + 2) × path count × (max_iterations + 8)`, and twice the coupon,
rate and observation entry count per path. Their sum must fit the supplied
`max_cash_evaluations`. This reserves work for both path validation and
composition. Per-primitive work bounds still apply. It is not a count of CPU
operations.

Every legal path and supplied prefix is validated before any inverse. Projection
freezes once before the batch. These are bounded batch calculations with no
latency guarantee.

The output keeps the reference, quote and quotation-policy revisions, all calendar
pins, and the source terms, context, cash and dependencies. Price consideration
uses the computed original-face dirty price. Inverse consideration uses the
observed dirty target (or clean plus purchase accrued), separately from the
approximate final inverse repricing. Signed quantities scale once. This unrounded
model consideration is not a fee, ledger posting or receipt, and it grants no
authority.

ISDA's [compounding supplement FAQ](https://www.isda.org/a/WlEgE/A44552375-v5.0-ISDA_FAQs-on-Compounding-Averaging-Supplement.pdf)
distinguishes lookback accrual weights, shifted observation weights and frozen
lockout observations. These distinctions guided the reuse of the existing primitives. The contract you
pass in determines its dates and lags. The results have not been checked against
an ISDA contract or a market. Independent Decimal60 controls on fictional bonds
were written down before the Rust results were generated, and they are kept
separate from the generated transport envelopes.

```sh
python3 tests/scripts/floating_workout_reference.py check
python3 tests/scripts/floating_workout_admission.py check
python3 tests/scripts/floating_workout_qualified.py check
cargo test --locked -p convex --all-features --test floating_workout
cargo run --locked -p convex --example floating_workout --features schemars
```

Fifty successful requests contain 70 independently calculated full quotations;
39 requests exercise typed failures. Two overnight all-path inverses have no
root within their supplied bounds and remain failures. Separate valid contracts
carry their own controls. The generated envelopes cannot replace these
independent references.

The existing C, Java, Excel, MCP and WASM dispatcher carries this request.
Not yet covered: execution under Windows/.NET/Excel, Java/JNI and wasm32/browser,
and independent financial, model, library and production certification.
See [fixed-rate workout quotations](WORKOUT-QUOTATIONS.md) and [migration](internal/ROUTE-MIGRATION.md).
