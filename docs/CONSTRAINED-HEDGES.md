# Supplied complete-factor hedge proposals

`convex::constrained_hedge` and shared `hedge` actions `aggregate` / `solve`
compose original checked `QuoteSensitivity` vectors. They retain the actual
five central/half-step PVs, keys, native quotation, normalization, bump/stability
policy, reference sizes, economic sides and source revisions. There is no
provider, database, MQ, clock, ledger, order or live-limit dependency.

This is the original LC-027 supplied-risk mathematics. It does not reprice
instruments, resolve the pinned economics/calculation manifests or certify their
contents. The original bond/CDS/IRS/futures pricing and risk owners must produce
those records. A product tag, an instrument pin or a supplied matrix cannot
certify full financial calculations. Scalar DV01/duration, guessed futures CTD,
CDS-to-bond aliases and heuristic costs cannot replace missing original factors.
The existing scalar advisor remains an explicitly indicative diagnostic.

## Inputs and admission

The strict `HedgeProblem` owns every original input. Nullable credit/FX/lot
fields must be present explicitly as a value or null; their omission fails. It supplies a common actual
snapshot, legal entity, settlement, reporting currency and current/pending state
revision, the complete expected holding manifest, full current and pending
quantities, factor policies and original candidate constraints. An omitted or
unrequested holding fails; a missing factor is missing input, never zero risk.
An explicit zero retains a checked original five-PV result.

Every `HedgeRiskVector` supplies the actual product, instrument/reference
revision, complete economics/state/market manifest pin and original calculation
pin, owning entity, settlement, legal credit scopes and positive native
`RiskPosition` reference size. All factor results must share its position and
original base PV. Keys and `QuoteRiskSettings` match the problem's complete
factor order exactly. Curve/helper/quote/model/convention versions, raw versus
node versus recalibrated basis, frozen/rebuilt response and monetary units
cannot be merged by tenor or label. Source manifests are explicit integration
points; the application resolves them without entering pure mathematics.

Current and pending quantities retain signed original units and reference side.
They are separately aggregated, then added once. A matching held candidate
requires exact vector and identity equality. Its final holding limits apply to
current + pending + proposed trade. Futures use exact integer contract inputs
and integer trade lots. Cash, CDS and IRS use original notional units; no per100,
face-to-contract or unsigned-notional conversion is inferred.

Credit, hazard, recovery and residual-basis factors supply exact original legal
entity, seniority, document, restructuring and recovery scope. A nonzero vector
must explicitly include that scope. CDS requires one actual credit identity;
IRS cannot carry an issuer-credit scope. Basis, credit and recovery remain
separate keys. These checks validate supplied identity consistency, without
qualifying a market/legal mapping or treating credit-spread exposure as bond basis.

Foreign native monetary risk and costs require a positive exact same-snapshot
FX observation. All records for a native currency use the identical original
observation. Conversion is applied once. Identity FX must be absent; missing,
wrong-currency or mixed-snapshot observations fail. Converted risk is not a
portfolio FX delta, cross-currency pricing model or FX hedge.

Candidate eligibility is a separate supplied decision revision and explanation.
False decisions fail. Buy/sell available size, signed trade and final holding
intervals, zero-anchored native trade lots and separate buy/sell costs are
explicit. Costs are nonnegative exact money per original reference size and
retain an independent observation pin. Zero is a supplied value. No execution
price, freshness, borrow availability or authoritative order check is inferred.

## Objective, constraints and numerical policy

Let `x_j` be a signed trade divided by its original reference quantity; `A_ij`
is its full reference risk converted once into reporting money, and `b_i` is
current plus pending exposure. The supplied-model objective is

```text
0.5 * sum_i weight_i * ((b_i + sum_j A_ij*x_j - target_i)/scale_i)^2
+ cost_weight * sum_j side_cost_j(x_j) / cost_scale
```

`side_cost` is `buy_per_reference*x` for positive x and
`-sell_per_reference*x` for negative x, with the original explicit FX conversion.
The positive factor scales and weights retain unlike factor normalization units;
there is no implicit comparison of currency-per-bp, fraction or price-point risk.

Every factor has closed lower/upper final exposure bounds. Every candidate has
original trade/holding/side-liquidity constraints. A reporting-money cost budget
is hard even when `cost_weight` is zero. Lot constraints apply to trades; existing
holdings need not themselves be on the new-trade lot lattice.

The operation admits 1..16 factors, 1..4 solve candidates, at most256 expected
holdings, native quantities/reference sizes bounded by1e15, and 1..100000 work
units. An empty candidate list is aggregate-only. The complete weighted/scaled
factor matrix must have full column rank; singular candidates fail explicitly.
Relative rank cutoff is in(0,1e-3], maximum design condition is in[1,1e4], and
solver backward/primal/dual tolerance is in(0,1e-6]. These settings have no
inferred defaults, ridge, retry or pseudoinverse fallback.

