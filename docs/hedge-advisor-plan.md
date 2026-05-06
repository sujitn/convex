# AI Hedge Advisor — Phase 3 Fix Plan

Source: `docs/hedge-advisor-investigation.md` (Phase 1) + `docs/hedge-advisor-gaps.md` (Phase 2). This plan turns the gap list into an ordered, reviewable implementation sequence with concrete files, tests, benchmarks, and risk notes.

**Top-line.** Zero new crates. Eleven commits, smallest first. ~M total effort. Demo target: Apple 4.85% May-2034 single-position hedged via `DurationFutures` + `InterestRateSwap`, narrated by a deterministic template.

---

## 3.1 Crate impact summary

### Dependency graph (before)

```
convex-mcp -> convex (umbrella, schemars feature) -> convex-analytics -> convex-bonds, convex-curves, convex-core, convex-math
                                                  -> convex-portfolio (separate)
                                                  -> convex-engine    (separate)
```

### Dependency graph (after)

```
convex-mcp -> convex (umbrella, schemars feature) -> convex-analytics -> convex-bonds, convex-curves, convex-core, convex-math
                                                                       └── (NEW) risk::profile, risk::hedging::{types, instruments, strategies, cost, compare, narrate}
                                                  -> convex-portfolio (separate, unchanged)
                                                  -> convex-engine    (separate, unchanged)
```

**No edge added or removed.** All work fits inside two existing crates.

### Crate-by-crate change list

| Crate | Files added | Files modified | LOC est. |
| --- | --- | --- | --- |
| `convex-analytics` | `src/risk/profile.rs`; `src/risk/hedging/types.rs`, `instruments.rs`, `cost.rs`, `compare.rs`, `narrate.rs`, `strategies/mod.rs`, `strategies/duration_futures.rs`, `strategies/interest_rate_swap.rs`; `benches/hedge_advisor.rs` | `src/risk/mod.rs` (declare `profile`), `src/risk/hedging/mod.rs` (declare new submodules), `src/lib.rs` prelude (re-exports), `Cargo.toml` (`[[bench]]`) | ~1500 |
| `convex-mcp` | — | `src/server.rs` (4 new `#[tool]` methods + their parameter/output structs) | ~400 |
| All other crates | none | none | 0 |
| **New crates** | **0** | — | — |

### Justification for not creating a new crate

Re-checked against the CLAUDE.md "Conventions for adding new functionality" table:
- "New analytic" → add to existing crate. Hedge advisor is an analytic.
- The `risk::hedging` module already exists with `Position`, `PortfolioRisk`, `HedgeRecommendation`. Adding domain types, strategies, and cost models there is the documented pattern.
- The MCP layer's pattern is "add `#[tool]` methods to `ConvexMcpServer`." Four new methods is a routine extension.
- The only candidate for a new crate (LLM narrator) is explicitly v2 — see `docs/hedge-advisor-gaps.md` §7.

---

## 3.2 Performance plan

### Hot paths touched

| Path | Existing latency | Risk |
| --- | --- | --- |
| `compute_position_risk` (1 price + 1 DV01 + N-tenor KRD) | ~1–10 µs per price (existing benches), KRD ≈ N × price | Allocation in the KRD `Vec<(f64, f64)>` per call. Acceptable: one Vec alloc per profile. |
| `propose_hedges` (2 strategies × ~10 fp ops) | n/a (new) | Trivial, dominated by upstream risk profile. |
| `compare_hedges` / `narrate_recommendation` | n/a (new) | Cold path; not benched. |
| Existing `convex-bonds`/`convex-engine` benches | published in `target/criterion/` | **Untouched code paths — no regression possible.** |

### Allocation budget for advisor end-to-end (single position, 4 KRD tenors, 2 strategies)

- 1 `Vec<(f64, f64)>` for KRD profile (4 elements).
- 2 `HedgeProposal` instances (each ~1 small `Vec<HedgeTrade>` + `TradeoffNotes` strings).
- 1 `ComparisonReport` with 2 rows.
- 1 narrator `String` (~500 bytes).

