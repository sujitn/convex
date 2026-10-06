# Supplied single-name CDS valuation

CVX-020 adds `convex-credit`, exposed as `convex::credit`. The crate owns checked
single-name terms, premium schedules, protection and accrual-on-default valuation,
and conventional running/clean/dirty upfront conversion. It reuses core dates,
calendars, schedules, identifiers and exact decimals, checked curves and the
bounded Brent solver. Its production dependencies contain no database, MQ,
provider, clock, network, async runtime or external pricing engine.

This application is based on the ISDA CDS Standard Model (version 1.8.3),
developed and supported in collaboration with Markit Group Ltd.

The original [ISDA public license](../licenses/ISDA-CDS-Standard-Model-Public-License-1.0.txt)
is retained separately from Convex's MIT license. Preserve its required
attribution when distributing or exposing this route. Native valuation results
carry the same `model_notice` for downstream integrations. This is a Rust
implementation with scoped executed comparison evidence; no ISDA endorsement,
independent market/model approval, production or library certificate is claimed.

## Start with the public example

```sh
cargo run --locked -p convex --example cds_valuation
cargo run --locked -p convex --example cds_valuation --no-default-features
cargo run --locked -p convex --example cds_valuation --features schemars
cargo test --locked -p convex-credit
cargo test --locked -p convex-curves
```

The example rebuilds checked JSON terms, binds actual supplied calendars,
constructs explicit discount/survival nodes, prices both legs and exercises
running/clean/dirty conversion. Default, minimal and schema-feature **builds**
do not imply a CDS schema, REST, C ABI, WASM or other host contract; those are
separate owning work.

`CdsTerms::build(&CdsCalendars)` produces private checked `CdsContract` state.
Supply every trade boundary, document and restructuring-clause revision,
reference entity, seniority, recovery, currency, unsigned positive notional,
coupon, premium schedule, calendar version and curve key. Unknown JSON fields,
unknown models/seniorities, empty identities and invalid revisions fail. A ticker
or currency never supplies market conventions. Exact decimal strings carry
notional, coupon and contractual upfront; f64 conversion occurs at the numerical
boundary. Successful model currency outputs are unrounded; ledger rounding is
an explicit caller policy.

## Dates, cash and quote units

The admitted mathematical convention is explicitly `CdsModel::Standard183`.
Trade is end of day and original step-in is T+1 **calendar** day, including
weekends. Original settlement must equal the supplied 0..366 business-day lag
under the separately bound settlement calendar. Premium schedules are quarterly,
use actual supplied stubs, and apply Following to intermediate accrual and payment
boundaries. The first supplied accrual start is retained. Protection ends on the
unadjusted maturity and final premium accrues through that day (exclusive accrual
end is maturity plus one). `QuarterlyTwentieth` checks actual March/June/September/
December twentieth maturities; `ExplicitDate` permits a supplied off-cycle stub.
Roll helpers return actual quarterly dates without inferring CDS2015 tenor rules.
Premium day count supports ACT/360 and ACT/365F; model curve time is ACT/365F.

`CdsPricingContext` explicitly supplies current surviving-entity valuation,
step-in, fair cash-settlement date and original initial-payment state. A current
step-in must follow valuation, cash settlement cannot precede it, and both offsets
are bounded by 366 calendar days. Dates on this context are supplied quote
boundaries, not inferred current-calendar offsets. A contracted initial upfront
must be `Pending` or `Settled` consistently with its original date; absent
initial cash must be `NotContracted`. Pending overdue cash needs lifecycle
treatment. Fair quote cash is distinct from outstanding contracted initial cash.
Completed accrual with delayed premium cash still outstanding fails explicitly
until a lifecycle route represents that entitlement. Fully elapsed contracts
return empty future legs and no par quote once those cash dates have passed.
Defaulted/auction/successor states are not admitted by these surviving terms.

All PVs and conditional premium amounts use trade direction: buyer receives
protection and pays premiums/positive initial cash; seller offsets each signed
component. Fair quote fields always describe **buyer-to-seller** amounts,
independently of the trade side:

| Field | Unit and relationship |
|---|---|
| `coupon_decimal`, `par_spread_decimal` | Annualized decimal rate; 0.01 = 100 bp |
| `clean_upfront_fraction` | Fraction of notional excluding accrued refund |
| `dirty_upfront_fraction` | Clean fraction less coupon × accrued year fraction |
| `cash_settlement_amount` | Notional × dirty fraction, unrounded currency |
| `clean_price_per100` | 100 × (1 − clean fraction) |
| `net_pv` | Signed protection + premium + outstanding initial cash PV |

Scheduled premium is contingent on survival through the day preceding its
exclusive accrual end; it is not a realized payment record. The default-accrual
flag selects whether accrued premium is paid on default. Integration uses the
standard model's half-day default-accrual convention, actual adjusted accrual
dates and normalized discount/survival ratios at current valuation.

## Curves and conversion

