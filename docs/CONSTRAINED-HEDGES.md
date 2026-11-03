# Constrained hedge proposals

`convex::constrained_hedge` and the shared `hedge` actions `aggregate` and `solve`
combine `QuoteSensitivity` vectors you pass in. They keep the five central and
half-step PVs, keys, quotation, normalization, bump and stability policy,
reference sizes, economic sides and source revisions. There is no provider,
database, message queue, clock, ledger, order or live-limit dependency.

This is the risk-aggregation and optimization mathematics only. It does not
reprice instruments, resolve the pinned economics and calculation manifests, or
check their contents. The bond, CDS, IRS and futures pricing and risk code must
produce the risk records. A product tag, an instrument pin or a matrix you pass in
cannot stand in for a full financial calculation. Scalar DV01 or duration, a
guessed cheapest-to-deliver, CDS-to-bond aliases and heuristic costs cannot
replace missing factors. The scalar advisor remains an indicative diagnostic.

## Inputs and validation

The strict `HedgeProblem` holds every input. Nullable credit, FX and lot fields
must be present explicitly as a value or null; leaving one out fails. The problem
supplies:

- a common snapshot, legal entity, settlement, reporting currency and
  current/pending state revision;
- the complete expected holding manifest and the full current and pending
  quantities;
- factor policies and candidate constraints.

An omitted or unrequested holding fails. A missing factor is missing input, never
zero risk. An explicit zero keeps a validated five-PV result.

Every `HedgeRiskVector` supplies:

- the product, instrument/reference revision, complete economics/state/market
  manifest pin and calculation pin;
- the owning entity, settlement and legal credit scopes;
- a positive `RiskPosition` reference size in native units.

All factor results must share its position and base PV. Keys and `QuoteRiskSettings`
must match the problem's factor order exactly. Curve, helper, quote, model and
convention versions, raw versus node versus recalibrated basis, frozen versus
rebuilt response, and monetary units cannot be merged by tenor or label. The
source manifests are integration points: the application resolves them outside
the pure mathematics.

Current and pending quantities keep their signed units and reference side. They
are aggregated separately, then added once. A matching held candidate requires
exact vector and identity equality. Its final holding limits apply to current plus
pending plus the proposed trade. Futures use integer contract inputs and integer
trade lots. Cash, CDS and IRS use original notional units; no per-100, face-to-contract or
unsigned-notional conversion is inferred.

Credit, hazard, recovery and residual-basis factors supply the legal entity,
seniority, document, restructuring and recovery scope. A nonzero vector must
include that scope explicitly. CDS requires one credit identity; IRS cannot carry
an issuer-credit scope. Basis, credit and recovery remain separate keys. These
checks confirm that the identities you pass in are consistent. They do not
validate a market or legal mapping, or treat credit-spread exposure as bond basis.

Foreign-currency risk and costs require a positive FX observation from the same
snapshot. All records for a currency use the same observation, and conversion is
applied once. Identity FX must be absent; missing, wrong-currency or
mixed-snapshot observations fail. Converted risk is not a portfolio FX delta, a
cross-currency pricing model or an FX hedge.

Candidate eligibility is a separate decision with its own revision and
explanation. A false decision fails. These are explicit inputs:

- buy and sell available size;
- signed trade and final holding intervals;
- zero-anchored trade lots;
- separate buy and sell costs.

Costs are nonnegative money per original reference size and carry an independent
observation pin. Zero is a value you supply. No execution price, freshness,
borrow availability or order check is inferred.

## Objective, constraints and numerical policy

Let `x_j` be a signed trade divided by its original reference quantity. `A_ij` is its full
reference risk converted once into reporting money, and `b_i` is current plus
pending exposure. The objective is

```text
0.5 * sum_i weight_i * ((b_i + sum_j A_ij*x_j - target_i)/scale_i)^2
+ cost_weight * sum_j side_cost_j(x_j) / cost_scale
```

`side_cost` is `buy_per_reference*x` for positive x and
`-sell_per_reference*x` for negative x, with the explicit FX conversion. The
positive factor scales and weights carry the normalization units of unlike
factors; there is no implicit comparison of currency-per-bp, fraction or
price-point risk.

Every factor has closed lower and upper bounds on final exposure. Every candidate
has trade, holding and side-liquidity constraints. A cost budget in reporting
money is a hard limit even when `cost_weight` is zero. Lot constraints apply to
trades; existing holdings need not be on the new-trade lot lattice.

The operation accepts:

- 1..16 factors and 1..4 solve candidates;
- at most 256 expected holdings;
- quantities and reference sizes bounded by 1e15;
- 1..100000 work units.

An empty candidate list means aggregate only. The weighted, scaled factor matrix
must have full column rank; singular candidates fail explicitly. The relative rank
cutoff is in (0,1e-3], the maximum design condition is in [1,1e4], and the solver
backward, primal and dual tolerance is in (0,1e-6]. These settings have no
defaults, ridge, retry or pseudoinverse fallback.

Final factor and cost tolerances are in [0,1e-6], in their own monetary units.
They do not inherit the factor or quantity scales or the solver stopping
precision. Quantity, holding, liquidity and lot constraints are exact Decimal
checks with zero tolerance. Continuous quantities are converted once to Decimal;
closed endpoints within the declared dimensionless solve bound are restored to
their exact values. Every constraint is then rechecked on the emitted quantities.
A nonzero quantity or reference ratio that underflows to zero fails; it cannot
become a zero trade or exposure. A failed postcondition stays unresolved.

