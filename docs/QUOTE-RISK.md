# Signed quote risk and portfolio coverage

CVX-024 exposes `convex::quote_risk` and
`convex_portfolio::signed_risk`. Both are pure libraries. They use the existing
complete bond, IRS/OIS, CDS, futures and curve workflows over caller-supplied
immutable data, with no database, broker, provider, server or clock.

Run the standalone full stepped/amortizing bond consumer:

```sh
cargo run --locked -p convex --example quote_risk
cargo run --locked -p convex-portfolio --example signed_risk
cargo test --locked -p convex-analytics --test quote_risk
cargo test --locked -p convex-portfolio --test signed_risk
python3 tests/scripts/quote_risk_reference.py check
```

## Monetary signs, units and quantity

For an annualized rate/spread coordinate, the reference calculation reports
`(PV(q+h)-PV(q-h))/(2*h)*0.0001` in currency per **+1bp**. The actual positive
canonical bump `h`, native units, Central method, five actual PVs, half-bump
result and numerical policy revision remain in `QuoteSensitivity`. Applying a
move already measured in bp means multiplying this value directly by that move.
`dv01_loss()` is the explicit negative of interest-rate signed PV01.

CDS upfront and recovery return a monetary derivative per **+1 fraction**;
hazard returns a derivative per **+1 annualized decimal rate**. Cash/futures
price delta is currency per **+1 price point**. A futures calibration-rate
+1bp move decreases its native `100-100*rate` quote by0.01 points. This is
distinct from positive futures-price delta. Clean and dirty cash/upfront inputs
remain separate types. No additional multiplier or bp conversion is inferred.

`RiskPosition` specifies exact signed quantity, the positive reference size
already contained in the native PV, currency and Notional/Contracts/TradeCopies
units. The engine scales once by their Decimal ratio. Native economic side
remains inside the reference trade; quantity is relative to that reference
side. Negative quantity reverses it. Futures counts must be integers. A per100
price must first become absolute native currency PV using original face; it
cannot be returned as currency PV. Reference callbacks return checked
`ModelMoney` and a wrong monetary currency fails.

## Complete keys and original coordinates

`QuoteRiskKey::new` checks an existing `RiskFactorKey` plus exact curve/quote
revisions, helper or actual grid coordinate, native unit, monetary normalization
and FrozenCurves/RebuildDependents response. Every dimension participates in
equality/hash. Market-quote and node risk, yield and bank-discount yield, deposit,
FRA, swap/OIS par, basis-swap spread, bond par coupon, repo, rate-future implied
rate, Z spread, OAS, residual basis, CDS running/clean-upfront/dirty-upfront,
hazard and recovery cannot silently share an aggregate key.

Convention, model/bump semantics and snapshot pins come from the caller's actual
admitted inputs. A tenor label is descriptive; helper IDs and exact original
grid indices identify the real bumped economics. No interpolation to a tenor
proxy or provider-name mapping is performed. Parallel quote coordinates retain
the exact sorted original ID/version/native-unit set; rate quotes in mixed
decimal/percent/bp units receive the same canonical annualized move through
their own original-unit conversions.

The core taxonomy adds variants; existing serialized variants and DV01/KRD
numerical meanings are retained. Rust consumers matching those enums
exhaustively need to handle the added variants. New risk JSON is a separate
native contract; it does not add v1 C/Excel/Java/MCP/WASM dispatch or generated
host schemas. CVX-063 owns those surfaces.

## Full rebuilding and reference pricing

`prepare_quote_risk_graph` composes actual `RateRiskCurveInput`,
`CdsCurveBuildInput` and `CashCurveBuildInput`, exact supplied curves, original
history and existing numerical controls. Rate helper revisions are explicit
additional snapshot pins because the older rate recipes lack that field.
Topology, duplicate identities, output overwrites and missing exact dependencies
are checked before fitting. All original helpers build through their existing
complete native builders; piecewise and global controls stay unchanged.
Administrator publication decisions are frozen during preparation and reused
for every shock. New requirements outside that set fail.

`evaluate_quote_graph` changes original native quotes, including uniform quoted
swap/basis/par-bond economics, and rebuilds the full supplied graph in dependency
order. All helpers must reprice at their unchanged native tolerances. Its result
retains build order, actual repricing, helper evaluation counts and consumed
publications. Government shocks rebuild issuer basis; CDS shocks rebuild credit
and credit-linked cash basis. Recovery is separate, updating original CDS legal
assumptions and the named par-recovery basis model when dependencies rebuild.

Scenario curves retain original keys **inside the hypothetical calculation**.
Their changed values are not published observation revisions or new live
snapshots. Source revisions and changed diagnostic quotes remain visible.
Ingestion must assign fresh immutable market/curve pins before publication.
Original inputs and last-good sets remain unchanged when any shock fails.

`curve_quote_sensitivity` invokes the same complete native reference trade
against each actual rebuilt curve set. The caller's pure pricing function must
retain economic terms, settlement/history/state, model parameters, actual
reference size and monetary units. Trade-specific recovery must bind to the
scenario's actual credit assumption. Calculation does not infer eligible
fixings, models, calendars or reference mappings. Full native scenarios are
available separately for diagnostics and reusable trade repricing.

Frozen node risk uses the actual continuous-zero or DF grid; DF shocks multiply
each selected value by `exp(-move*time)`. Residual basis uses its raw basis grid.
Hazard shocks alter actual forward intervals of a log-linear survival grid,
holding recovery separate and refusing negative intensity. Frozen recovery
holds survival probabilities fixed. Such diagnostics have no original quote
repricing claim. Supplied-node shocks can rebuild dependent full recipes;
calibrated-node shocks requesting dependent rebuilding fail explicitly. Use
market-quote recalibration for those risks. No representation is resampled or
silently replaced with a different interpolation/model.

