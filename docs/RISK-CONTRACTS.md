# Scalar risk and weekend arithmetic

This document covers the scalar risk helpers in `convex-analytics` (duration,
convexity, DV01, portfolio aggregation, hedge units, VaR), the stress helpers in
`convex-portfolio`, and the weekend-only date operations. They work on numeric
exposures that you pass in. Instrument valuation, curve calibration, FX
conversion, scenario construction and execution live elsewhere. A scalar result
does not amount to a full book valuation, or to a model, market, latency or
production certification.

"Checked" below means the code validates its input and returns an error instead
of guessing.

## Risk values and aggregation

`Duration`, `Convexity` and `DV01` keep their Decimal representation and signed
values. Use the checked `TryFrom<f64>` for floating inputs. Nonfinite values,
Decimal range failure and conversion of a nonzero input into zero are errors.
Decimal construction is total; the locked Decimal representation (96-bit
coefficient and scale 0–28) always has a finite binary64 projection.

`aggregate_portfolio_risk` accepts at most 65,536 positions in one
caller-declared common currency and unit. IDs must be nonempty, unique and at
most 8,192 bytes. Checked Decimal sums preserve signed market value and DV01.
Weighted duration is `sum(value * duration) / sum(value)`, and only when the
absolute binary64 net-value guard `1e-10` is met. Otherwise it is unavailable; a
zero-value book can still have nonzero DV01. Empty exposure has zero value/DV01
and no defined duration ratio. No FX rate, price or instrument identity is
inferred.

Duration hedge units are `-(target_duration * target_value) /
(hedge_duration * hedge_price)`. DV01 hedge units are
`-target_dv01 / hedge_unit_dv01`. Signs, denominator guards and genuine zero
targets are preserved. Nonfinite inputs, overflow and nonzero underflow fail.

`KeyRateDurations::at_tenor` returns an exact finite positive unique factor,
without a tolerance or tenor rounding. Invalid or ambiguous collections have no
match. Holding aggregation calls the checked `aggregate_key_rate_exposure`, with
exact factors, dirty-value currency risk, per-factor coverage and nullable
unavailable results. Read the [portfolio contract](PORTFOLIO-KEY-RATE-RISK.md)
before using a partial profile as full-book risk. Scenario interpolation, stress
percentages and the signed mixed-product risk code have their own requirements.

The checked floating constructor does not cover every deserialization path.
Older derived-risk decoding and the raw JSON parser can turn a nonzero value
into zero when it falls below the binary64 range. The shared bounded parser now
rejects such erased numeric tokens before formal parsing, and the library
ingress and key-rate REST endpoints reuse it. Older derived-risk deserialization
and the legacy v1 entry points still need this validation; their wrong zeros are
not valid risk. Contract-money fields use a separate checked plain-decimal-string
parser.

## Scalar VaR

Both methods return a signed loss quantile, in the exposure's currency. A
negative quantile is a gain at that confidence. Confidence is finite and strictly
between zero and one; horizon is a positive number of days. Invalid inputs are
rejected even for a zero exposure.

Parametric VaR assumes zero-mean Gaussian P&L and a daily standard deviation:

`loss_quantile = inverse_normal(confidence) * abs(exposure) * volatility * sqrt(days)`.

`parametric_var` takes volatility as a daily fractional portfolio standard
deviation. `parametric_var_from_dv01` takes signed currency-per-bp DV01 and daily
yield volatility in bp. Volatility must be finite and nonnegative. The three
public Z-score constants are rounded approximations; interpolation between them
is no longer the calculation kernel.

The normal inverse uses the locked portable erfc/log kernels and a bounded solve
on `[0,40]` in log-survival space. A decreasing Mills series beyond 26 avoids tail
underflow. The solve has at most 80 iterations and the tail series at most 64
terms. It uses bracket-width convergence so that extreme-tail probabilities that
can be represented do not require an unattainable absolute probability residual.
The existing approximate CDF, its tests and the solver defaults are unchanged.

Historical VaR accepts 1–1,000,000 finite daily returns. It orders the losses
`-signed_portfolio_value * return` and selects rank
`ceil(exact_binary64_confidence * observation_count)`, starting at 1. Integer
decomposition of the probability avoids a rounded `1-confidence` and
out-of-range indices. At decimal-looking binary64 boundaries this exact-rank rule
can differ from a library that rounds the product before taking ceil. There is
no percentile interpolation and no absolute-value sign change. Losses are scaled
by `sqrt(days)` under an IID assumption; this does not supply real multi-day
scenarios or covariance estimation.

