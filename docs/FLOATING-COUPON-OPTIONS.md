# Term coupon options

`convex::library::LibraryOperation::TermCouponOptions` prices floating coupons
with caps and floors, one **segment** at a time. The pure functions are in
`convex_bonds::options::floating_coupon` and the full-term composition is in
`convex_analytics::floating_coupon`. Each reset, notional, coupon, spread, gearing,
cap and floor boundary is kept. The payoff on each segment is
`min(cap, max(floor, gearing * index + coupon + spread))`, omitting absent bounds.
This operation does not interpret the bounds as a cap on the sum of a coupon's
multiple reset rates.

Pass the bond terms and calendar revisions, the settlement and fixing context, the
fixing history with publication times, a separate index projection curve and
collateral discount curve, and `TermCouponOptionInputs`. Each input is validated.
The projection curve is optional only when every required fixing is already
known. Missing published history fails before any forecast.

For each future observation/payment pair, supply a `TermCouponModelPoint` with
source, revision and information cutoff. `normal` uses annual decimal rate
volatility; `shifted_lognormal` uses annual relative volatility and a nonnegative
decimal displacement. Both use ACT/365F observation expiry. The forward adjustment
you supply converts the projected forward to its payment-measure mean; the library
does not infer it. Negative gearing reverses the option side. A zero-variance
strike kink reports the derivatives as unavailable.

Published fixings use exact Decimal cash arithmetic and the contractual rounding
at the coupon payment boundary. Future stochastic cash requires `Unrounded`:
rounding its expectation would not price the expectation of rounded cash.
`NegativeCouponPolicy::Reject` requires a compatible payoff support; it never adds
an implicit zero floor. Uncertainty about a pending publication needs an
additional conditional model and fails explicitly.

The result keeps the terms, context, options, rate lineage, segment dates, expected
currency cash and model derivatives. PV includes principal repayments and entitled
coupon cash. Dirty price and model accrued use the face amount; clean price
subtracts the same model accrued. Model expectations and accrued are not payment
records or ledger entries.

The shared dispatcher only resolves the resources you pass in and preserves the
errors from the lower crates.

Not yet covered: overnight options, whole-coupon options beyond the normal law
below, callable joint models, stochastic cash rounding, calibration, inverse
quotations, and book-level risk and lifecycle. This implementation has not had an
independent convention or model review.

## Whole-coupon Gaussian law

Supply `options.whole_coupon` to apply each bound to the entire coupon. When the
field is omitted, the segment behavior above applies. For coupon
segments with weights `w = notional * year_fraction`, the effective coupon rate
is `sum(w * (gearing * index + coupon + spread)) / sum(w)`. Cap and floor must
be constant over that coupon; stepped bounds with this scope are rejected.

Each future observation/payment row supplies terminal decimal-rate loadings on
common independent unit normal factors and a payment-measure mean adjustment. Rows
for one coupon share the same factor dimension (1–16). Their covariance is the
factor Gram matrix. The library aggregates the signed cash loadings before pricing
the whole coupon, so correlation and offsetting gearing are kept. The loadings
already include the remaining expiry; no volatility clock, independence,
correlation or convexity adjustment is inferred. All rows keep their source,
positive revision and information cutoff. Missing, duplicate, future-information
and unused rows fail.

`expected_coupon_pro_rata_notional_accrual` is a model accrued policy you select:
expected bounded coupon cash times the elapsed notional-accrual weight divided by
the full weight. It does not assert a market accrued convention. Known coupons keep
exact Decimal bounds and payment rounding; future rounded cash still fails. Results
expose the whole-coupon cash mean, the common cash loadings, the standard
deviation, the bounded expectation and the segment-by-segment proof. A sum of
shifted-lognormal rates, an overnight law or a callable path law each needs its
own model; this normal law does not substitute for them.

## Graph sensitivities and hedge columns

`term_coupon_graph_risk` takes the same coupon input, a calibration graph, a
validated quote-risk key, a signed position, a snapshot and a central/half-step
policy. Preparation freezes the publication decisions, history and model
revisions. Each of the five valuations queries the scenario projection and
discount curves and reprices the full option law. The baseline check binds the
complete immutable curve states and dependencies.

Results keep the signed monetary risk and all five coupon valuations, with the
graph build order, the helper repricing residuals and the evaluation counts. Known
fixings stay fixed while permitted future forwards change. The reference face and
currency scale the position once. Missing inputs, wrong roles, units or face,
unstable bumps and failed pricing return no risk.

`term_coupon_hedge_risk` takes the existing `GraphHedgeRiskInput`. It computes all
requested factors, including zero exposures, and returns the `HedgeRiskVector`
plus the baseline and factor valuations. That vector can enter
`aggregate_hedge_risk` or the constrained hedge solver together with current and
pending quantities, factor policies and application identities. A calculated
vector gives no authority to fill, post cash, set limits or place orders.

