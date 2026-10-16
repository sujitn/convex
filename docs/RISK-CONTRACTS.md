# Scalar risk and weekend arithmetic

These pure native helpers operate on supplied numeric exposures. Instrument
valuation, curve calibration, FX conversion, scenario construction and execution
remain with their existing owners. A scalar result does not establish complete
book valuation or a model, market, latency or production certificate.

## Risk values and aggregation

`Duration`, `Convexity` and `DV01` retain their Decimal representation and signed
values. Use checked `TryFrom<f64>` for floating inputs. Nonfinite values, Decimal
range failure and conversion of a nonzero input into zero are errors. Decimal
construction is total; the locked Decimal representation (96 bit coefficient and scale 0–28) representation always
has a finite binary64 projection.

`aggregate_portfolio_risk` accepts at most 65,536 positions in one caller-declared
common currency/unit. IDs must be nonempty, unique and at most 8,192 bytes. Checked
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
admitted match. Supplied Holding aggregation delegates the checked native
`aggregate_key_rate_exposure` owner, with exact factors, dirty-value currency risk,
per-factor coverage and nullable unavailable results. Read the
[portfolio contract](PORTFOLIO-KEY-RATE-RISK.md) before using a partial profile as
full-book risk. Scenario interpolation, stress percentages and the separate
signed mixed-product risk owner retain their own requirements.

The checked floating constructor does not qualify every deserialization path.
Actual retained probes demonstrate nonzero values becoming zero in older derived
risk decoding and in the original raw JSON parser below the binary64 range. The
shared bounded parser now rejects erased numeric tokens before formal parsing;
full-library ingress and supplied key-rate REST reuse that pure core owner. Older
derived risk deserialization and legacy v1 entry points retain their separate
required admission work; their wrong zeros are not validated risk. Exact contract-money fields retain their separate checked
plain-decimal-string owner.

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
on `[0,40]` in log-survival space. A decreasing Mills series beyond 26 avoids tail
underflow. The solve has at most 80 iterations and the tail series at most 64 terms.
It uses bracket-width convergence so representable extreme-tail probabilities
do not require an unattainable absolute probability residual. The existing
approximate CDF, all original tests and solver defaults stay unchanged.

Historical VaR accepts 1–1,000,000 finite daily returns. It orders actual losses
`-signed_portfolio_value * return` and selects rank
`ceil(exact_binary64_confidence * observation_count)`, starting at 1. Integer
decomposition of the actual probability avoids a rounded `1-confidence` and
out-of-range indices. At decimal-looking binary64 boundaries this exact-rank
contract can differ from a library that rounds the product before taking ceil.
No percentile interpolation or absolute-value sign change occurs. Losses are
scaled by `sqrt(days)` under the explicit retained IID assumption; this does not
supply real multi-day scenarios or covariance estimation.

Independent Python NormalDist inverses, Fraction ranks and Decimal scaling with 90 significant digits
precede native execution. New scoped bounds are absolute 2e-12 for the normal
inverse; VaR uses max(absolute 1e-8, relative 3e-14), and hedge units use
max(absolute 1e-8, relative 1e-14). Existing references/tolerances remain unchanged.
The original rounded 95% example 16,450 ±10 still admits the corrected inverse's
result 16,448.5362695147. These are engineering references, without an independently
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
150 year financial cap is invented for weekend arithmetic. Use a supplied checked
holiday Calendar for instrument economics.


## Typed risk-decimal decoding

Duration, Convexity and DV01 share one checked Decimal decoder. Healthy native
Decimal values and serialized representation stay unchanged. Finite numeric
inputs keep their established shortest-decimal interpretation; strings admit
only exactly representable Decimal mantissas and supported exponents, without
rounding nonzero digits or overflow. Decimal text is bounded to8,192bytes.
This differs from native TryFromf64, which explicitly converts binary model bits.

Raw JSON input must first use convex_core::contracts::json::parse to reject
numeric tokens erased by an upstream binary64 parser. Generic Deserialize sees
the supplied parsed value and cannot recover a discarded original token. Keep
raw admission and typed Decimal admission separate. The supplied library and
key-rate REST paths already share that bounded raw owner. V1 and other
remaining decode/valuation/risk routes keep their explicit owning work.


## Cancellation-safe supplied Holding factor risk

`aggregate_key_rate_exposure` retains exact supplied Decimal contributions through
dirty price, par and explicit FX, weighted factor numerators and weights, covered
market value, currency DV01 and profile totals. Large signed risks can cancel
without erasing a small residual or overflowing an intermediate Decimal. The
private fixed-scale integer helper uses the already locked `num-bigint` package;
existing holding, factor and total-work bounds remain. No provider, I/O or runtime
dependency enters the analytics crate.

Only reported Decimal values round, using nearest/even at the greatest scale up
to 28 whose coefficient fits 96 bits. An exact magnitude above Decimal's maximum
or a nonzero value that rounds to zero returns an error. An actual exact zero is
valid. Missing factors remain unavailable with explicit coverage. Presentation
weights never change actual common-base currency risk. The declared inputs must
still identify a common base currency and consistent price/risk convention.

This repairs supplied Holding factor accumulation. Broader stress, scenario,
interpolation, portfolio measures and production/latency qualification require
their own complete contracts and executed checks.
