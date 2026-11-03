# CDS valuation

This document covers single-name CDS valuation in the `convex-credit` crate
(also available as `convex::credit`). The crate provides checked single-name
terms, premium schedules, protection and accrual-on-default valuation, and
conversion between running, clean and dirty upfront quotes. "Checked" means it
validates its input and returns an error instead of guessing.

It reuses core dates, calendars, schedules, identifiers and exact decimals, the
checked curves, and the bounded Brent solver. Its production dependencies
contain no database, MQ, provider, clock, network, async runtime or external
pricing engine.

This application is based on the ISDA CDS Standard Model (version 1.8.3),
developed and supported in collaboration with Markit Group Ltd.

The [ISDA public license](../licenses/ISDA-CDS-Standard-Model-Public-License-1.0.txt)
is kept separately from Convex's MIT license; the crate also ships it as
`crates/convex-credit/LICENSE-ISDA-CDS-STANDARD-MODEL.txt`. Preserve its
required attribution when distributing or exposing this route. Valuation results
carry the same `model_notice` for downstream integrations.

This is a Rust implementation, compared against the ISDA C model as described
below. ISDA does not endorse it, and it has not been independently validated
against a vendor system.

## Start with the public example

```sh
cargo run --locked -p convex --example cds_valuation
cargo run --locked -p convex --example cds_valuation --no-default-features
cargo run --locked -p convex --example cds_valuation --features schemars
cargo test --locked -p convex-credit
cargo test --locked -p convex-curves
```

The example rebuilds checked JSON terms, binds the calendars you pass in,
constructs explicit discount/survival nodes, prices both legs and exercises
running/clean/dirty conversion. Building with the default, minimal or schema
features does not imply a CDS schema, REST, C ABI, WASM or other host contract;
those are not yet provided.

`CdsTerms::build(&CdsCalendars)` produces private checked `CdsContract` state.
You supply every trade boundary, document and restructuring-clause revision,
the reference entity, seniority, recovery, currency, unsigned positive notional,
coupon, premium schedule, calendar version and curve key. Unknown JSON fields,
unknown models/seniorities, empty identities and invalid revisions fail. A ticker
or currency never supplies market conventions. Exact decimal strings carry
notional, coupon and contractual upfront; f64 conversion occurs at the numerical
boundary. Successful model currency outputs are unrounded; ledger rounding is
an explicit caller policy.

## Dates, cash and quote units

The only supported convention is `CdsModel::Standard183`. Trade is end of day
and step-in is T+1 **calendar** day, including weekends. Settlement must equal
the 0..366 business-day lag you pass in, under the separately bound settlement
calendar.

Premium schedules are quarterly and use the stubs you pass in. Following is
applied to intermediate accrual and payment boundaries. The first accrual start
is kept. Protection ends on the unadjusted maturity and the final premium
accrues through that day (the exclusive accrual end is maturity plus one).

`QuarterlyTwentieth` checks March/June/September/December twentieth maturities;
`ExplicitDate` permits an off-cycle stub. Roll helpers return quarterly dates
without inferring CDS2015 tenor rules. Premium day count supports ACT/360 and
ACT/365F; model curve time is ACT/365F.

`CdsPricingContext` takes the current surviving-entity valuation date, step-in,
fair cash-settlement date and initial-payment state. A current step-in must
follow valuation, cash settlement cannot precede it, and both offsets are
bounded by 366 calendar days. These dates are quote boundaries, not offsets
inferred from a calendar.

A contracted initial upfront must be `Pending` or `Settled`, consistently with
its date. Absent initial cash must be `NotContracted`. Pending overdue cash
needs lifecycle treatment. Fair quote cash is distinct from outstanding
contracted initial cash. Completed accrual with delayed premium cash still
outstanding fails explicitly until a lifecycle route represents that
entitlement. Fully elapsed contracts return empty future legs and no par quote
once those cash dates have passed. Defaulted, auction and successor states are
not accepted by these surviving terms.

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
standard model's half-day default-accrual convention, the adjusted accrual
dates and normalized discount/survival ratios at current valuation.

## Curves and conversion

`price_cds` takes a same-currency, same-collateral discount curve (exact key)
and a same-currency `Credit` survival curve whose entity, seniority and recovery
match the terms. Direct nodes must represent discount factors or survival
probabilities, use ACT/365F log-linear interpolation and forbid extrapolation.
Curve construction validates the required origins and query domains. Inputs can
predate the current valuation and are normalized at that date.

