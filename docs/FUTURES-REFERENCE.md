# Futures and cash hedge reference data

This covers the reference-data contracts and input validation for bond futures,
overnight and term rate futures, and cash bond hedge candidates. It adds three
master kinds to the existing reference repository, with no new database, port,
crate, vendor dependency or runtime. Full futures valuation, carry, CTD selection
and switch risk are described in [futures valuation](FUTURES-VALUATION.md).

## Use and import

Pure callers use `convex::futures_reference` (also `convex_core::futures` and
`convex_analytics::futures_reference`). Storage callers use
`convex_ports::futures_reference` and the existing `ReferenceRepository` port.
You pass in all facts and timestamps; the pure calculations read no clock, file
or feed.

Run the example from the repository root:

```sh
cargo run --locked -p convex --example futures_reference
cargo run --locked -p convex --example futures_reference --no-default-features
cargo run --locked -p convex --example futures_reference --features schemars
cargo test --locked -p convex-analytics -p convex-ports -p convex-ext-file -p convex-ext-redb --test futures_reference
```

The example prints the exact invoice arithmetic and the reasons that block
historical data. Building with `schemars` checks feature compatibility only; it
does not add futures host or schema dispatch.

The strict [reference import](../tests/fixtures/futures/reference-import.json)
contains:

- ten security identities and their registrations;
- calendar, convention and entity resources;
- the contract ZNZ6 and its basket;
- SR3U6;
- a cash universe;
- an exchange-symbol mapping.

The existing offline importer can validate it:

```sh
cargo run --locked -p convex-server --example reference_master -- validate tests/fixtures/futures/reference-import.json
```

To embed it, decode with `load_reference_import` from `convex-ext-file` and apply
the atomic command and receipt API. Read the stored facts with `get_master_at`,
`get_master_version` and `resolve_at`.

The new keys are `MasterKey::Futures(InstrumentId)`, `Basket(ReferenceDataId)`
and `CashUniverse(ReferenceDataId)`. Mappings use `ReferenceTarget::Futures`.
Existing bond construction and `for_valuation` remain bond operations.

## Terms and units

`FuturesContract` keeps the exchange, product root, contract symbol and named
month separate from the last trading, expiry and settlement dates.
`listed_from` is the earliest date this reference revision may be used. Do not
present it as the first listing date unless you have a source for that.

A contract needs:

- a currency per price point;
- contiguous dated outright tick regimes;
- an exact settlement-calendar revision;
- explicit time-zone text;
- cash posting precision.

Session schedules and time-zone interpretation are left to later application
workflows.

The strictly tagged settlement terms are disjoint:

| Family | Required facts |
|---|---|
| Bond | Face per contract, first notice/delivery, last delivery, exact delivery calendar and basket revision. Face divided by 100 equals the price-point multiplier. |
| Overnight rate | Index, half-open observation/value window, arithmetic or compounded averaging, ACT/360 or ACT/365F, calendar and final settlement following observation end. |
| Term rate | Index, fixing date, half-open underlying value window, day count, calendar and final settlement following the fixing rather than underlying maturity. |

