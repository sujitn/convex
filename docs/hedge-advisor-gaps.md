# AI Hedge Advisor — Phase 2 Gap Report

Source: `docs/hedge-advisor-investigation.md`. Each gap is sized for the **v1 demo scope** (single-position, single-currency, two strategies, template narrator). Severity is from the demo's perspective; "Low" gaps are real but deferred.

**Top-line crate impact: zero new crates.** Every gap can be closed inside `convex-analytics` (new modules under `src/risk/hedging/`) and `convex-mcp` (new `#[tool]` methods). The case for and against a new crate is argued in §7.

---

## Gap 1 — Hedge advisor domain types (RiskProfile, HedgeProposal, etc.)

- **What is missing.** The advisor needs `RiskProfile`, `HedgeInstrument`, `HedgeTrade`, `HedgeProposal`, `TradeoffNotes`, `Constraints`, `ComparisonReport`, `Provenance`, `ResidualRisk`. None of these exist.
- **Why it matters.** These are the wire types the four MCP tools exchange. Without them the agent has nothing to round-trip.
- **Severity.** **Blocker.**
- **Effort.** S (a few hours — types + derives + tests for serde/JsonSchema round-trip).
- **Crate placement.** `convex-analytics::risk::hedging::types` (new module). The crate already owns `risk::hedging::{Position, PortfolioRisk, HedgeRecommendation, HedgeDirection}` (`risk/hedging/{portfolio,hedge_ratio}.rs`). Adding domain DTOs alongside is consistent with charter ("Unified analytics engine"). **No new crate.**
- **Performance.** Cold path. These structs are JSON-bound, exchanged once per tool call. Stick to owned `Vec` / `String` for ergonomics; avoid `Cow` / lifetimes.
- **Convention notes.**
  - All numeric fields carry units in name (`dv01_per_bp`, `duration_years`, `notional_usd`, `cost_bps`).
  - Every output struct embeds `provenance: Provenance`.
  - Every struct derives `Debug, Clone, Serialize, Deserialize, JsonSchema` (gated by the existing `schemars` feature on `convex-core`).

---

## Gap 2 — `compute_position_risk` does not exist as a callable function

- **What is missing.** A function that, given `(bond, mark, notional, settlement, curve, key_rate_tenors)`, returns a `RiskProfile { dv01, modified_duration, key_rate_durations: Vec<(tenor, dv01)>, currency, provenance }`.
- **Why it matters.** The advisor's first step. It must be a thin orchestration over existing primitives — no new pricing math.
- **Severity.** **Blocker.**
- **Effort.** S. The wiring already exists: `price_from_mark` (analytics/pricing.rs:47), `BondRiskCalculator` (risk/calculator.rs), `key_rate_profile` (curves/bumping/key_rate.rs:370). New code is glue + scaling-by-notional + provenance.
- **Crate placement.** **`convex-analytics::risk::profile` (new sibling module of `dv01`, `duration`, `convexity`, `hedging`).** This is a *risk* function — the hedging module should consume a `RiskProfile`, not own it. Co-locating it with `BondRiskCalculator` makes it discoverable to anyone computing per-position risk without requiring the hedge advisor. The advisor then just imports `risk::profile::compute_position_risk`. **No new crate.**
- **Performance.** Hot path. One bond pricing + one DV01 + one KRD profile (12 tenors → 12 reprices). Existing benches for these dominate; expect order ~10–100µs total. Avoid extra allocations: pass curve by reference, use `key_rate_profile`'s existing `Vec<(f64, f64)>` output without copying.

> **Resolves the open question from §"Open question to resolve in Phase 3".** `compute_position_risk` is a risk-module citizen, not a hedging-module citizen. The Phase 3 plan will reflect this placement.

---

## Gap 3 — Hedge instrument analytical specs (extensible, not future-only)