The search separates signed cost regions, enumerates independent active
constraints up to the free dimension, and checks each strictly convex region's KKT
system with complete pivoting and backward error on the original system. At most
three residual corrections reuse the same factorization and must satisfy the
unchanged backward-error bound; unresolved solves still fail. A bounded SVD checks
the scaled design and the independence of the active constraints; each call has an
explicit 1..1000 iteration bound. A decomposition failure is an unresolved
numerical outcome, never proof of infeasibility.

The continuous relaxation is a separate diagnostic, including fractional contract
quantities before trade-lot checks. Then every allowed lot combination is
enumerated in candidate order and ascending lot order, and the remaining
continuous quantities are optimized at each point. An empty later lattice axis is
detected before the outer enumeration, so the search terminates even when earlier
ranges are huge. Lattice bounds use the exact Decimal remainder before rounding to
an integer index, so a lower-bound remainder cannot vanish in quotient precision.

Every active-set attempt and every lattice point spends the same work budget. SVD
call counts are also reported. These counts are not inner FLOPs, elapsed time or
a latency guarantee.

The final proposal minimizes the objective within the declared numeric search
policy, over the completely searched lattice and the conditional continuous
problems. Exact ties keep the first lattice point in candidate order. There is no
nearest-lot shortcut, guessed feasibility, automatic work extension or promotion
of an unfinished incumbent. Large baskets need a different solver; this small
bounded operation is not a general optimizer.

The design applies the convex quadratic and KKT framework in
[Boyd and Vandenberghe](https://web.stanford.edu/~boyd/cvxbook/) to this small
problem. [nalgebra's decomposition guide](https://www.nalgebra.rs/docs/user_guide/decompositions_and_lapack/)
documents the bounded SVD. These sources informed the design; they have not been
used as financial comparators.

## Results and failures

`aggregate_hedge_risk` returns the problem, the separate current and pending
factor sums and the full exposure before trading. `solve_hedge_proposal` also
returns the rank, singular values and condition number, the work used, the
continuous relaxation and the final proposal. A solution contains:

- the emitted trades and final holdings in native units, and the trades as
  multiples of the reference size;
- all exposures after trading and the target residuals;
- side costs, the tracking and total objective, and every indexed constraint
  measurement.

Only `feasible` carries `proposal`. The other statuses mean:

- `infeasible`: the completed continuous search has no feasible point under the
  numerical policy.
- `discrete_infeasible`: a continuous point exists but no point of the lot lattice
  is feasible.
- `rank_deficient`, `ill_conditioned`, `work_exhausted` and `numerical_failure`
  are reported separately.

A failed report may keep its completed continuous diagnostic; it never carries a
final proposal. The success envelope means a diagnostic was calculated, so
consumers MUST inspect `status` and `proposal`. Every report says
`authority: proposal_only`.

Malformed or incompatible risk, units, quantity, provenance or eligibility fails
with `invalid_input`; absent expected risk or FX is `missing_input`; dimension
bounds are `resource_limit`. A numerical diagnostic does not authorize an order.
Pending state, position, credit and execution limits, reference and legal
validation, source freshness, expiry, fencing, durable orders, fills and posting
are application work outside this library.

## Example and tests

```sh
cargo run --locked -p convex --example constrained_hedge
cargo run --locked -p convex --example constrained_hedge --no-default-features
cargo run --locked -p convex --example constrained_hedge --features schemars
python3 tests/scripts/constrained_hedge_reference.py
cargo test --locked --all-features -p convex --test constrained_hedge -p convex-ffi --test constrained_hedge -p convex-mcp --test constrained_hedge -p convex-wasm --test constrained_hedge -- --test-threads=1
```

Pick the request object from
[the frozen test corpus](../tests/fixtures/library/constrained-hedge-admitted.json)
and run it with `convex::library::execute_json`, the existing `convex_library` C
export, or any generic library client. No C symbol or client-specific solver is
added. Testing in Java, .NET, Excel and a wasm32 browser is separate and not yet
done.

The raw corpus keeps the rejected integer `CurveKey` version spelling. A separate
corpus uses the opaque string revision without changing financial values, answers
or tolerances. The interacting and conditional-lot controls were declared before
their first run.

The independent Decimal60 analytic and finite-lattice controls use fictional full
five-PV risk. They test the solver mathematics and input validation; they do not
test instrument pricing or legal and market suitability. Existing numerical
references, solver defaults and library records are unchanged.

The [direct contextual ICMA bridge](ICMA-YIELD-RISK.md) computes all five
full fixed-cash quotations and emits a positive face-reference column. Its
consumer checks same-security current/pending/lot/cost/reporting-FX composition
with calculated columns. Other instruments still need their complete common-factor
calculations.

## Complete common-factor hedge columns

The `graph_hedge_risk` operation builds complete bond, CDS, IRS/OIS and
rates-futures columns for the aggregator and proposal solver, using the full
product and graph revaluation code. See
[quantity binding, inputs, controls and remaining limits](GRAPH-HEDGE-RISK.md).
Own-security ICMA yield remains a separate factor.

Not yet covered here: optional models, credit-survival cash products and
all-member delivery-futures risk (the last has its own page, linked from the graph
hedge page). Managed-runtime and browser execution and production and latency
qualification are also open. The solver checks are not executed financial
comparisons or independent certification. This work is partial and has not been certified for production use.

Credit-survival and issuer-basis cash have their own `cash_basis_hedge_risk`
route. See [cash basis hedge risk](CASH-BASIS-HEDGE-RISK.md) for the
security, mapping and factor contract, the ICMA and rebuild controls, the
mixed-book scope and what remains.
