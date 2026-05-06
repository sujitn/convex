# AI Hedge Advisor — Phase 1 Investigation

Read-only audit of the Convex workspace, framing what the hedge advisor can reuse vs. what is genuinely missing. Each finding cites `file:line` and is tagged ✅ (exists and usable), ⚠️ (exists but needs adaptation), ❌ (missing).

The investigation answered the eight areas from the prompt against the workspace at HEAD on branch `accuracy-tier1-fixes`.

> **v1 status (post-implementation).** All blocker gaps closed; advisor ships end-to-end via four MCP tools. Demo `Apple 4.85% '34 long $10mm` round-trips through `compute_position_risk → propose_hedges → compare_hedges → narrate_recommendation`. See `docs/perf-baselines.md` for benchmarks and `docs/hedge-advisor-plan.md` for the implementation log.

---

## 1.1 Risk and pricing infrastructure

| Item | Status | Evidence |
| --- | --- | --- |
| **`compute_position_risk` advisor entry point** | ✅ (added) | `risk/profile.rs::compute_position_risk` — wires `price_from_mark` + `BondRiskCalculator` + Bloomberg-parity KRD via `KeyRateBump` + `ZSpreadCalculator::price_with_spread`. |
| **Hedge advisor MCP tools** | ✅ (added) | `convex-mcp/src/server.rs` — `compute_position_risk`, `propose_hedges`, `compare_hedges`, `narrate_recommendation`. End-to-end test at `server.rs::hedge_advisor_e2e_apple_10y`. |
| **Bond future + swap leg risk** | ✅ (added) | `risk/hedging/instruments.rs` — `bond_future_risk` (synthetic CTD per contract code, Z-flat to curve, ÷ CF) + `interest_rate_swap_risk` (synthetic fixed leg, sign-flipped for `PayFixed`). |
| **DurationFutures + InterestRateSwap strategies** | ✅ (added) | `risk/hedging/strategies.rs` — DV01-neutral sizing, residual KRD computed via `residual_from`, par-swap rate from curve DFs. |
| **HeuristicCostModel** | ✅ (added) | `risk/hedging/cost.rs` — labeled `name() == "heuristic_v1"`, echoed on every proposal's `Provenance`. |
| **Template narrator** | ✅ (added) | `risk/hedging/narrate.rs` — deterministic; no LLM. |
| DV01 / PV01 | ✅ | `crates/convex-analytics/src/risk/dv01.rs:68` `dv01_from_duration` (analytical) and `:84` `dv01_from_prices` (finite-diff). Wrapper at `risk/calculator.rs:160` `BondRiskCalculator::dv01_per_100`. |
| Modified duration | ✅ | `risk/duration/modified.rs:19` `modified_duration` divides Macaulay by the appropriate `1+y/f` divisor across compounding modes. |
| Macaulay duration | ✅ | `risk/duration/macaulay.rs:35` PV-weighted cash-flow times; handles continuous, simple, periodic. |
| Key-rate duration | ✅ | `risk/duration/key_rate.rs:73` `key_rate_duration_at_tenor` (FD at one tenor) + `STANDARD_KEY_RATE_TENORS` (12 tenors at line 11). End-to-end profile: `convex-curves/src/bumping/key_rate.rs:370` `key_rate_profile<T,F>(curve, price_fn, shift_bps) -> Vec<(tenor, dv01)>`. The advisor can call this directly. |
| Effective duration / convexity | ✅ | `risk/duration/effective.rs:34`, `risk/convexity/effective.rs:24`. Already used for callables. |
| Spread duration | ✅ | `risk/duration/spread_duration.rs:20`; FD on a parallel spread shock. |
| Partial spread DV01 (per benchmark / segment) | ❌ | Only generic spread duration exists. No per-benchmark or per-segment partial spread DV01. Easy to derive (shock spread on one segment, reprice). |
| Hedge ratios | ✅ | `risk/hedging/hedge_ratio.rs:18` `dv01_hedge_ratio` and `:39` `duration_hedge_ratio`. `HedgeRecommendation { notional, direction, residual_dv01 }` at `:60`. |
| Lightweight portfolio aggregation | ✅ | `risk/hedging/portfolio.rs:23` `Position { id, market_value, duration, dv01 }` and `:55` `aggregate_portfolio_risk` (single-pass fold → `PortfolioRisk`). |
| Curve bumping primitive | ✅ | `convex-curves/src/bumping/{parallel,key_rate,scenario}.rs`. `KeyRateBump::new(tenor, bps)` (triangular weight, zero-copy `BumpedCurve`). `ScenarioBump` composes multiple shocks. |
| FX delta | ❌ | Grep across the workspace returns zero. Not blocking for v1 (scope is single-currency). |

