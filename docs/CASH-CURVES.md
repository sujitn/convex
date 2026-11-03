# Cash curves and dated repo carry

`convex::cash_calibration::build_cash_curve_set` fits Government, IssuerSpread
and Repo curves from cash instruments: bonds, bills, repo loans and par-coupon
quotes. It builds on `BondTerms`/`BondContract` schedules, settlement, coupon
entitlement and accrued interest, validated curve contracts, CDS survival
inputs, and the shared bounded root and least-squares solvers.

The calculation is pure. The caller resolves the immutable inputs and calendars
and passes them in. Data providers, storage, messaging, clocks and publication
stay outside the analytics code.

"Validated" in this document means the function checks its input and returns an
error instead of guessing.

[Product-price parametric fitting](PRODUCT-PARAMETRIC-CALIBRATION.md) adds an
explicit Nelson-Siegel/Svensson continuous-zero mode for Government and Repo
curves. The signed issuer residual basis and the credit recovery models are
covered in their own documents.

Run the standalone example and the independent reference check:

```sh
cargo run --locked -p convex --example cash_calibration
cargo run --locked -p convex --example cash_calibration --no-default-features
cargo run --locked -p convex --example cash_calibration --features schemars
python3 tests/scripts/cash_calibration_reference.py check
cargo test --locked -p convex-analytics --test cash_calibration
```

