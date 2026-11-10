# Book limits

A limit bounds one measure of one book. The server evaluates it against the same Live
results that analytics by book read. Each limit is reported as `within`, `warning`,
`unknown` or `breach`, with the value behind the status.

P14 builds this in four steps. P14a adds the measures, the limit type (`convex-ports`
`limits`) and the evaluation (`convex-engine` `limits::evaluate_limits`). P14b stores a
limit set per book and serves its status on request. P14c evaluates limit sets as Live
results change. P14d checks a proposed trade against them.

## A limit

```json
{"limit_id":"dv01","measure":{"kind":"dv01"},"lower":"-50000","upper":"50000","warning_fraction":"0.8"}
```

Each limit has these parts:
- **An id**, unique in the book's set. A set holds at most 64 limits.
- **A measure**, from the table below.
- **A member**, for a measure that has members: one instrument or group (`{"one":"ISSUER-1"}`),
  or `"each"`.
- **Bounds.** A limit needs a lower bound, an upper bound or both. Both are inclusive,
  are given in the measure's unit, and are exact decimal strings. Amounts are in the
  book's reporting currency, and there is no FX.
- **An optional warning fraction**, strictly between 0 and 1. It applies only to bounds
  that face away from zero: an upper bound above 0, or a lower bound below 0. The band
  runs from the fraction of the bound to the bound.
- **An optional temporary excess** (`excess`): a wider lower bound, a wider upper bound or
  both, an `expires_at` and a `reason` of 1 to 1,024 bytes. Each bound it names widens one
  the limit has, strictly and on its own side.

Validation refuses anything else with a fixed reason, and infers no default.

## Measures

| `kind` | Member | Value | Unit |
|---|---|---|---|
| `face` | `instrument` | the position's signed face; zero if the book does not hold it | face |
| `market_value` | | net dirty value, `(clean + accrued) × face / 100`, summed | book currency |
| `gross_market_value` | | the sum of the positions' absolute dirty values | book currency |
| `dv01` | | `dv01_mid × face / 100`, summed | book currency per bp |
| `cs01` | | `cs01_mid × face / 100`, summed | book currency per bp |
| `key_rate_dv01` | `tenor` (dated) | `value × key-rate duration / 10,000`, summed | book currency per bp |
| `modified_duration` | | `Σ value × duration / net value` | years |
| `group_market_value` | `by`, `group` | a group's net value | book currency |
| `group_share` | `by`, `group` | a group's net value over gross value | fraction |

Market values are admitted by the same rule as analytics by book
(`portfolio_analytics::admit_quote`). So `market_value`, `dv01` and `modified_duration`
equal what `POST /api/v1/portfolio/analytics` returns for the same book. Groups are by
issuer, country of risk or sector, taken from the priced bond's terms, or by rating, taken
from the position (the only rating the server holds). The analyzer's existing breakdowns
by supplied labels are unchanged.

A duration weighted by net value is ill-posed for a long/short book whose net value is
near zero. For such a book, DV01, CS01 and gross value are the measures to bound.

## Status

- **`breach`**: the value is above the upper bound or below the lower bound. A value
  exactly at a bound is within it.
- **`warning`**: the value is within every bound, but past the warning fraction of one.
  A value exactly at the edge of the band is not past it.
- **`unknown`**: an input is missing. The status carries its reason, and it never counts
  as within.
- **`within`**: otherwise.

The status is decided on exact state. A ratio (`modified_duration`, `group_share`) is
compared as its numerator against `bound × denominator`. Only the reported values are
rounded, half to even, and neither keeps trailing zeros:
- `value` is rounded at Decimal precision.
- `utilisation` is the value over each bound that faces away from zero, taking the
  larger. It is rounded at Decimal precision, then to eight places. It is for display.

**Status is a function of its inputs, not of its history.** The same book, set, results
and time give the same status, whatever the status before. The server's limit monitor
damps a value hovering at a bound in what it *records*, by holding a change to a less
severe status for a while; status and the pre-trade check are unaffected (see the server
README, "The clear hold").

**A temporary excess** applies strictly before its `expires_at`, which
`evaluate_limits` compares with the time it is given and reads from no clock. While it
applies, the limit is judged against the excess bounds, with its warning band and utilisation
on them, and a side the excess does not name keeps the limit's own bound. The evaluation
reports `excess`, with the limit's own bounds, the expiry and the reason, whatever the
status. From its expiry the limit is judged against its own bounds again.

**A limit on each member** judges the largest and the smallest member and keeps the
worse of the two. Severity decides first (`within` < `warning` < `unknown` < `breach`),
then utilisation, then the earlier member. `member` names the one kept. A limit on each
member of a book that holds none is within, with no value.

## On the server

A book's limit set is stored, changed by two subjects and evaluated on request through
`/api/v1/books/{book_id}/limits` (P14b); see [the server's guide](../convex-server/README.md#book-limits).
The status route builds each `LimitPosition` from the accepted runtime result for the
position, read through `runtime_bond_quote`, and from the priced bond version's
classifications.

## Why a limit is unknown

Book-level reasons are checked first, in this order:
1. `book_currency_required`: the book names no reporting currency.
2. `unsupported_quantity_unit`: a position is in units, not face.
3. `missing_result`: a position has no accepted runtime result.
4. `foreign_currency`: a result is in another currency than the book's.
5. `quote_refused`: the analyzer refuses a result (stale, mixed settlement dates, no price).

Then the measure's own:
- `measure_not_computed`: a result lacks the measure, named as the runtime names it
  (`cs01_mid`, `krd_5y_mid`).
- `missing_classification`: a position lacks the classification a group limit reads.
  It could belong to any group, so no group's value is known.
- `zero_net_value`: the value is weighted by net value, which is zero.
- `zero_gross_value`: the value is a share of gross value, which is zero.
- `arithmetic_bound`: exact arithmetic passed its fixed bound.

A face limit reads no market data, so a missing result never makes it unknown. A
position in units makes a face limit on it unknown. A limit on each face is unknown when
some position is in units, unless a member it can see is in breach. Each reason names at
most 16 instruments and gives the full count.

Input outside the contract is an error, not a status: an invalid set, more than 1,000
positions, or an instrument held twice.

## Cost

`limit_benchmarks` (`ci` profile, WSL2) measures a book at the bounds: 1,000 positions, a
third of them short, against 64 limits over every measure.

| Case | Time |
|---|---:|
| `limits/evaluate`, 64 limits (2026-11-03, before the G48 arithmetic change) | 5.26 ms |
| `limits/market_value`, one limit: admitting the market values (2026-11-03) | 0.88 ms |
| `limits/evaluate`, 64 limits (2026-11-09) | 3.40–3.49 ms |
| `limits/evaluate_excess`, 64 limits, each with an excess in force (2026-11-09) | 3.43–3.46 ms |

Most of the cost is exact `BoundedDecimal` arithmetic. Key-rate DV01 at ten tenors takes
about 3 ms, and DV01 with CS01 about 1 ms.
