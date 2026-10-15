# Supplied term coupon options

`convex::library::LibraryOperation::TermCouponOptions` prices explicitly bounded
term coupon **segments**. Each original reset, notional, coupon, spread, gearing,
cap and floor boundary is retained. The payoff on each segment is
`min(cap, max(floor, gearing * index + coupon + spread))`, omitting absent bounds.
This operation does not interpret the bounds as a cap on the sum of a coupon's
multiple reset rates.

Supply the complete checked bond terms and calendar revisions, settlement and
fixing context, actual fixing history and publication times, a separate index
projection curve and collateral discount curve, and `TermCouponOptionInputs`.
Projection is optional only when every required fixing is already known. All
missing published history fails before forecasting.

For each future observation/payment pair, supply a `TermCouponModelPoint` with
source, revision and information cutoff. `normal` uses annual decimal rate
volatility; `shifted_lognormal` uses annual relative volatility and a nonnegative
decimal displacement. Both use ACT/365F observation expiry. The supplied forward
adjustment explicitly converts the projected forward to its payment-measure
mean; the library does not infer that adjustment. Negative gearing reverses the
option side. A zero-variance strike kink reports unavailable derivatives.

Published fixings use exact Decimal cash arithmetic and contractual rounding at
the coupon payment boundary. Future stochastic cash requires `Unrounded`:
rounding its expectation would not price the expectation of rounded cash.
`NegativeCouponPolicy::Reject` requires a compatible payoff support; it never
adds an implicit zero floor. Pending-publication uncertainty needs an additional
conditional model and fails explicitly.

The result retains the complete terms, context, options, actual rate lineage,
segment dates, expected currency cash and model derivatives. PV includes actual
principal repayments and entitled coupon cash. Dirty price and model accrued
use the original face; clean price subtracts that same model accrued. Model
expectations and accrued are not payment records or ledger entries.

The pure leaf functions live in `convex_bonds::options::floating_coupon`; full
term composition lives in `convex_analytics::floating_coupon`. The shared
dispatcher only resolves supplied resources and preserves native errors.

Whole-coupon and overnight options, callable joint models, stochastic cash
rounding, calibration, inverse quotations and complete book risk/lifecycle
qualification remain required work. This scoped implementation does not certify
those original capabilities or replace independent convention/model approval.

## Whole-coupon Gaussian law

Supply `options.whole_coupon` to apply each bound to the entire coupon. The
original segment behavior is retained when that field is omitted. For coupon
segments with weights `w = notional * year_fraction`, the effective coupon rate
is `sum(w * (gearing * index + coupon + spread)) / sum(w)`. Cap and floor must
be constant over that coupon; stepped bounds with this scope are rejected.

Each future observation/payment row supplies terminal decimal-rate loadings on
common independent unit normal factors and an explicit payment-measure mean
adjustment. Rows for one coupon share the same factor dimension (1–16). Their
covariance is the factor Gram matrix. The library aggregates signed cash
loadings before pricing the whole coupon, retaining correlation and offsetting
gearing. Loadings already include the actual remaining expiry; no volatility
clock, independence, correlation or convexity adjustment is inferred. All rows
retain their source, positive revision and information cutoff. Missing,
duplicate, future-information and unused rows fail.

`expected_coupon_pro_rata_notional_accrual` is an explicitly selected model
accrued policy: expected bounded coupon cash times elapsed notional-accrual
weight divided by full weight. It does not assert a market accrued convention.
Known coupons retain exact Decimal bounds and payment rounding; future rounded
cash still fails. Results expose the whole cash mean, common cash loadings,
standard deviation, bounded expectation and complete original segment proof.
A sum of shifted-lognormal rates, overnight or callable/path law requires its
own model. This supplied normal law does not substitute for those requirements.

## Graph sensitivities and hedge columns

`term_coupon_graph_risk` accepts the same complete coupon input, an original
calibration graph, a checked quote-risk key, signed position, snapshot and
central/half-step policy. Preparation freezes the original publication decisions,
history and model revisions. Each of the five valuations queries the actual
scenario projection and discount curves and reprices the full option law.
Baseline admission binds complete immutable native curve states and dependencies.

Results retain signed monetary risk and all five complete coupon valuations,
with graph build order, native helper repricing residuals and evaluation counts.
Known fixings stay fixed while permitted future forwards change. Original
reference face and currency bind position scaling once; missing inputs, wrong
roles/units/face, unstable bumps or failed pricing return no accepted risk.

`term_coupon_hedge_risk` accepts the existing `GraphHedgeRiskInput`. It computes
all requested factors, including actual zero exposures, and returns the existing
`HedgeRiskVector` plus the full native baseline and factor valuations. That
vector can enter `aggregate_hedge_risk` or the constrained hedge solver with
explicit current/pending quantities, factor policies and application identities.
A calculated vector supplies no fill, cash-posting, limit or order authority.

