# Supplied-cash yield pricing

`convex_bonds::pricing::yield_cash` prices explicit positive discount years and
cash amounts with an explicit periodic, annual, continuous or simple discount
law. Cash units and input order are retained. Signed cash components may be
priced when the net price is positive. These inputs do not establish entitlement,
an instrument, a calendar, ICMA reference periods or a market quotation convention.

`YieldSolver::solve_cash` uses the same native law for every trial and final
repricing. It accepts nonnegative cash with positive exposure, a positive target,
an initial annual decimal rate in the law's domain, a finite positive absolute
price-residual tolerance and a bounded iteration setting. Signed cash can have
multiple yields and requires a separate explicit root-selection contract.
Invalid inputs, unsupported convention aliases, domain violations, overflow,
nonzero underflow and failed convergence return errors. True zero cash components
and negative yields within the actual discount domain remain valid.

The existing Newton/Brent algorithms, defaults and starting points are retained.
No failed trial becomes an accepted price or root. The final result retains its
actual iteration count and must reprice within the original native residual
bound. Numerical price and derivative checks do not establish independent model
or market approval.

`YieldSolver::dirty_price_from_yield`, `clean_price_from_yield` and both native
and analytics `YieldEngine::price_from_yield` now return `Result`. Propagate the
error with `?`. The analytics `StandardYieldEngine` reexports the actual native
type and delegates its trait calls; it no longer contains a second solver.
Its accrued method now delegates the [checked accrual owner](ACCRUED-METHODS.md).
The retained dated-time preparation still needs its complete contextual migration.
In particular, retained ICMA stub/reference
shortcuts are not qualified by the supplied-cash numerical checks.

The strict shared `cash_yield` operation dispatches `price` or `solve` to these
native methods. A price reports `input_cash` units. An inverse reports an
`annual_decimal_rate`, its original native iterations and residual in `input_cash`.
The full operation is available through the existing shared C/Java/MCP/WASM
adapters; source-bound execution evidence records which hosts actually ran.
Windows/Excel remains explicitly unvalidated on this machine.

```sh
cargo test --locked -p convex-analytics --test yield_pricing
cargo run --locked -p convex --example yield_pricing
```

CVX-063 remains open for the original contextual conventions, accrued/calendar
mechanics, broader product/model/calibration/claim/physical/mixed-book scope and
all required operation evidence. Independent CVX-064/065 approval and production
latency/distribution gates retain their own requirements.