- **What is missing.** Closed-form, DV01-bearing analytical DTOs for hedge legs. Today, `convex-curves::calibration::instruments::{Future, Swap, Fra, Ois}` exist as `CalibrationInstrument` impls only — they don't compute their own position-level DV01 from a `Mark`.
- **Why it matters.** The strategies need to size and price their legs against the position's risk profile.
- **Severity.** **Blocker** for v1.
- **Design — open extensibility from day one.** `HedgeInstrument` is a tagged enum, not a concrete future-only struct. The trait that backs it is what every variant implements:

  ```rust
  pub trait HedgeLeg {
      fn dv01(&self, ctx: &MarketContext) -> Result<f64, AnalyticsError>;
      fn key_rate_profile(&self, ctx: &MarketContext) -> Result<Vec<(f64, f64)>, AnalyticsError>;
      fn cost_bps(&self, model: &dyn CostModel) -> f64;
      fn currency(&self) -> Currency;
      fn description(&self) -> String;
  }

  pub enum HedgeInstrument {
      BondFuture(BondFutureSpec),     // v1
      InterestRateSwap(IRSwapSpec),   // v1
      CashBond(CashBondSpec),         // v1.x — wraps existing convex-bonds::FixedRateBond
      Etf(EtfProxySpec),              // v2 — duration-matched bond ETF
      KeyRateFuture(KeyRateFutureSpec), // v2 — UST tenor-bucketed futures
      InflationSwap(InflationSwapSpec), // v2
      // Adding a strategy = add a variant + impl HedgeLeg. No other code path changes.
  }
  ```

  v1 ships **two variants** (BondFuture, InterestRateSwap). The `HedgeLeg` trait + tagged-enum dispatch is the seam that lets v1.x add `CashBond` (treasury hedge) and `Etf` (proxy hedge) without touching strategies that already work.

- **v1 specs.**
  - `BondFutureSpec`: `{ underlying_tenor: f64, conversion_factor: f64, contract_size_usd: f64, ctd_dv01_per_100: Decimal }`. DV01 = `ctd_dv01_per_100 × contract_size / 100 / conversion_factor`. CTD selection deferred to v2; for v1 use a representative deliverable per tenor.
  - `IRSwapSpec`: `{ tenor_years: f64, fixed_rate: Decimal, fixed_frequency, fixed_day_count, floating_index, notional_usd: Decimal }`. DV01 = `Σ fixed-leg PV01s` from the discount curve (post-LIBOR floating DV01 ≈ 0 at reset).

- **Effort.** M (1–2 days) for trait + two specs + tests. Each variant added later is S.

- **Crate placement.** `convex-analytics::risk::hedging::instruments` (new module). **No new crate.** A future is not a bond, so `convex-bonds::Bond` is the wrong trait; the lighter `HedgeLeg` is purpose-built.

- **Performance.** Cold path; one `.dv01()` call per proposal.

- **Future v2 promotion.** If CTD/repo modeling becomes a real concern, `BondFutureSpec`'s body migrates to a richer pricer; the `HedgeLeg` interface stays stable. Strategies stay decoupled from the upgrade.

---

## Gap 4 — `HedgeStrategy` trait + concrete strategies (DurationFutures, InterestRateSwap)

- **What is missing.** The trait that turns `(RiskProfile, Constraints)` into `HedgeProposal`, plus the two concrete strategies for v1.
- **Why it matters.** This is the advisor's core extensibility seam — every additional strategy (KeyRateFutures, ETFProxy, CashBondPair) plugs in here.
- **Severity.** **Blocker** for v1.
- **Effort.** M (1–2 days). Trait + two strategies + tests. Each strategy is straightforward DV01-matching arithmetic.
- **Trait shape.**
  ```rust
  pub trait HedgeStrategy {
      fn name(&self) -> &'static str;
      fn propose(
          &self,
          risk: &RiskProfile,
          constraints: &Constraints,
          ctx: &StrategyContext,
      ) -> Result<HedgeProposal, AnalyticsError>;
  }
  ```
- **Crate placement.** `convex-analytics::risk::hedging::strategies` (new module, `mod.rs` + `duration_futures.rs` + `interest_rate_swap.rs`). **No new crate.**
- **Performance.** Cold path per proposal. Each strategy: 1 KRD profile (already amortized in `RiskProfile`) + ~10 arithmetic ops + 1 cost-model call. Sub-microsecond after the upstream risk profile is computed.

---

## Gap 5 — Heuristic cost model

