# Supplied-input valuation, inverse and risk workflows

`convex::workflow` composes the checked bond, IRS/OIS, single-name CDS and
futures libraries. Supply actual immutable contracts, calendars, curves,
histories, administrator publication times and model settings. Pure calculation
does not resolve a provider identifier, read a repository, choose a convention
from a currency name, consult a clock or post cash. Exact curve/resource keys
refer to values already present in the supplied bundle.

The closed input enums select existing native product APIs; they are not a new
payoff hierarchy. Native modules retain economic validation, model mathematics,
calibration residuals and work limits. `convex::portfolio::signed_risk` owns
expected trade/factor manifests, legal-entity reporting scope, coverage and
pinned FX conversion. Infrastructure can wrap these functions independently.

## Start with the complete example

```sh
cargo run --locked -p convex --example standalone_book
cargo run --locked -p convex --example standalone_book --no-default-features
cargo run --locked -p convex --example standalone_book --features schemars
python3 tests/scripts/standalone_reference.py check
cargo test --locked -p convex-analytics --test standalone_workflow
cargo test --locked -p convex --test standalone_book
```

The [example](../crates/convex/examples/standalone_book.rs) builds eight actual
curve outputs from complete deposit/FRA/partly fixed SOFR-futures/OIS/IRS/basis,
government/issuer/repo and CDS recipes. It prices a stepped amortizing bond,
off-par IRS and OIS, a protection-buyer CDS and a partly fixed rate future.
Each of the five trades has signed risk to two exact market-quote coordinates,
using full graph rebuilding at both central and half steps. The JSON output
retains original helper residuals, quantity units, monetary meaning, per-currency
and EUR reporting coverage, a full bond revaluation, a checked yield inverse
and an indicative integer futures hedge.

Complete reporting requires explicitly supplied local-currency gross exposure
and same-snapshot FX. Separate outputs demonstrate unknown gross exposure and
a missing expected trade/factor; both remain Incomplete. Physical face amounts,
trade copies and contracts are never summed as monetary gross exposure.

The [mixed-book bundle](../tests/fixtures/workflow/mixed-book.json) is synthetic.
Its new CDS curves use complete supplied legal terms, calendars and spreads,
with a separately supplied flat ACT/365F discount grid. This does not convert
the OIS calibration into a native RFR CDS bootstrap. Prior complete rate/cash
fixtures are reused unchanged. All model, calendar, quantity, exposure, FX and
stability assumptions remain explicit. The hedge lacks execution costs and
eligibility; it cannot authorize or execute an order.

## Public operations and requirements

| Operation | Supplied inputs and result |
|---|---|
| `construction_requirements`, `construct` | Full native bond/swap/CDS terms and independent calendar bindings, or a complete futures reference. Retains original checked schedules, profiles and legal terms. |
| `PublicationSchedule`, `SuppliedMarket` | Exact resolved curve values, eligible history and explicit administrator decisions. Duplicate publication entries fail; shared graph entries must agree before caller deduplication. |
| `bond_requirements`, `swap_requirements`, `future_requirements` | Original full observation/reset requirements. `inspect_fixing_requirements` distinguishes absent due history from permitted forecasts without inventing a forecast rate. |
| `declared_curve_requirements` | Potential exact curve keys. Native price diagnostics determine actual consumption; an unused projection requirement is not reported as consumed. |
| `cashflows` | Full entitled bond cash or signed swap leg projections, independent of discount pricing; CDS retains conditional premium/default/protection integration and initial-payment context. Expected cash is not a recorded payment. |
| `price`, `ValuationInputs::monetary_value` | Full native diagnostics and reference-size measurement. Bonds/swaps/CDS/optional models return economic PV. Futures return an explicitly labelled unrounded quote level times the original currency-per-point multiplier. |
| `inverse` | Disjoint checked bond, uniform swap fair rate, native CDS conversion, native optional OAS or equivalent remaining futures-rate request; complete native results retained. |
| `curve_quote_risk` | Complete original rate/CDS/cash graph, exact quote key/revision/native unit, position, snapshot and stability policy. Native full repricing and half-step checks remain mandatory. |
| `revalue_curves`, `revalue_optional` | Full same-economics baseline and shocked results; terms, eligible history and model settings stay fixed. Floating bonds reproject future cash at each curve scenario. |
| `capability` | Native route availability and explicit NotApplicable/Unsupported. Availability is not input admission, convention coverage or a library/model/market certificate. |

Publication schedules admit at most 65,536 entries. Requirements admit at most
65,536 unique observations, resolved in existing bounded history batches.
Missing administrator decisions and missing eligible published history remain
distinct; projections never substitute for due history. Native family-specific
cash, recipe, grid and quote work bounds still apply.

## Bond quotation and inverse domains

`PreparedBondCash::fixed` retains actual entitled coupon/principal dates,
profile steps, principal allocations, accrued/rebate and original face.
`price_yield` uses an explicit `DatedYieldConvention`: discount every actual
payment from settlement with the supplied day count and compounding. This is a
dated-cash yield, not an inferred Street yield, YTW or contextual ICMA model.
Annual decimal rates must be finite and within ±10, and every actual discount
factor must satisfy its native compounding domain.

