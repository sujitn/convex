# Supplied workout quotations

`convex::workflow::{quote_workout_price, quote_workout_yield,
quote_workout_worst}` quote complete fixed, stepped and amortizing bond cash
under an explicit `DatedYieldConvention`. `BondContract::fixed_workout_projection`
owns early-redemption cash composition. The existing native coupon/profile,
principal, rounding, dated-discount and bounded inverse calculations are reused.

Supply the original checked `BondContract`, actual purchase and exercise calendars
with their original identities, and `WorkoutContext`. The shared `workout` request
contains full `BondInput` terms/calendar revisions, separate security reference
and quote revisions, the context, convention and one action:

| `action.kind` | Required action inputs | Result |
|---|---|---|
| `price` | `selection`, annual decimal `decimal_yield` | Full native workout cash and price |
| `yield` | `selection`, original-face clean/dirty `target`, original `BondInverseSettings` | Same-pricer yield, work and final price residual |
| `worst` | One clean/dirty `target`, `WorkoutSolveSettings` | Every eligible maturity/call/put yield and the selected full quotation |

`selection` is `{"kind":"maturity"}` or
`{"kind":"exercise","provision":0,"date":"2027-06-15"}`. The provision
index refers to the complete original exercise array; an arbitrary date cannot
create a call or put. Exercise is strictly after purchase settlement and before
contractual maturity. Maturity retains its original redemption multiplier.

The context retains `reference_date`, `trade_date`, `settlement`, `on_payment` and
`notice`. Original purchase lag, payment calendar and settlement adjustment must
reproduce the supplied settlement. Information must be known by settlement.
New notice uses `{"kind":"unnotified","earliest_notice":"2026-05-05"}`;
the caller supplies its applicable policy date rather than an embedded trading
or regulatory default. Already issued notice uses
`{"kind":"notified","provision":0,"exercise":"2027-06-15",
"notice_date":"2027-06-01"}`. It must be known at the reference date, timely
under the original actual calendar, and match the exact original provision/date.
It forces that path; conditional maturity or another exercise is rejected.

For unnotified `worst`, European dates, every original Bermudan date and **every
future calendar date** in each American window are inspected. Original protection
and notice eligibility apply. Weekends are not removed from an exercise window
unless the original contract requires that; actual business-day notice and cash
offsets still use the supplied calendar. Output order is maturity, then original
provision order and date order. No call date is replaced by a coupon-date proxy.
The minimum issuer-call/maturity yield is selected. Puts remain separate holder
choices and cannot reduce this worst-call measure. Equal numerical yields keep
the first candidate in that stated order. Supplied notice instead forces its one
path, identified by `forced_by_notice`.

Early exercise retires outstanding principal **after** same-day scheduled
reductions, matching native lifecycle ordering. Every completed unpaid coupon and
effective principal repayment remains a claim on its original payment date,
including cash arriving after early redemption. Future coupons and later principal
reductions are removed. `PayAccrued` uses the original segmented partial coupon;
`IncludedInPrice` adds no partial coupon. Original-face denominators and purchase
accrual/rebates remain. Redemption plus the rounded partial coupon is rounded once
under the original cash policy; the final rounding allocation is carried in the
redemption component. Separate raw redemption, partial coupon and combined cash
remain visible. Expected cash neither records exercise nor asserts settlement.

Every single-path result retains complete source terms/cash, original context,
selection, notice/exercise/cash dates, actual used cash and native price/inverse.
All-date results retain source terms/cash once through the selected full quotation
and compact diagnostics for every quoted path, including puts. Settings, target,
residuals, iterations, actual price evaluations and cash-discount work remain
explicit. Any path failure returns no partial accepted result, including an
unselected put failure. Original closed inverse domains, identification, mixed-sign
and final native-price postconditions are unchanged.

`max_dates` is caller-declared in `1..=16,384`. It counts actual future contractual
dates examined, including protection/notice exclusions. `max_cash_evaluations` is
in `1..=67,108,864`. Before solving, each candidate reserves the original unpaid
cash count plus at most two exercise components, multiplied by the unchanged
inverse iteration ceiling plus eight endpoint/final-call slots. The full total
must fit the supplied budget. Actual cash-discount evaluations must remain within
that reservation. There is no truncation, adaptive relaxation or budget retry.
The existing shared JSON ingress/output limits also apply. Full-window calculations
are batch work; no colocation or tail-latency qualification is implied.

These are conditional **dated cash yields**, without inferred Street quotation,
exercise probabilities, volatility, option value or OAS. Normal optional market
pricing retains its original model guards. Complete term/overnight floating
cash uses the separate [frozen-projection DM workflow](FLOATING-WORKOUTS.md),
with actual history, independent curve roles and explicit shifted prefixes.
Contextual ICMA yield,
make-whole/trigger terms, ambiguous ex-coupon partial ownership and overlapping
partial accrual intervals fail explicitly. Full market/convention/model/host
qualification remains open.

[MSRB Rule G-15](https://www.msrb.org/Rules-and-Interpretations/MSRB-Rules/General/Rule-G-15)
distinguishes issuer call/maturity quotations from agreed put-date quotations and
retains notice considerations for continuously callable securities. These
distinctions inform this API; no municipal confirmation or TRACE compliance is
claimed. Independent Decimal(60) controls were declared before candidate valuation
in `tests/fixtures/library/workout-reference.json`. They cover positive/negative
yields, four compounding choices, steps/amortization, both accrued treatments,
actual holidays, delayed claims, rounding and all 13 eligible paths. Native
envelopes used for adapter parity are separate transport expectations.

```sh
python3 tests/scripts/workout_quotation_reference.py check
python3 tests/scripts/library_workout_inputs.py
cargo test --locked -p convex-analytics --all-features --test workout_quotation
cargo run --locked -p convex --example library_workout --features schemars
```

The shared JSON/C/Java/Excel/MCP/WASM dispatcher carries this request unchanged.
Actual Windows/Excel and wasm32/browser execution still require those hosts.
See [library API](LIBRARY-API.md) and [route migration](ROUTE-MIGRATION.md).

Explicit contextual ACT/ACT ICMA price and same-cash yield inversion are now
available over full original fixed/stepped/amortizing and legal workout cash.
The checked owner requires an independently versioned complete nominal reference
mesh, exact original policy/security/quote/calendar pins, payment coverage and
signed original-face quantities. See [the full contract](ICMA-QUOTATIONS.md).
Contractual accrual, quotation compounding and actual cash dates stay separate;
Street/model/market and unavailable host qualification remains open.
