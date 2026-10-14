# Supplied term coupon options

`convex::library::LibraryOperation::TermCouponOptions` prices explicitly bounded
term coupon **segments**. Each original reset, notional, coupon, spread, gearing,
cap and floor boundary is retained. The payoff on each segment is
`min(cap, max(floor, gearing * index + coupon + spread))`, omitting absent bounds.
This operation does not interpret the bounds as a cap on the sum of a coupon's
multiple reset rates.

Supply the complete checked bond terms and calendar revisions, settlement and
fixing context, actual fixing history and publication times, a separate index
projection curve and collateral discount curve, and `TermCouponOptionInputs`.
Projection is optional only when every required fixing is already known. All
missing published history fails before forecasting.

For each future observation/payment pair, supply a `TermCouponModelPoint` with
source, revision and information cutoff. `normal` uses annual decimal rate
volatility; `shifted_lognormal` uses annual relative volatility and a nonnegative
decimal displacement. Both use ACT/365F observation expiry. The supplied forward
adjustment explicitly converts the projected forward to its payment-measure
mean; the library does not infer that adjustment. Negative gearing reverses the
option side. A zero-variance strike kink reports unavailable derivatives.

Published fixings use exact Decimal cash arithmetic and contractual rounding at
the coupon payment boundary. Future stochastic cash requires `Unrounded`:
rounding its expectation would not price the expectation of rounded cash.
`NegativeCouponPolicy::Reject` requires a compatible payoff support; it never
adds an implicit zero floor. Pending-publication uncertainty needs an additional
conditional model and fails explicitly.

The result retains the complete terms, context, options, actual rate lineage,
segment dates, expected currency cash and model derivatives. PV includes actual
principal repayments and entitled coupon cash. Dirty price and model accrued
use the original face; clean price subtracts that same model accrued. Model
expectations and accrued are not payment records or ledger entries.

The pure leaf functions live in `convex_bonds::options::floating_coupon`; full
term composition lives in `convex_analytics::floating_coupon`. The shared
dispatcher only resolves supplied resources and preserves native errors.

Whole-coupon and overnight options, callable joint models, stochastic cash
rounding, calibration, inverse quotations and complete book risk/lifecycle
qualification remain required work. This scoped implementation does not certify
those original capabilities or replace independent convention/model approval.
