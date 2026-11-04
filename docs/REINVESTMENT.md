# Reinvestment projection

`convex_bonds::pricing::reinvestment` projects dated cash flows that you pass in
through a constant-rate account model. Its errors are `Result`s: the functions
validate their input and return an error instead of guessing.

It requires:

- Settlement and maturity dates.
- Positive initial cash.
- Separate valuation and reinvestment rates.
- A positive periodic frequency.
- An explicit reporting days-per-year.
- A positive maximum horizon in calendar days.

Every amount uses the same cash units. Dates are bounded and sorted, and at most
4096 transfers are accepted. Transfers on the same day keep their order.

Growth is `(1 + annual_rate / frequency)^(frequency * actual_days / 365)`. The
kernel evaluates logarithmic growth before exponentiation:

- For small period rates it uses the portable `log1p` kernel, so a rounded unit
  base cannot lose or amplify the rate at large frequencies.
- Near the negative domain boundary it takes the logarithm of the checked Decimal
  base. Rounding the period rate itself to `-1` must not destroy a positive base.
- A binary64-to-Decimal conversion that loses more than `1e-12` of the factor is
  a precision error, including for nonzero values that round away.

These checks use the same locked portable kernels on native and WebAssembly
targets.

The residual account grows at the valuation rate and pays each transfer. A
separate reinvestment account receives that transfer and grows at the
reinvestment rate. Signed transfers and negative rates inside the positive-base
domain are valid. At the effective horizon the two accounts are added, and the
simple return is annualized on the reporting basis. The horizon is clamped to
maturity before the date addition. Reaching maturity requires a terminal cash
record, which may be an explicit zero payout, and it closes the residual account.
Missing terminal cash is an error.

These now return checked `Result` errors where they used to panic or give a wrong
answer: frequency zero, NaN-to-unit growth, lost nonzero rate precision, expired
intervals, unordered cash, and Decimal overflow or underflow. The model does not
construct instrument cash, mark claims as paid, reconcile receipts or report
realized P&L. Bond forwards need their funding and income curve roles and
instrument terms.

The shared `reinvestment` operation exposes `project`, `growth`, `classify` and
`remaining_years` through JSON, C, Java, Excel, MCP and WASM. Projection terms and
cash use the Rust types, and monetary and rate fields are exact Decimal strings.
`classify` takes explicit thresholds and an enabled flag. An invalid remaining
time fails even when the policy is disabled.

## Migration notes

Replace analytics `money_market_yield` and `money_market_yield_with_horizon` with
`constant_rate_return` and `constant_rate_return_with_horizon`. These apply the
flat rate you pass to both accounts and call the same projection code. They keep
the cash units and return a decimal rate, not a money-market quotation.

`ShortDateCalculator` construction, rule loading, threshold setters, time queries
and selectors now return `Result`. Its duplicate, unused `RollForwardMethod`
selector is removed, and the enabled flag is honored.

The old YAS calculator inferred a money-market reporting basis from the currency
and silently discarded projection errors. Its money-market output, DTO and schema
field and Excel row are removed. Its `true_yield = ytm` output was fabricated and
is also removed. The unused `YasAnalysis` / `YasAnalysisBuilder` facade is
retired, because it filled in zero for missing yields, spreads and risk. Use
`YASCalculator` for the supported diagnostics, and keep missing values as missing
in your own records. A flat account return or an ordinary YTM does not give a
vendor "true yield".

## Evidence

Independent 110-digit Decimal references cover separate rates, unit scaling,
signed and zero terminal cash, same-day components, horizon clamping and negative
rates. QuantLib compound-factor comparisons are engineering checks. They do not
approve any instrument convention or show production readiness. Not yet
covered: dated yield conventions across all instruments; model, calibration,
optional, claim, physical and mixed-book coverage; Windows/Excel execution; and
independent validation.
