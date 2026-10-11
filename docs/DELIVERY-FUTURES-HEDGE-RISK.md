# Full delivery-futures basket risk and hedges

The supplied-term library now calculates graph risk, scenarios and hedge columns
for a complete physical bond-futures basket. Each scenario reprices every actual
cash security, reapplies its original financing and coupon carry, and selects the
CTD again. It preserves all alternatives, including exact ties and CTD switches.
The original fixed-date cash/carry, invoice, curve/calibration, differentiation
and hedge owners perform the financial calculations.

Run the complete independently controlled consumer:

```sh
cargo run --locked -p convex --example delivery_hedge
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test delivery_hedge -- --test-threads=1
cargo test --locked --all-features -p convex --test delivery_boundaries -- --test-threads=1
```

It checks 79 complete transport requests: 62 full-basket risk/scenario and typed
boundary controls, two fresh five-factor columns, and 15 full hedge-book requests.
The direct embedding test also checks 14 complete controls against the original
native basket owner and rejects a different complete curve recipe sharing the
same ID/revision. The fixtures are synthetic engineering controls, with explicit
source quality, availability and original revisions. They are not exchange
security data or a market/convention certificate.

## Operations and integration

| Existing shared operation | Complete input | Output |
| --- | --- | --- |
| `calibration` / `product_risk` | `product.kind=bond_future`, complete `BondFutureInput`, stamped discount curve, explicit member budget, original graph, factor, signed contracts, snapshot and differentiation settings | Five full baskets, actual signed monetary sensitivity, frozen-member and one-sided diagnostics, ties and switches |
| `calibration` / `product_scenario` | Same full product/graph/position, coordinate and signed delta | Full baseline/shocked baskets and signed quote-level change |
| `graph_hedge_risk` | Same full product/graph, exact hedge identity, positive one-contract reference, all required factors/settings, snapshot and work bounds | Full per-factor basket results and the original `HedgeRiskVector` |
| `hedge` / `aggregate`, `solve` | Fresh complete vectors, original current/pending holdings, candidate lots/limits/liquidity, side costs, eligibility and reporting FX | Original exposure report or bounded `proposal_only` hedge result |

These extend existing correct operations. No alternate route, database, queue,
market-data adapter, global state or clock is added. Applications resolve all
inputs through their existing provider-neutral ports before calling the pure
library. Rust embedding uses `bond_futures_graph_risk`,
`bond_futures_graph_scenario` and `workflow::delivery_futures_hedge_risk`.
Shared JSON, `convex_library`, MCP and WASM delegate to the same calculation.
Java and Excel receive dedicated complete-envelope tests and CI generation;
their actual host execution is a separate qualification gate.

Full runnable requests and independent answers are in
[the admitted controls](../tests/fixtures/library/delivery-hedge-decimal.json),
[quote rebuild controls](../tests/fixtures/library/delivery-hedge-rebuilt.json) and
[non-tied switch controls](../tests/fixtures/library/delivery-hedge-frontier.json).
For one original request:

```sh
python3 - <<'PY'
import json
from pathlib import Path
document = json.loads(Path('tests/fixtures/library/delivery-hedge-decimal.json').read_bytes())
print(json.dumps(document['cases'][0]['request']))
PY
```

## Factor meaning and complete repricing

Borrow the native discount from `graph.baseline().curves`. It must be that
original immutable materialized curve, with the exact original complete recipe,
including source, construction, role, time basis and dependencies. A different
fit or resolved dependency can share a helper/derived recipe, so recipe equality
alone cannot admit a foreign curve. The shared facade resolves this original
graph owner automatically. Cloned curve sets retain the same immutable owners.
Every member
has one exact security/revision, full checked bond contract/calendar set, actual
observed clean quote and quote source, conversion factor, repo/haircut, own-cash
policy and coupon-carry policy. The original owner rejects incomplete or duplicate
baskets, unsupported terms and missing/stale/contradictory source facts.

Curve moves price the complete original dated cash under the actual scenario
discount curve. Observed-minus-original-model clean residual is held fixed,
then the resulting scenario clean enters the original full carry pricer. Quote
coordinates rebuild the actual calibration graph and retain complete recipes,
native-unit helper repricing, dependency order, measured work and frozen fixing
publication decisions. Node moves use the original graph semantics.

Raw member factors require the exact security ID, original source ID/revision,
native currency and `spot` tenor:

- `RepoRate` with `DecimalRate` changes only that member's actual repo.
- `CashCleanPrice` with `PricePer100` changes only that member's clean residual
  basis, preserving its original cash model and quotation context.

