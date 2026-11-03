# Workout quotations for fixed-rate bonds

`convex::workflow::{quote_workout_price, quote_workout_yield,
quote_workout_worst}` quote the full cash of a fixed, stepped or amortizing bond
under an explicit `DatedYieldConvention`. A workout is a call, put or maturity
date that the price or yield is quoted to.
`BondContract::fixed_workout_projection` builds the early-redemption cash. The
existing coupon, profile, principal, rounding, dated-discount and bounded inverse
calculations are reused.

Pass in a validated `BondContract`, the purchase and exercise calendars with
their identities, and a `WorkoutContext`. The shared `workout` request contains:

- the full `BondInput` terms and calendar revisions;
- separate security reference and quote revisions;
- the context, the convention and one action.

| `action.kind` | Required action inputs | Result |
|---|---|---|
| `price` | `selection`, annual decimal `decimal_yield` | Full workout cash and price |
| `yield` | `selection`, clean/dirty `target` per unit of face, `BondInverseSettings` | Same-pricer yield, work and final price residual |
| `worst` | One clean/dirty `target`, `WorkoutSolveSettings` | Every eligible maturity/call/put yield and the selected full quotation |

`selection` is `{"kind":"maturity"}` or
`{"kind":"exercise","provision":0,"date":"2027-06-15"}`. The provision
index refers to the full exercise array of the contract; an arbitrary date cannot
create a call or put. Exercise is strictly after purchase settlement and before
contractual maturity. Maturity keeps its redemption multiplier.

The context holds `reference_date`, `trade_date`, `settlement`, `on_payment` and
`notice`. The purchase lag, payment calendar and settlement adjustment must
reproduce the settlement date you pass in. Information must be known by
settlement. New notice uses
`{"kind":"unnotified","earliest_notice":"2026-05-05"}`; you supply the policy
date, because there is no built-in trading or regulatory default. Already issued notice uses
`{"kind":"notified","provision":0,"exercise":"2027-06-15",
"notice_date":"2027-06-01"}`. It must be known at the reference date, timely
under the actual calendar, and match the exact provision and date.
It forces that path; conditional maturity or another exercise is rejected.

For unnotified `worst`, the code inspects European dates, every Bermudan date and
**every future calendar date** in each American window. Protection and notice
eligibility apply. Weekends are not removed from an exercise window unless the
contract requires it; business-day notice and cash offsets still use the calendar
you pass in. Output order is maturity, then provision order and date order. No call date is replaced by a coupon-date proxy.
The minimum issuer-call/maturity yield is selected. Puts remain separate holder
choices and cannot reduce this worst-call measure. Equal numerical yields keep
the first candidate in that stated order. Supplied notice instead forces its one
path, identified by `forced_by_notice`.

Early exercise retires outstanding principal **after** same-day scheduled
reductions, matching the lifecycle ordering. Every completed unpaid coupon and
effective principal repayment remains a claim on its payment date,
including cash arriving after early redemption. Future coupons and later principal
reductions are removed. `PayAccrued` uses the segmented partial coupon; `IncludedInPrice` adds no partial
coupon. Face denominators and purchase accrual/rebates stay. Redemption plus the
rounded partial coupon is rounded once under the cash policy, and the final
rounding allocation is carried in the redemption component. Raw redemption,
partial coupon and combined cash are all shown. Expected cash does not record an
exercise or assert settlement.

Every single-path result keeps the full source terms and cash, the context, the
selection, the notice, exercise and cash dates, the cash used, and the price and
inverse. All-date results keep the source terms and cash once, in the selected
full quotation, plus compact diagnostics for every quoted path, including puts.
Settings, target, residuals, iterations, price evaluations and cash-discount work
are reported. Any path failure returns no partial result, including the failure
of an unselected put. The closed inverse domains, identification checks,
mixed-sign rejection and final price check are unchanged.

`max_dates` is caller-declared in `1..=16,384`. It counts actual future contractual
dates examined, including protection/notice exclusions. `max_cash_evaluations` is
in `1..=67,108,864`. Before solving, each candidate reserves the unpaid cash count plus at most two
exercise components, multiplied by the inverse iteration ceiling plus eight
endpoint and final-call slots. The total must fit the budget you pass in, and
the cash-discount evaluations actually used must stay within that reservation.
There is no truncation, adaptive relaxation or budget retry. The shared JSON
input and output limits also apply. Full-window calculations are batch work and
have no latency guarantee.

These are conditional **dated cash yields**. They include no Street quotation,
exercise probability, volatility, option value or OAS. Normal optional-bond
pricing keeps its model checks. Floating-rate cash, term or overnight, uses the
separate [frozen-projection DM workflow](FLOATING-WORKOUTS.md), with fixing
history, separate curve roles and explicit shifted prefixes. Make-whole and
trigger terms, ambiguous ex-coupon partial ownership and overlapping partial
accrual intervals fail with an explicit error. Contextual ICMA yield is covered
below.

Not yet covered: validation against market conventions and models.

[MSRB Rule G-15](https://www.msrb.org/Rules-and-Interpretations/MSRB-Rules/General/Rule-G-15)
distinguishes issuer call/maturity quotations from agreed put-date quotations and
keeps notice considerations for continuously callable securities. This API
follows those distinctions, but it has not been checked for municipal
confirmation or TRACE compliance. Independent Decimal(60) expected values were
fixed before the Rust results were generated, in
`tests/fixtures/library/workout-reference.json`. They cover positive and
negative yields, four compounding choices, steps and amortization, both accrued
treatments, holidays, delayed claims, rounding and all 13 eligible paths. The
generated envelopes used for adapter parity are kept separate.

```sh
python3 tests/scripts/workout_quotation_reference.py check
python3 tests/scripts/library_workout_inputs.py
cargo test --locked -p convex-analytics --all-features --test workout_quotation
cargo run --locked -p convex --example library_workout --features schemars
```

The shared JSON, C, Java, Excel, MCP and WASM dispatcher carries this request
unchanged. Not yet covered: execution under Windows/Excel and wasm32/browser.
See [library API](LIBRARY-API.md) and [route migration](internal/ROUTE-MIGRATION.md).

Contextual ACT/ACT ICMA price and same-cash yield inversion are available over
fixed, stepped and amortizing cash and legal workout cash. They require a
separately versioned, complete nominal reference mesh, exact policy, security,
quote and calendar pins, payment coverage, and signed quantities in units of
face. See [the full contract](ICMA-QUOTATIONS.md). Contractual accrual,
quotation compounding and cash dates stay separate.

`icma_workout` gives contextual price and yield for all paths, plus a
well-defined worst-call selection. It reuses the same legal-date enumeration and
the same ICMA code. See [full ICMA workouts](ICMA-WORKOUTS.md). The
context-free `workout` operation remains the route for the explicit dated-cash
convention.