`reprice_cash_helpers` reprices one complete cash recipe on the curves you give
it, without fitting or replacing them. It uses the same contract and model
validation and the same quote conversion as the fit. It returns the full
valuations, revisions, residuals and work counts in source order. A finite
result that is off-market has `within_tolerance:false`; it does not count as an
accepted calibration. The
[shared cash-helper route](LIBRARY-API.md#direct-cash-helper-repricing-diagnostics)
adds strict full-term JSON/C input and the existing client dispatch.

The example fits non-flat positive and negative government, residual-basis and
repo curves, and prices dated carry. It also fits a separate credit-bond basis
against a six-quote CDS calibration. It checks independent cash, profile, date
and parameter references, and prints the quote residuals in each quote's own
unit and the number of helper valuations.

The bond and carry observations are synthetic. The credit dependency uses the
ISDA comparison input and capture already in the repository. The synthetic bond
model has not been validated against a market implementation.

## Helper economics and quotations

Each `CashCurveRecipe` holds:

- an exact `CurveRecipe`;
- strictly increasing parameter dates;
- every helper in source order;
- each quote's unit and absolute tolerance;
- the bootstrap, model and overlap choices.

`CashCurveBuildInput` passes matching `CashHelperResources` in the same order.
A bond helper contains the full `BondTerms`, the trade and settlement dates,
coupon entitlement before or after the payment, the quote revision, and the last
relevant cash pillar. Calendar identities and versions must match the terms, and
the calendar must produce the stated settlement date from the trade date, lag
and adjustment.

| Observation | Conversion and model residual |
|---|---|
| CleanPrice | PricePer100 of original face; dirty equals clean plus accrued/rebate. |
| DirtyPrice | PricePer100 of original face; clean equals dirty minus accrued/rebate. |
| CashflowYield | Decimal/percent/basis-point annualized yield with explicit dated-cash day count, compounding, inverse bracket, work budget and dirty-price residual bound. Each entitled coupon and principal date is discounted. |
| ContextualIcmaYield | Explicit source, nominal-mesh, frequency, compounding, inverse, quantity and work settings, passed through the same ICMA cash code. Quote units are kept as given; the model inverse and diagnostics are bounded. See [ICMA cash helpers](ICMA-CASH-HELPERS.md). |
| BankDiscountYield | Explicit simple discount basis and decimal/percent/basis-point quote; one future par principal, zero coupons and accrued. Dirty price equals 100 times (1 minus discount yield times year fraction). |
| ParCouponRate | Uniform decimal/percent/basis-point coupon of a full, unrounded nominal par bond you pass in. Clean price is 100; the unit and zero-coupon cash exposures determine the implied coupon. The observation is never a spot-zero node. |
| RepoLoanHelper | Business start and end dates, pinned calendar, collateral definition and interest basis. A simple annualized decimal/percent/basis-point loan rate reprices from the exact repo discount ratio. |

Prices are normalized by original face. This includes stepped or amortizing
principal, redemption premiums and dated principal cash passed through
`BondContract`. Fixed coupon profiles are not averaged. Fixed helper terms
serialize their Decimal amounts and profiles as strings, so JSON roundtrips are
exact and existing host DTOs do not change. Strict tags reject fields that do
not apply. Deserialize, then build and validate, before using any input.

CashflowYield is a dated-cash convention. It does not implement every street,
ICMA, exchange or national bond yield convention. ICMA yield fractions need the
separate ContextualIcmaYield settings. ICMA coupon accrual is still available
through the bond schedule.

Negative yields are supported when their discount domain is valid. The monotone
yield inverse rejects negative entitled cash, while price and par helpers can
keep negative coupons. Bracket endpoints must be valid for every cash date. A
trial price outside the inverse bracket fails; the bracket is not widened.

Par helpers need these inputs:

- a uniform coupon profile that matches the quote;
- unrounded cash that allows negative values;
- nominal bullet principal;
- no explicit principal-price overrides.

Zero and unit coupon projections reuse the same schedules, entitled dates and
accrued interest. Bank-discount helpers cannot treat coupon bonds or redemption
premiums as par bills. Floating and optional bonds need their own pricing models
and are rejected here.

## Government, issuer, credit and funding roles

Government and Repo fits produce ACT/365F continuous-zero grids with Linear
interpolation. Government curves carry an exact benchmark identity. Repo curves
carry an exact collateral and funding definition. Government cash can be priced
with `convex::bond_contract::price_fixed_government`. Collateral discount roles
are validated separately by the existing plain bond route.

IssuerSpread curves contain signed annualized residual basis nodes under a named
pricing model. `BuiltCurve::issuer_spread` and Raw queries return the basis.
These curves cannot supply discount factors, probabilities, hazards or CDS
spreads. Direct Linear basis grids support explicit None, raw Flat and raw
Linear extension. Building a raw grid does not price a bond.

`price_basis_bond` accepts two explicit models:

| Model ID and contract | Valuation at settlement |
|---|---|
| `reference_zero_spread_v1` / ReferenceZeroSpread | Same-currency named government or collateral discount reference. Promised cash uses reference discount multiplied by exp(-s(t) times t), normalized by that combined factor at settlement. No survival or recovery is modeled. |
| `credit_par_recovery_promised_basis_v1` / CreditParRecovery | Collateral discount, CDS or survival curve, legal entity, seniority, currency and recovery. Surviving promised cash uses discount times survival times exp(-s(t) times t), normalized at settlement. Immediate default recovery of current par principal is a separate settlement PV. The residual basis affects promised cash only. |

The credit model deliberately leaves out coupon recovery and accrued recovery.
It uses the credit engine's discounted default-mass moment over log-linear
DF/survival segments, split at principal profile changes and at both curves'
knots. Recovery does not include the residual basis factor.

The discount and survival curves must have:

- a common reference date;
- ACT/365F time;
- LogLinear DF/survival representation;
- no extrapolation.

Principal effective and payment dates must coincide. Delayed principal, default
and lifecycle state are unsupported. This is a recovery-of-par residual model,
not a general credit-bond engine and not an ISDA bond standard. The survival
curve can be passed in or built by the CDS calibration before this cash batch.

## Calibration, dependencies and rebuilds

The whole graph rejects:

- duplicate immutable curve IDs;
- overwrites of IDs you passed in;
- missing exact versions;
- cycles;
- unused aliases or dependencies;
- mismatched models or financial roles.

Every full bond is prepared before a solver runs. Credit model metadata and
delayed principal compatibility are also checked up front. Independent roots are
solved before their dependents, in a deterministic order by identity.

A helper pillar must cover the last payment the helper uses, including payment
delays and business-day adjustment beyond unadjusted maturity. Parameter grids
cannot truncate a helper.

Piecewise fitting needs:

- one helper per pillar;
- Reject overlap;
- a finite closed parameter bracket with 1..256 Brent iterations.

Linear interpolation lets earlier helpers keep their full pricing. Brent uses
the quote's own residual tolerance. Contextual ICMA helpers also use their
explicit inverse annual parameter precision.

Global fitting uses the shared bounded scaled least-squares solver. It needs
explicit starts, brackets, positive scales and weights in each quote's unit,
numerical rank controls, and objective and iteration budgets. Scales and weights
stay paired with their observations in source order before the objective is put
in canonical order. RetainAllGlobal keeps every overlapping helper. Inconsistent
quotes or an under-identified fit fail. `GlobalFitSettings.initial_zeros`,
`lower_zero` and `upper_zero` are annualized residual-basis parameters for
IssuerSpreads and continuous zeros for Government and Repo.

After fitting, every helper is repriced in its own quote unit. No approximate or
partial curve set is returned. The output includes:

- the recipes and dependencies;
- the solved parameters;
- settlement and model cash;
- separate promised and recovery PV;
- residuals in each quote's unit;
- solver iterations and rank;
- the measured number of helper valuations.

Unsupported interpolation or representations, invalid domains, brackets, rank,
iterations, residuals and work limits fail with typed errors. None, Flat and
Linear extrapolation are explicit, and no method is substituted.

`CashBondHelper::with_quote` replaces the observation and keeps the cash terms.
A par replacement updates the uniform coupon profile. Ingestion assigns new
revisions to the quote, the output and the dependents. To rebuild, build the
whole required subgraph from the immutable recipes and exact dependencies, and
publish only the complete successful result. Rebuild tests cover government quote
propagation and CDS quote-to-credit-to-basis propagation. They check that the
terms, recovery assumptions and the caller's last good set are preserved.

## Dated repo and carry

`repo_rate_over_period` needs the exact Repo, collateral and currency. It returns
P(start)/P(end), the year fraction and the period-equivalent annualized simple
rate on the quotation basis you state. It never substitutes a government or
collateral discount curve.

`price_repo_bond_forward` takes:

- a purchase of a nominal bullet bond;
- a pinned business delivery date;
- an explicit haircut;
- separate own-cash funding when the haircut is nonzero;
- an explicit coupon reinvestment policy, either HeldCash or separately funded.

The modeled dirty forward equals:

- the dirty spot price, plus
- repo cost on the (1 - haircut) share, plus
- own-cash cost on the haircut share, minus
- each received coupon's value at delivery.

Repo and own-cash costs use purchase-to-delivery factors. Each coupon uses its
own payment-to-delivery factor. Delivery accrued comes from the full bond
contract, and the clean forward subtracts it. Growth factors keep negative repo
and funding economics. Reported carry rates are an ACT/360 simple-equivalent
view; the curve factors determine the cash costs.

`futures_price_points` divides the clean forward by a conversion factor you
supply. Fixed-date carry excludes optional delivery, CTD selection,
variation-margin funding, positions, realized cash and executable proposals.

The carry route requires:

- purchase after the payment;
- fixed nominal bullet principal;
- no ex-coupon term;
- no principal-price override;
- delivery strictly before maturity;
- no par-coupon spot observation.

Stepped fixed coupons are supported in full. Other securities and collateral,
amortizing forwards, special collateral and lifecycle conventions would need
additional explicit models. The existing basket/cash workflows remain
available. Their tests reproduce every dated cash-futures alternative, with repo
structures that match each funding and coupon horizon and with unchanged
expected results.

## Bounds and tests

One batch accepts at most:

- 128 helpers in total;
- 128 parameters;
- 256 curves, including dependencies you pass in;
- 16,384 helper valuations.

Profiles and principal inputs are bounded at 4,096 before any copies, root
budgets at 256 and credit integration points at 4,096. The existing schedule,
curve, date and shared solver bounds still apply. Finite annualized parameter
brackets stay in [-10,10]. These are termination and resource limits, not a
latency guarantee.

The independent reference is written in standard-library Python with Decimal(60)
arithmetic. It derives dated cash, accrued, par, yield, zeros, recovery and carry
from explicit inputs without importing or running Convex. Its tolerances were
declared before the candidate numbers were compared. Existing numerical pins,
external captures and defaults are unchanged. The fixture, reference script,
focused tests and example were added alongside them.

Not yet covered: independent review of the numerics, complete market,
convention and model packs, canonical risk, lifecycle and P&L, host and schema
dispatch, distributed publication, and latency measurement.

The [snapshot replay contract](CALIBRATION-SNAPSHOT-REPLAY.md) describes how to
capture a complete product graph, compare fitted outputs, solver and helper
diagnostics, and replay quote risk. The calculations stay independent of
providers, storage, message queues and clocks. Existing schema 1 and 2
identities and numerical controls are unchanged.

Exponential UFR/speed fitting over exact immutable analytic bases, with the
required one-sided joined forwards, quote risk and schema 3 replay, is covered by
[the UFR product contract](PRODUCT-UFR-CALIBRATION.md). Helpers recipes and
mixed-unit parameters and diagnostics are kept. No sampled objective, regulatory
procedure or unsupported base is substituted.

Government-spread-to-price is available through the `government_spread_price`
shared operation. It keeps the dated target cash, the reference and
spread-quote revisions, every benchmark inverse, the signed consideration, and
separate price and spread roundtrip bounds. See
[the inverse contract](GOVERNMENT-SPREAD-INVERSE.md). The removed convenience
proxies remain absent.

Credit-survival and issuer-basis cash have a separate `cash_basis_hedge_risk`
route. See [cash basis hedge risk](CASH-BASIS-HEDGE-RISK.md) for the
security, mapping and factor contract, ICMA and rebuild controls, mixed-book
scope, and what is not yet covered.
