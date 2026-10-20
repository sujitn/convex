# Supplied portfolio key-rate exposure

`aggregate_key_rate_exposure` is the checked native owner of supplied holding
aggregation. It does not price instruments or build curves. Supply each actual
holding, its full dirty valuation basis, explicit FX to one common base currency,
and independently calculated factor durations. Keep source revisions with the
caller’s immutable valuation inputs. A tenor identifies an exact finite positive
binary64 factor; nearby factors are distinct and no milliyear bucket is used.

For each requested factor, the result preserves original order, covered and total
holding counts, covered dirty market value, optional known weighted duration and
optional known currency DV01. Missing input stays unavailable. A supplied zero
sensitivity is an actual zero and counts as covered. A zero dirty valuation cannot normalize relative duration and is rejected when
relative factors are supplied. Use the existing absolute-sensitivity workflow for
zero-value instruments; no zero absolute risk is inferred. Empty books
and missing profiles never receive invented zero duration or contributions.

Currency DV01 is the sum of each covered holding’s dirty value times its supplied
factor duration divided by10,000. Dirty value uses actual par times(clean price+
supplied accrued interest)times explicit FX divided by100. Positive DV01 denotes
loss for an upward1bp move; signed factor sensitivities are retained. MarketValue
presentation uses dirty value; ParValue and EqualWeight change the presentation
ratio only. They never change actual currency exposure. Negative supplied accrual
is allowed when the full dirty value remains nonnegative. No missing accrual or
FX is forecast here. The existing positive-par Holding view does not replace the
separate signed mixed-instrument risk and hedge owner.

The original optional profile and partial-DV01 views delegate the checked owner.
The optional profile lists known weighted factors only; inspect requested tenors
and coverage before using it as a full-book profile. The partial-DV01 view returns
known supplied exposures only. Use the full result for diagnostics and per-factor
coverage. Incorrect, duplicate or oversized whole inputs fail without silently
dropping a holding. Arithmetic is checked for Decimal range and nonzero underflow.
Bounds are65,536holdings,4,096requested or per-holding factors,1,048,576holding/factor
work or supplied factor rows, and unique nonempty identities of at most8,192bytes.
Aggregation is deterministic across configuration parallel flags.

The REST endpoint `/api/v1/portfolio/key-rate-duration` consumes supplied same-base
positions and full dirty prices in `market_price`. Supply notional and price as
checked plain decimal strings; numeric cash, exponent notation and decimal
rounding are rejected by the existing exact-money owner. Do not send a clean quote as
that full-price input. Duration, contribution and bucket metrics are nullable;
unknown factors are retained as unavailable points, and zero-sum contribution
ratios are null. `known_partial_dv01` reports covered exposure; `partial_dv01` is
available only when every supplied holding declares that factor. Explicit
`coverage_complete` and `duration_available` report separate facts. The portfolio
name and identity are echoed; `weighting` is optional and defaults to MarketValue.
Unrecognized input fields and invalid complete sets are rejected. The shared
pure core JSON owner rejects erased nonzero model numbers before typed extraction
and retains the existing byte, node, depth, item, text and duplicate-field bounds. The demo cannot
infer key-rate sensitivities by distributing duration around maturity, and cannot
infer VaR by multiplying NAV by a constant. Those fabricated results are removed.

The basis is consistent with QuantLib1.40’s distinction between cash-flow PV/dirty
price and clean price after accrued subtraction, and its use of PV when converting
normalized duration to currency change. See the primary
[bond source](https://github.com/lballabio/QuantLib/blob/v1.40/ql/pricingengines/bond/bondfunctions.cpp)
and [cash-flow risk source](https://github.com/lballabio/QuantLib/blob/v1.40/ql/cashflows/cashflows.cpp).
This is a model-basis inference, not an external convention pack or independent
market/model certificate. New synthetic reference cases use exact standard-library
Fraction arithmetic and Decimal90 formatting, independently of candidate output.

Full key-rate stress percentage/P&L, scenario interpolation and all other original
native/model/host/claim/physical/causal requirements remain owning work unless
separately qualified. Preserve CTD alternatives, switches and actual cash/security
ownership. Only unavailable actual platform execution may defer; no production,
latency, library or market/model certification is inferred.
