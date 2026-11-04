# Futures and cash hedge valuation

`convex::futures`, in `convex-analytics`, values bond and rate futures and prices
the cash hedge. It takes the versioned [futures reference terms](FUTURES-REFERENCE.md),
bond contracts, curves, fixing histories, calendars and financing as inputs, and
validates each of them. No provider, database, message queue, network, clock or
global registry enters the calculation. Applications resolve the inputs through
their ports and keep the returned input report alongside the result.

```sh
cargo run --locked -p convex --example futures_valuation
cargo test --locked -p convex-analytics --test futures_valuation
```

The example uses all ten archived December ZN conversion factors and test bond
terms and carry, then prices a partly fixed SOFR window. Synthetic cash quotes,
schedules, rates and calendars are labelled as such. Every result is
`IndicativeOnly`, even when all source prerequisites pass. It gives no authority to
place orders or post settlement.

## Functions and units

| Operation | Inputs | Result and risk basis |
| --- | --- | --- |
| `price_bond_future` | Actual contract, complete basket, every full security, observed clean cash prices, security repo/haircut, own-cash and coupon policy, actual purchase and delivery dates | All carry/accrued/CF/invoice/basis alternatives, explicit CTD ranking, independent minimum forward price |
| `bond_futures_risk` | Same complete bundle, integer position, exact axis and central bump; discount curve for zero risk | Reselected and frozen fair-deliverable sensitivity, selected/fair CTD switches, retained residual cash basis |
| `rate_futures_requirements` | Actual overnight or term window, exact calendar and supplied publication callback | All original dated fixing requirements before any projection |
| `price_rate_future` | Clearing precision/revision rules, actual history and manifest, optional exact index projection and explicit convexity source | Known/remaining weights, original revisions and forward queries, unrounded model points, expected final settlement if fully fixed |
| `rate_futures_risk` | Same inputs, signed integer contracts, positive decimal forward bump | Currency per +1bp in remaining simple forwards, with known history and quote-level convexity frozen |
| `remaining_rate_from_price` | Original valuation and exact contract, full futures points | Residual aggregate simple accumulation rate; None for a matching fully fixed quote |
| `price_cash_hedge` / `cash_hedge_risk` | Actual full security and cash universe, proposed side/face/owned inventory/horizon, separate discount, explicit haircut/borrow-fee policies | Full model value, actual side quote, separate costs and proposed cash, absent estimates for missing economics; signed full-cash discount-zero risk |
| `futures_settlement_effect` | Actual prior/new marks, normal clearing rules, exact settlement calendar and integer position | Proposed variation from rounded per-contract monetary levels; no ledger mutation |
| `futures_quote_risk` / `round_futures_price` | Actual contract/date, points/count and explicit rounding | Exact dated quote/tick currency, rounded price and residual points |
| `round_futures_hedge` / `plan_futures_roll` | Supplied signed candidate risk; for roll, actual front/back contracts, quotes, comparable canonical factor and explicit costs | Integer quantity and residual factor risk; named roll legs and costs, without fills or automatic contract choice |

Quotes use points or clean per100 original face. Rates and bumps are decimal
annualized rates (`0.0001` = 1bp). CF is dimensionless. Currency scaling uses
actual contract multiplier and signed integer quantity exactly once. Ordinary
quote delta/tick currency is distinct from projection/discount/repo risk. Security
basis risk is currency per clean price point, with a separately named unit.
All monetary, observation-rate and exact reference values serialize as Decimal
strings. Native curve calculations and queries use checked finite `f64`; those
outputs have declared numerical tolerances rather than cent-exact guarantees.

## Physical bond futures

The fixed-date deterministic model accepts fixed-coupon bullet securities that
survive delivery, with par principal. Stepped fixed coupons flow through the full
bond projection. Optional, ex-coupon, floating, amortizing, non-par and indexed
deliverables would need additional delivery models and are refused. No
issue or maturity date, coupon schedule or conversion factor is inferred from a
tenor, yield or country. Every member of the basket needs one economic input;
duplicates, missing members and wrong revisions fail.

For per100 amounts, let `P` be observed clean, `As` purchase accrued, `Ad` delivery
accrued, `D=P+As`, `h` haircut, `r` security repo, `u` own-cash rate, `C` received
coupons carried to delivery under their explicit policy and `CF` the published
factor. With each source's own day count:

