# Yield pricing from cash flows

This document covers `convex_bonds::pricing::yield_cash`. It prices cash amounts
you pass in, at positive discount times in years, with a periodic, annual,
continuous or simple discount law. Cash units and input order are kept. Signed
cash components can be priced when the net price is positive. These inputs do
not establish entitlement, an instrument, a calendar, ICMA reference periods or
a market quotation convention.

`YieldSolver::solve_cash` uses the same discount law for every trial and for the
final repricing. It accepts:

- nonnegative cash with positive exposure;
- a positive target price;
- an initial annual decimal rate inside the law's domain;
- a finite positive absolute price-residual tolerance;
- a bounded iteration limit.

Signed cash can have multiple yields and needs a separate root-selection rule.
These cases return errors: invalid inputs, unsupported convention aliases, domain
violations, overflow, nonzero underflow and failed convergence. True zero cash
components and negative yields inside the discount domain are valid.

The Newton/Brent algorithms, defaults and starting points are unchanged. No
failed trial becomes an accepted price or root. The final result keeps its
iteration count and must reprice within the residual bound. These numerical
checks do not validate the model against a market.

`YieldSolver::dirty_price_from_yield`, `clean_price_from_yield` and both the
`convex-bonds` and analytics `YieldEngine::price_from_yield` now return
`Result`. Propagate the error with `?`. The analytics `StandardYieldEngine`
reexports the `convex-bonds` type and delegates its trait calls; it no longer
contains a second solver. Its accrued method delegates to the
[checked accrual code](ACCRUED-METHODS.md). The dated-time preparation still
needs to move to the contextual version, and the ICMA stub/reference shortcuts
it keeps are not covered by the cash-based numerical checks.

The strict shared `cash_yield` operation dispatches `price` or `solve` to these
methods. A price reports `input_cash` units. An inverse reports an
`annual_decimal_rate`, its iteration count and a residual in `input_cash`. The
operation is available through the shared C/Java/MCP/WASM adapters; the
execution evidence records which hosts actually ran. Windows/Excel has not been
validated.

```sh
cargo test --locked -p convex-analytics --test yield_pricing
cargo run --locked -p convex --example yield_pricing
```

Not yet covered: contextual conventions, accrued and calendar mechanics, and the
broader product, model, calibration, claim, physical and mixed-book scope.
