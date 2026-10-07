# Dated futures and cash hedge reference inputs

CVX-022 supplies native reference and input-admission contracts for bond futures,
overnight/term rate futures and actual cash bond hedge candidates. It adds three
closed master kinds to the existing reference repository, without a new database,
port, crate, vendor dependency or runtime. CVX-023 owns full futures valuation,
carry, CTD selection and switch risk.

## Native use and import

Pure callers use `convex::futures_reference` (also `convex_core::futures` and
`convex_analytics::futures_reference`). Storage callers use
`convex_ports::futures_reference` and the existing `ReferenceRepository` port.
Supply all facts and timestamps; pure calculations read no clock, file or feed.

Run the archived reference example from the repository root:

```sh
cargo run --locked -p convex --example futures_reference
cargo run --locked -p convex --example futures_reference --no-default-features
cargo run --locked -p convex --example futures_reference --features schemars
cargo test --locked -p convex-analytics -p convex-ports -p convex-ext-file -p convex-ext-redb --test futures_reference
```

The example prints exact invoice arithmetic and historical blocking reasons.
Enabling `schemars` exercises consumer feature compatibility; it does not create
futures host/schema dispatch.

The strict [reference import](../tests/fixtures/futures/reference-import.json)
contains ten actual security identities, their explicit engineering registrations,
calendar/convention/entity resources, ZNZ6, its basket, SR3U6, a cash universe and
an exchange-symbol mapping. The existing offline importer can validate it:

```sh
cargo run --locked -p convex-server --example reference_master -- validate tests/fixtures/futures/reference-import.json
```

For embedding, decode with `load_reference_import` from `convex-ext-file` and
apply the unchanged atomic command/receipt API. Read retained facts with
`get_master_at`, `get_master_version` and `resolve_at`. New keys are
`MasterKey::Futures(InstrumentId)`, `Basket(ReferenceDataId)` and
`CashUniverse(ReferenceDataId)`; mappings use `ReferenceTarget::Futures`.
Existing bond construction and `for_valuation` remain bond operations.

## Terms and exact units

`FuturesContract` separates exchange, product root, actual contract symbol and
named month from last trading, expiry and settlement dates. `listed_from` is
the earliest admitted use of this reference revision; it must not be presented
as a captured first listing date without source evidence. Supply a currency per
price point, contiguous dated outright tick regimes, an exact settlement-calendar
revision, explicit time-zone text and cash posting precision. Session schedules
and time-zone interpretation belong to subsequent application/product workflows.

The strictly tagged settlement terms are disjoint:

| Family | Required facts |
|---|---|
| Bond | Face per contract, first notice/delivery, last delivery, exact delivery calendar and basket revision. Face divided by 100 equals the price-point multiplier. |
| Overnight rate | Index, half-open observation/value window, arithmetic or compounded averaging, ACT/360 or ACT/365F, calendar and final settlement following observation end. |
| Term rate | Index, fixing date, half-open underlying value window, day count, calendar and final settlement following the fixing rather than underlying maturity. |

