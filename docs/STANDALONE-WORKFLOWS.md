# Valuation, inverse and risk workflows

`convex::workflow` ties together the bond, IRS/OIS, single-name CDS and futures
libraries for pricing, yield and spread inversion, and curve risk. You pass in
the contracts, calendars, curves, histories, administrator publication times and
model settings. The calculation does not look up a provider identifier, read a
repository, choose a convention from a currency name, consult a clock or post
cash. Curve and resource keys refer to values already in the bundle you pass.

"Checked" below means the function validates its input and returns an error
instead of guessing.

The input enums select existing product APIs. They are not a new payoff
hierarchy. The product modules keep their own economic validation, model math,
calibration residuals and work limits. `convex::portfolio::signed_risk` owns the
expected trade and factor manifests, legal-entity reporting scope, coverage and
pinned FX conversion. Infrastructure can wrap these functions separately.

## Start with the example

```sh
cargo run --locked -p convex --example standalone_book
cargo run --locked -p convex --example standalone_book --no-default-features
cargo run --locked -p convex --example standalone_book --features schemars
python3 tests/scripts/standalone_reference.py check
cargo test --locked -p convex-analytics --test standalone_workflow
cargo test --locked -p convex --test standalone_book
```

The [example](../crates/convex/examples/standalone_book.rs) builds eight curves
from complete recipes: deposit, FRA, partly fixed SOFR-futures, OIS, IRS, basis,
government, issuer, repo and CDS. It prices a stepped amortizing bond, an off-par
IRS and OIS, a protection-buyer CDS and a partly fixed rate future.

Each of the five trades has signed risk to two market-quote coordinates, using a
full graph rebuild at both central and half steps. The JSON output keeps:

- The helper residuals.
- Quantity units and monetary meaning.
- Per-currency and EUR reporting coverage.
- A full bond revaluation.
- A checked yield inverse.
- An indicative integer futures hedge.

Complete reporting needs local-currency gross exposure and same-snapshot FX that
you supply. Separate outputs show unknown gross exposure and a missing expected
trade or factor, and both are Incomplete. Face amounts, trade copies and contracts
are never summed as monetary gross exposure.

The [mixed-book bundle](../tests/fixtures/workflow/mixed-book.json) is synthetic.
Its CDS curves use complete legal terms, calendars and spreads that you supply,
with a separately supplied flat ACT/365F discount grid. This does not turn the
OIS calibration into an RFR CDS bootstrap. The earlier rate and cash fixtures are
reused unchanged. Model, calendar, quantity, exposure, FX and stability
assumptions are all explicit. The hedge has no execution costs or eligibility
check, and it cannot authorize or place an order.

## Operations and requirements

| Operation | Inputs and result |
|---|---|
| `construction_requirements`, `construct` | Full bond, swap or CDS terms and independent calendar bindings, or a complete futures reference. Keeps the checked schedules, profiles and legal terms. |
| `PublicationSchedule`, `SuppliedMarket` | Resolved curve values, eligible history and administrator decisions. Duplicate publication entries fail; shared graph entries must agree before the caller deduplicates. |
| `bond_requirements`, `swap_requirements`, `future_requirements` | Full observation and reset requirements. `inspect_fixing_requirements` separates absent due history from permitted forecasts, without inventing a forecast rate. |
| `declared_curve_requirements` | Potential curve keys. Price diagnostics determine what was actually used; an unused projection requirement is not reported as consumed. |
| `cashflows` | Full entitled bond cash or signed swap leg projections, independent of discount pricing. CDS keeps conditional premium, default and protection integration and the initial-payment context. Expected cash is not a recorded payment. |
| `price`, `ValuationInputs::monetary_value` | Full diagnostics and reference-size measurement. Bonds, swaps, CDS and optional models return economic PV. Futures return a labelled unrounded quote level times the currency-per-point multiplier. |
| `inverse` | Disjoint requests: checked bond, uniform swap fair rate, CDS conversion, optional OAS, or the remaining futures rate. The full result is kept. |
| `curve_quote_risk` | Full rate, CDS or cash graph, quote key, revision and native unit, position, snapshot and stability policy. Full repricing and half-step checks are mandatory. |
| `revalue_curves`, `revalue_optional` | Baseline and shocked results with the same economics. Terms, eligible history and model settings stay fixed. Floating bonds reproject future cash in each curve scenario. |
| `capability` | Route availability, with explicit NotApplicable and Unsupported. Availability does not mean the input is valid or that conventions are covered. |

Publication schedules take at most 65,536 entries. Requirements take at most
65,536 unique observations, resolved in bounded history batches. Missing
administrator decisions and missing eligible published history are distinct, and a
projection never substitutes for due history. The cash, recipe, grid and quote
work bounds of each family still apply.

## Bond quotation and inverse domains

`PreparedBondCash::fixed` keeps the entitled coupon and principal dates, profile
steps, principal allocations, accrued interest, rebate and original face.
`price_yield` takes an explicit `DatedYieldConvention`: discount every payment
from settlement with the day count and compounding you supply. This is a
dated-cash yield. It is not a Street yield, a YTW or an ICMA model. Annual decimal
rates must be finite and within ±10, and every discount factor must satisfy the
domain of its compounding.