The union of both curves' knots and the coupon/protection boundaries gives exact
piecewise exponential integration for the declared curves, including negative
discount forwards. Unsupported zero-rate, derived or helper representations and
methods fail. No government-discount substitute or mid-period approximation is
inserted.

`price_flat_hazard` declares a constant ACT/365F intensity separately from the
curve lineage. `convert_cds` fits that same pricer to `RunningSpread`,
`CleanUpfront` or `DirtyUpfront`, keeping the coupon. It never substitutes
spread/(1−recovery). You control a closed nonnegative hazard bracket up to 5,
the quote tolerance and the iteration limit; brackets are not expanded.
Defaults are [0,1], absolute residual 1e-12 and 100 iterations. Iterations are
capped at 256. Failed, nonidentified, full-recovery and matured conversions,
missing inputs and final residual failures return errors, not converted quotes.

Inherited limits include 1,024 nodes per curve, 4,096 schedule periods, a
150 × 366 calendar-day contractual horizon, and checked calendar searches.
The joint integration grid has an additional 4,096-point ceiling. All inputs and
callbacks come from the caller; custom calendar callbacks must terminate without
panic. Latency and capacity budgets have not been measured. Hosts can shard
independent valuations over immutable inputs; the crate has no distributed
scheduler or cache.

## ISDA comparison and reproduction

The [USD SOFR RFR grid](https://www.cdsmodel.com/rfr-test-grids.html)
dated 2022-06-21 contains 7,164 cases: six maturities, coupons of 100/500 bp,
199 spreads from 10 to 1,000 bp, and recoveries of 0.20/0.25/0.40. The retained
ZIP is the publisher's file. `grid.json` preserves all cell strings and 19 RFR
quotes without editing or recalculating the workbook. The official NYM file
contains the specified June-20 holidays; it is not a full US bank holiday
calendar. No additional holidays or current conventions were substituted.

The [ISDA model 1.8.3 source](https://www.cdsmodel.com/download.html)
was verified against its ZIP, compiled without source edits and executed in
separate test tooling. Raw outputs, model version, source hashes, compiler
identity and all 53 successful build/execution commands are frozen.

The capture driver passes unadjusted tenor maturities to the model's RFR helper,
which applies Modified Following internally; pre-adjusting them changes annual
coupon anchors. Its 36-node discount curve, normalized to valuation, is passed
to the Rust code. **SOFR quote-to-curve calibration is not part of this
comparison.**

The ISDA model results and the Rust conversion results must both match every
published row, settlement/step-in date and these bounds. The bounds were set
before the comparison and use zero relative tolerance:

| Measure | Absolute bound |
|---|---|
| Clean upfront fraction | 1e-8 of notional |
| Cash settlement | 0.5 USD, converter currency-unit precision |
| Clean price per 100 | 1e-6 |
| Accrued days | 1e-8 calendar days |
| Accrued premium | 1e-6 USD |

CI reproduces the **current Rust** 35,820 comparisons offline, checks the
workbook, capture, license and input bytes, and rejects missing or reordered rows,
changed dates/units, nonfinite values, altered bounds and bad solver residuals:

```sh
python3 tests/scripts/isda_cds_compare.py check
python3 -m unittest discover -s tests/scripts -p test_isda_cds_compare.py
cargo build --locked -p convex-credit --example cds_comparison
python3 tests/scripts/isda_cds_compare.py offline \
  --candidate-binary target/debug/examples/cds_comparison \
  --report /tmp/cds-comparison-report.json
```

Use a fresh report path; runs never overwrite earlier reports. To run the
ISDA C model again, pass the downloaded ZIP
(SHA-256 `bee02772ef45c3ff93de995a1655b3c5231aec4e257173bc4d21d47c362e0db0`),
clang/clang++ and a new output directory:

```sh
python3 tests/scripts/isda_cds_capture.py \
  --source-archive /path/to/isda_cds_model_c_v1.8.3.zip \
  --output /tmp/isda183-reproduction
```

The upstream portability build force-includes its own `ldate.h` declaration;
the financial source and formulas are unchanged. Build binaries and extracted
upstream sources are temporary and are not production dependencies. Compiler or
platform differences need a fresh run; CI uses the frozen output and does not
run the ISDA engine.

## Related work and gaps

[Multi-pillar credit calibration, signed quote risk, immediate default
scenarios and bond/CDS mapping](CREDIT-CALIBRATION.md) are documented
separately, as is RFR curve calibration.

Not yet covered: CDS indices, convention packs, default/auction/successor and
paid-cash lifecycle, common result and host dispatch, and independent
validation for USD, EUR and GBP. This document covers single-name valuation and
conversion, checked against the ISDA grid above.
