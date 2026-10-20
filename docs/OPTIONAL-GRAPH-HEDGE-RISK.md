# Optional bonds: complete graph risk and hedge columns

CVX-063 composes the original supplied optional-bond model with graph risk and
the existing constrained hedge calculation. Optional cash is priced by its
original checked HW1F stopping model, including complete contractual cash,
notional/coupon steps, accrual, notice, settlement and eligible observations.
The implementation remains a pure library. It accepts supplied immutable
resources and proposes hedge quantities; server authorization, providers,
storage, message queues, live orders and cash posting remain outside it.

This is a bounded engineering increment. CVX-063 remains partial, unaccepted
and uncertified. The entire mandatory coverage matrix remains Planned.

## Run the increment

```sh
cargo run --locked -p convex --example optional_graph_hedge > /tmp/optional-graph-hedge.json
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test optional_graph_hedge -- --test-threads=1
cargo test --locked --all-features -p convex --test optional_graph_boundaries -- --test-threads=1
```

The runnable consumer calculates fresh full columns, checks independent
financial controls, compares the complete original native risk certificates,
and constructs the full hedge books from those fresh columns. Its JSON output
contains requests and complete actual envelopes for transport comparisons.
Native output is never the independent financial reference.

Read the full inputs in
[the admitted controls](../tests/fixtures/library/optional-graph-hedge-resolved.json),
[direct operations](../tests/fixtures/library/optional-graph-hedge-direct.json),
[additional boundaries](../tests/fixtures/library/optional-graph-hedge-qualification.json),
[calibrated full-OAS control](../tests/fixtures/library/optional-graph-hedge-calibrated.json)
and [complete book templates](../tests/fixtures/library/optional-graph-hedge-policy-book.json).
The original raw trials remain alongside their separately declared admission
controls. The superseded calibrated trial is retained but does not qualify.

## Existing operations and new native actions

`graph_hedge_risk` now accepts `product.kind = "optional_bond"`. Its product
contains the existing complete `OptionalInput`, exact `instrument` reference
pin, fractional `oas_decimal`, and explicit separate `policies.rates` and
`policies.model`. Its graph, hedge identity, snapshot, reference position,
factor manifest and evaluation reservation remain the original contracts.
`calibration.action.kind = "product_risk"` accepts the same product and returns
one canonical sensitivity plus both original native risk certificates.

The existing `optional_bond` operation gains two distinct actions:

| Action | Required additional inputs | Result |
| --- | --- | --- |
| `model_scenario` | `oas_decimal`, `parameter`, signed `delta` | Full price, grid/exercise diagnostics and original model provenance |
| `model_risk` | `oas_decimal`, `parameter`, positive `bump`, explicit `settings` | Actual first/second parameter derivatives on every grid and full base/up/down prices |

`parameter` is `mean_reversion` or `volatility`. The existing native rate
`risk`, rate `scenario` and OAS-only `quote_risk` remain correct operations.
There is no alias replacing their original safeguards. Generic optional
product scenarios and bucket reconciliation still require their owning
integration and return typed unsupported outcomes.

The shared request schema, owned UTF-8 C response, MCP library entry point and
native-compiled WASM export call the same original library implementation.
Java and Excel transport tests and CI generation are extended. Those managed
hosts, Windows and actual wasm32/browser execution have not run on this host.

## Factor identity, money and native units

All factors retain the existing checked `QuoteRiskKey`. The original
`RiskCoordinate` enum is unchanged. Native input unit is `DecimalRate`, and
response is `frozen_curves` for this integration.

| Factor | Complete coordinate identity | Reported sensitivity |
| --- | --- | --- |
| Discount parallel | Original `Zero` discount curve ID/version, `node = null` | Currency per +1bp |
| Projection parallel | Original independent `Zero` projection ID/version, `node = null` | Currency per +1bp |
| Joint parallel | `Raw("joint_zero")`, `joint_zero_rate`, `zero_node`; `curve_pair` target with **both** original IDs and opaque string revisions | Currency per simultaneous +1bp to both roles |
| Optional OAS | `Raw` original security ID and exact security revision, `oas_spread`, matching instrument target | Currency per +1bp additive discount OAS |
| HW1F mean reversion | `Raw("mean_reversion")`, `hw1f_mean_reversion`, `model_parameter`; `model` target with exact original model ID/revision | Currency derivative per unit of mean reversion in 1/year |
| HW1F diffusion | `Raw("volatility")`, `hw1f_volatility`, same exact model target | Currency derivative per unit of short-rate diffusion in 1/sqrt(year) |

The Raw revision separately identifies the supplied coordinate definition;
the actual source curve/model string revisions remain in the complete target.
Different curve pairs or model revisions cannot net as equal keys. The HW1F
diffusion parameter is distinct from a swaption market-volatility quotation.
An independently supplied model-semantics version remains part of every key.

The hedge reference uses exact positive original face and `quantity_unit =
"notional"`. The graph price and currency PV use that original face; they are
not reconstructed from a per100 invoice or an outstanding amortized balance.
Direct product risk also admits exact signed complete trade copies with a
unit reference. Scaling occurs once in the original five-PV normalizer.
Native model-risk diagnostics report derivatives of dirty price per100 face;
the canonical monetary column separately reports its stated position scale.