`reference_quote_sensitivity` is the lower-level full-pricer callback route for
raw yield/Z/cash/futures quotations and already explicit native reference
scenarios. Every valuation failure and half-step stability failure aborts an
accepted result. Optional OAS must use `optional_oas_quote_sensitivity`: it
requires native HW1F resolution and separate duration/convexity refinement at
both h and h/2, preserving existing rejection of undersampled bumps. Generic
price-only OAS callbacks are refused. Other optional model/curve scenarios must
also satisfy their original model-specific risk controls in the owning pricer.

`reconcile_quote_buckets` requires the complete declared original quote set and
an actually simultaneous parallel repricing. `reconcile_curve_node_buckets`
additionally checks every informative node against the prepared actual grid;
missing/duplicate indices fail even if their sensitivity happens to be zero.
Both use explicit caller-admitted monetary discrepancy tolerances. A failed
reconciliation remains failed and must not qualify an additive approximation.

## Portfolio and FX

`aggregate_signed_quote_risk` takes the **complete expected trade manifest**,
required full factor keys and Present/Missing outcomes. An absent required
outcome is Missing. It cannot discover trades omitted from the caller manifest.
Duplicate/unrequested factors, mixed snapshots, positions or original PVs,
invalid numerical records and unauthorized entity scope fail. One entity is
explicit; multi-entity reporting requires the caller's explicit permitted list
and authorization/configuration revision. Calculation does not grant authority.

Per-currency coverage retains expected/covered trades and factor pairs, known
gross exposure, covered known gross exposure and the count of unknown exposures.
Unknown exposure stays unknown. Local and reporting sums retain partial values
as `known_value`; all-missing contributions remain None. Incomplete rows identify
their missing trades. Gross exposure is a nonnegative caller-defined currency
metric, not absolute net PV or a hidden duration weight.

Positive same-snapshot `RiskFxPin` observations convert monetary values once.
Local values survive missing FX while the reporting result is Incomplete.
`RiskFxConversionExposure` retains each actual FX pin and
`d(reporting sensitivity)/d(FX)=local sensitivity`. This is induced conversion
exposure of a risk result; it is **not** the full portfolio's FX delta or a
cross-currency hedge/execution model. Quote-factor currency remains in its key.

## Bounded engineering scope

Risk graphs admit at most64 full recipes,128 original helpers and256 total curve
outputs/supplied identities, within existing family calibration/grid/history/
cash/solver bounds. Publication manifests admit32,768 unique decisions. Each
central result requires baseline, +/-h and +/-h/2 valuations; each scenario has
bounded native calibration work. This is a reference batch calculation, with
no latency or memory certificate and no AAD/cache/live execution inference.
Portfolio bounds are4,096 trades,128 required factors per trade,65,536 total
required trade-factor pairs and256 FX observations. Numeric overflow fails.

Independent Decimal(60) full stepped/amortizing cash references and native
cross-product, quote/node, recovery, bucket/parallel, quantity, fixing and
failure checks provide scoped engineering evidence. Existing price/risk
expectations, captures and solver defaults remain unchanged. Model-specific
unsupported inputs and conservative resolution limits remain errors.
All2,154 mandatory library records stay Planned; quantitative, market/model,
host, lifecycle/P&L, deployment and latency certificates remain separate work.

ORE separates sensitivity scenario generation by market-factor type and runs
valuation through an explicit analysis workflow. These primary source reads
informed the separation of factors and scenario composition here; this task
does not claim an ORE runtime comparison or reproduce every ORE model.
[ORE sensitivity workflow](https://github.com/OpenSourceRisk/Engine/blob/master/OREAnalytics/orea/engine/sensitivityanalysis.cpp),
[ORE factor scenario generation](https://github.com/OpenSourceRisk/Engine/blob/master/OREAnalytics/orea/scenario/sensitivityscenariogenerator.cpp).


Complete original product graph capture, fitted-output/solver/helper diagnostic
comparison and original quote-risk replay are documented in
[the schema-3 replay contract](CALIBRATION-SNAPSHOT-REPLAY.md). Pure native calculations remain provider/storage/MQ/clock neutral.
Existing schema1/2 identities and original numerical controls remain; CVX-063
and all mandatory library/market/model/host/production/latency certificates stay open.

Full-product exponential UFR/speed fitting over exact immutable analytic bases, required one-sided joined forwards, original quote-risk and schema3 replay use [the explicit UFR product contract](PRODUCT-UFR-CALIBRATION.md). Complete Helpers recipes and actual mixed-unit parameters/diagnostics are retained. No sampled objective, regulatory procedure or unsupported base is substituted.

Complete checked original risk vectors compose through the pure
[constrained hedge proposal operation](CONSTRAINED-HEDGES.md). It preserves
original full key/bump/model/reference semantics and central/half PVs, with
complete current/pending exposure, exact native quantity and legal credit/FX
admission. Explicit bounded constrained/lot searches return measured residual,
cost/constraint/rank/work diagnostics without execution authority. Original
pricing/risk owners and every existing financial reference remain unchanged.


Complete contextual ICMA yield cash helpers now compose through the original full graph and five-PV risk owner. Original native units, policy version, actual bound calendars, full cash and dependent recipes survive each rebuild. Per-build original work ceilings and full/half-bump stability stay enforced; see [the helper contract](ICMA-CASH-HELPERS.md).

Direct own-security contextual ICMA yield sensitivity and positive reference
hedge columns use one prepared cash/time context and the original five-PV owner;
see [ICMA yield risk](ICMA-YIELD-RISK.md). Raw yield retains its distinct axis.