SR3U6's September 2026 named month refers to observation from 2026-09-16 inclusive
to 2026-12-16 exclusive. Last trade is 2026-12-15 and final settlement 2026-12-16.
Its point multiplier is USD 2,500. The supplied 0.005 and 0.0025 outright ticks
give USD 12.50 and 6.25 cash increments. These follow
[CME's SOFR contract explanation](https://www.cmegroup.com/education/articles-and-reports/understanding-sofr-futures).

`tick_at` selects the supplied date's exact tick. `validate_order_units` checks
nonzero signed integer contracts and an exact on-tick outright price; it never
rounds orders. This checks units only. `rate_from_price` converts
`(100 - price_points) / 100` to a decimal annual rate, including negative rates.
`calibration_rate` also requires an explicit convexity policy: `NotAdmitted`
fails; `ZeroAssumption` needs evidence; `Supplied` needs a signed decimal
adjustment, model identity and evidence. No volatility or model is guessed.

`DeliveryBasket` retains the exact owner contract revision, named month,
currency, completeness assertion, actual security revisions and published
conversion factors. Reciprocal contract/basket pins must agree. Correcting one
requires an atomic compatible update to both; older pairs remain immutable for
replay. No latest-version substitution occurs.

`bond_delivery_invoice` computes one contract's converted clean cash from price,
point multiplier and pinned factor, rounds that component to posting precision
using half-away-from-zero, then adds already-rounded accrued cash supplied in
currency per contract. It neither calculates accrued interest nor selects CTD,
checks delivery business days, records settlement or values a futures position.
The ZN Chapter 19 example, price 100.796875 and factor 0.9633, gives USD 97,097.63
converted clean cash. The [CME rulebook](https://www.cmegroup.com/rulebook/CBOT/II/19.pdf)
supplies the multiplier, outright tick, delivery rules and rounding example.

## Cash universe and admission

`CashHedgeUniverse` retains all actual candidate identities and revisions.
Each supplies currency, settlement, exact lot face, separately dated quote,
optional bid/ask prices per 100 face, bid/ask size in face, funding, borrow and
cash costs per 100 face. Missing fields stay absent. Funding supplies the exact
security revision, currency, settlement start/end, decimal repo rate, day count
and haircut. Borrow supplies available face, last available date, nonnegative
decimal fee and day count. Every source has its own ID/version, quality,
source/availability/validity times and evidence.

`assess_cash_hedge` takes an exact candidate pin, requested face, holding horizon
and `Buy`, `Sell` or `Short`. Buys require ask and ask size; sales require bid and
bid size plus explicitly supplied owned face. Shorts require actual borrow size
and horizon. Every direction requires funding and costs. Missing/stale inputs,
off-lot quantities and insufficient size remain visible as stable reasons.

Both `assess_futures_reference` and `assess_cash_hedge` take independent `as_of`
and `known_at` timestamps. Later information can support an explicit restatement.
`Synthetic`, `Indicative` and `Historical` quality block admission. Source time
after economic time, availability after the information cutoff, expired validity,
incomplete baskets or missing cash prerequisites also block it. Issues sort and
deduplicate by original resource/reason. Invalid structures return errors.
Bond futures require their exact basket; rates reject all basket/CTD inputs.

The date-level trading gate conservatively closes at the start of `last_trade`.
`tick_at` permits that date for arithmetic. A later session-aware application
must resolve actual exchange calendars and intraday cutoffs before final-day
trading. `eligible: true` is only a necessary data prerequisite: actual futures
quotes, positions/risk limits, entitlements, certified valuation and order
authorization remain separate gates.

The approximate advisor now serializes `execution_status: "indicative_only"` on
all `HedgeProposal` and `ComparisonReport` outputs. Older JSON without the field
defaults to that status; `"executable"` is not an accepted enum value. Rust struct
literals need the field, normally `Default::default()`. Advisor formulas remain
unchanged and confer no execution eligibility.

## Exchange provenance and limits

The [reference facts](../tests/fixtures/futures/cme-2026-09-24.json) retain CME's
2026-09-24 Treasury conversion-factor workbook identity and exact cell locations.
The complete December 2026 ZN column has ten issues (I161:I170; count I178), with
actual CUSIPs, factors, coupon, issue and maturity dates. The
[CME publication page](https://www.cmegroup.com/trading/interest-rates/treasury-conversion-factors.html)
linked the workbook captured on 2026-10-07, SHA-256
`97308fafb418b43846cdd8e24be173bab08a49d46125f5bf086bc7dab276455d`.
Original bytes and extraction receipts remain in private task evidence.

Critical dates are explicitly rule-derived rather than a captured exchange
calendar feed. SIFMA/calendar dispatch, first-stub/EOM and remaining bond
registration settings are explicit engineering assumptions, not Treasury
cashflow certification. All fixture sources are `Historical`; declared validity
is a fixture interval rather than a provider freshness SLA. The cash universe
contains no invented prices, sizes, funding, borrow or costs. Modified source
quality/times and added cash facts in tests are engineering controls, not live
market observations or new exchange publications.

## Bounds, storage and next work

Validation bounds named years to 1900–2200, intervals to 100 years, rate windows
to 366 days, term fixing offsets to 31 days, tick regimes to 32, baskets/cash
universes to 128 unique securities, text to 512 bytes and posting precision to
8 places. Multipliers and order quantities are at most 1 billion; generic
face/size/cost amounts at most 10^15. Cash/bond prices are in (0,1000]; rate
conversion uses [0,200], while order checking requires a positive price.
Factors are in (0,10], repo rates [-1,1], haircuts [0,1), borrow fees [0,1].
These ceilings certify no capacity, model validity or latency.

Memory and redb reuse compare-and-set, atomic receipts and bitemporal history.
Imports validate exact security, contract, basket and calendar pins before
committing any prefix. No table, schema version or dependency is added. Earlier
executables cannot decode new closed master kinds/commands: upgrade reference
readers before introducing them, retain a closed database backup and use the
existing isolated restore procedure for rollback. See the
[reference master contract](../crates/convex-ports/REFERENCE_MASTER.md).

Continue with CVX-023 for bond-futures carry/CTD/all-deliverable repricing and
rate-futures averaging/fixings/valuation/risk. Margin, roll/delivery lifecycle,
executable cross-product hedging, host dispatch, provider certification and
production/latency gates remain their owning work. All mandatory library records
remain Planned; CVX-022 accepts this scoped reference and replay engineering.
