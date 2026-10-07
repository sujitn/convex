# Supplied futures and cash hedge valuation

CVX-023 adds `convex::futures` in the existing pure analytics library. It consumes
the complete versioned [futures reference terms](FUTURES-REFERENCE.md), checked
bond contracts, explicit curves, fixing histories, calendars and financing.
No provider, database, message queue, network, clock or global registry enters
the calculation. Applications resolve immutable inputs through their existing
ports and retain the returned input report alongside their result.

```sh
cargo run --locked -p convex --example futures_valuation
cargo test --locked -p convex-analytics --test futures_valuation
```

The example supplies all ten archived December ZN conversion factors and full
engineering bond terms/carry, then prices a partly fixed SOFR window. Synthetic
cash quotes, schedules, rates and calendars remain labelled. Every result is
`IndicativeOnly`, including when all source prerequisites pass. Source eligibility
does not grant order, settlement posting or production authority.

## Native APIs and units

| Operation | Complete supplied inputs | Result and risk basis |
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

The fixed-date deterministic model admits checked fixed-coupon nominal bullet
securities surviving delivery, with par principal. Actual stepped fixed coupons
flow through the full bond projection. Optional, ex-coupon, floating, amortizing,
non-par and indexed deliverables need complete additional delivery models and
are refused. No issue/maturity, coupon schedule or conversion factor is inferred
from a tenor, yield or country. Every exact member of the supplied basket needs
one economic input; duplicates, missing members and wrong revisions fail.

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
are independent selection criteria. Different financing/cash/CF can make them
disagree. The minimum forward determines this model's fair futures points;
the explicitly selected CTD remains separately reported. All alternatives and
the runner-up gap are retained, with stable identity/version tie breaking.
Invoice cash uses contracted face, published CF, separately rounded clean invoice
and actual rounded accrued currency, through the existing invoice function.
The unrounded basis profit diagnostic is labelled separately.

Parallel discount-zero risk reprices every actual remaining security cashflow
at the curve recipe's time basis, holds observed-minus-model clean basis fixed,
and reapplies funding/coupon carry. Repo risk changes the supplied security repos
while freezing cash, haircut and reinvestment inputs. Basis risk changes one
exact member's observed clean basis. Every scenario retains all alternatives,
both selection and fair-forward switches, and the base fair member's frozen
sensitivity. Neither raw scenario measure claims input-quote recalibration.

This model always reports unvalued delivery timing, wildcard and end-month
optionality and unvalued daily-margin financing. Supplied fixed delivery dates
are explicit scenario assumptions. Futures level-times-multiplier is a marking
convention, not an upfront cash purchase or discounted futures PV.

## Overnight and term rate futures

Overnight observation grids reuse the checked shared schedule with exact supplied
business calendars. Full rate quotation intervals, clipped accrual weights,
weekend/holiday tails and arithmetic versus compounding remain distinct. Term
futures consume one actual fixing with its separate underlying value interval;
their projection risk becomes zero when that fixing is known, even if the value
period has not started. Fixing/value business dates are checked explicitly.

Actual due history is resolved for the whole contract before querying a curve.
Both publication and receipt must meet the information cutoff. Before expected
publication, the caller explicitly allows forecasts or requires publication.
Missing due observations fail; a curve cannot fabricate a historical fixing.
Published observations retain exact rates, source, revision and timestamps;
the manifest source must match. Forecasts retain `FutureProjection` versus
`AwaitingPublication` and exact consumed curve query lineage.

`FuturesFixings` explicitly selects `FirstPublished` or `LatestEligible`, matching
the clearing rules. First publication requires original revision1; a correction
cannot replace it. All supplied revisions are validated before selection, including
unconsumed history. This avoids treating corrected SOFR as first-published CME
settlement. The choice is contract-specific, never inferred from the index name.

Projection requires an exact same-currency index role. A discount curve is not
substituted for projection. `FuturesCurve` binds a numeric `ReferenceStamp` version
to its decimal string in `CurveKey`; callers with arbitrary curve version labels
must provide an explicit reference mapping before this route. There is no latest
version lookup. A fully fixed future consumes no projection or convexity model.

While projections remain, convexity is explicitly absent/not admitted, explicitly
zero, or supplied with an exact model revision. The reference convention is
`forward = futures_rate + adjustment`, so model futures rate is the window's
curve-equivalent rate minus adjustment. This is a supplied whole-quote adjustment,
held fixed under risk; no automatic partial-period proration is inferred.
Once fully fixed, adjustment is zero. No stochastic convexity model is fitted here.

