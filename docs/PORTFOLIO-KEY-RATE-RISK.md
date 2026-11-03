# Portfolio key-rate exposure

`aggregate_key_rate_exposure` aggregates key-rate risk across the holdings of a
portfolio. It does not price instruments or build curves. You pass in each
holding, its full dirty valuation basis, FX to one common base currency, and
durations per factor calculated elsewhere. The REST endpoint
`/api/v1/portfolio/key-rate-duration` uses it.

Keep source revisions with your own immutable valuation inputs. A tenor
identifies an exact finite positive binary64 factor. Nearby factors are
distinct, and no milliyear bucket is used.

For each requested factor, the result keeps the original order, the covered and
total holding counts, the covered dirty market value, and, when known, the
weighted duration and the currency DV01. Missing input stays unavailable. A zero
sensitivity you supply is a real zero and counts as covered.

A zero dirty valuation cannot normalize relative duration, so it is rejected
when relative factors are supplied. Use the existing absolute-sensitivity
workflow for zero-value instruments. No zero absolute risk is inferred. Empty
books and missing profiles never get an invented zero duration or contribution.

Currency DV01 is the sum, over covered holdings, of dirty value times the
factor duration divided by 10,000. Dirty value is par times (clean price +
accrued interest) times FX divided by 100. Positive DV01 means a loss for an
upward 1bp move, and signed factor sensitivities are kept.

`MarketValue` weighting uses dirty value. `ParValue` and `EqualWeight` change
only the presentation ratio and never the currency exposure. Negative accrued
interest is allowed when the full dirty value is still nonnegative. Missing
accrued interest and FX are not forecast. The positive-par Holding view does
not replace the signed mixed-instrument risk and hedge code.

The optional profile and partial-DV01 views call the same function. The profile
lists only the factors with known weighted duration, so check the requested
tenors and coverage before using it as a full-book profile. The partial-DV01
view returns only the known exposures. Use the full result for diagnostics and
per-factor coverage.

Incorrect, duplicate or oversized inputs fail, and no holding is silently
dropped. Arithmetic is checked for Decimal range and for nonzero underflow.
Bounds:

- 65,536 holdings;
- 4,096 requested or per-holding factors;
- 1,048,576 holding/factor work items or factor rows;
- unique nonempty identities of at most 8,192 bytes.

Aggregation gives the same result regardless of the configuration's parallel
flags.

## REST endpoint

`/api/v1/portfolio/key-rate-duration` takes positions in one base currency, with
full dirty prices in `market_price`. Send notional and price as plain decimal
strings. Numeric cash, exponent notation and decimal rounding are rejected by
the exact-money parser. Do not send a clean quote as the full-price input.

- Duration, contribution and bucket metrics can be null. Unknown factors are
  kept as unavailable points, and zero-sum contribution ratios are null.
- `known_partial_dv01` reports the covered exposure. `partial_dv01` is
  available only when every holding declares that factor.
- `coverage_complete` and `duration_available` report separate facts.
- The portfolio name and identity are echoed. `weighting` is optional and
  defaults to MarketValue.
- Unrecognized input fields and invalid sets are rejected.

The shared core JSON parser rejects erased nonzero model numbers before typed
extraction. The byte, node, depth, item, text and duplicate-field bounds are
unchanged. The demo cannot infer key-rate sensitivities by distributing
duration around maturity, and cannot infer VaR by multiplying NAV by a
constant. Those invented results have been removed.

## Basis

The basis matches QuantLib 1.40's distinction between cash-flow PV (dirty
price) and clean price after subtracting accrued interest, and its use of PV
when converting normalized duration to a currency change. See the
[bond source](https://github.com/lballabio/QuantLib/blob/v1.40/ql/pricingengines/bond/bondfunctions.cpp)
and [cash-flow risk source](https://github.com/lballabio/QuantLib/blob/v1.40/ql/cashflows/cashflows.cpp).
This is our reading of that basis. It is not an external convention pack or an
independent market or model validation, and no production, latency, library or
market certification is implied. The reference cases use exact
standard-library Fraction arithmetic and Decimal90 formatting, independent of
the implementation's output.

Not covered: key-rate stress percentages and P&L, scenario interpolation, and
the broader model, host, claim and lifecycle requirements. CTD alternatives,
switches and actual cash and security ownership are not modeled here.
