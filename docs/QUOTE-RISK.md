# Signed quote risk and portfolio coverage

This document covers quote-level risk (PV change per market quote) for bonds,
IRS/OIS, CDS and futures, and how it is aggregated across a portfolio. The code
is in `convex::quote_risk` and `convex_portfolio::signed_risk`. Both are pure
libraries. They use the existing bond, IRS/OIS, CDS, futures and curve
workflows over immutable data you pass in, with no database, broker, provider,
server or clock.

```sh
cargo run --locked -p convex --example quote_risk
cargo run --locked -p convex-portfolio --example signed_risk
cargo test --locked -p convex-analytics --test quote_risk
cargo test --locked -p convex-portfolio --test signed_risk
python3 tests/scripts/quote_risk_reference.py check
```

The first example is a stepped, amortizing bond.

## Signs, units and quantity

For an annualized rate or spread coordinate, the reference calculation reports
`(PV(q+h)-PV(q-h))/(2*h)*0.0001` in currency per **+1bp**. `QuoteSensitivity`
keeps the positive canonical bump `h`, the units, the Central method, the five
PVs, the half-bump result and the numerical policy revision. To apply a move
already measured in bp, multiply this value directly by that move.
`dv01_loss()` is the negative of the signed interest-rate PV01.

Other coordinates:

- CDS upfront and recovery return a monetary derivative per **+1 fraction**.
- Hazard returns a derivative per **+1 annualized decimal rate**.
- Cash and futures price delta is currency per **+1 price point**.
- A futures calibration-rate +1bp move decreases its `100-100*rate` quote by
  0.01 points. This differs from the positive futures price delta.

Clean and dirty cash and upfront inputs are separate types. No extra multiplier
or bp conversion is inferred.

`RiskPosition` specifies:

- a signed quantity;
- the positive reference size already contained in the PV;
- the currency;
- the units: Notional, Contracts or TradeCopies.

The engine scales once by their Decimal ratio. The economic side stays inside
the reference trade, and the quantity is relative to that side. A negative
quantity reverses it. Futures counts must be integers. A per100 price must
first become an absolute currency PV using the face. It cannot be returned as a
currency PV. Reference callbacks return checked `ModelMoney`, and a wrong
currency fails.

## Keys and coordinates

`QuoteRiskKey::new` checks a `RiskFactorKey` plus:

- the curve and quote revisions;
- the helper or grid coordinate, and the native unit;
- the monetary normalization;
- the FrozenCurves or RebuildDependents response.

Every dimension takes part in equality and hashing. These cannot silently share
an aggregate key:

- market-quote and node risk;
- yield and bank-discount yield;
- deposit, FRA, swap/OIS par, basis-swap spread and bond par coupon;
- repo, rate-future implied rate, Z spread, OAS and residual basis;
- CDS running, clean-upfront and dirty-upfront;
- hazard and recovery.

Convention, model and bump semantics and snapshot pins come from the inputs you
pass in. A tenor label is descriptive. Helper IDs and grid indices identify the
bumped economics. No interpolation to a tenor proxy and no provider-name
mapping is done. Parallel quote coordinates keep the sorted ID, version and
unit set. Rate quotes in mixed decimal, percent and bp units get the same
annualized move through their own unit conversions.

The core taxonomy adds variants, and existing serialized variants and DV01/KRD
meanings are kept. Rust code that matches those enums exhaustively must handle
the new variants. The new risk JSON is a separate Rust contract. It adds no v1
C, Excel, Java, MCP or WASM dispatch and no generated host schemas yet.

## Rebuilding and reference pricing

`prepare_quote_risk_graph` combines `RateRiskCurveInput`, `CdsCurveBuildInput`
and `CashCurveBuildInput`, curves you supply, the history and the numerical
controls. Rate helper revisions are extra snapshot pins, because the older rate
recipes lack that field. Topology, duplicate identities, output overwrites and
missing dependencies are checked before fitting. All helpers build through
their complete builders, and the piecewise and global controls are unchanged.
Administrator publication decisions are frozen during preparation and reused for
every shock. New requirements outside that set fail.