**Synthesis.** The hedge advisor can reuse every quantitative primitive it needs — DV01, modified/effective duration, KRD via `key_rate_profile`, spread duration, hedge ratios, and curve bumping. The lightweight `Position` in `convex-analytics::risk::hedging::portfolio` is the natural input shape for `aggregate_portfolio_risk` and is what hedge proposals should produce as residuals. Real instrument provenance (bond + classification + currency + fx) lives in `convex-portfolio::Holding` (§1.2).

---

## 1.2 Position and book modeling

| Item | Status | Evidence |
| --- | --- | --- |
| Rich position type | ✅ | `convex-portfolio/src/types/holding.rs:206` `Holding { id, identifiers, par_amount: Decimal, market_price: Decimal, accrued_interest: Decimal, fx_rate, currency, analytics: HoldingAnalytics, classification }` with `#[derive(Debug, Clone, Serialize, Deserialize)]`. Builder at `:315`. |
| Lightweight risk position | ✅ | `convex-analytics/src/risk/hedging/portfolio.rs:23` `Position { id, market_value: f64, duration, dv01 }` — minimal numeric shape. |
| Portfolio container | ✅ | `convex-portfolio/src/portfolio/portfolio.rs:8` `Portfolio { id, name, base_currency, as_of_date, holdings: Vec<Holding>, cash: Vec<CashPosition>, shares_outstanding, liabilities }`, builder at `portfolio/builder.rs`. |
| Notional representation | ✅ | `Decimal` `par_amount` (absolute notional, not per-share). Market value computed `par × price/100` (`holding.rs:246`). |
| Currency tracking | ✅ | Holding-level `currency` + `fx_rate`, portfolio-level `base_currency`. `Portfolio::is_multi_currency()` at `:175`. |
| Immutability / Clone | ✅ | `Holding` is `Clone + Serialize + Deserialize` (no `Copy` due to `String`/`Decimal`). Portfolio owns `Vec<Holding>` directly — no `Arc` indirection. |
| `JsonSchema` derive | ⚠️ | `Holding`/`Portfolio` do not derive `JsonSchema` today. The MCP `price_bond` path never serializes a `Holding` — it accepts a `BondRef`. For v1 the advisor can match this pattern. |
| Aggregation pattern | ✅ | `Portfolio::securities_market_value`, `total_dv01`, `nav`, `calculate_weights` (`:58–148`). The `convex-portfolio/src/analytics/{nav,risk,key_rates,spreads,liquidity,credit}.rs` modules consume pre-computed `HoldingAnalytics` and roll up. |
| Mark on the position | ❌ | `Holding` stores `market_price: Decimal` only. There is no `Mark` field (no spread/yield variant). The advisor must compute the position's risk by feeding `Mark::Price{value, kind: Clean}` into `price_from_mark`. The hedge proposal layer should keep position state mark-agnostic for the same reason — store the user's `Mark` separately and reduce to risk on demand. |

**Synthesis.** Two position types coexist: `convex-portfolio::Holding` (rich, persistable, classification-bearing) and `convex-analytics::risk::hedging::Position` (numeric DTO for aggregation). Both belong; the hedge advisor will *consume* a `Holding`-or-`BondRef` + `Mark` and *produce* `Position`-style risk profiles for residual analysis. No portfolio-side refactor is required for v1 (single-position scope).

---

## 1.3 Instrument modeling