OAS represents the exact supplied optional security quotation. It does not
invent CDS legal-default exposure. Only `oas_spread` is excluded from the
hedge factor's mandatory CDS credit-reference rule; all other credit/hazard/
recovery mapping and document/seniority/recovery checks remain unchanged.
Optional vectors carry no fabricated legal credit scope.

## Original price and risk guards

Every accepted factor invokes the original native risk owner at **both** h
and h/2. Rates and OAS must pass the original two-finest-state-cell bump floor,
price convergence and both successive duration/convexity refinements.
An unresolved half-step rejects the entire column, even when h succeeds.
The public price-only OAS callback still rejects optional risk.

Parameter scenarios change only the named checked HW1F constant and rebuild
the same original dated trees against the same complete discount inputs.
Original cash, exercised branches, future stochastic reset structure,
deterministic projection basis and known eligible revisions remain intact.
Each scenario retains a `ParameterScenario` containing the complete original
assumed or calibrated model and the signed native move. Calibration quotes,
original fit diagnostics and residuals survive; no shocked quote fit is implied.

Model-risk settings require explicit positive absolute and nonnegative
relative first- and second-derivative bounds. All prices pass their original
price guards, and **both** final successive first/second refinements must pass.
The rate-state floor has different units and is not a parameter-resolution
certificate. h/h2 stability remains independently mandatory for the canonical
column. Parameter values retain the original native [0,10] mean-reversion
and [0,1] diffusion domains; central bump is positive and at most 0.1.

The resulting five monetary PVs are original base, +h, -h, +h/2 and -h/2.
The full h/h2 native certificates accompany the canonical column. Optional
hedge output additionally includes the original baseline price and complete
hedge vector. Baselines must agree across all columns. No partial accepted
vector or zero fallback is emitted on failure.

## Supported graph and contract domain

The complete prepared optional curve recipes must equal the actual graph
baseline recipes, including values, roles, currencies and source policy.
Parallel transforms currently admit ACT/365F native interpolation grids:
linear continuous zero rates or log-linear discount factors. All log-linear
DF model queries must remain within the original grid; raw Flat/Linear DF
tails have different economics and are rejected. Native Nodes/MoneyMarket/
Helpers grid sources retain their original admission and fitting guards.
Derived, segmented and parametric source transforms require separate work.

Floating optional cash retains distinct same-currency projection/index and
collateral discount roles. Future aligned term-reset coupons respond to the
same stochastic state; their projected amounts are not frozen deterministic
coupon proxies. Eligible published revisions remain fixed. Existing supported
European/Bermudan fixed-price model constraints are preserved. American,
make-whole, triggered, cap/floor, overnight/path-dependent or off-cycle FRN
option structures continue to require additional complete state/models.

This increment does not implement shaped-node optional risk, recalibrated
market quote/volatility risk, complete optional graph bucket reconciliation,
or all-member bond-delivery-futures hedge columns. Those gaps remain explicit.

## Hedge book and bounded work

The controls use a complete six-factor optional term-FRN and rate-futures
book: discount, projection, joint, OAS, mean reversion and diffusion. The full
original futures pricer actually reprices every unconsumed OAS/model move;
the resulting zero exposures are never inserted as supplied constants.
Joint risk reprices both exact curve grids through the original graph owner.

Fresh columns enter the existing aggregator and proposal solver. Current
positions, pending quantities, expected holding IDs, exact instrument/economics/
calculation/entity/snapshot pins, source cost observations, universe decisions,
side liquidity/costs, lots and explicit same-snapshot USD/EUR FX are retained.
The independently controlled close-out is -350 original face and -3 futures
contracts, with zero final holdings and supplied costs 10.375 USD or 9.3375 EUR.
The result has `proposal_only` authority.

The original bridge reserves five canonical callbacks per factor and admits
1..16 factors / at most80 callbacks. Optional native certificates execute
six full price evaluations per factor plus one returned baseline; the output
discloses `native_price_evaluations = 6*N + 1`. Each price retains original
tree/grid/node-work bounds, including the32-million conservative visit budget
per grid. The callback reservation is not total calibration/tree work or a
latency estimate. Original hedge limits16 factors,4 candidates,256 holdings
and100000 bounded solve-work attempts remain. No dependency or default changes.

## Evidence and next increment

Independent controls use Decimal60 dated profiles/cash/accrual and closed
truncated Gaussian moments of the complete portfolio at actual notice, plus
common-state FRN reset economics and exact close-out arithmetic. The new
scoped per100 price bound is0.006; rate exposure bound0.0001; mean-reversion
derivative1; diffusion derivative2. Exact cash bound1e-10 and original native
price/rate policies remain. These declared coarse-step engineering controls
do not establish universal 1bp optional risk, market/model accuracy or latency.

Private source-bound commands, declarations, retained failures, conservation,
qualification and commit bindings are in
`.ai/production-readiness/evidence/CVX-063-Optional-Hedges`. All646 older
quantitative records/files/defaults and closed historical evidence remain.
Unavailable hosts and independent model/library/production certificates stay
open. The next bounded increment is all-member bond-delivery-futures risk and
hedge-factor assembly, preserving every original basket/carry/quote source,
CTD-switch scenario and indicative futures monetary meaning. CVX-025/031
continues to own authoritative server limits, pending state, fencing and orders.
