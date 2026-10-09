# Supplied cash curves and dated repo carry

`convex::cash_calibration::build_cash_curve_set` fits Government, IssuerSpread
and Repo curves from complete original cash instruments. It composes supplied
BondTerms/BondContract schedules, profiles, settlement, entitlement and accrued,
checked curve contracts, original CDS survival inputs, and shared bounded root
and least-squares solvers. The calculation is pure. Callers resolve immutable
inputs and calendars; providers, storage, messaging, clocks and publication stay
outside analytics.

[Full product-price parametric fitting](PRODUCT-PARAMETRIC-CALIBRATION.md) adds
explicit NS/Svensson continuous-zero mode for Government/Repo. Signed issuer
residual basis and credit recovery models retain their separate original owners.

Run the repository's standalone consumer and independent reference checks:

```sh
cargo run --locked -p convex --example cash_calibration
cargo run --locked -p convex --example cash_calibration --no-default-features
cargo run --locked -p convex --example cash_calibration --features schemars
python3 tests/scripts/cash_calibration_reference.py check
cargo test --locked -p convex-analytics --test cash_calibration
```

`reprice_cash_helpers` diagnoses one complete original recipe on exact supplied
curves without fitting or replacing them. It reuses full contract/model admission
and original native quote conversion, retaining source-order full valuations,
revisions, residuals and work. Finite off-market results have
`within_tolerance:false`; they do not accept a calibration. The
[shared cash-helper route](LIBRARY-API.md#direct-cash-helper-repricing-diagnostics)
adds strict full-term JSON/C and existing client dispatch.

The consumer fits nonflat positive and negative government, residual-basis and
repo curves, prices actual dated carry, and fits a separate credit-bond basis
against the complete original six-quote CDS calibration. It checks independent
cash/profile/date/parameter references and prints original native quote residuals
and actual helper valuation counts. New bond/carry observations are synthetic.
The credit dependency uses the retained actual ISDA comparison input/capture;
this does not turn the synthetic bond model into an independently certified
market implementation.

## Original economics and quotations

Each `CashCurveRecipe` retains an exact `CurveRecipe`, strictly increasing
parameter dates, every original helper in source order, original native quote
units and absolute tolerances, and explicit bootstrap/model/overlap choices.
`CashCurveBuildInput` supplies matching `CashHelperResources` in that same order.
A bond helper contains full original `BondTerms`, actual trade and settlement
and pre/post-payment entitlement, quote revision and last relevant cash pillar.
Supplied calendar identities/versions must match terms, and the calendar must
produce the stated settlement from the actual trade date, lag and adjustment.

| Original observation | Checked conversion and model residual |
|---|---|
| CleanPrice | PricePer100 of original face; dirty equals clean plus actual accrued/rebate. |
| DirtyPrice | PricePer100 of original face; clean equals dirty minus actual accrued/rebate. |
| CashflowYield | Decimal/percent/basis-point annualized yield with explicit dated-cash day count, compounding, inverse bracket, work budget and dirty-price residual bound. Each actual entitled coupon/principal date is discounted. |
| BankDiscountYield | Explicit simple discount basis and decimal/percent/basis-point quote; one future par principal, zero coupons and accrued. Dirty price equals 100 times (1 minus discount yield times actual year fraction). |
| ParCouponRate | Original uniform decimal/percent/basis-point coupon of a supplied full unrounded nominal par bond. Clean price is 100; actual unit/zero-coupon cash exposures determine the implied coupon. The observation is never a spot-zero node. |
| RepoLoanHelper | Actual business start/end dates, pinned calendar, collateral definition and interest basis. Original simple annualized decimal/percent/basis-point loan rate reprices from the exact repo discount ratio. |

Prices normalize by original face, including stepped/amortizing principal,
redemption premiums and dated principal cash supplied through BondContract.
Fixed coupon profiles are not averaged. Fixed helper terms serialize their
contractual Decimal amounts/profiles as strings, preserving exact native JSON
roundtrips without changing existing host DTOs. Strict tags reject inapplicable
fields; deserialize then build/validate before using any input.

CashflowYield is an explicitly dated-cash convention. It does not claim every
street, ICMA, exchange or national bond yield convention. Contextual ICMA yield
fractions require another complete quotation pack; ICMA coupon accrual remains
available through the original bond schedule. Negative yields are supported
when their declared discount domain is valid. Negative entitled cash is rejected
by the monotone yield inverse, while explicit price/par helpers can retain
negative coupons. Bracket endpoints must be valid for every actual cash date;
a trial price outside the inverse bracket fails rather than extending bounds.

Par helpers require uniform coupon profiles matching the exact original quote,
unrounded Allow-negative cash, nominal bullet principal and no explicit
principal-price overrides. Zero/unit coupon projections reuse identical actual
schedules, entitled dates and accrued. Bank-discount helpers cannot reinterpret
coupon bonds or redemption premiums as par bills. Floating/optional helpers
need their owning complete pricing models and are rejected here.

## Distinct government, issuer, credit and funding roles

Government and Repo fits produce ACT/365F continuous-zero grids with Linear
interpolation. Government curves carry an exact benchmark identity; Repo curves
carry an exact collateral/funding definition. Government cash can be priced
through `convex::bond_contract::price_fixed_government`; collateral discount
roles remain separately checked by the existing plain bond route.

IssuerSpread curves contain signed annualized residual basis nodes under a
named pricing model. `BuiltCurve::issuer_spread`/Raw queries return the basis.
These curves cannot supply discount factors, probabilities, hazards or CDS
spreads. Direct Linear basis grids support explicit None, raw Flat and raw Linear
extension. A raw grid's construction alone does not price a bond.

`price_basis_bond` admits two explicit models:

| Model ID and contract | Actual settlement valuation |
|---|---|
| `reference_zero_spread_v1` / ReferenceZeroSpread | Exact same-currency named government or collateral discount reference. Promised cash uses reference discount multiplied by exp(-s(t) times t), normalized by that combined factor at settlement. No survival/recovery claim is made. |
| `credit_par_recovery_promised_basis_v1` / CreditParRecovery | Exact collateral discount, CDS/supplied survival, legal entity, seniority, currency and recovery. Surviving promised cash uses discount times survival times exp(-s(t) times t), normalized at settlement. Immediate default recovery of current par principal is a separate settlement PV. Residual basis affects promised cash only. |

The credit model deliberately excludes coupon recovery and accrued recovery.
It shares the credit engine's exact discounted default-mass moment over
log-linear DF/survival segments, split at actual principal profile changes and
both curve knot sets. Recovery excludes the residual basis factor. Discount and
survival must have a common reference date, ACT/365F, LogLinear DF/survival
representation and no extrapolation. Principal effective and payment dates
must coincide; delayed principal/default/lifecycle state is unsupported.
This is a declared recovery-of-par residual model, not a general credit-bond
engine or an implied ISDA bond standard. A survival dependency may be supplied
or built by the existing complete CDS calibration before this atomic cash batch.

## Calibration, dependencies and rebuilds

The entire graph rejects duplicate immutable curve IDs, overwrites of supplied
IDs, missing exact versions, cycles, unused aliases/dependencies and mismatched
models/financial roles. Every full original bond is prepared before a solver;
credit model metadata and delayed principal compatibility are also preflighted.
Deterministic identity ordering resolves independent roots before dependents.
A helper pillar must cover its actual last consumed payment, including delays
and business adjustment beyond unadjusted maturity. Parameter grids cannot
truncate an original helper.

Piecewise fitting requires one original helper per actual pillar, Reject overlap
and a finite closed parameter bracket with 1..256 Brent iterations. Linear
interpolation allows causal earlier helpers to retain their full pricing.
Brent terminates against that helper's original native quote residual tolerance.

Global fitting uses the shared bounded scaled least-squares solver. Explicit
starts, brackets, positive native scales/weights, numerical rank controls and
objective/iteration budgets are required. Source-order scales/weights stay
paired with their original observations before canonical objective ordering.
RetainAllGlobal preserves every overlapping original helper; inconsistent
quotes or deficient identification fail. `GlobalFitSettings.initial_zeros`,
`lower_zero` and `upper_zero` are annualized residual-basis parameters for
IssuerSpreads and continuous zeros for Government/Repo.

Every original helper is repriced in its unchanged native unit after fitting.
No accepted approximate or partial curve set is returned. Outputs retain exact
recipes/dependencies, solved parameters, original settlement/model cash,
separate promised/recovery PV, native residuals, actual solver iterations/rank
and measured helper valuations. Unsupported interpolation/representations,
invalid domains, brackets, rank, iterations, residuals and work limits fail with
typed errors. None/Flat/Linear extrapolation is explicit; no method is substituted.

`CashBondHelper::with_quote` replaces the observation and retains original
cash terms; a par replacement updates its uniform coupon profile. Ingestion
assigns the new quote/output/dependent revisions. Build the complete required
subgraph from immutable original recipes and exact dependencies, then publish
only the successful whole result. Rebuild tests exercise both government quote
propagation and CDS quote-to-credit-to-basis propagation while preserving
original terms, recovery assumptions and the caller's last-good set.

## Actual dated repo and carry

`repo_rate_over_period` requires the exact Repo/collateral/currency and returns
P(start)/P(end), actual year fraction and period-equivalent annualized simple
rate on the caller's stated quotation basis. It never substitutes a government
or collateral discount curve. `price_repo_bond_forward` accepts a full observed
nominal bullet purchase, an actual pinned business delivery date, explicit
haircut, separate own-cash funding iff the haircut is nonzero, and an explicit
HeldCash or separately funded coupon reinvestment policy.

The modeled dirty forward equals actual dirty spot plus repo cost on the
(1-haircut) share plus own-cash cost on the haircut share minus each actual
received coupon's delivery value. Repo and own-cash use purchase-to-delivery
factors. Each coupon uses its own payment-to-delivery factor. Delivery accrued
comes from the complete original contract; clean forward subtracts that accrued.
Growth factors retain negative repo/funding economics. Reported carry rates use
an explicit ACT/360 simple-equivalent view; the actual curve factors determine
cash costs. `futures_price_points` divides clean forward by an actually supplied
conversion factor. Fixed-date carry excludes optional delivery, CTD selection,
variation-margin funding, positions, realized cash and executable proposals.

The supported carry route requires post-payment purchase, fixed nominal bullet
principal, no ex-coupon term, no principal-price override, delivery strictly
before maturity and no par-coupon spot observation. Stepped fixed coupons remain
complete. Other security/collateral, amortizing forward, special-collateral and
lifecycle conventions require additional explicit models. Existing CVX-023
basket/cash workflows remain available: tests reproduce every original dated
cash-futures alternative with repo structures that match each actual funding
and coupon horizon, retaining their unchanged original expectations.

## Bounds and evidence

One batch admits at most 128 total original helpers and 128 parameters,
256 curves including supplied dependencies and 16,384 actual helper valuations.
Profiles/principal inputs are bounded at 4,096 before copies, root budgets at
256 and credit integration points at 4,096. Existing schedule, curve/date and
shared solver bounds still apply. Finite annualized parameter brackets stay in
[-10,10]. These are termination/resource controls; they are not a latency SLA.

The independent standard-library Decimal(60) builder derives dated cash,
accrued, par, yield, zeros, recovery and carry from explicit inputs without
importing or executing native Convex. Its declared tolerances precede candidate
numeric comparisons. Original existing numerical pins, external captures,
defaults and all mandatory library requirements remain unchanged. The fixture,
reference builder, focused tests and facade consumer are additive engineering
evidence. Independent human quantitative approval, complete market/convention/
model packs, canonical risk, lifecycle/P&L, host/schema dispatch, distributed
publication, production/library approval and latency certification remain
separate owning work.


Complete original product graph capture, fitted-output/solver/helper diagnostic
comparison and original quote-risk replay are documented in
[the schema-3 replay contract](CALIBRATION-SNAPSHOT-REPLAY.md). Pure native calculations remain provider/storage/MQ/clock neutral.
Existing schema1/2 identities and original numerical controls remain; CVX-063
and all mandatory library/market/model/host/production/latency certificates stay open.

Full-product exponential UFR/speed fitting over exact immutable analytic bases, required one-sided joined forwards, original quote-risk and schema3 replay use [the explicit UFR product contract](PRODUCT-UFR-CALIBRATION.md). Complete Helpers recipes and actual mixed-unit parameters/diagnostics are retained. No sampled objective, regulatory procedure or unsupported base is substituted.

Full actual-security G-spread-to-price is available through the additive
`government_spread_price` shared operation. It retains complete dated target
cash, independent reference/spread-quote revisions, every actual benchmark
inverse, signed consideration and separate price/spread roundtrip bounds.
See [the full inverse contract](GOVERNMENT-SPREAD-INVERSE.md). Original removed
convenience proxies remain absent; market/Street and unavailable host
qualification stay open. CVX-063 remains partial and uncertified.