| Item | Status | Evidence |
| --- | --- | --- |
| Bond shapes | ✅ | `convex-bonds/src/instruments/{fixed_rate,callable,floating_rate,zero_coupon,sinking_fund,callable_frn}.rs`. Each implements `Bond + BondCashFlow` plus role-specific traits (`FixedCouponBond`, `EmbeddedOptionBond`, `FloatingCouponBond`, `AmortizingBond`). |
| Bond traits unified | ✅ | `convex-bonds/src/traits/` exposes `Bond`, `FixedCouponBond`, `FloatingCouponBond`, `EmbeddedOptionBond`, `AmortizingBond`, `BondCashFlow`. |
| `Future` / `Swap` / `FRA` / `OIS` types | ⚠️ | `convex-curves/src/calibration/instruments.rs` has `Future` (:456), `Swap` (:577), `Fra` (:317), `Ois` (:747). They implement `CalibrationInstrument` only — they exist to *build curves*, not to be held as positions. They have no DV01, no notional in the position sense, no mark. |
| Bond futures (CTD-aware) | ❌ | No deliverable basket, conversion factor, or repo financing model anywhere. |
| Cash IRS as a position | ❌ | The curve-side `Swap` does not compute a position-level NPV/DV01 from a `Mark`. |
| ETF instrument | ⚠️ | `convex-portfolio/src/etf/{basket,nav,sec,mod}.rs` implements creation/redemption analytics but does not define a "holdable ETF" position type. ETF *proxy* hedging would need a notional + delta abstraction. |
| Generic `Instrument` trait | ❌ | No unified `Instrument` trait spans bonds, swaps, futures. `BondType` enum at `convex-traits/src/reference_data.rs:25` enumerates bond shapes only. |

**Where new hedge instruments fit.** Two viable placements; both avoid a new crate.

- **A. `convex-bonds/src/instruments/`.** Pro: existing `Bond`-trait infrastructure, FFI/MCP routing, registry, and `Holding` consumer expect this shape. Con: a `BondFuture` is not a bond — coupon/accrual semantics don't apply, and shoehorning would dilute the `Bond` contract.
- **B. New `crates/convex-bonds/src/instruments/derivatives/` submodule (or `convex-portfolio/src/instruments/`).** Pro: clean separation, lets `BondFuture` and `InterestRateSwap` carry their own `RiskFromMark` / `Hedgeable` trait. Con: adds a parallel trait surface; needs a tiny abstraction the rest of the codebase doesn't yet have.

For the v1 advisor, **a thin DTO-only approach is sufficient**: define `HedgeInstrument` (tagged enum: `BondFutureSpec | IRSwapSpec`) inside the new `risk::hedging::strategies` module, and compute their DV01 with closed-form approximations (futures DV01 ≈ CTD bond DV01 / conversion factor; swap DV01 from leg PVs). This defers the architectural question until a v2 needs holdable instrument positions.

**Synthesis.** Bonds are well typed; derivatives are curve-only today. New hedge instruments should ship as analytical DTOs under `convex-analytics::risk::hedging::strategies` for v1, with a clear path to promote them to `convex-bonds/src/instruments/derivatives/` when CTD/repo modeling lands.

---

## 1.4 MCP tool surface

| Item | Status | Evidence |
| --- | --- | --- |
| Transport | ✅ | `convex-mcp/src/main.rs:62` stdio (default), `:82` Axum-mounted streamable HTTP at `/mcp` (feature-gated). |
| SDK | ✅ | `rmcp = 0.12` with `server, macros` features (`Cargo.toml:23–49`). |
| Schema derivation | ✅ | All `Parameters<…>` structs `#[derive(JsonSchema)]` via `rmcp::schemars` (server.rs imports `use rmcp::schemars::JsonSchema` at top). |
| Tool registration | ✅ | `#[tool_router]` macro on `impl ConvexMcpServer` at `server.rs:583`. Each tool is a method with `#[tool(description=…)]`. Adding a tool = add a method. |
| Existing tools | ✅ | `create_bond`, `price_bond` (Mark-aware), `calculate_yield`, `create_curve`, `get_zero_rate`, `bootstrap_curve`, `compute_spread` (Z/I/G), `make_whole_call_price`, `list_all_bonds`, `list_all_curves`. (server.rs:586–937). |
| Mark on the wire | ✅ | `price_bond` already takes `params.mark: Mark` directly (server.rs:631). The hedge advisor can use `Mark` end-to-end without translation. |
| Inline-or-stored references | ✅ | `BondRef` / `CurveRef` accept either a registry id or a full inline spec. The hedge advisor's tools can use the same pattern. |
| Output schemas | ⚠️ | Outputs are returned via `Self::json_result(&Output)` which serializes through `serde_json` into a `Content::text(...)`. They are *not* surfaced as MCP `outputSchema`. Field names carry units (`clean_price_per_100`, `ytm_pct`, `z_spread_bps`) but the client cannot statically rely on the shape. |
| Provenance on outputs | ⚠️ | `PriceBondOutput` carries `bond_id`, `curve_id`, `currency`, `day_count` (server.rs:486). `RiskResponse` (DTO at `convex-analytics/src/dto.rs:374`) does *not* carry curve/convention provenance. |
| Error envelope | ✅ | `convex-mcp/src/error.rs:17` three typed variants (`InvalidInput`, `ConvergenceFailure`, `CalculationFailed`) → JSON-RPC -32602 / -32603 with `data.code` discriminator. |

