# Scalar risk and weekend arithmetic

These pure native helpers operate on supplied numeric exposures. Instrument
valuation, curve calibration, FX conversion, scenario construction and execution
remain with their existing owners. A scalar result does not establish complete
book valuation or a model, market, latency or production certificate.

## Risk values and aggregation

`Duration`, `Convexity` and `DV01` retain their Decimal representation and signed
values. Use checked `TryFrom<f64>` for floating inputs. Nonfinite values, Decimal
range failure and conversion of a nonzero input into zero are errors. Decimal
construction is total; the locked Decimal96bit/scale0..28 representation always
has a finite binary64 projection.

`aggregate_portfolio_risk` accepts at most65,536 positions in one caller-declared
common currency/unit. IDs must be nonempty, unique and at most8192bytes. Checked
Decimal sums preserve signed market value and DV01. Weighted duration is
`sum(value * duration) / sum(value)` only when the original absolute binary64
net-value guard `1e-10` is met. Otherwise it is unavailable; a zero-value book can
still have nonzero actual DV01. Empty exposure has zero value/DV01 and no defined
duration ratio. No FX rate, price or instrument identity is inferred.

Duration hedge units are `-(target_duration * target_value) /
(hedge_duration * hedge_price)`. DV01 hedge units are
`-target_dv01 / hedge_unit_dv01`. Preserve signs, original denominator guards and
genuine zero targets. Nonfinite inputs, overflow and nonzero underflow fail.

`KeyRateDurations::at_tenor` returns an exact finite positive unique factor,
without a tolerance or tenor rounding. Invalid or ambiguous collections have no
admitted match. This getter does not certify the separate older Holding key-rate
aggregator: its milliyear grouping, incomplete-factor/weight handling and missing
HTTP-result presentation still require complete native migration.

## Scalar VaR

Both methods return a signed loss quantile, in the supplied exposure's currency.
A negative quantile is a gain at that confidence. Confidence is finite and
strictly between zero and one; horizon is a positive number of days. Invalid
inputs are rejected even for a zero exposure.

Parametric VaR assumes zero-mean Gaussian P&L and a daily standard deviation:

`loss_quantile = inverse_normal(confidence) * abs(exposure) * volatility * sqrt(days)`.

`parametric_var` takes volatility as a daily fractional portfolio standard
deviation. `parametric_var_from_dv01` takes actual signed currency-per-bp DV01 and
daily yield volatility in bp. Volatility must be finite and nonnegative. The
three retained public Z-score constants are rounded approximations; interpolation
between them is no longer the calculation kernel.

The normal inverse uses the locked portable erfc/log kernels and a bounded solve
on `[0,40]` in log-survival space. A decreasing Mills series beyond26avoids tail
underflow. The solve has at most80iterations and the tail series at most64terms.
It uses bracket-width convergence so representable extreme-tail probabilities
do not require an unattainable absolute probability residual. The existing
approximate CDF, all original tests and solver defaults stay unchanged.

Historical VaR accepts1..1,000,000finite daily returns. It orders actual losses
`-signed_portfolio_value * return` and selects rank
`ceil(exact_binary64_confidence * observation_count)`, starting at1. Integer
decomposition of the actual probability avoids a rounded `1-confidence` and
out-of-range indices. At decimal-looking binary64 boundaries this exact-rank
contract can differ from a library that rounds the product before taking ceil.
No percentile interpolation or absolute-value sign change occurs. Losses are
scaled by `sqrt(days)` under the explicit retained IID assumption; this does not
supply real multi-day scenarios or covariance estimation.

Independent Python NormalDist inverses, Fraction ranks and Decimal90 scaling
precede native execution. New scoped bounds are absolute2e-12for the normal
inverse; VaR uses max(absolute1e-8, relative3e-14), and hedge units use
max(absolute1e-8, relative1e-14). Existing references/tolerances remain unchanged.
The original rounded95% example16450 +/-10still admits the corrected inverse's
result16448.5362695147. These are engineering references, without an independently
approved convention or model pack.

The normal inverse definition follows [NIST](https://www.itl.nist.gov/div898/handbook/eda/section3/eda3661.htm).
The independent implementation is [Python NormalDist](https://docs.python.org/3/library/statistics.html#statistics.NormalDist.inv_cdf).
The empirical inverse convention is described by [NumPy's inverted CDF method](https://numpy.org/doc/stable/reference/generated/numpy.quantile.html).
Exact binary64 rank evaluation and the scalar model assumptions above are this
library's explicit contracts, not publisher or regulatory approval.

## Weekend-only Date operations

All four methods return checked Results. Next/previous weekday retain a weekday
and otherwise select Monday/Friday. Business-day addition excludes the start and
counts the destination; zero retains the date. Signed counts use `(earlier,later]`
and the sign of the supplied direction. Complete weeks take constant work, with
at most six daily steps. The representable calendar range is the bound; no
financial150-year cap is invented for weekend arithmetic. Use a supplied checked
holiday Calendar for instrument economics.