Independent final factor and cost tolerances are in[0,1e-6] in their ORIGINAL
monetary units. They do not inherit factor/quantity scales or solver stopping
precision. Native quantity, holding, liquidity and lot constraints are exact
Decimal checks with zero tolerance. Continuous model quantities are converted
once to Decimal; closed endpoints within the declared dimensionless solve bound
are restored to their exact original values. Every constraint is then rechecked
on the emitted quantities. Nonzero native/model quantity or reference-ratio
underflow fails; it cannot become a zero trade or exposure. A failed
postcondition remains unresolved.

The native search separates signed cost regions, enumerates independent active
constraints up to the free dimension and checks each strictly convex region's
KKT system with complete pivoting and original-system backward error. Bounded
SVD checks the original scaled design and active-constraint independence; each
call has an explicit1..1000 iteration bound. A decomposition failure is an
unresolved numerical outcome, never proof of infeasibility.

The continuous relaxation is a separate diagnostic, including fractional
contract quantities before trade-lot admission. Then every admitted native lot
combination is enumerated in original candidate order and ascending lot order.
Remaining continuous quantities are optimized conditionally at each point.
An empty later lattice axis is checked before outer enumeration, preserving
termination even for huge earlier ranges. Native lattice bounds use the exact Decimal remainder before integer-index
rounding; a lower-bound remainder cannot disappear in quotient precision.
Every active-set attempt and complete
lattice point spends the same original work budget. SVD call counts are also
reported; these counts are not inner FLOPs, elapsed time or a latency SLA.

The final proposal minimizes the supplied objective within the declared numeric
search policy over the completely searched lattice and conditional continuous
problems. Exact objective ties retain the first original-order lattice point.
There is no nearest-lot shortcut, guessed feasibility, automatic work extension
or promotion of an unfinished incumbent. Large baskets need a separate qualified
solver/operation; this small bounded operation is not a universal optimizer.

This design applies the convex quadratic/KKT framework described by
[Boyd and Vandenberghe](https://web.stanford.edu/~boyd/cvxbook/) to this explicitly
small supplied problem. [nalgebra's decomposition guide](https://www.nalgebra.rs/docs/user_guide/decompositions_and_lapack/)
documents bounded SVD availability. Those primary sources inform the design;
they are not executed financial comparators or independent certification.

## Results and failures

`aggregate_hedge_risk` retains the complete problem, separate current and pending
factor sums and full before exposure. `solve_hedge_proposal` additionally retains
rank/singular values/condition, actual work, continuous relaxation and final
proposal. Solutions contain exact emitted native trades/final holdings, original
reference multiples, all after exposures and target residuals, side costs,
tracking/total objective and every original indexed constraint measurement.

Only `feasible` carries `proposal`. `infeasible` means the completed continuous
search has no admitted feasible point under its explicit numerical policy;
`discrete_infeasible` means a continuous point exists but the complete admitted
lot lattice has none. `rank_deficient`, `ill_conditioned`, `work_exhausted` and
`numerical_failure` remain distinct. A failed report may retain its completed
continuous diagnostic; it never carries a final proposal. The success envelope
means a diagnostic was calculated, so consumers MUST inspect `status` and
`proposal`. Every report says `authority: proposal_only`.

Malformed or incompatible risk/units/quantity/provenance/eligibility fails with
`invalid_input`; absent expected risk or FX is `missing_input`; dimension bounds
are `resource_limit`. A numerical diagnostic does not authorize an order.
Authoritative pending state, position/credit/execution limits, reference/legal
validation, source freshness, expiry, fencing, durable orders, fills and posting
remain application work under CVX-025/031 and related tasks.

## Consumer and evidence

```sh
cargo run --locked -p convex --example constrained_hedge
cargo run --locked -p convex --example constrained_hedge --no-default-features
cargo run --locked -p convex --example constrained_hedge --features schemars
python3 tests/scripts/constrained_hedge_reference.py
cargo test --locked --all-features -p convex --test constrained_hedge -p convex-ffi --test constrained_hedge -p convex-mcp --test constrained_hedge -p convex-wasm --test constrained_hedge -- --test-threads=1
```

Select the request object in
[the frozen original corpus](../tests/fixtures/library/constrained-hedge-admitted.json)
and use `convex::library::execute_json`, the existing `convex_library` C export,
or generic advertised library clients. No C symbol or client-specific solver
is added. Actual Java/.NET/Excel/wasm32/browser qualification remains separate.

The original raw corpus retains the failed integer CurveKey-version spelling.
A separate admitted corpus uses the existing opaque string revision without
changing financial values, answers or tolerances. Interacting/conditional-lot
controls were separately declared before their own first execution.

The independently declared Decimal60 analytic/finite-lattice controls use
fictional supplied full five-PV risk. They test solver mathematics and admission;
they do not qualify original instrument pricing or legal/market suitability.
Existing numerical references, solver defaults and mandatory library records
remain unchanged. CVX-063 and all market/model/library/production/latency
certificates remain partial/unaccepted/unverified.

The [direct contextual ICMA bridge](ICMA-YIELD-RISK.md) computes all five full
fixed-cash quotations and emits a positive original-face reference column.
Its consumer verifies same-security current/pending/lot/cost/reporting-FX
composition with actual calculated columns. Other instruments still need
their complete common-factor calculations.