Model price is `100 - 100*quoted_decimal_rate`. Final rate precision and tie rule
are explicit clearing facts, independent of order ticks. Expected final price is
available only once all contracted observations are published. It is not an
actual clearing event. Simple-forward risk bumps only unfixed observations and
reprices the actual weighted aggregation; no artificial 90-day duration replaces
the remaining window. Fully fixed remaining exposure is zero. The inverse returns
the equivalent simple accumulation rate over unfixed weights, not a constant
daily forward or calibrated curve. It validates the original cached aggregates;
rebuild from original inputs when accepting untrusted persisted results.

## Cash, settlement and roll boundaries

Cash model PV uses full checked plain fixed/stepped/amortizing contract cash and
an explicit collateral discount curve. Buy uses ask; owned sale and short use bid.
Model value, observed side value, residual basis and costs stay distinct.
Signed proposed cash is negative for a purchase and positive for a sale, less
explicit costs. Missing side price/cost means absent proposed cash; missing
funding/loan or insufficient horizon means absent corresponding estimates and
retained admission blocks. Funding estimates explicitly hold the initial dirty
balance constant. They do not model coupon cash, principal paydown, realized
funding/P&L or lifecycle. Stock-loan fees require an explicit face-versus-observed
dirty-value base. Short permission and sizes remain prerequisite checks.

Normal futures variation rounds each per-contract monetary price level first,
subtracts those values, then multiplies by the signed count. This preserves lot
partition and multi-day telescoping. Same-day trade-to-settlement is valid;
duplicate daily transitions, out-of-order events, reopening after final settlement,
non-business settlement and incorrect final dates/precision fail. The application
still owns event deduplication, positions/fills and authoritative cash posting.

Tick and integer rounding are explicit: nearest away from zero, nearest with ties
up, down, up or toward zero. Continuous hedge counts and residual risk survive
rounding, including meaningful zero counts. Missing/zero candidate risk fails.
Roll planning consumes named supplied front/back contracts of the same product,
exchange, currency, compatible payoff/index/averaging/basis, a later named month,
exact common canonical rate-risk factor, current on-tick quotes and dated costs.
It preserves supplied front factor exposure with rounded back contracts and
reports the residual. The quoted calendar spread is not cash/P&L. No root symbol,
synthetic next contract, basis matching, fill or execution authority is invented.

## Evidence, bounds and remaining gates

New [valuation fixtures](../tests/fixtures/futures/valuation.json) use independent
Python Decimal(60) cash schedules, carry, full-cash curve shifts and actual weighted
rate formulas. Ten actual CME factor/security IDs remain pinned from CVX-022;
all new cash schedules/quotes/funding/rate histories/calendars are engineering
controls. Nine bond and eighteen rate central scenarios use half/base/double bump
sizes. Original expectations and tolerances were not derived from candidate
outputs. Representable fixing inputs explicitly use 24 decimal places; independent
expected outputs retain their high precision.

[Exchange fixtures](../tests/fixtures/futures/exchange-settlement.json) retain
CME's hypothetical 2017 examples, before SOFR's April2018 launch: they are never
administrator history. The independently computed final prices are `98.9495` for
the compounded 91-day example and `98.959` for the 31-day arithmetic example.
Published ZF/ZT margin examples preserve exact per-contract rounding. Direct PDF
downloads failed; actual browser-reader text and primary page/table locations are
retained in private evidence, with no claim of original PDF bytes or checksum.
Primary methods: [CME SOFR examples](https://www.cmegroup.com/education/files/sofr-futures-settlement-calculation-methodologies.pdf),
[Treasury margin rounding](https://www.cmegroup.com/trading/interest-rates/files/treasury-futures-price-rounding-conventions-2020.pdf),
[normal money calculations](https://www.cmegroup.com/clearing/files/CME-Money-Calculations-Futures-and-Options.pdf),
[SR3 rules](https://www.cmegroup.com/rulebook/CME/IV/400/460.pdf) and
[Treasury cash/carry](https://www.cmegroup.com/education/files/understanding-treasury-futures.pdf).

Existing contract bounds apply:128 members,32 tick regimes,366-day rate windows,
bounded text/rates and checked Decimal arithmetic. History has at most65,536
observations; the shared observation schedule has at most4,096 entries. Consumed
source stamps are bounded to1,024 and conflicting facts for one revision fail.
Signed counts are bounded to1billion. Central rate bumps are in `[1e-8,0.05]`;
clean-basis bumps in `[1e-8,10]`. Curves retain their own construction/query bounds.
These are workload/error limits, not a latency certificate.

All earlier numerical artifacts/defaults, C exports, Cargo manifests/lock and
mandatory library records remain protected. CI requires dedicated tests/Clippy
and default/minimal/schema facade consumers. Shared signed factor integration,
key-rate/market-quote calibration, full market/convention/convexity/optional-delivery
packs, authoritative lifecycle/P&L, host dispatch, hedge optimization/admission,
independent human quantitative approval and measured multiserver/colocation
performance remain their owning tasks. No complete library or production
certificate is inferred from this scoped engineering implementation.