These operations freeze the model assumptions. They do not calculate model-parameter
risk or recalibrate the model, and they are not yet part of every mixed-book
scenario or lifecycle path. The HW1F/OAS lattice resolution and risk-refinement
guards are unchanged.

### Model-parameter scenarios

`PreparedTermCouponModel::model_scenario` and the shared
`term_coupon_model_scenario` operation move one identified model parameter. The coordinate includes index, observation date, payment date,
source identity and source revision. Normal annual decimal-rate volatility,
shifted-lognormal annual relative volatility, decimal-rate displacement and one
terminal normal-factor loading have distinct selectors and units.

The result keeps the base and hypothetical valuations, the original parameter, the
move you supplied and the resulting value. It reuses the curves, terms, calendars,
profile steps, history, publication decisions, information cutoff and every other
model parameter, and changes no market publication or source revision. Wrong
coordinates, incompatible model families, absent factors, nonfinite moves and
invalid resulting model domains return errors. The prepared view keeps the
immutable curve states described below; a different curve state cannot replace the
baseline.

Run `cargo run --locked -p convex --example coupon_model_scenarios`. Six
independently declared full-contract scenario values use an absolute currency-PV
bound of `1e-8`; 21 rejection cases cover identity, model, history, publication,
domain and strict-decoding failures. This operation does not provide calibration,
inverse valuation, canonical parameter-risk units or mixed-book lifecycle
integration.

### Curve state comparison

`BuiltCurve::same_valuation_state` compares the validated recipe, the construction
diagnostics, the fitted grid or parameters and the complete dependency states.
Derived spread, equivalent and rolled views, segmented and delegated sources, UFR
joins, parametric models and native grids keep their query, domain and tail
policies. Identical states that were rebuilt independently compare equal. Matching
recipes or selected prices cannot hide changed fitted values or dependency states.
The comparison visits bounded pairs of states; it samples no query points and
creates no replacement grid.

Prepared coupon models keep immutable curve references and use this comparison for
the baseline check. The curve functions still do every valuation. 26 independently
declared full-contract scenarios cover 13 equivalent flat curve families, including
UFR-tail and rolled, delegated and segmented queries. Controls also distinguish
changed fitted grids or parameters under identical product recipes and repricing
diagnostics. 34 complete shared requests keep the source, version, domain, history
and model failures.

Run `cargo run --locked -p convex --example coupon_curve_state` and
`cargo test --locked -p convex-curves --test curve_state`. Accepting these curve
families does not add a graph-risk coordinate, calibrate a model, or finish the
optional-bond and mixed-book lifecycle work.

### Native parameter derivatives

`PreparedTermCouponModel::parameter_derivatives` and the shared
`term_coupon_parameter_derivatives` operation evaluate the complete
contract at `0`, `+h`, `-h`, `+h/2` and `-h/2` of one exact supplied parameter.
All five full valuations, the parameter with its source, point and revision, the
first and second currency-PV derivatives and their half-step values are kept.
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
bounds of `1e-8`, `2e-7` and `0.002` were declared before the code was run. The
second bound accounts for cancellation in full currency PVs at the declared
bumps. Each caller's refinement policy remains independently explicit. 26 shared
requests include 6 financial and 20 typed failure controls.

This route gives central derivatives. Not yet covered: hedge-factor bindings by
source and parameter, calibration, inverse valuation, one-sided boundary risk and
mixed-book lifecycle integration.

### Canonical parameter risk and complete hedge columns

`term_coupon_parameter_sensitivity` and the shared
`term_coupon_parameter_risk` operation retain the full derivative proof
and normalize those same five currency PVs for an explicitly signed position.
`coupon_parameter_key` accepts the exact typed index, observation date, payment
date, source, revision and parameter. It stores their complete canonical JSON
in the existing raw coordinate; `coupon_parameter_coordinate` recovers the
typed point. The representation is bounded to 1024 bytes and must match canonical
serialization exactly. No hashes, shortened labels or omitted fields are used.
The four quote types and their
units distinguish annual normal rate volatility, annual relative Black
volatility, decimal displacement and a terminal normal factor loading.
Parameter keys require model family, model-parameter basis, spot bucket and
frozen response. The model target is the original source identity and its
positive revision rendered as a decimal string. Convention and model semantics
remain caller-supplied pins; they are never provider lookups.

Supply both the native reference derivative policy and signed position risk
settings with the same bump. Both apply: native first/second refinement and
arithmetic resolution pass before signed reference-face scaling, then the
existing canonical h/h2 check applies to scaled PVs. Currency and the original
positive face must match. A zero holding still evaluates the derivative proof.
Generic quote callbacks and curve scenarios reject coupon parameter keys;
parameter units cannot be used as curve-helper market quotes.