Reference values were computed independently before the Rust code ran: Python
NormalDist inverses, Fraction ranks and Decimal scaling with 90 significant
digits. The new bounds are absolute 2e-12 for the normal inverse; VaR uses
max(absolute 1e-8, relative 3e-14), and hedge units use max(absolute 1e-8,
relative 1e-14). Existing references and tolerances are unchanged. The rounded
95% example 16,450 ±10 still accepts the corrected inverse's result
16,448.5362695147. No convention or model pack has been independently approved.

The normal inverse definition follows [NIST](https://www.itl.nist.gov/div898/handbook/eda/section3/eda3661.htm).
The independent implementation is [Python NormalDist](https://docs.python.org/3/library/statistics.html#statistics.NormalDist.inv_cdf).
The empirical inverse convention is described by [NumPy's inverted CDF method](https://numpy.org/doc/stable/reference/generated/numpy.quantile.html).
The exact binary64 rank evaluation and the scalar model assumptions above are
this library's own choices, not a publisher's or regulator's.

## Weekend-only Date operations

All four methods return checked Results. Next/previous weekday keep a weekday
and otherwise select Monday/Friday. Business-day addition excludes the start and
counts the destination; zero keeps the date. Signed counts use `(earlier,later]`
and the sign of the direction you pass. Complete weeks take constant work, with at
most six daily steps. The representable calendar range is the bound; no 150-year
financial cap is applied to weekend arithmetic. Use a checked holiday Calendar
for instrument economics.

## Typed risk-decimal decoding

Duration, Convexity and DV01 share one checked Decimal decoder. Healthy Decimal
values and the serialized representation are unchanged. Finite numeric inputs
keep their established shortest-decimal interpretation. Strings accept only
exactly representable Decimal mantissas and supported exponents, without
rounding nonzero digits or overflow. Decimal text is bounded to 8,192 bytes.
This differs from `TryFrom<f64>`, which converts the binary value.

Raw JSON input must first go through `convex_core::contracts::json::parse` to
reject numeric tokens erased by an upstream binary64 parser. Generic Deserialize
sees the already parsed value and cannot recover a discarded token. Keep raw
JSON validation and typed Decimal validation separate. The library and key-rate
REST paths already share that bounded parser. The v1 and other remaining
decode, valuation and risk routes have not been moved over yet.

## Cancellation-safe Holding factor risk

`aggregate_key_rate_exposure` keeps exact Decimal contributions through dirty
price, par and FX, weighted factor numerators and weights, covered market value,
currency DV01 and profile totals. Large signed risks can cancel without erasing
a small residual or overflowing an intermediate Decimal. The private fixed-scale
integer helper uses the already locked `num-bigint` package; the holding, factor
and total-work bounds are unchanged. No provider, I/O or runtime dependency
enters the analytics crate.

Only reported Decimal values round, using nearest/even at the greatest scale up
to 28 whose coefficient fits 96 bits. An exact magnitude above Decimal's maximum
or a nonzero value that rounds to zero returns an error. An exact zero is valid.
Missing factors stay unavailable, with explicit coverage. Presentation weights
never change common-base currency risk. The inputs must still identify a common
base currency and a consistent price/risk convention.

This repairs Holding factor accumulation only. Broader stress, scenario,
interpolation and portfolio measures need their own contracts and tests.

## Holding stress estimates

`convex_portfolio::stress::run_stress_scenario` returns a checked, instantaneous
Taylor estimate for bond holdings. It is separate from the full-revaluation
mixed-book scenario and causal P&L APIs. You pass in the actual valuation date,
reporting currency, positions, compatible sensitivities and explicit FX. No
market data, clock, database or provider is accessed.

`StressPolicy` declares modified-yield, matched effective-curve or key-rate risk,
and first or second order. A modified-yield move applies to each holding's own
yield sensitivity; it is not an inferred common curve move. Effective risk
requires its matching effective duration and convexity. Second-order scalar rate
estimates require an explicit convexity, including an explicit zero. Spread
estimates require spread duration. There is no fallback to another risk family
or to a missing convexity. Rate and spread components are additive; cross terms
are not included. Use the full-revaluation model scenario when you need complete
revaluation, quote recalibration, optionality, CTD switch or cross-factor
interaction.

Currency contributions use signed par times `(clean + accrued) / 100` times the
direct FX conversion. Presentation weights do not enter this API. Cash and
base-currency liabilities are unchanged by these instantaneous bond shocks. A
foreign position or cash row requires one matching positive, direct, same-date
reporting FX pin. Base-currency rows require unit FX. Missing quotes,
classification shifts, risk or requested factors cannot become successful zero
amounts. A zero scalar shock can have an exact zero effect without a
sensitivity; all requested key-rate factors still require explicit coverage.

Discrete shifts use exact tenor identity. Linear interpolation has a separate
API and rejects extrapolation. Pivot shapes declare their 0/30-year endpoints
and a zero pivot; they reject out-of-range queries. The 2s10s constructors place
the shifts at 2 and 10 years, with a zero at 5 years. Key-rate estimates are
first order; second-order requests need a full cross-gamma model and return
`Unsupported` here. Invalid, missing, unsupported and arithmetic failures remain
distinct.

The private exact accumulator keeps six Decimal factors, the half coefficient and
price/basis-point scaling until the final report. Fixed scale 184 covers the 179
required places, and a 1,536-bit ceiling exceeds the accepted coefficient bound
of 1,230 bits. Key-rate rounding is unchanged: nearest/even at the greatest
representable scale up to 28, with errors on nonzero underflow or range excess.
The bounds on positions, factors, scenarios and total requested factor work are
checked before calculation. No dependency or solver default changes.

`StressResult` reports exact-string Decimal currency components and an optional
percentage. Percentage is undefined at zero initial NAV. Batch outcomes keep
unavailable scenarios and their original errors. Summaries count attempted,
available, unavailable and percentage-defined outcomes, and average only the
appropriate available values in the same book, date, currency and policy.

Each batch keeps one shared economic context: signed positions, clean/accrued/FX
amounts, risk, classifications, cash, liabilities and the complete policy.
Summaries reject mismatched contexts, including unavailable outcomes. Equal book
IDs or NAVs alone cannot establish identity. Results serialize the factor and FX
policy without serializing the book.

Holding exposure/profile Duration and monetary fields also serialize as exact
strings through the core serializer. This avoids the workspace's default
`serde-float` precision loss on these outputs. Tenors and the declared
profile-total presentation keep their binary64 types. Global Duration
serialization and the shared request/envelope contracts are unchanged.

## Checked Holding amounts and aggregates

Holding and cash amounts, portfolio NAV and per-share values return checked
Results. Exact Decimal products and signed sums are kept through the final
reported value; overflow and nonzero erasure fail. Negative positions stay
signed. Currency identity compares the codes against an explicit reporting
currency, independently of the numerical FX rate.

Weighted risk, yield and spread presentations return `Result<Option<f64>>`. A
missing required measure leaves the whole average unavailable; it does not
renormalize to a covered subset. Invalid numbers fail. Zero total weight has no
normalized average. Currency DV01/CS01 return checked optional Decimals and
serialize as exact strings; a nonempty incomplete book is not zero.

These binary64 sensitivity summaries are distinct from complete-key quote risk,
model revaluation and calibrated hedge factors. The quote-risk, lifecycle and
causal P&L routes keep their own contracts and CTD switches. The removed
include_incomplete setting is rejected by strict config input; it cannot
silently change coverage policy.

The engine quote-book summary requires every position and requested scalar
measure, unique consumed quotes, a common settlement date and the quote lineage.
Its timestamp is the latest consumed quote timestamp. Missing prices, accrued or
risk cannot drop a position or become a successful zero. Foreign quotes require
an FX-bound route and fail on this convenience route. Factor and classification
coverage are explicit; incomplete factor labels keep null durations. Weighted
summaries and contributions are separate from full model and market validation.

Contribution reports require an explicit duration or spread family. Their common
exact allocation code rejects missing requested sensitivities and keeps signed
numerators and denominators until the final output. Currency totals stay exact;
normalized weights and contribution percentages are optional at zero
denominators. Per-member scalar displays are binary64 presentations, separate
from the exact currency totals. The HTTP contribution request supplies
`duration_measure` (modified/effective) and `spread_measure` (z_spread/oas/
g_spread/i_spread/asw) for each requested normalized analysis. It cannot select
a member's other measure implicitly.