`price_spread` uses the actual same-currency collateral curve or a named
government benchmark, anchored by D(payment)/D(settlement), plus a continuous
ACT/365F annual spread. Spreads must be finite and within ±1. Fixed cash has
Z-spread semantics. `PreparedBondCash::floating` first resolves all due history
and eligible forecasts with the exact independent index projection source;
spread inversion on this prepared cash is a **frozen-projection discount
margin**. Curve scenarios on `FloatingBond` reproject future coupons instead.
Neither route flattens stepped coupons or replaces principal cash with a bullet.

`solve_bond_yield` and `solve_bond_spread` accept explicit Clean/Dirty per100
original-face targets. Controls require a finite ordered closed bracket within
the respective domain, price tolerance in (0,1], separate positive parameter
tolerance ≤1e-6 and 1..256 iterations. The solver never expands the bracket or
relaxes controls. A final full native reprice must satisfy the original price
tolerance; width or parameter convergence alone is insufficient. Results retain
original target/controls, actual iterations/evaluations, residual and cash price.

Nonnegative entitled cash identifies a monotone inverse when positive future
cash exists. Constant/no-exposure or numerically unidentifiable cash returns
NonUnique for a matching target and NoRoot otherwise. Mixed-sign cash is
Unsupported pending a complete multiple-root model; no arbitrary root is
selected. Clean targets use original accrued, including negative rebates.

Uniform IRS/OIS fair rates use the original full profile/cash/annuity model;
rounded or non-linear cash fails its native restriction, and an empty remaining
fixed leg is NonUnique. CDS running/clean/dirty conversion retains original
intensity bounds, accrued, outstanding initial cash, model attribution and final
native quote residual. Original zero-recovery-exposure model restrictions stay
explicit. Optional OAS uses the original prepared model and every grid guard.
A fully fixed rate future is NonUnique for a remaining-rate request after its
complete native snapshot has been validated.

## Monetary units, failures and model guards

Signed positions scale once. Bond/FRN and CDS positions use Notional with
reference size exactly equal to original face/notional. Swap and optional
contracts use TradeCopies with reference one; their original profile sizes and
trade sides remain in the contract. Futures use integer Contracts with reference
one and the original currency-per-point multiplier. Currency/unit/reference
mismatches fail before scenario or risk scaling.

Futures quote levels are not discounted lifetime PV. Their full revaluation
changes and sensitivities are indicative unrounded mark effects, not clearing
cash or P&L posting. Optional price is not unconditional cash; use native bond
cash conditional on no exercise and the actual model exercise diagnostics.
Futures cash requires explicit margin/delivery events in the lifecycle layer.

`WorkflowError` keeps MissingInput, NotApplicable, Unsupported, NonUnique,
NoRoot, NoConvergence, Invalid and original product/model failures separate.
Missing inputs enumerate exact curves, observations, administrator decisions or
projection sources. Accepted inverses do not contain fabricated roots, zero
values, suppressed failures or invented work counts.

Generic graph risk supports actual curve-based bond/FRN, IRS/OIS, CDS and rate
future representations. It requires the supplied original valuation to agree
with the prepared graph baseline; the caller also owns exact immutable snapshot
provenance. Raw dated-yield curve risk is NotApplicable. Optional and bond-future
generic graph paths are Unsupported: use `optional_oas_quote_sensitivity`,
`PreparedOptionalBond::risk` and the native all-member `bond_futures_risk` routes.
Those preserve model resolution/refinement and the full CTD alternatives.
Arbitrary small optional bumps may still fail original accuracy guards.
Model-specific credit recovery/default and raw quotation risk stay available
through their owning native APIs; the wrapper does not invent new taxonomy.

Aggregation requires every expected trade/factor, including failed trades.
Partial known sums remain Incomplete; all-missing values remain absent. Unknown
gross exposure and absent/stale FX remain visible. Reporting FX converts risk
with retained conversion exposure; it is not a full cross-currency risk model.

## Validation scope

The [independent reference builder](../tests/scripts/standalone_reference.py)
uses Decimal(60) full stepped/amortizing cash and fixed 240-step bisection.
Six yield/Z cases include negative rates and fractional spreads; both clean and
dirty inversions use predeclared 1e-9 per100 price and 1e-10 rate bounds. It does
not import Convex, execute candidates or read their outputs. Native IRS/OIS,
CDS, FRN, optional and futures tests additionally exercise actual native guards,
roundtrips, quantities and failure outcomes.

This is supplied-input engineering. Convention/model/market packs, independent
quantitative approval, lifecycle/cash posting/P&L, full host/schema dispatch,
library/production certificates and multi-server/colocation latency evidence
remain separate work. The example is bounded batch reference calculation.
Every earlier numerical artifact/default and every mandatory Planned library
record remains protected.