**Notes from `docs/mcp-audit.md` and `docs/mcp-gaps.md`.** Both documents pre-date the current Mark wiring and now overstate the gap. The Mark enum *is* implemented, parsed end-to-end through `price_bond`, and accepted by `RiskRequest`/`SpreadRequest` (`dto.rs:317`). Other gaps remain: stateful registries (G-2), hard-coded conventions on `create_bond` (G-3), no MCP `outputSchema` (G-4), no provenance on risk/spread responses (G-5). The hedge advisor should design *with* these gaps in mind — i.e., echo provenance in its own outputs even if the upstream `RiskResponse` doesn't yet.

**Synthesis.** The MCP surface is in good shape: macro-driven registration, schema-derived inputs, Mark-native, and error-typed. Adding four new tools (`compute_position_risk`, `propose_hedges`, `compare_hedges`, `narrate_recommendation`) is purely additive. We should hand-attach provenance fields (`curves_used`, `cost_model`, `conventions`) to every advisor output since the upstream DTOs don't carry them.

---

## 1.5 Cost and market-data abstractions

| Item | Status | Evidence |
| --- | --- | --- |
| Market-data sources | ✅ | `convex-traits/src/market_data.rs` defines `QuoteSource` (:71), `CurveInputSource` (:163), `IndexFixingSource` (:212), `VolatilitySource` (:326), `FxRateSource` (:394), `InflationFixingSource` (:467), `EtfQuoteSource` (:543). Aggregator struct `MarketDataProvider` at `:696` holds `Arc` source pointers. |
| Bond quote DTO | ✅ | `BondQuote` at `:577` (bid/mid/ask price + yield + size + timestamp + staleness). |
| Liquidity model | ✅ | `convex-portfolio/src/analytics/liquidity.rs`: `weighted_bid_ask_spread` (:189), `LiquidityBucket::classify` (:256), `liquidity_distribution` (:341), days-to-liquidate from ADV heuristic (:426). |
| Execution cost / commission / slippage | ❌ | No `TransactionCost`, `Slippage`, `Commission`, or `ExecutionCost` types anywhere. |
| Centralized constants module | ❌ | Defaults are scattered: `dto.rs:62` `default_thirty_360_us`, `:159` `default_act_360`, liquidity thresholds at `liquidity.rs:258`. There is no `convex-core::defaults` or `convex-analytics::constants`. |
| Provenance on outputs | ❌ | `PricingResult`, `RiskResponse`, `SpreadResponse` carry numeric fields only. Curve identity, build method, conventions, mark-source are not echoed. |

**Synthesis.** Market-data abstractions are richer than expected — every source the hedge advisor would need (quotes, curves, FX, vols, ETF quotes) already has a trait. The cost story is the real gap: no transaction-cost or slippage abstraction exists. For the v1 advisor we should ship a clearly labeled `HeuristicCostModel` (constant bp/contract per asset class) inside `convex-analytics::risk::hedging::cost` and tag every output with a `cost_model: "heuristic_v1"` field so the source is unambiguous. A `MarketCostSource` trait can be added later when real feeds arrive.

---

## 1.6 LLM integration

