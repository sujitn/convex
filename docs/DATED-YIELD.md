# Dated cash discount time

`convex_bonds::pricing::discount_time::prepare_dated_cash` prepares actual
settlement-to-payment years and unchanged input cash. Price, inverse yield and
analytical yield risk use this same timing owner. Cash must already belong to
the recipient under the original instrument's schedule, calendars and entitlement
state. This operation performs no calendar adjustment, coupon construction,
payment confirmation or instrument quotation-policy selection.

Supply `DiscountTimeBasis` explicitly:

| Basis | Required context |
| --- | --- |
| `Calendar` | A date-only day count, evaluated from settlement to each actual payment |
| `Icma` | Original `ScheduleSpec` and complete anchored nominal reference periods, including explicit payment tails |
| `Act365L` | Actual coupon frequency and contiguous actual coupon periods covering settlement through every payment |
| `Thirty360EIsda` | Original termination date for the February maturity exception |

`Calendar` rejects ICMA, ACT/365L and 30E/360 ISDA. Missing periods, gaps,
overlaps, wrong anchors, absent delayed-payment coverage and mismatched discount
roles fail. A period without cash still contributes time; several amounts on
one date retain the same time. No nominal months, stub threshold, cash index,
absolute-day/clamping correction or date-only substitute determines context.
Coupon frequency and yield compounding remain separate inputs.

`StandardYieldEngine` consumes `rules.discount_day_count`. Its optional
`with_discount_basis` binds original contextual timing without changing
`rules.accrual_day_count`. `YieldSolver::with_discount_basis` provides the same
context to its dated methods. Without that context, both engines admit only
date-only day counts. `dated_cash_yield_risk_with_basis` consumes the same full
basis; the older date-only risk function delegates to it and rejects dependent
conventions. Fixed-bond contract/workout quotation and entitlement keep their
existing complete owners.

The shared `cash_yield` operation exposes `dated_price`, `dated_solve` and
`dated_risk`. Each accepts actual `cash`, `settlement` and `basis`. Price/solve
take `CashYieldCompounding`; risk takes the explicit core `Compounding` enum.
Each amount is an exact plain decimal **string**. Price retains signed cash;
the unique-root inverse and risk require nonnegative cash with positive exposure.
Full signed-cash root selection remains separate required work.

Bounds are 1–4096 sorted strictly future cash records and 1–4096 supplied mesh
periods, with dates in years 1–9999 and at most 150 × 366 days per admitted
interval/complete mesh. Equal-date components retain input order. Work is proportional to
cash entries × supplied mesh periods; the reused ICMA primitive validates and
accumulates its mesh on each payment, after the initial context validation. Past or settlement-date
cash fails; it is not dropped. Native domain, finite-value, arithmetic and
solver residual/work guards remain in force.

Run the direct independent controls with:

```sh
cargo test --locked -p convex-analytics --test dated_yield
cargo test --locked -p convex --test dated_yield
cargo run --locked -p convex --example dated_yield
```

The controls retain three actual original failures and predeclared independent
Fraction/Decimal110 timing, prices, inverses and derivatives. These engineering
checks do not approve a market convention, model, host platform, latency target
or production deployment.
