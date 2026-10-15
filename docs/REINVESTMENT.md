# Checked supplied-cash projection

`convex_bonds::pricing::reinvestment` owns a constant-rate account model over
original dated cash. It requires settlement, maturity, positive initial cash,
separate valuation and reinvestment rates, positive periodic frequency, explicit
reporting days per year and a positive maximum calendar-day horizon. Every amount
uses the same original cash units. Dates are bounded and sorted; at most 4096
transfers are accepted. Same-day components retain their original order.

Growth is `(1 + annual_rate / frequency)^(frequency * actual_days / 365)`.
The residual account grows at the valuation rate and pays each supplied transfer;
the separate reinvestment account receives that transfer and grows at the
reinvestment rate. Signed transfers and negative rates inside the positive-base
domain remain valid. At the effective horizon the two accounts are added, then
simple return is annualized on the explicit reporting basis. The horizon is
clamped to maturity before date addition. Reaching maturity requires an actual
terminal cash record, including an explicitly supplied zero payout, and closes
the residual account. Missing terminal cash is an error.

Checked `Result` errors replace panics, frequency-zero success, NaN-to-unit growth,
lost nonzero rate precision, expired intervals, unordered cash and unchecked
Decimal overflow/underflow. This numerical model does not construct instrument
cash, mark paid claims, reconcile receipts or report realized P&L. Bond forwards
require their actual funding and income curve roles and instrument terms.

The shared `reinvestment` operation exposes `project`, `growth`, `classify` and
`remaining_years` through native JSON, C, Java, Excel shared JSON, MCP and WASM.
Projection terms and cash are the native types; monetary/rate fields use exact
Decimal strings. Classification consumes explicit thresholds and its enabled
flag. Invalid remaining time fails even when the policy is disabled.

Migrate analytics `money_market_yield` / `money_market_yield_with_horizon` to
`constant_rate_return` / `constant_rate_return_with_horizon`. These adapters
explicitly apply the supplied flat rate to both accounts and delegate to the
native owner. They retain original cash units and return a decimal rate, not a
market money-market quotation. `ShortDateCalculator` construction, rule loading,
threshold setters, time queries and selectors now return `Result`. Its duplicate
unused `RollForwardMethod` selector is removed; the enabled flag is honored.

The old YAS calculator inferred a money-market reporting basis from currency and
silently discarded projection errors. Its money-market output, DTO/schema field
and Excel row are removed. The calculator's fabricated `true_yield = ytm` output
is also removed. Separately supplied true-yield analysis remains available.
Neither a flat account return nor ordinary YTM establishes a vendor true yield.

Independent 110-digit Decimal references cover separate rates, original unit
scaling, signed and zero terminal cash, same-day components, horizon clamping and
negative rates. QuantLib compound-factor comparisons are engineering evidence;
they do not approve instrument conventions or production readiness. Full dated
yield conventions, model/calibration/optional/claim/physical/mixed-book coverage
remain required under CVX-063. Windows/Excel execution and independent approval
remain explicitly unvalidated.