Every other member is actually repriced under each move. A raw factor not
consumed by an admitted cash hedge still calculates all five full original
valuations; its zero exposure is not a supplied constant. The existing native
parallel-repo route remains useful for an explicitly parallel scenario; this
integration does not invent one source revision for unrelated security repos.

## CTD changes and numerical admission

The selected minimum-net-basis, maximum-implied-repo or minimum-forward member
remains distinct from the minimum-forward member determining this fixed-date
model's fair futures price. Every evaluation reports both winners and their
complete alternatives. Exact Decimal tie sets retain the original identity/
revision order; a positive near-tie gap is not rounded into a tie.

Risk evaluates exactly `0, +h, -h, +h/2, -h/2`. The original central/half-step
stability policy must pass. It returns reselected monetary sensitivity plus the
separate original-fair-member frozen diagnostic, plus-side/minus-side secants,
selection switches, fair-member switches, ties and `ctd_stable`. A switch or tie
sets `ctd_stable=false`. These are finite-bump measurements at the supplied
coordinate and size; they do not assert a smooth infinitesimal derivative or
value the probability of a future switch. Unresolved half-step risk returns a
typed failure and no accepted hedge column. Full signed scenario repricing is
available independently of derivative admission.

Independent controls retain the original larger-bump instability cases and
separately declared smaller-bump controls under the same tolerances. Non-tied
controls cross a genuine positive baseline selection gap. Both shock signs,
negative positions, reordered baskets, identical ties, distinct competing
maturities, one-member baskets and complete quote rebuilds are exercised.

The carry, net-basis and implied-repo relationships follow the concepts described
in [CME's CTD introduction](https://www.cmegroup.com/education/courses/introduction-to-treasuries/get-to-know-treasuries-ctd).
Conversion factors and clean cash versus converted futures basis are described
in [CME's basis introduction](https://www.cmegroup.com/education/courses/introduction-to-treasuries/the-basics-of-treasuries-basis).
These sources explain the economic relationships; the implementation and
synthetic controls do not establish exchange-specific convention coverage.

## Money, work and hedge authority

Futures money is unrounded fair price points times the supplied currency per
point per contract. The signed integer contract quantity scales it once.
The native hedge reference is exactly one positive contract; cash retains its
original positive face reference. This is quote-level marking and scenario
change, distinct from discounted lifetime PV, rounded daily margin, actual
cash posting and realized P&L. Reporting FX is explicitly attached to fresh
vectors with the same snapshot; it is not guessed from currency names.

Full USD and EUR five-factor cash/delivery books preserve current/pending
holdings, exact lots, position/liquidity limits, original cost/eligibility
observations and FX. The independent close-out controls require sale of 350
cash face and three futures contracts, zero final holdings and all factor
constraints, with side costs 7.25 USD or 6.525 EUR. Ineligible, missing, stale,
mismatched, illiquid and unaffordable alternatives fail admission or return the
original infeasible proposal outcome. They do not grant order authority.

`BondFuturesGraphPolicy` reserves `12 * members * factors` member-price/carry
visits before calculation: original carry, original cash model and five actual
cash/carry evaluations. It admits 1..128 members and 1..16 factors, within an
explicit budget of 1..24,576. A two-price scenario conservatively uses the same
reservation. The independent graph/calibration, five-factor-callback and hedge
solver bounds still apply. These work limits do not measure latency.

The original deterministic model admits full fixed-coupon nominal bullet/par
deliverables, including stepped coupons. Optional, floating, amortizing,
non-par, indexed and ex-coupon deliverables remain unsupported by this delivery
model. Delivery timing, wildcard, end-month options and daily-margin financing
remain unvalued. Generic delivery bucket risk remains unsupported; this increment
uses the full dedicated basket API. Additional factor mappings, stochastic
delivery optionality, lifecycle/P&L, large-book solver robustness and measured
multiserver/colocation latency remain owning work. Server limit/pending/fencing/
execution authority remains CVX-025/031.

All prior numerical controls/defaults, historical evidence, 74 route definitions,
mandatory 30 areas/174 combinations/2,154 Planned records, pure normal dependencies,
manifests/lock and 26 C exports remain protected. Native adapter outputs prove
transport only. CVX-063 stays partial, unaccepted and uncertified; actual Java/
JNI/Windows/.NET/Excel/wasm32/browser and independent financial/model/library/
host/production/latency certificates remain separate gates.