```
repo_cost = D * (1-h) * r * repo_year_fraction
own_cash_cost = D * h * u * own_cash_year_fraction
forward_clean = D + repo_cost + own_cash_cost - C - Ad
forward_points = forward_clean / CF
gross_basis = P - market_points * CF
net_basis = forward_clean - market_points * CF
implied_repo = (market_points*CF + Ad + C - D - own_cash_cost)
               / (D*(1-h)*repo_year_fraction)
```

Own-cash rates are mandatory for nonzero haircut and refused when unused with
zero haircut. Coupons received after purchase, through delivery inclusive, are
either held as cash under an explicit assumption or reinvested under a separate
dated simple rate. All funding inputs cover the entire requested horizon.
Negative repo and own-cash rates are supported within the declared bounds.

Minimum net basis, maximum implied repo and minimum conversion-adjusted forward
are independent selection criteria. Different financing, cash and CF can make them
disagree. The minimum forward determines the model's fair futures points; the
selected cheapest-to-deliver (CTD) is reported separately. All alternatives and the
runner-up gap are kept, and ties are broken by identity and version. Invoice cash
uses the contracted face, the published CF, a separately rounded clean invoice and
the rounded accrued currency, through the existing invoice function. The
unrounded basis profit diagnostic is labelled separately.

Parallel discount-zero risk reprices every remaining security cashflow at the
curve recipe's time basis, holds the observed-minus-model clean basis fixed, and
reapplies funding and coupon carry. Repo risk changes the security repos you pass
in, while holding cash, haircut and reinvestment inputs fixed. Basis risk changes
one member's observed clean basis. Every scenario keeps all alternatives, both the
selection and fair-forward switches, and the frozen sensitivity of the base fair
member. Neither raw scenario measure recalibrates the input quotes.

The model always reports delivery timing, wildcard and end-month optionality, and
daily-margin financing as unvalued. The fixed delivery dates you pass in are
scenario assumptions. Futures level times multiplier is a marking convention, not
an upfront cash purchase or a discounted futures PV.

## Overnight and term rate futures

Overnight observation grids reuse the shared schedule with the business calendars
you pass in. Rate quotation intervals, clipped accrual weights, weekend and holiday
tails, and arithmetic versus compounding are kept distinct. Term futures use one
fixing with a separate underlying value interval; their projection risk becomes
zero when that fixing is known, even if the value period has not started. Fixing
and value business dates are checked explicitly.

Due history is resolved for the whole contract before a curve is queried. Both
publication and receipt must meet the information cutoff. Before the expected
publication, the caller either allows forecasts or requires publication. Missing
due observations fail; a curve cannot invent a historical fixing. Published
observations keep their rates, source, revision and timestamps, and the manifest
source must match. Forecasts keep the distinction between `FutureProjection` and
`AwaitingPublication`, and the curve queries consumed.

`FuturesFixings` explicitly selects `FirstPublished` or `LatestEligible`, matching
the clearing rules. First publication requires revision 1; a correction cannot
replace it. All revisions are validated before selection, including history that
is not consumed. This avoids treating corrected SOFR as first-published CME
settlement. The choice is specific to the contract and never inferred from the
index name.

Projection requires an index role in the same currency. A discount curve is not
substituted for projection. `FuturesCurve` binds a numeric `ReferenceStamp` version
to its decimal string in `CurveKey`; callers with other curve version labels must
provide an explicit mapping first. There is no lookup of the latest version. A fully
fixed future needs no projection or convexity model.

While projections remain, convexity is one of: absent (not accepted), explicitly
zero, or supplied with a model revision. The reference convention is
`forward = futures_rate + adjustment`, so the model futures rate is the window's
curve-equivalent rate minus the adjustment. This is a whole-quote adjustment that
you supply and that is held fixed under risk; no partial-period proration is
inferred. Once fully fixed, the adjustment is zero. No stochastic convexity model
is fitted here.

Model price is `100 - 100*quoted_decimal_rate`. Final rate precision and the tie
rule are clearing facts, independent of order ticks. Expected final price is
available only once all contracted observations are published. It is not a
clearing event. Simple-forward risk bumps only the unfixed observations and
reprices the weighted aggregation; no artificial 90-day duration replaces the
remaining window. Fully fixed remaining exposure is zero. The inverse returns the
equivalent simple accumulation rate over the unfixed weights, not a constant daily
forward or a calibrated curve. It validates the cached aggregates; rebuild from the
original inputs when accepting untrusted persisted results.

## Cash, settlement and roll boundaries