| Item | Status | Evidence |
| --- | --- | --- |
| LLM SDK in any `Cargo.toml` | ❌ | No `anthropic-sdk-rust`, `async-anthropic`, `openai`, `bedrock` anywhere in the workspace. |
| LLM API calls in source | ❌ | Grep for `anthropic`, `openai`, `claude`, `messages.create`, `chat.completions`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY` returns zero matches. |
| Narrative templating utilities | ❌ | No `narrate`, `narrator`, `recommendation` text-builder. `HedgeRecommendation` at `risk/hedging/hedge_ratio.rs:60` is a numeric DTO. `convex-portfolio/src/analytics/summary.rs` is a numeric summary too. |
| The MCP server *as* the LLM surface | ✅ | `convex-mcp` already exposes the schema-typed surface that an external LLM (Claude Desktop, agent harness) can call. The advisor's "narration" can be done *outside* the workspace by the calling agent — the workspace just needs to return rich enough structured output. |

**Synthesis.** Zero LLM infrastructure exists — and per the v1 scope, that's the right state to stay in. The v1 narrator must be **template-only** (deterministic Rust string formatting consuming the structured `ComparisonReport`). LLM-based narration is explicitly v2. This keeps the core dependency-free and lets external agents do the freeform writing if they want.

---

## 1.7 Performance benchmarks

| Item | Status | Evidence |
| --- | --- | --- |
| Benchmark harness | ✅ | `criterion = "0.8"` in workspace `Cargo.toml`. |
| Existing benches | ✅ | `convex-analytics/benches/spread_pv_kernel.rs` (single-eval + Brent envelope), `convex-bonds/benches/trinomial_tree.rs` (straight + callable), `convex-engine/benches/pricing_benchmarks.rs` (single bond, batch sequential/parallel sized 10–1000, ETF iNAV, portfolio analytics, duration contribution, curve operations). |
| Output baseline numbers in repo | ❌ | No `µs`/`ns`/`microsecond` strings in any markdown. README mentions "Run benchmarks" without a published table. Criterion artifacts live only in `target/criterion/`. |
| Bench profile | ✅ | `[profile.bench]` inherits release with debug info preserved (workspace `Cargo.toml`). |

**Synthesis.** Criterion is wired and a generous suite already exists. There is no published baseline, so the regression bar is "run before-and-after on the same machine." For the hedge advisor we should: (a) add an `advisor_bench.rs` that prices a bond + computes DV01 + computes a 12-tenor KRD profile in one bench, (b) check `propose_hedges(DurationFutures + IRSwap)` ends end-to-end in well under 1 ms, (c) record the numbers as a check-in baseline in `docs/hedge-advisor-plan.md` or `docs/perf-baselines.md`.

---

## 1.8 The Mark contract

| Item | Status | Evidence |
| --- | --- | --- |
| `Mark` enum | ✅ | `convex-core/src/types/mark.rs:39` `Mark { Price{value, kind}, Yield{value, frequency}, Spread{value: Spread, benchmark} }`. Tagged `#[serde(tag = "mark", rename_all = "snake_case")]`. |
| Derives | ⚠️ | `Debug, Clone, PartialEq, Eq, Serialize, Deserialize` and `JsonSchema` (feature-gated by `schemars`). **Not `Copy`** — the `Spread` variant carries a `String` benchmark id. The advisor must clone it where needed (cheap; one short string). |
| `Mark::from_str` | ✅ | Implemented (mark.rs:90). Accepts `99.5`, `99.5C`, `99-16`, `99-16+`, `4.65%@SA`, `+125bps@USD.SOFR`, `125 OAS@USD.TSY`, etc. |
| Mark-aware pricing entry | ✅ | `convex-analytics/src/pricing.rs:47` `price_from_mark<B: Bond + FixedCouponBond>(bond, settle, mark, curve, quote_freq) -> PricingResult`. Reduces every variant to a dirty price; YTM derived from clean. |
| Spread variant fully wired | ⚠️ | `pricing.rs:89` rejects any `SpreadType` other than `ZSpread`: `"{} mark not yet supported (only Z-spread)"`. OAS/I-spread/G-spread marks don't yet round-trip through `price_from_mark`. The v1 advisor ingests price or yield marks (or Z-spread) — that's enough. |
| Mark on the wire | ✅ | `convex-analytics/src/dto.rs:317` `MarkInput { Text(String), Parsed(Mark) }` (untagged) used by `PricingRequest`, `RiskRequest`, `SpreadRequest`. The MCP `price_bond` already deserializes it directly. |
| FRN engine route | ⚠️ | `convex-engine/src/pricing_router.rs:68` exposes `PricingInput` with `market_price_bid/mid/ask` (no Mark). The advisor should route through `convex-analytics::pricing::price_from_mark` rather than the engine's price-only path. |

**Synthesis.** Mark sovereignty is real and end-to-end for the price/yield path. Spread marks are partially wired (Z-spread only), which is enough for v1. The advisor should *consume* `Mark` on input (so a trader can say "long $10mm AAPL @ +85 over UST") and *re-emit* the post-hedge marks of every leg as structured `Mark`s in its proposals — this preserves trader sovereignty across the round-trip.