SR3U6's September 2026 named month refers to observation from 2026-09-16
inclusive to 2026-12-16 exclusive. Last trade is 2026-12-15 and final settlement
is 2026-12-16. Its point multiplier is USD 2,500. The 0.005 and 0.0025 outright
ticks give USD 12.50 and 6.25 cash increments. These follow
[CME's SOFR contract explanation](https://www.cmegroup.com/education/articles-and-reports/understanding-sofr-futures).

- `tick_at` selects the tick for the date you give.
- `validate_order_units` checks for a nonzero signed integer number of contracts
  and an exact on-tick outright price. It never rounds orders, and it checks
  units only.
- `rate_from_price` converts `(100 - price_points) / 100` to a decimal annual
  rate, including negative rates.
- `calibration_rate` also needs an explicit convexity policy. `NotAdmitted`
  fails. `ZeroAssumption` needs evidence. `Supplied` needs a signed decimal
  adjustment, a model identity and evidence. No volatility or model is guessed.

`DeliveryBasket` keeps the contract revision, named month, currency, a
completeness assertion, the security revisions and the published conversion
factors. The contract and basket pins must refer to each other. To correct one,
update both together in one atomic step, and older pairs stay immutable for
replay. The latest version is never substituted.

`bond_delivery_invoice` computes one contract's converted clean cash from the
price, the point multiplier and the pinned factor. It rounds that component to
posting precision using half-away-from-zero, then adds the already-rounded
accrued cash you supply in currency per contract. It does not:

- calculate accrued interest;
- select the CTD;
- check delivery business days;
- record settlement;
- value a futures position.

The ZN Chapter 19 example, with price 100.796875 and factor 0.9633, gives
USD 97,097.63 of converted clean cash. The
[CME rulebook](https://www.cmegroup.com/rulebook/CBOT/II/19.pdf) gives the
multiplier, outright tick, delivery rules and rounding example.

## Cash universe and eligibility checks

`CashHedgeUniverse` keeps every candidate identity and revision. Each candidate
supplies:

- currency, settlement and exact lot face;
- a separately dated quote;
- optional bid and ask prices per 100 face;
- bid and ask size in face;
- funding, borrow and cash costs per 100 face.

Missing fields stay absent. Funding gives the security revision, currency,
settlement start and end, decimal repo rate, day count and haircut. Borrow gives
the available face, the last available date, a nonnegative decimal fee and the
day count. Every source has its own ID and version, quality, source,
availability and validity times, and evidence.

`assess_cash_hedge` takes a candidate pin, a requested face, a holding horizon
and `Buy`, `Sell` or `Short`.

- Buys need an ask and ask size.
- Sales need a bid and bid size, plus the owned face you supply.
- Shorts need the borrow size and horizon.
- Every direction needs funding and costs.

Missing or stale inputs, off-lot quantities and insufficient size are reported
as stable reasons.

`assess_futures_reference` and `assess_cash_hedge` both take independent `as_of`
and `known_at` timestamps. Later information can support an explicit restatement.
These conditions block eligibility:

- `Synthetic`, `Indicative` or `Historical` quality;
- source time after economic time;
- availability after the information cutoff;
- expired validity;
- incomplete baskets;
- missing cash prerequisites.

Issues are sorted and deduplicated by resource and reason. Invalid structures
return errors. Bond futures require their exact basket, and rate futures reject
any basket or CTD input.

The date-level trading gate closes at the start of `last_trade`, which is the
conservative choice. `tick_at` still accepts that date for arithmetic. A later
session-aware application must use exchange calendars and intraday cutoffs to
decide final-day trading.

`eligible: true` is only a necessary condition on the data. Actual futures
quotes, position and risk limits, entitlements, validated valuation and order
authorization are separate checks.

The approximate advisor now serializes `execution_status: "indicative_only"` on
all `HedgeProposal` and `ComparisonReport` outputs. Older JSON without the field
defaults to that status. `"executable"` is not an accepted value. Rust struct
literals need the field, normally `Default::default()`. The advisor formulas are
unchanged and give no execution eligibility.

## Exchange provenance and limits

The [reference facts](../tests/fixtures/futures/cme-2026-09-24.json) record the
identity of CME's 2026-09-24 Treasury conversion-factor workbook and exact cell
locations. The December 2026 ZN column has ten issues (I161:I170; count I178),
with CUSIPs, factors, coupon, issue and maturity dates. The
[CME publication page](https://www.cmegroup.com/trading/interest-rates/treasury-conversion-factors.html)
linked the workbook captured on 2026-10-07, SHA-256
`97308fafb418b43846cdd8e24be173bab08a49d46125f5bf086bc7dab276455d`.

The critical dates are derived from rules, not taken from an exchange calendar
feed. The SIFMA calendar dispatch, first-stub/EOM and remaining bond
registration settings are assumptions for testing, not a Treasury cashflow
validation. All fixture sources are `Historical`, and the declared validity is a
fixture interval, not a provider freshness guarantee.

The cash universe contains no invented prices, sizes, funding, borrow or costs.
Where tests change source quality or times, or add cash facts, these are test
controls. They are not live market observations or new exchange publications.

## Bounds, storage and next steps

Validation bounds:

- named years are 1900 to 2200;
- intervals are at most 100 years;
- rate windows are at most 366 days;
- term fixing offsets are at most 31 days;
- tick regimes: at most 32;
- baskets and cash universes: at most 128 unique securities;
- text: at most 512 bytes;
- posting precision: at most 8 places.

Multipliers and order quantities are at most 1 billion. Face, size and cost
amounts are at most 10^15. Cash and bond prices are in (0,1000], rate conversion
uses [0,200], and order checking needs a positive price. Factors are in (0,10],
repo rates in [-1,1], haircuts in [0,1) and borrow fees in [0,1]. These ceilings
say nothing about capacity, model validity or latency.

The memory and redb stores reuse compare-and-set, atomic receipts and bitemporal
history. Imports validate the security, contract, basket and calendar pins before
committing any prefix. No table, schema version or dependency is added.

Earlier executables cannot decode the new master kinds and commands. Upgrade
reference readers before introducing them. Keep a backup of the closed database
and use the existing isolated restore procedure to roll back. See the
[reference master contract](../crates/convex-ports/REFERENCE_MASTER.md).

Futures carry, CTD and all-deliverable repricing, and rate-futures averaging,
fixings, valuation and risk, are covered in [futures valuation](FUTURES-VALUATION.md).
Not yet covered: margin, roll and delivery lifecycle, executable cross-product
hedging, host dispatch, provider validation and latency measurement.
This work is partial and has not been certified for production use.