`price_spread` uses the same-currency collateral curve, or a named government
benchmark, anchored by D(payment)/D(settlement), plus a continuous ACT/365F annual
spread. Spreads must be finite and within ±1. For fixed cash this is a Z-spread.

`PreparedBondCash::floating` first resolves all due history and eligible
forecasts using the independent index projection source. Spread inversion on this
prepared cash is a **frozen-projection discount margin**. Curve scenarios on
`FloatingBond` reproject future coupons instead. Neither route flattens stepped
coupons or replaces principal cash with a bullet.

`solve_bond_yield` and `solve_bond_spread` take Clean or Dirty targets per 100
of original face. The controls are:

- A finite, ordered, closed bracket within the domain.
- A price tolerance in (0,1].
- A separate positive parameter tolerance of at most 1e-6.
- 1..256 iterations.

The solver never expands the bracket or relaxes the controls. A final full reprice
must meet the price tolerance, so bracket width or parameter convergence alone is
not enough. Results keep the target and controls, the iterations and evaluations,
the residual and the cash price.

When positive future cash exists, nonnegative entitled cash gives a monotone
inverse. Constant cash, cash with no exposure, or cash that cannot be told apart
numerically returns NonUnique for a matching target and NoRoot otherwise.
Mixed-sign cash is Unsupported, because that would need a multiple-root model,
and no root is picked arbitrarily. Clean targets use the accrued interest,
including negative rebates.

Uniform IRS/OIS fair rates use the full profile, cash and annuity model. Rounded
or nonlinear cash fails, and an empty remaining fixed leg is NonUnique. CDS
running, clean and dirty conversion keeps the intensity bounds, accrued interest,
outstanding initial cash, model attribution and final quote residual. The
restrictions for zero-recovery exposure stay explicit. Optional OAS uses the
prepared model and every grid check. A fully fixed rate future is NonUnique for a
remaining-rate request once its snapshot has been validated.

## Monetary units, failures and model checks

Signed positions scale once.

- Bonds, FRNs and CDS use Notional, with reference size equal to the original face
  or notional.
- Swaps and optional contracts use TradeCopies with reference one. Their profile
  sizes and trade sides stay in the contract.
- Futures use integer Contracts with reference one and the currency-per-point
  multiplier.

Currency, unit or reference mismatches fail before any scenario or risk scaling.

Futures quote levels are not discounted lifetime PV. Their revaluation changes and
sensitivities are indicative unrounded mark effects, not clearing cash or P&L
posting. An optional price is not unconditional cash. Use the bond cash
conditional on no exercise and the model's exercise diagnostics. Futures cash
needs margin and delivery events from the lifecycle layer.

`WorkflowError` keeps these apart: MissingInput, NotApplicable, Unsupported,
NonUnique, NoRoot, NoConvergence, Invalid and the product and model failures.
Missing inputs list the curves, observations, administrator decisions or
projection sources. Accepted inverses never contain fabricated roots, zero values,
suppressed failures or invented work counts.

Generic graph risk supports curve-based bond and FRN, IRS/OIS, CDS and rate
futures. It requires the valuation you pass to agree with the prepared graph
baseline, and you own the snapshot provenance. Risk on a raw dated yield is
NotApplicable. Generic graph paths for optional bonds and bond futures are
Unsupported. Use `optional_oas_quote_sensitivity`, `PreparedOptionalBond::risk`
and the all-member `bond_futures_risk` routes instead, which keep the model
resolution, refinement and all CTD alternatives. Arbitrary small optional bumps
can still fail the accuracy checks. Credit recovery and default risk and raw
quotation risk remain with their own APIs, and the wrapper adds no new taxonomy.

Aggregation requires every expected trade and factor, including failed trades. A
partial known sum is Incomplete, and values that are all missing stay absent.
Unknown gross exposure and absent or stale FX stay visible. Reporting FX converts
risk and keeps the conversion exposure. It is not a full cross-currency risk
model.

## Validation

The [independent reference builder](../tests/scripts/standalone_reference.py)
uses Decimal(60) stepped and amortizing cash and a fixed 240-step bisection. Six
yield and Z-spread cases include negative rates and fractional spreads. Clean and
dirty inversions use predeclared bounds of 1e-9 per 100 of price and 1e-10 for
rates. The script does not import Convex or read its outputs. Rust tests for
IRS/OIS, CDS, FRN, optional bonds and futures also exercise the checks, round
trips, quantities and failures.

The example is a bounded batch calculation. Not yet covered: convention, model
and market packs, independent quantitative review, lifecycle, cash posting and
P&L, full host and schema dispatch, and latency under colocation or multiple
servers.

Mixed-book revaluation and model P&L attribution use the same product and graph
code, and they keep every delivery CTD switch and the separate meanings of cash PV
and futures quote level. See [the mixed-book contract](MIXED-BOOK-SCENARIOS.md)
and run `cargo run --locked -p convex --example mixed_scenario`.

The `book_pnl` route reconstructs the information available at each point,
replays lifecycle and cash facts, and keeps nonlinear product values and all-member
CTD switches. [CAUSAL-BOOK-PNL.md](CAUSAL-BOOK-PNL.md) describes the inputs, the
separate quote and cash bases, fills and receipts, attribution, work bounds and
limits.