**Total: under 10 short-lived allocations per end-to-end call.** No `Vec` resizes if we `with_capacity` the KRD.

### Bench plan

New file `crates/convex-analytics/benches/hedge_advisor.rs` with three groups:

1. `risk_profile_apple_10y` — `compute_position_risk` for the demo bond against a 12-tenor SOFR curve.
2. `propose_two_strategies` — `propose_hedges` consuming a pre-built `RiskProfile`.
3. `end_to_end` — full `RiskProfile → propose → compare → narrate` chain.

**Targets:**

- `risk_profile_apple_10y`: < 100 µs (dominated by KRD = 12 × FixedRateBond price; existing single-bond price is ~1 µs).
- `propose_two_strategies`: < 5 µs.
- `end_to_end`: < 200 µs.

**Regression bar.** Re-run `convex-bonds/benches/trinomial_tree.rs` and `convex-engine/benches/pricing_benchmarks.rs` before-and-after the advisor PR. Any change > 5% on those existing benches is a regression to investigate. New advisor benches go into `docs/perf-baselines.md` as the v1 baseline for future PRs to regress against.

### Stack-vs-heap decisions

- `RiskProfile`, `HedgeProposal`, `HedgeTrade`, `TradeoffNotes`, `ComparisonReport` — owned (heap) for serde/serialization. Cold path; ergonomics wins over zero-alloc.
- `compute_position_risk` internals — pass curve as `&dyn RateCurveDyn` (existing pattern). Use `key_rate_profile`'s output `Vec` directly, no copy.
- `HedgeStrategy::propose` — takes `&RiskProfile`, `&Constraints`, `&StrategyContext`. Returns owned `HedgeProposal`. No lifetimes leaked into the trait.
- Cost model is `&dyn CostModel`, not generic — keeps `Vec<Box<dyn HedgeStrategy>>` possible for v2's strategy registry.

---

## 3.3 Implementation sequence

Eleven commits. Each is reviewable on its own; tests precede impl where TDD applies. **Stop and surface anything unexpected** at every step.

### Commit 1 — domain types (S, ~3 hours)

**Files added.**
- `crates/convex-analytics/src/risk/profile.rs` (just `RiskProfile` struct + provenance — function comes in commit 2).
- `crates/convex-analytics/src/risk/hedging/types.rs` (everything else: `HedgeInstrument` enum stub, `HedgeTrade`, `HedgeProposal`, `TradeoffNotes`, `Constraints`, `ComparisonReport`, `ComparisonRow`, `ResidualRisk`, `Provenance`, `MarketContext`, `NarrationStyle`, error variants).

**Files modified.**
- `crates/convex-analytics/src/risk/mod.rs` — `pub mod profile;` + re-export `RiskProfile`.
- `crates/convex-analytics/src/risk/hedging/mod.rs` — `pub mod types;` + re-export.
- `crates/convex-analytics/src/lib.rs` prelude — add the new types.

**Tests added.**
- `cargo test -p convex-analytics --lib risk::hedging::types::serde_round_trip` for every new type.
- `cargo test -p convex-analytics --lib risk::profile::serde_round_trip`.
- A schemars test that asserts `RiskProfile::json_schema()` and `HedgeProposal::json_schema()` produce non-empty schemas (the MCP tools depend on this).

**Benchmarks.** None.

**Estimated effort.** ~3 hours.

**Decisions to confirm.** Field names + units. Proposed:
- `dv01_per_bp: f64` (P&L from 1bp parallel rate shift)
- `modified_duration_years: f64`
- `key_rate_durations: Vec<KeyRateDuration>` where `KeyRateDuration { tenor_years: f64, dv01_per_bp: f64 }`
- `notional_usd: Decimal`
- `cost_bps: f64`
- `cost_total_usd: Decimal`

### Commit 2 — `compute_position_risk` (S, ~3 hours)

**Files modified.**
- `crates/convex-analytics/src/risk/profile.rs` — add `compute_position_risk<B>(...)` body.