Cash model PV uses the full cash of plain fixed, stepped and amortizing contracts
and an explicit collateral discount curve. A buy uses the ask; an owned sale and a
short use the bid. Model value, observed side value, residual basis and costs stay
distinct. Proposed cash is signed: negative for a purchase and positive for a sale,
less explicit costs. A missing side price or cost means no proposed cash. Missing
funding or loan inputs, or an insufficient horizon, mean the corresponding
estimates are absent and the blocks stay recorded. Funding estimates hold the
initial dirty balance constant; they do not model coupon cash, principal paydown,
realized funding or P&L, or lifecycle. Stock-loan fees need an explicit base of
either face or observed dirty value. Short permission and sizes are prerequisite
checks.

Normal futures variation rounds each per-contract monetary price level first,
subtracts those values, then multiplies by the signed count. This preserves lot
partition and multi-day telescoping. Same-day trade-to-settlement is valid.
Duplicate daily transitions, out-of-order events, reopening after final settlement,
non-business settlement, and incorrect final dates or precision fail. The
application still owns event deduplication, positions and fills, and the posting of
cash.

Tick and integer rounding are explicit: nearest away from zero, nearest with ties
up, down, up or toward zero. Continuous hedge counts and residual risk survive
rounding, including meaningful zero counts. Missing or zero candidate risk fails.
Roll planning takes front and back contracts that you name. They must have the same
product, exchange and currency and compatible payoff, index, averaging and basis,
with the back contract in a later named month. It also needs a common canonical
rate-risk factor, current on-tick quotes and dated costs. It keeps the front
contract's factor exposure with rounded back contracts and reports the residual.
The quoted calendar spread is not cash or P&L. No root symbol, synthetic next
contract, basis matching, fill or execution authority is invented.

## Evidence, bounds and remaining gates

The [valuation fixtures](../tests/fixtures/futures/valuation.json) use independent
Python Decimal(60) cash schedules, carry, full-cash curve shifts and weighted rate
formulas. The ten CME factor and security IDs are pinned from the futures
reference data; the cash schedules, quotes, funding, rate histories and calendars
are test data. Nine bond and eighteen rate central scenarios use half, base and
double bump sizes. Expectations and tolerances were not derived from the code's
output. Fixing inputs that are exactly representable use 24 decimal places, and the
independent expected outputs keep their full precision.

The [exchange fixtures](../tests/fixtures/futures/exchange-settlement.json) hold
CME's hypothetical 2017 examples, from before SOFR launched in April 2018; they are
not administrator history. The independently computed final prices are `98.9495`
for the compounded 91-day example and `98.959` for the 31-day arithmetic example.
The published ZF/ZT margin examples preserve exact per-contract rounding. The PDFs
could not be downloaded directly, so the source text was read in a browser and its
page and table locations are kept in private notes; the original PDF bytes and
checksum are not recorded.
Primary methods: [CME SOFR examples](https://www.cmegroup.com/education/files/sofr-futures-settlement-calculation-methodologies.pdf),
[Treasury margin rounding](https://www.cmegroup.com/trading/interest-rates/files/treasury-futures-price-rounding-conventions-2020.pdf),
[normal money calculations](https://www.cmegroup.com/clearing/files/CME-Money-Calculations-Futures-and-Options.pdf),
[SR3 rules](https://www.cmegroup.com/rulebook/CME/IV/400/460.pdf) and
[Treasury cash/carry](https://www.cmegroup.com/education/files/understanding-treasury-futures.pdf).

The contract bounds are: 128 members, 32 tick regimes, 366-day rate windows,
bounded text and rates, and checked Decimal arithmetic. History has at most 65,536
observations, and the shared observation schedule has at most 4,096 entries.
Consumed source stamps are bounded to 1,024, and conflicting facts for one revision
fail. Signed counts are bounded to 1 billion. Central rate bumps are in
`[1e-8,0.05]` and clean-basis bumps in `[1e-8,10]`. Curves keep their own
construction and query bounds. These are workload and error limits, not latency
figures.

CI runs the dedicated tests and Clippy, and the default, minimal and schema
facade builds. Not yet covered: shared signed factor integration, key-rate and
market-quote calibration, full market, convention, convexity and optional-delivery
test packs, lifecycle and P&L, bindings for other hosts, hedge optimization, an
independent quantitative review, and measured multiserver performance. No
library or production certification is implied.

[Delivery futures basket risk](DELIVERY-FUTURES-HEDGE-RISK.md) covers all-member
graph and scenario risk and delivery-futures hedge columns, including exact ties
and CTD switches. Run `cargo run --locked -p convex --example delivery_hedge`. It
keeps the fixed-date model and does not value the delivery options.