- **What is missing.** A clearly labeled cost model: `cost_bps_for(asset_class, tenor) -> Decimal`. No `TransactionCost` or `Slippage` exists anywhere.
- **Why it matters.** The advisor's "Cost" column needs *something* defensible and obviously labeled as a heuristic. Real feeds are v2.
- **Severity.** **High** (not a blocker — could ship with literal constants — but the demo's tradeoff narrative needs comparable cost numbers).
- **Effort.** S (a few hours). One trait, one default impl backed by a small `&'static [(AssetClass, Tenor, f64)]` table.
- **Trait shape.**
  ```rust
  pub trait CostModel {
      fn cost_bps(&self, instrument: &HedgeInstrument) -> f64;
      fn name(&self) -> &'static str;
  }
  pub struct HeuristicCostModel; // const-table backed, name() == "heuristic_v1"
  ```
- **Crate placement.** `convex-analytics::risk::hedging::cost`. **No new crate.**
- **Performance.** Cold path.
- **Provenance.** Every `HedgeProposal` echoes `cost_model: &'static str` so the trader knows the source.

---

## Gap 6 — `compare_hedges` aggregator

- **What is missing.** The aggregator that takes `Vec<HedgeProposal>` and produces `ComparisonReport { rows: Vec<ComparisonRow>, columns: Vec<&'static str>, recommendation_seed: Option<RecommendationSeed> }`.
- **Why it matters.** Side-by-side comparison is the trader's primary UI. The output is also the input to the narrator.
- **Severity.** **Blocker** for v1.
- **Effort.** S (a few hours). Pure transformation; no math.
- **Crate placement.** `convex-analytics::risk::hedging::compare`. **No new crate.**
- **Performance.** Cold path; <2 proposals in v1.
- **Determinism note.** Sort order matters for stable narration: keep insertion order from `propose_hedges`.

---

## Gap 7 — Template narrator

- **What is missing.** A `narrate(report: &ComparisonReport, style: NarrationStyle) -> String` that produces a deterministic paragraph from structured data. No LLM call.
- **Why it matters.** v1 explicitly excludes LLM narration; the deterministic narrator is the demo deliverable.
- **Severity.** **Blocker** for v1.
- **Effort.** S (a few hours). String formatting with explicit format strings.
- **Crate placement.** `convex-analytics::risk::hedging::narrate`. **No new crate.**
- **LLM placement argument (deferred to v2).**
  - **For inside `convex-analytics`:** simplest, keeps the advisor self-contained.
  - **Against:** introduces an HTTP client and an API key dependency to the analytics crate, polluting an otherwise pure-math library and breaking the hexagonal boundary.
  - **Verdict:** when v2 LLM narration arrives, it lives in a *new* crate `convex-narrator` (or in `convex-mcp` itself, since MCP already owns transport concerns). v1 stays template-only inside analytics. **No new crate for v1.**
- **Performance.** Cold path; not a constraint.

---

## Gap 8 — Four new MCP tools

- **What is missing.** `compute_position_risk`, `propose_hedges`, `compare_hedges`, `narrate_recommendation` — all unregistered.
- **Why it matters.** The advisor's external surface.
- **Severity.** **Blocker** for v1.
- **Effort.** S (an afternoon). Each tool is ~30 LOC of method on the `#[tool_router]` impl in `convex-mcp/src/server.rs`. The macro derives the schema and the router.
- **Crate placement.** `convex-mcp/src/server.rs` (extend the existing `#[tool_router] impl ConvexMcpServer`). **No new crate.**
- **Performance.** Tool dispatch overhead is microseconds; the math underneath is what we benchmark.
- **Provenance pattern.** Each tool's output struct must include the `provenance: Provenance` echo from the underlying analytics types — no new MCP-specific provenance plumbing.

---

## Gap 9 — Provenance fields on advisor outputs

- **What is missing.** Upstream `RiskResponse`, `SpreadResponse`, `PricingResult` carry numeric fields only — no `curves_used`, `cost_model`, `conventions`, `computed_at`, `advisor_version`.
- **Why it matters.** Trader sovereignty requires the trader to see *why* a number is what it is. We can't fix the upstream DTOs in scope, but the advisor can attach its own `Provenance` to every output.
- **Severity.** **High.**
- **Effort.** S. One small struct, embedded in every advisor output.
- **Crate placement.** `convex-analytics::risk::hedging::types::Provenance`. **No new crate.**
- **Performance.** Cold path. `Provenance` is small (a Vec<String> of curve ids, a `&'static str`, two enum fields).

---

## Gap 10 — Advisor benchmark

- **What is missing.** No bench measures the end-to-end "compute risk → propose → compare" path.
- **Why it matters.** The prompt's <5% regression bar needs a *baseline* to regress against. Today the closest we have is `convex-engine/benches/pricing_benchmarks.rs::single_bond_price`.
- **Severity.** **Medium.**
- **Effort.** S. One new file `convex-analytics/benches/hedge_advisor.rs` with three groups: `risk_profile_single_bond`, `propose_two_strategies`, `end_to_end`.
- **Crate placement.** `convex-analytics/benches/`. **No new crate.**
- **Performance plan.** Record before/after numbers in `docs/hedge-advisor-plan.md`. Watch for unintended allocations in the hot KRD loop.

---

## Gap 11 — Spread-mark variants beyond Z (deferred)

- **What is missing.** `price_from_mark` rejects OAS / I-spread / G-spread marks (`pricing.rs:89`).
- **Why it matters.** A trader who quotes "+50 OAS" can't run the advisor today.
- **Severity.** **Low** for v1 (the demo uses a price/Z-spread mark).
- **Effort.** M when tackled. Each variant requires its own root-finder over the existing spread calculator.
- **Crate placement.** Wherever it gets done, `convex-analytics::pricing`. **Out of v1 scope.**

---

## Gap 12 — FX delta (deferred)

- **What is missing.** No FX risk anywhere.
- **Severity.** **Low** for v1 (single-currency scope).
- **Crate placement.** When it lands, `convex-analytics::risk::fx_delta`. **Out of v1 scope.**

---

## Gap 13 — Per-benchmark partial spread DV01 (deferred)

- **What is missing.** Only generic spread duration exists.
- **Severity.** **Low** for v1.
- **Crate placement.** When it lands, `convex-analytics::risk::duration::partial_spread`. **Out of v1 scope.**

---

## Where a new crate could plausibly be justified — and why we still aren't proposing one

The only candidate is **LLM narration** (Gap 7's v2 form). Once the narrator calls Anthropic over HTTP, it brings in `reqwest`/`anthropic-sdk` and an API-key environment dependency — neither belongs in `convex-analytics`, which today is pure math + bonds + curves. At that point the right thing is a new crate `convex-narrator` *or* placement inside `convex-mcp` (which already owns transport).

**For v1 we are not building that.** The template narrator is deterministic, dependency-free, and lives in `convex-analytics::risk::hedging::narrate`. A new crate today would be premature. We will revisit when v2 LLM narration is approved.

---

## Crate impact summary (v1)

| Crate | Change |
| --- | --- |
| `convex-analytics` | Add `risk::profile` (new module — `RiskProfile` + `compute_position_risk`). Add `risk::hedging::{types, instruments, strategies, cost, compare, narrate}` (new modules). Add `[[bench]] hedge_advisor`. Re-export new types from `risk::mod.rs`, `risk::hedging::mod.rs`, and the prelude. |
| `convex-mcp` | Add four `#[tool]` methods to the existing `#[tool_router] impl ConvexMcpServer`. Bring in any new schema-bearing types from `convex-analytics` via the `convex` umbrella (already `features = ["schemars"]`). |
| All other crates | Untouched. |
| New crates | **None.** |

---

## Performance bar

- **No regression > 5%** on existing `convex-bonds` and `convex-engine` benches (we don't touch those code paths).
- **Advisor end-to-end (single bond, 4 KRD tenors, 2 strategies)** target: < 200 µs on the bench machine. KRD profile dominates; we reuse `key_rate_profile` directly.
- **No heap allocations** in `RiskProfile`/`HedgeProposal` *construction* hot-path beyond the KRD `Vec<(f64, f64)>` already owned by the curve bumping primitive. JSON serialization is allowed to allocate.

---

## Resolved design decisions (per review)

1. **`compute_position_risk` is a risk-module citizen, not a hedging-module citizen.** It lives at `convex_analytics::risk::profile::compute_position_risk`, alongside `BondRiskCalculator`. The hedging module *consumes* `RiskProfile` but does not own it. This keeps the function discoverable to any caller computing per-position risk, without making them depend on the hedging stack.

2. **`HedgeInstrument` is an open tagged enum, not future-only.** v1 ships `BondFuture` and `InterestRateSwap`. The `HedgeLeg` trait + enum-variant pattern is the extensibility seam — `CashBond`, `Etf`, `KeyRateFuture`, `InflationSwap` slot in as new variants without touching existing strategies. v1 deliberately limits to two variants for demo scope; the architecture does not.

These decisions feed directly into `docs/hedge-advisor-plan.md` (Phase 3).