---

## Summary of gaps to address

| # | Gap | Severity | Where it'll live |
| --- | --- | --- | --- |
| 1 | New domain types: `RiskProfile`, `HedgeInstrument`, `HedgeProposal`, `HedgeTrade`, `TradeoffNotes`, `ComparisonReport`, `Constraints`, `Provenance` | High | `convex-analytics::risk::hedging::types` (existing module) |
| 2 | `HedgeStrategy` trait + `DurationFutures` and `InterestRateSwap` strategies | High | `convex-analytics::risk::hedging::strategies` (new module in existing crate) |
| 3 | Heuristic cost model (commission bp/contract per asset class) | Medium | `convex-analytics::risk::hedging::cost` (new module in existing crate) |
| 4 | Template narrator (no LLM) | Medium | `convex-analytics::risk::hedging::narrate` (new module in existing crate) |
| 5 | Four MCP tools: `compute_position_risk`, `propose_hedges`, `compare_hedges`, `narrate_recommendation` | High | `convex-mcp/src/server.rs` (extend existing `#[tool_router]` impl) |
| 6 | Provenance fields on every advisor output | High | DTOs in (1) carry `curves_used: Vec<String>`, `cost_model: &'static str`, `conventions: Conventions` |
| 7 | Advisor-specific bench in `convex-analytics/benches/` | Low | New file, single-position end-to-end timing |
| 8 | Optional: light wrapper `HedgeInstrument` analytical DTOs (BondFutureSpec, IRSwapSpec) with closed-form DV01 | High | Co-located with strategies under (2) |
| 9 | Spread-variant `price_from_mark` for OAS / I-spread / G-spread | Low (deferred) | Out of v1 scope |
| 10 | FX delta | Low (deferred) | Out of v1 scope |
| 11 | Per-benchmark partial spread DV01 | Low (deferred) | Out of v1 scope |

**Crate impact: zero new crates needed for v1.** All work fits inside `convex-analytics` (new modules under `risk/hedging/`) and `convex-mcp` (new `#[tool]` methods).

---

## Reusable primitives — the shopping list

When we build the strategies, we will lean on these:

- `convex_analytics::pricing::price_from_mark` — reduce any `Mark` to dirty price + accrued + YTM.
- `convex_analytics::risk::dv01::{dv01_from_duration, dv01_from_prices}` — DV01 in either form.
- `convex_analytics::risk::duration::{modified, macaulay, effective, key_rate, spread_duration}` — the full duration ladder.
- `convex_curves::bumping::{ParallelBump, KeyRateBump, key_rate_profile, ScenarioBump}` — every shock we need.
- `convex_analytics::risk::hedging::{hedge_ratio, portfolio}` — DV01/duration ratios + portfolio aggregator.
- `convex_core::types::{Mark, Price, Yield, Spread, SpreadType, Frequency, Currency, Date}` — type-safe inputs throughout.
- `convex_curves::calibration::instruments::{Future, Swap}` — these are *curve-side* models but they encode the right deliverable/leg arithmetic that an analytical `BondFutureSpec`/`IRSwapSpec` DV01 estimator can mirror.

---

## Open questions to resolve in Phase 2

1. **Strategy placement.** Does `risk::hedging::strategies` fit `convex-analytics`'s charter? (My read: yes — it's analytics on top of bond risk, no new infra.)
2. **`HedgeInstrument` shape.** Closed-form DV01-only DTOs vs. real `convex-bonds` instruments. Phase 2 will recommend DTO-only for v1.
3. **Where the `Mark`-bearing input enters the advisor.** I propose `compute_position_risk` takes `bond: BondRef`, `mark: Mark`, `notional: Decimal`, `curve: CurveRef`, `key_rate_tenors: Vec<f64>` (default 2/5/10/30) — mirrors `RiskRequest` exactly.
4. **Provenance schema.** A small `Provenance { curves: Vec<String>, cost_model: &'static str, conventions: ConventionsSnapshot, computed_at: DateTime, advisor_version: &'static str }` echoed on every output.
5. **Whether to reuse `risk::hedging::Position` for residual reporting** or to introduce a new `ResidualRisk` DTO that also carries KRD vector. (Lean: new DTO, KRD is the whole point.)

These get answered in `docs/hedge-advisor-gaps.md` (Phase 2).