`term_coupon_complete_hedge_risk` combines actual graph and parameter columns
in the supplied order and returns the existing `HedgeRiskVector` plus every
complete derivative proof. Each parameter needs one exact policy. Duplicate
economic points under different metadata, missing or unused policies and
insufficient five-price work reservations fail. The existing graph-only
`term_coupon_hedge_risk` route retains its scope. The constrained solver accepts
the resulting vector with explicit original holdings, pending quantities, lots,
costs and limits; its result remains a proposal without execution authority.

Run `cargo run --locked -p convex --example coupon_parameter_hedges`.
Twenty independent signed and zero parameter controls and a five-factor mixed
curve/model hedge control keep the full economics and the reference bounds. Not yet
covered: model calibration and inverse, mixed-book parameter scenarios and
lifecycle, one-sided boundary risk and the remaining optional conventions and
models.

### Full currency parameter price inverse

`PreparedTermCouponModel::parameter_price_inverse` and the shared
`term_coupon_parameter_inverse` operation select one parameter within an explicit
native-unit bracket using full original contract PV. Supply the original point,
same-currency `ModelMoney` target, absolute currency price tolerance, parameter
width tolerance and actual iteration/price-call ceilings. Parameter values are
assigned exactly; they are not reconstructed through a possibly cancelling delta.
Every price retains original terms, curves, histories, publication decisions and
all other model fields. The final complete valuation is freshly repriced.

Endpoint price-policy acceptance, exact native zero and interior price-plus-width
convergence are reported separately. Both distinct endpoints meeting the target
at the declared precision fail as non-identified. An unbracketed target returns
actual endpoint residuals; it does not prove absence of all roots of a potentially
nonmonotone payoff. The result explicitly makes no global uniqueness claim.
Arithmetic resolution, original model domains, work and final residual guards
cannot be bypassed by an accepted partial parameter or a relaxed retry. The
arithmetic guard does not certify model/pricer accuracy. Baseline, every actual
parameter/PV/residual evaluation, remaining bracket and full final model proof stay.

Run `cargo run --locked -p convex --example coupon_parameter_inverse`.
Seven independent full-contract targets, declared in advance, cover normal and Black
volatility, displacement, two whole-normal loadings and both endpoint directions.
PV 1e-8 and parameter 2e-7 comparison bounds were declared before the code was run.
Not yet covered: quote and surface calibration and global identification,
one-sided parameter risk, mixed-book parameter scenarios and lifecycle, and the
remaining models. This operation finds a single root in the bracket.

## Explicit directional parameter sensitivities

Use `prepared.parameter_directional_derivatives` with the full coordinate and
`CouponDirectionalDerivativeSettings`. Select forward or backward explicitly and
supply the first and second bump and refinement policy. The thin shared operation
is `term_coupon_directional_derivatives`. Six complete prices at signed multipliers
0, 0.5, 1, 1.5, 2 and 3 keep the parameters and the full economics, model, history,
publication and curve state. Every actual domain must pass;
the operation does not select a side, clip a point or invent an opposite price.

Both three-point first derivatives and four-point second derivatives compare
full and half steps. The backward first derivative still measures change per
increasing native parameter. Separate first/second cancellation guards must fit
the caller policy; they do not bound total model/pricer error. Every successful
result retains the six actual full prices, original side/settings/source units,
all derivatives and resolution diagnostics. A domain or precision failure has
no accepted partial sensitivity.

This dedicated directional result cannot populate the existing Central
QuoteSensitivity or hedge vectors by relabeling its samples. Directional
canonical aggregation/hedging and complete surface/model identification remain
separate work. The existing central derivative, risk, hedge and selected inverse
operations retain their original contracts.

## Signed directional parameter risk

`term_coupon_directional_parameter_sensitivity` binds the canonical coupon
parameter key to the face, signed quantity, currency and snapshot.
`CouponDirectionalRiskSettings` supplies the directional precision and separate
signed first and second `RiskTolerance` policies with a policy version. The shared
operation is `term_coupon_directional_parameter_risk`.

Both native refinement/resolution guards apply before position scaling, including
for zero holdings. Accepted native derivatives and all six full PVs use the same
single signed scale. Separate signed first/second refinements and scaled arithmetic
guards must pass. Results retain the exact key, position, snapshot, settings,
full derivative proof, scaled PVs and both derivative orders. Curvature explicitly
uses currency_per_model_parameter_squared. These arithmetic guards do not bound
total model/pricer error.

This dedicated directional result preserves its side and native parameter units.
Existing Central QuoteSensitivity and hedge vectors require their own actual
central proof; full directional hedge/aggregation integration remains separate.