**What it does.** Calls `price_from_mark` → `BondRiskCalculator::all_metrics()` → `key_rate_profile`. Scales DV01 by notional. Stamps a `Provenance`.

**Tests added.**
- Round-trip: known-yield mark on a hardcoded fixed bond should produce known DV01 within 1e-8.
- KRD sum should approximately equal parallel DV01 within 1% (sanity check).
- Test that `Mark::Price{Clean}` and equivalent `Mark::Yield` produce the same DV01.

**Benchmarks.** None yet.

**Estimated effort.** ~3 hours.

### Commit 3 — `HedgeLeg` trait + `BondFutureSpec` (M, ~6 hours)

**Files added.**
- `crates/convex-analytics/src/risk/hedging/instruments.rs` — `HedgeLeg` trait + `HedgeInstrument` enum + `BondFutureSpec` impl. (Other variants stubbed with `unimplemented!` and `#[allow(dead_code)]` for now; we fill `IRSwapSpec` next commit.)

**Tests added.**
- `BondFutureSpec` DV01 against a hand-computed reference (CTD DV01 ÷ conversion factor × contract size ÷ 100). Use representative TY (10Y) numbers.
- `key_rate_profile` for `BondFutureSpec`: bump-and-reprice the underlying CTD; the future's KRD = CTD KRD ÷ conversion factor.

**Benchmarks.** None yet.

**Estimated effort.** ~6 hours.

### Commit 4 — `IRSwapSpec` (M, ~6 hours)

**Files modified.**
- `crates/convex-analytics/src/risk/hedging/instruments.rs` — fill in `IRSwapSpec::dv01` and `IRSwapSpec::key_rate_profile`.

**What it does.** DV01 = `Σ` discount-factor-weighted year-fraction times notional times 1bp on the fixed leg (post-LIBOR floating ≈ 0 at reset). KRD = bump curve at each tenor and recompute the fixed-leg PV01.

**Tests added.**
- 5Y / 10Y / 30Y SOFR-style swaps against hand-computed PV01 within 0.1%.
- Sign convention: pay-fixed / receive-floating → DV01 < 0 (loses money when rates rise).
- Currency check: GBP swap with SONIA curve produces DV01 in GBP.

**Benchmarks.** None yet.

**Estimated effort.** ~6 hours.

### Commit 5 — `HeuristicCostModel` (S, ~2 hours)

**Files added.**
- `crates/convex-analytics/src/risk/hedging/cost.rs` — `CostModel` trait + `HeuristicCostModel` (const-table backed).