These operations freeze supplied model assumptions. They do not calculate
model-parameter risk or model recalibration, nor add the coupon models to every
mixed-book scenario/lifecycle path. Those integrations remain required. Existing
HW1F/OAS lattice resolution and risk-refinement guards retain their own owner.

### Supplied model-parameter scenarios

`PreparedTermCouponModel::model_scenario` and the shared
`term_coupon_model_scenario` operation move one explicitly identified original
model parameter. The coordinate includes index, observation date, payment date,
source identity and source revision. Normal annual decimal-rate volatility,
shifted-lognormal annual relative volatility, decimal-rate displacement and one
terminal normal-factor loading have distinct selectors and units.

The result retains the full original and hypothetical valuations, the original
parameter, the supplied move and its resulting value. It reuses the original
curves, complete terms, calendars, profile steps, history, publication decisions,
information cutoff and every other model parameter. It changes no market
publication or source revision. Wrong coordinates, incompatible model families,
absent factors, nonfinite moves and invalid resulting model domains return errors.
The prepared view retains the actual immutable native curve states described
below; a different curve state cannot replace the original baseline.

Run `cargo run --locked -p convex --example coupon_model_scenarios`. Six
independently declared full-contract scenario values use the original absolute
currency-PV bound of `1e-8`; 21 rejection cases cover identity, model, history,
publication, domain and strict-decoding failures. This operation does not supply
calibration, inverse valuation, canonical parameter-risk units or mixed-book
lifecycle integration. Those original requirements remain open.

### Exact original curve state

`BuiltCurve::same_valuation_state` compares the full validated recipe,
construction diagnostics, actual fitted grid or parameters and the complete
dependency states. Derived spread/equivalent/rolled views, segmented and delegated
sources, UFR joins, parametric models and native grids retain their original
query, domain and tail policies. Independently rebuilt identical states compare
equal. Matching recipes or selected prices cannot hide changed fitted values or
dependency states. Comparison visits bounded pairs of original native states;
it samples no query points and creates no replacement grid.

Prepared coupon models retain immutable curve references and use this comparison
for baseline admission. The full original curve functions still own every
valuation.26 independently declared full-contract scenarios cover13 equivalent
flat curve families, including actual UFR-tail and rolled/delegated/segmented
queries. Native controls distinguish changed actual fitted grids/parameters
under identical product recipes and original repricing diagnostics.34 complete
shared requests retain source/version/domain/history/model failures.

Run `cargo run --locked -p convex --example coupon_curve_state` and
`cargo test --locked -p convex-curves --test curve_state`. Curve-family admission
does not add a new graph-risk coordinate, calibrate a model, or complete the
remaining optional-bond and mixed-book lifecycle requirements.

### Native parameter derivatives

`PreparedTermCouponModel::parameter_derivatives` and the shared
`term_coupon_parameter_derivatives` operation evaluate the complete native
contract at `0`, `+h`, `-h`, `+h/2` and `-h/2` of one exact supplied parameter.
All five full valuations, the original parameter and source/point/revision,
first and second currency-PV derivatives and their half-step values are retained.
Units follow the selected annual normal volatility, annual relative volatility,
displacement or terminal factor loading; these results are not rate PV01.

Supply the native-unit bump and separate absolute/relative first and second
refinement policies. Both policies must pass. Actual parameter-domain failure,
including a central move crossing a volatility boundary, aborts the result;
parameters are not clipped. A conservative binary64 cancellation floor must
fit inside each declared derivative allowance. Both arithmetic floors are
reported; they do not certify model or pricer error. Unresolved PV cancellation
fails even if the two finite differences agree. An unstable derivative returns a typed numerical
failure with its order, full/half values and declared allowance. Constant collars
retain genuine zero derivatives. All original curves, terms, cash/profile/reset
steps, known fixing revisions, publication decisions and other model fields stay
fixed. No extra publication callback or market revision is introduced.

Run `cargo run --locked -p convex --example coupon_parameter_derivatives`.
Independent Decimal100 equations declare five complete parameter cases; an
exact constant-collar control adds genuine zero risk. PV/first/second comparison
bounds of `1e-8`, `2e-7` and `0.002` were declared before native execution. The
second bound accounts for cancellation in full currency PVs at the declared
bumps. Each caller's refinement policy remains independently explicit.26shared
requests include6financial and20typed failure controls.

This route supplies native central derivatives. Canonical source/parameter
hedge-factor bindings, calibration, inverse valuation, one-sided boundary risk
and complete mixed-book lifecycle integration remain subsequent requirements.
