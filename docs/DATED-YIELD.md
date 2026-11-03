# Dated cash discount time

This document covers `convex_bonds::pricing::discount_time::prepare_dated_cash`.
It turns settlement and payment dates into discount years and passes the input
cash through unchanged. Price, inverse yield and analytical yield risk all use
this same timing code.

The cash must already belong to the recipient under the instrument's schedule,
calendars and entitlement state. This operation does no calendar adjustment,
coupon construction, payment confirmation or quotation-policy selection.

Pass a `DiscountTimeBasis` explicitly:

| Basis | Required context |
| --- | --- |
| `Calendar` | A date-only day count, evaluated from settlement to each payment |
| `Icma` | The `ScheduleSpec` and complete anchored nominal reference periods, including explicit payment tails |
| `Act365L` | Coupon frequency and contiguous coupon periods covering settlement through every payment |
| `Thirty360EIsda` | Termination date for the February maturity exception |

`Calendar` rejects ICMA, ACT/365L and 30E/360 ISDA. These cases fail:

- missing periods, gaps, overlaps or wrong anchors;
- absent delayed-payment coverage;
- mismatched discount roles.

A period without cash still contributes time, and several amounts on one date
share the same time. Nominal months, a stub threshold, a cash index, an
absolute-day or clamping correction, and a date-only substitute never determine
the context. Coupon frequency and yield compounding remain separate inputs.

`StandardYieldEngine` uses `rules.discount_day_count`. Its optional
`with_discount_basis` binds the contextual timing without changing
`rules.accrual_day_count`. `YieldSolver::with_discount_basis` gives the same
context to its dated methods. Without that context, both engines accept only
date-only day counts. `dated_cash_yield_risk_with_basis` takes the same full
basis; the older date-only risk function delegates to it and rejects dependent
conventions. Fixed-bond contract/workout quotation and entitlement keep their
existing implementations.

The shared `cash_yield` operation exposes `dated_price`, `dated_solve` and
`dated_risk`. Each takes `cash`, `settlement` and `basis`. Price and solve take
`CashYieldCompounding`; risk takes the core `Compounding` enum. Each amount is
an exact plain decimal **string**. Price keeps signed cash. The unique-root
inverse and risk require nonnegative cash with positive exposure. Root
selection for signed cash is not yet implemented.

Bounds:

- 1–4096 sorted, strictly future cash records.
- 1–4096 mesh periods.
- Dates in years 1–9999, with at most 150 × 366 days per interval and per
  complete mesh.

Equal-date components keep input order. Work is proportional to cash entries ×
mesh periods: after the initial context validation, the ICMA primitive
validates and accumulates its mesh on each payment. Past or settlement-date cash
fails; it is not dropped. The domain, finite-value, arithmetic and solver
residual/work guards remain in force.

Run the direct independent controls with:

```sh
cargo test --locked -p convex-analytics --test dated_yield
cargo test --locked -p convex --test dated_yield
cargo run --locked -p convex --example dated_yield
```

The controls keep three recorded failures and independently declared
Fraction/Decimal110 timing, prices, inverses and derivatives. They do not
validate a market convention, model, host platform or latency target.

# Discount accuracy

Periodic pricing of the cash you pass in evaluates
`exp(-time * frequency * log1p(rate / frequency))`, multiplying the smallest and
largest factor magnitudes first. This keeps tiny period rates at high
frequencies and avoids overflowing an intermediate product when the full
discount exponent is representable. Near the negative domain boundary the
positive base comes from `(frequency + rate) / frequency`. Annual pricing uses
`log1p(rate)`. The same Rust kernel supplies both price engines and inverse
trial prices. Invalid domains, nonfinite arithmetic and nonzero underflow still
return errors.

ACT/ACT AFB counts whole anniversaries backwards from the end date. A February
28 anniversary becomes February 29 in a leap year; the remainder includes a leap
day only in the interval with start included and end excluded. Reversed dates
negate the forward calculation. This correction covers the AFB day count only;
it has not been compared with a vendor quotation convention.