`price_cds` consumes an exact-key, same-currency/collateral discount curve and a
same-currency `Credit` survival curve whose entity, seniority and recovery match
the terms. Direct nodes must represent discount factors or survival probabilities,
use ACT/365F log-linear interpolation and explicitly forbid extrapolation.
Required origins and query domains are validated by curve construction. Inputs
can predate current valuation and are normalized at that date. The union of
both curves' knots and actual coupon/protection boundaries gives exact piecewise
exponential integration for the declared curves, including negative discount
forwards. Unsupported zero-rate/derived/helper representations or methods fail;
no government-discount substitute or mid-period approximation is inserted.

`price_flat_hazard` declares a constant ACT/365F intensity separately from supplied
curve lineage. `convert_cds` fits that same complete pricer to `RunningSpread`,
`CleanUpfront` or `DirtyUpfront`, retaining the actual coupon. It never substitutes
spread/(1−recovery). The caller controls a closed nonnegative hazard bracket up
to 5, native quote tolerance and iteration limit; brackets are not expanded.
Defaults are [0,1], absolute native residual 1e-12 and 100 iterations. Iterations
are capped at 256. Failed/nonidentified/full-recovery/matured conversions, missing
inputs and final residual failures return errors, not converted quotes.

Inherited limits include 1,024 nodes per curve, 4,096 schedule periods, a
150 × 366 calendar-day contractual horizon, and checked calendar searches.
The joint integration grid has an additional 4,096-point ceiling. All inputs
and callbacks are supplied; custom calendar callbacks must terminate without
panic. Product latency/capacity budgets and allocation optimizations require
measured workloads in their owning tasks. Hosts can shard independent valuations
over immutable inputs; task CVX-020 introduces no distributed scheduler or cache.

## Original ISDA comparison and reproduction

The [original USD SOFR RFR grid](https://www.cdsmodel.com/rfr-test-grids.html)
dated 2022-06-21 contains 7,164 cases: six maturities, coupons of 100/500 bp,
199 spreads from 10 to 1,000 bp, and recoveries of 0.20/0.25/0.40. The retained
ZIP is original publisher bytes. `grid.json` preserves all original cell strings
and 19 RFR quotes without editing or recalculating the workbook. The official
NYM file contains the specified June-20 holidays; it is not a full US bank
holiday calendar. No additional holidays or today's conventions were substituted.

The original [ISDA model 1.8.3 source](https://www.cdsmodel.com/download.html)
was verified against its ZIP, compiled without source edits and executed in
separate test tooling. Raw outputs, model version, complete source hashes,
compiler identity and all 53 successful build/execution commands are frozen.
The capture driver passes unadjusted tenor maturities to the model's RFR helper,
which applies Modified Following internally; pre-adjusting them changes annual
coupon anchors. Its complete 36-node discount curve, normalized to valuation,
is supplied to Rust. **Native SOFR quote-to-curve calibration is not claimed.**

Both actual original-model and native conversion results must match every
published row, settlement/step-in date and these bounds, declared before native
comparison and using zero relative tolerance:

| Measure | Absolute bound |
|---|---|
| Clean upfront fraction | 1e-8 of notional |
| Cash settlement | 0.5 USD, converter currency-unit precision |
| Clean price per 100 | 1e-6 |
| Accrued days | 1e-8 calendar days |
| Accrued premium | 1e-6 USD |

CI reproduces the **current native** 35,820 comparisons offline, checks original
workbook/capture/license/input bytes, and rejects missing or reordered rows,
changed dates/units, nonfinite values, altered bounds and bad solver residuals:

```sh
python3 tests/scripts/isda_cds_compare.py check
python3 -m unittest discover -s tests/scripts -p test_isda_cds_compare.py
cargo build --locked -p convex-credit --example cds_comparison
python3 tests/scripts/isda_cds_compare.py offline \
  --candidate-binary target/debug/examples/cds_comparison \
  --report /tmp/cds-comparison-report.json
```

Use a fresh report path; runs never overwrite evidence. To independently
execute the original C model again, supply the original downloaded ZIP
(SHA-256 `bee02772ef45c3ff93de995a1655b3c5231aec4e257173bc4d21d47c362e0db0`),
clang/clang++ and a new output directory:

```sh
python3 tests/scripts/isda_cds_capture.py \
  --source-archive /path/to/isda_cds_model_c_v1.8.3.zip \
  --output /tmp/isda183-reproduction
```

The upstream portability build force-includes its own `ldate.h` declaration;
financial source/formulas are unchanged. Build binaries and extracted upstream
sources are temporary, outside production dependencies. Compiler/platform
differences need a fresh actual execution; frozen output is not a live-engine
execution in CI.

## Remaining production scope

CVX-021 owns multi-pillar single-name credit calibration and propagated quote
CS01/JTD; CVX-057 owns native RFR calibration. CDS indices, convention-pack
admission, default/auction/successor and paid-cash lifecycle, common result/host
dispatch, independent human quantitative approval and complete USD/EUR/GBP
market/model/library certificates remain their owning tasks. All 2,154 mandatory
library records remain Planned. This task establishes the native single-name
valuation and conversion subset with actual original-grid evidence.