**Tests added.**
- Each known asset class returns its tabled value.
- `cost_bps` is positive everywhere.
- `name()` returns the literal `"heuristic_v1"` (the advisor's outputs reference this string).

**Benchmarks.** None.

**Estimated effort.** ~2 hours.

### Commit 6 — `HedgeStrategy` trait + `DurationFutures` (M, ~6 hours)

**Files added.**
- `crates/convex-analytics/src/risk/hedging/strategies/mod.rs` — `HedgeStrategy` trait + `StrategyContext`.
- `crates/convex-analytics/src/risk/hedging/strategies/duration_futures.rs` — `DurationFutures` impl.

**What it does.** Sizes the future to neutralize the position's parallel DV01. Computes residual KRD (curvature exposure) and reports it. Computes cost from the cost model.

**Tests added.**
- Long $10mm 10Y position → recommended short of TY futures matches DV01 ratio within 0.1%.
- Residual DV01 after hedge < $100 (tight neutralization) for parallel shift.
- Residual KRD shows curvature exposure (non-trivial 2Y/30Y residuals when hedging a 10Y bond with a 10Y future).

**Benchmarks.** None yet.

**Estimated effort.** ~6 hours.

### Commit 7 — `InterestRateSwap` strategy (M, ~6 hours)

**Files added.**
- `crates/convex-analytics/src/risk/hedging/strategies/interest_rate_swap.rs` — `InterestRateSwap` strategy.

**What it does.** Constructs a tenor-matched payer (or receiver, depending on position sign) swap. Sizes notional to match DV01. Computes residual + cost.

**Tests added.**
- Long bond → recommended pay-fixed swap (positive DV01 → short DV01 hedge).
- Tenor-matched swap residual KRD is *smaller* than the duration-future hedge of the same position (a real, demonstrable tradeoff).
- Notional currency matches position currency.

**Benchmarks.** None yet.

**Estimated effort.** ~6 hours.

### Commit 8 — `compare_hedges` aggregator (S, ~2 hours)

**Files added.**
- `crates/convex-analytics/src/risk/hedging/compare.rs` — `compare_hedges(proposals: &[HedgeProposal]) -> ComparisonReport`.

**Tests added.**
- Output has one `ComparisonRow` per input proposal.
- Columns: notional, dv01 hedged, residual dv01, residual KRD norm, cost bps, cost total, narrative seed.
- Stable order (insertion).

**Benchmarks.** None.

**Estimated effort.** ~2 hours.

### Commit 9 — template narrator (S, ~3 hours)

**Files added.**
- `crates/convex-analytics/src/risk/hedging/narrate.rs` — `narrate(report: &ComparisonReport, style: NarrationStyle) -> String`.

**Style.** v1 supports a single `NarrationStyle::TraderBrief`. Uses explicit `format!` strings.

**Tests added.**
- Output contains every proposal name.
- Output mentions cost in bps for each proposal.
- Output picks one as the recommended choice based on a deterministic rule (lowest cost, tie-broken by smallest residual KRD norm). The rule is documented in the narrator's doc-comment.
- Pure deterministic — same input twice → identical bytes.

**Benchmarks.** None.

**Estimated effort.** ~3 hours.

### Commit 10 — four MCP tools (S, ~4 hours)

**Files modified.**
- `crates/convex-mcp/src/server.rs` — add four `#[tool]` methods + their `Parameters<…>` and output structs:
  - `compute_position_risk(bond: BondRef, mark: Mark, notional_usd: f64, settlement, curve: CurveRef, key_rate_tenors?) -> RiskProfileOutput`
  - `propose_hedges(risk: RiskProfile, constraints?, market: MarketContextRef) -> ProposalsOutput`
  - `compare_hedges(proposals: Vec<HedgeProposal>) -> ComparisonReport`
  - `narrate_recommendation(comparison: ComparisonReport, style?) -> NarrationOutput`

**Tests added.**
- Round-trip the demo Apple 10Y scenario through the four tools sequentially in a `#[tokio::test]`.
- Schema-derivation smoke test: each tool's parameters struct produces a valid JSON Schema (the rmcp macro handles registration but we assert no panic).

**Benchmarks.** None.

**Estimated effort.** ~4 hours.

### Commit 11 — bench + README + investigation update (S, ~3 hours)

**Files added.**
- `crates/convex-analytics/benches/hedge_advisor.rs` — three Criterion groups.

**Files modified.**
- `crates/convex-analytics/Cargo.toml` — add `[[bench]] hedge_advisor`.
- `README.md` — new "Hedge Advisor" section with the demo invocation.
- `docs/hedge-advisor-investigation.md` — flip ❌→✅ for closed gaps; leave ❌ for explicitly deferred items.
- `docs/perf-baselines.md` (new) — record the three new benches' numbers as the v1 baseline.

**Tests added.** None.

**Benchmarks run.**
- `cargo bench -p convex-analytics --bench hedge_advisor` (record).
- `cargo bench -p convex-bonds --bench trinomial_tree` and `cargo bench -p convex-engine` for regression check (compare to pre-PR `target/criterion/`).

**Estimated effort.** ~3 hours.

### Total

| Commit | Effort |
| --- | --- |
| 1. types | 3h |
| 2. compute | 3h |
| 3. HedgeLeg + BondFutureSpec | 6h |
| 4. IRSwapSpec | 6h |
| 5. cost | 2h |
| 6. DurationFutures | 6h |
| 7. InterestRateSwap | 6h |
| 8. compare | 2h |
| 9. narrate | 3h |
| 10. MCP tools | 4h |
| 11. bench + docs | 3h |
| **Total** | **~44h** (≈1 working week, with buffer) |

---

## 3.4 Risks and tradeoffs

### What could go wrong

1. **`BondFutureSpec` DV01 is approximate.** v1 uses a representative deliverable per tenor + a static conversion factor. Real CTD selection depends on yield, repo, basis. Mitigation: tag every `BondFutureSpec` output with `cost_model: "heuristic_v1"` and document the assumptions in the strategy's doc-comment. v2 promotes to a real CTD optimizer.

2. **`IRSwapSpec` ignores DV01 of the floating leg.** Post-LIBOR floating-leg DV01 is small but nonzero between resets. v1 assumes 0. Mitigation: documented limitation; magnitude is < 1% of fixed-leg DV01 on weekly resets.

3. **Cost model is not real.** `heuristic_v1` is plausible-looking constants. A trader using this for execution would be misled. Mitigation: every output names the model. README's "Hedge Advisor" section opens with a "**This is a research tool, not an execution recommender**" disclaimer.

4. **Spread-mark Z-only restriction.** `price_from_mark` rejects OAS / I / G spread marks (`pricing.rs:89`). The advisor inherits this. Mitigation: documented; v1 demo uses a price mark and a Z-spread mark.

5. **No CTD basis or repo financing.** The future's price-yield relationship is approximated. Real basis traders care about this; v1 traders matching DV01 do not.

6. **Sign-convention bugs.** Hedge direction (long/short) signs are easy to flip. Mitigation: every strategy test explicitly asserts the recommended direction (long position → short hedge).

### Where the design is making compromises

- **`HedgeInstrument` as a tagged enum, not a `Box<dyn HedgeLeg>`.** Pro: serde works out of the box, JSON output is human-readable, no v-table. Con: closed set requires a code change to add a variant. v1 has 2 variants; v2 might have 6. If the count grows past ~10 we revisit.
- **No `KeyRateConstraints` in v1.** A real trader would say "constrain residual 10Y KRD < $X." v1 reports residual KRD but does not constrain on it.
- **Single-position scope.** No book-level hedging. The architecture supports it (each strategy already takes a `RiskProfile`; portfolio-level just aggregates first), but v1 doesn't ship it.

### What is deferred to v2

- CTD optionality, repo financing, basis trading
- Volatility regime (vol-aware OAS hedging)
- LLM narration (template-only in v1)
- `KeyRateFutures`, `ETFProxy`, `CashBondPair`, `InflationSwap` strategies
- Real cost feeds (`MarketCostSource` trait)
- Multi-position book hedging
- Cross-currency hedges (FX delta gap)
- Constraints on residual KRD, tenor exposure, etc.
- Spread-mark variants beyond Z-spread

---

## 3.5 Demo plan

### Scenario (single, fully scripted)

> **Position:** long $10mm Apple 4.85% May-2034 (hardcoded as a `FixedRateBond` in the test fixture).
> **Curves:** USD SOFR (built via `bootstrap_curve` MCP tool from a hardcoded list of deposit/swap rates).
> **Mark:** spread of `+85bp@USD.TSY.10Y` (Z-spread to UST 10Y).
> **Request:** propose hedges, compare, narrate.

### Agent flow

```
1. compute_position_risk(
     bond:       { fixed_rate: { coupon_rate_pct: 4.85, maturity: 2034-05-10, ... } },
     mark:       "+85bps@USD.TSY",
     notional_usd: 10_000_000,
     settlement: 2026-05-04,
     curve:      "usd_sofr",
     key_rate_tenors: [2.0, 5.0, 10.0, 30.0]
   )
   -> RiskProfile { dv01_per_bp, modified_duration_years, key_rate_durations: [2Y, 5Y, 10Y, 30Y], currency: USD, provenance }

2. propose_hedges(risk: <previous output>) 
   -> [
        HedgeProposal {
          strategy: "DurationFutures",
          trades: [{ instrument: BondFuture(TY10Y), side: Short, contracts: ~$N$ }],
          residual: ResidualRisk { residual_dv01_per_bp, residual_krd: [...] },
          cost_bps, cost_total_usd,
          tradeoffs: TradeoffNotes { strengths: ["Lowest cost", "Liquid"], weaknesses: ["Curvature exposure remains"] },
          provenance
        },
        HedgeProposal {
          strategy: "InterestRateSwap",
          trades: [{ instrument: IRSwap(10Y SOFR Pay-Fixed), side: PayFixed, notional_usd: ~$M$ }],
          residual: ResidualRisk { ... smaller curvature residual ... },
          cost_bps, cost_total_usd,
          tradeoffs: { strengths: ["Tenor-matched, lower curvature residual"], weaknesses: ["Higher cost", "Bilateral"] },
          provenance
        }
      ]

3. compare_hedges(proposals: <previous output>)
   -> ComparisonReport with two rows side by side, recommendation_seed pointing to one.

4. narrate_recommendation(comparison: <previous output>, style: TraderBrief)
   -> "You're long $10mm Apple 4.85% '34 with $7,200 of DV01. Two hedges fit your shape: a short of ~72 TY contracts is the cheapest at 0.5 bp but leaves curvature on; a $10mm pay-fixed 10Y SOFR swap costs 1.2 bp but neutralizes the 10Y bucket more cleanly. Lean cheap (TY) for tactical, swap for cleaner book risk."
```

### Verification

- A `#[tokio::test]` in `convex-mcp/tests/hedge_advisor_e2e.rs` calls all four tools in sequence and asserts the structural shape of every output (notional, currency, presence of provenance, narrative non-empty).
- A manual MCP smoke test from Claude Desktop against the running server, calling each tool. Output JSON pasted into `excel/SMOKE_TEST.md` style documentation under `docs/hedge-advisor-smoke.md`.

### Recording / video plan (deferred)

Outline only:
- Open `claude.ai/code` or Claude Desktop with `convex-mcp-server` registered.
- Paste the Apple 10Y scenario as a natural-language prompt.
- Show the agent calling each tool, reading each structured output, ending with the narrator's paragraph.
- ~3 minute screen capture, no narration, no editing in v1.

Not building this in v1; just leaving the skeleton.

---

## Definition-of-done checklist

(Mirrored from the prompt; this is what we'll grade Phase 4 against.)

- [ ] Four MCP tools registered and callable.
- [ ] Apple 10Y demo scenario runs end-to-end via `cargo test` and from Claude Desktop.
- [ ] Output JSON includes full provenance on every advisor output.
- [ ] At least two strategies produce comparable proposals.
- [ ] Template narrator produces a coherent recommendation paragraph.
- [ ] No regression > 5% on existing benches (`convex-bonds`, `convex-engine`).
- [ ] New code has unit tests covering the happy path + sign-convention edge cases.
- [ ] README updated with hedge advisor section + example invocation.
- [ ] `docs/perf-baselines.md` records advisor benchmark numbers.
- [ ] `docs/hedge-advisor-investigation.md` updated: closed gaps flipped ❌→✅.
- [ ] **Zero new crates created.**

---

## Stop-and-confirm gates during Phase 4

After **Commit 1** (types): present the type surface — names, units, derives — for confirmation before wiring functions to it. Names are hard to change later.

After **Commit 4** (`IRSwapSpec`): present the `HedgeLeg` interface and the two concrete impls. This is the extensibility seam; if it's wrong, every later commit is wrong.

After **Commit 7** (both strategies): present a sample `RiskProfile + HedgeProposal × 2` JSON output. Visual sanity check before MCP wiring.

After **Commit 11** (bench + docs): present the bench numbers and the regression check. End-of-Phase-4 review.

Anything unexpected mid-implementation → stop and surface, per the prompt's reporting cadence rule.