`evaluate_quote_graph` changes the native quotes, including uniform quoted
swap, basis and par-bond economics, and rebuilds the graph in dependency order.
All helpers must reprice within their tolerances. The result keeps the build
order, repricing, helper evaluation counts and consumed publications.

- Government shocks rebuild issuer basis.
- CDS shocks rebuild credit and credit-linked cash basis.
- Recovery is separate. It updates the CDS legal assumptions and the named
  par-recovery basis model when dependencies rebuild.

Scenario curves keep their keys **inside the hypothetical calculation**. Their
changed values are not published observation revisions or new live snapshots.
Source revisions and changed diagnostic quotes stay visible. Ingestion must
assign fresh immutable market and curve pins before publication. Inputs and
last good sets are unchanged when a shock fails.

`curve_quote_sensitivity` runs the same reference trade against each rebuilt
curve set. Your pricing function must keep the economic terms, the
settlement, history and state, the model parameters, the reference size and the
monetary units. Trade-specific recovery must bind to the scenario's credit
assumption. The calculation does not infer fixings, models, calendars or
reference mappings. Full scenarios are also available for diagnostics and
reusable trade repricing.

Frozen node risk uses the continuous-zero or DF grid. DF shocks multiply each
selected value by `exp(-move*time)`. Residual basis uses its raw basis grid.
Hazard shocks change the forward intervals of a log-linear survival grid, hold
recovery separate, and refuse negative intensity. Frozen recovery holds survival
probabilities fixed. These diagnostics do not reprice the original quotes.

Shocks to nodes you supplied can rebuild dependent recipes. Shocks to
calibrated nodes that ask for dependent rebuilding fail, and you should use
market-quote recalibration for those risks. No representation is resampled or
silently replaced with another interpolation or model.

`reference_quote_sensitivity` is the lower-level callback route for yield, Z,
cash and futures quotations and explicit reference scenarios. Any valuation
failure or half-step stability failure aborts the result.

Optional OAS must use `optional_oas_quote_sensitivity`. It needs HW1F
resolution and separate duration and convexity refinement at both h and h/2,
and it still rejects undersampled bumps. Generic price-only OAS callbacks are
refused. Other optional model and curve scenarios must also pass the risk
checks of their own pricer.

`reconcile_quote_buckets` needs the complete declared quote set and a truly
simultaneous parallel repricing. `reconcile_curve_node_buckets` also checks
every informative node against the prepared grid. Missing or duplicate indices
fail even if their sensitivity is zero. Both use monetary discrepancy
tolerances that the caller supplies. A failed reconciliation stays failed and
must not be treated as a valid additive approximation.

## Portfolio and FX

`aggregate_signed_quote_risk` takes the complete expected trade manifest, the
required factor keys and Present/Missing outcomes. An absent required outcome
is Missing. It cannot discover trades missing from your manifest. These fail:

- duplicate or unrequested factors;
- mixed snapshots, positions or PVs;
- invalid numerical records;
- unauthorized entity scope.

One entity is explicit. Multi-entity reporting needs your permitted list and
the authorization and configuration revision. The calculation does not grant
authority.

Per-currency coverage keeps the expected and covered trades and factor pairs,
the known gross exposure, the covered known gross exposure and the count of
unknown exposures. Unknown exposure stays unknown. Local and reporting sums
keep partial values as `known_value`, and all-missing contributions stay None.
Incomplete rows name their missing trades. Gross exposure is a nonnegative
currency metric that the caller defines. It is not absolute net PV or a hidden
duration weight.

Positive same-snapshot `RiskFxPin` observations convert monetary values once.
Local values survive missing FX while the reporting result is Incomplete.
`RiskFxConversionExposure` keeps each FX pin and
`d(reporting sensitivity)/d(FX)=local sensitivity`. This is the conversion
exposure induced on a risk result. It is **not** the portfolio's full FX delta
or a cross-currency hedge or execution model. Quote-factor currency stays in its
key.

## Limits and tests

Limits:

- Risk graphs take at most 64 full recipes, 128 helpers and 256 curve outputs
  and supplied identities, within the existing family calibration, grid,
  history, cash and solver bounds.
- Publication manifests take 32,768 unique decisions.
- Each central result needs a baseline, +/-h and +/-h/2 valuations. Each
  scenario has bounded calibration work.
- Portfolios take 4,096 trades, 128 required factors per trade, 65,536 total
  trade-factor pairs and 256 FX observations.
- Numeric overflow fails.

This is a reference batch calculation. There is no latency or memory claim, and
nothing here implies AAD, caching or live execution.

Decimal(60) references for stepped and amortizing cash, plus Rust checks for
cross-product, quote and node, recovery, bucket and parallel, quantity, fixing
and failure cases, back this. Existing price and risk expectations, captures
and solver defaults are unchanged. Unsupported model inputs and conservative
resolution limits remain errors.

Not yet covered: independent quantitative and market/model validation, host
coverage, lifecycle and P&L, deployment and latency.

ORE separates sensitivity scenario generation by market-factor type and runs
valuation through an explicit analysis workflow. Reading these ORE sources
informed how factors and scenario composition are separated here. We have not
run a comparison against ORE and do not reproduce every ORE model.
[ORE sensitivity workflow](https://github.com/OpenSourceRisk/Engine/blob/master/OREAnalytics/orea/engine/sensitivityanalysis.cpp),
[ORE factor scenario generation](https://github.com/OpenSourceRisk/Engine/blob/master/OREAnalytics/orea/scenario/sensitivityscenariogenerator.cpp).

## Related documents

- Product graph capture, comparison of fitted output, solver and helper
  diagnostics, and quote-risk replay are in
  [the schema-3 replay contract](CALIBRATION-SNAPSHOT-REPLAY.md). The
  calculations here stay independent of providers, storage, message queues and
  clocks. Schema 1 and 2 identities and numerical controls are unchanged.
- Exponential UFR and speed fitting over exact analytic bases, required
  one-sided joined forwards, quote risk and schema 3 replay are described in
  [the UFR product contract](PRODUCT-UFR-CALIBRATION.md). Complete `Helpers`
  recipes and mixed-unit parameters and diagnostics are kept. No sampled
  objective, regulatory procedure or unsupported base is substituted.
- Risk vectors compose through the pure
  [constrained hedge proposal operation](CONSTRAINED-HEDGES.md). It keeps the
  key, bump, model and reference semantics and the central and half-step PVs,
  with current and pending exposure, native quantity and legal credit and FX
  checks. Bounded constrained and lot searches return the measured residual and
  cost, constraint, rank and work diagnostics. They carry no execution
  authority.
- ICMA yield cash helpers compose through the full graph and five-PV risk code.
  Units, policy version, bound calendars, cash and dependent recipes survive each
  rebuild. Per-build work ceilings and full and half-bump stability are still
  enforced. See [the helper contract](ICMA-CASH-HELPERS.md).
- Own-security ICMA yield sensitivity and positive reference hedge columns use
  one prepared cash and time context and the five-PV code. See
  [ICMA yield risk](ICMA-YIELD-RISK.md). Raw yield keeps its own axis.

## Common-factor hedge columns

The `graph_hedge_risk` operation composes the product and graph revaluation
code into columns for bonds, CDS, IRS/OIS and rates futures, for the existing
hedge aggregator and proposal solver. See
[quantity binding, inputs, controls and limits](GRAPH-HEDGE-RISK.md).
Own-security ICMA yield is a separate factor. Optional bonds are covered in
[OPTIONAL-GRAPH-HEDGE-RISK.md](OPTIONAL-GRAPH-HEDGE-RISK.md). Credit-survival
cash products and all-member delivery-futures risk are not covered.

Credit-survival and issuer-basis cash use the separate `cash_basis_hedge_risk`
route. See [cash basis hedge risk](CASH-BASIS-HEDGE-RISK.md) for the security
and mapping contract, the ICMA and rebuild controls, the mixed-book scope and
what remains.
