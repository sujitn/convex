# PnL Narrator — Phase 1 Investigation

Read-only audit of the Convex workspace, framing what the PnL attribution
narrator can reuse vs. what is genuinely missing. Each finding cites
`file:line` and is tagged ✅ (exists and usable), ⚠️ (exists but needs
adaptation), ❌ (missing).

The PnL narrator extends the hedge advisor (`compute_position_risk`,
`propose_hedges`, `compare_hedges`, `narrate_recommendation`, shipped on
`feat/hedge-advisor`, PR #83). Findings are framed against the same
trader-sovereign / type-safety / provenance / performance constraints and the
demo continuity requirement (same book + the swap from the hedge).

> **Scope reminder.** v1 = two MCP tools (`attribute_pnl`,
> `narrate_attribution`), single currency, static book, two dates, full
> revaluation + factor decomposition, template narrator. Everything else is
> explicitly deferred.

> **v1 status (post-implementation).** Shipped on `feat/pnl-narrator` in 6
> commits. All blocker gaps closed; the PnL narrator runs end-to-end via the
> `attribute_pnl` → `narrate_attribution` MCP tools. The 4-position demo book
> (OAT + BTP + Bund + the pay-fixed EUR swap, May 7 → May 8 2026)
> round-trips: the swap absorbs ~42% of the bonds' rate-move loss — last
> week's hedge in this week's PnL. Closed since this audit:
> - **§1.3 ❌→✅** `risk::pnl::decompose` projects the observed move onto
>   level/slope/curvature by least squares (curvature basis = a closure over
>   `ScenarioBump::custom`; `convex-curves` untouched).
> - **§1.4 ⚠️/❌→✅** `ResolvedBook`/`ResolvedPosition` (tagged bond|swap);
>   `InterestRateSwapPnlSpec` is the **fixed-maturity** swap (gap-4 fix), so
>   the static swap ages correctly over the period.
> - **§1.5 ⚠️→✅** held spread taken exactly from a Z-spread mark (machine-zero
>   residual); per-benchmark spread attribution.
> - **§1.6/1.7/1.8 →✅** `narrate_attribution` clones the hedge-advisor
>   narrator pattern; two `#[tool]` methods added; tagged-enum book input.
> - **Gap 0 confirmed:** the pricing core was **not** modified —
>   `price_from_mark` already takes the valuation date. `risk_profile_apple_10y`
>   is flat at ~22 µs; `attribute_pnl` on the demo book ≈ 128 µs.
>
> Items deferred by design remain ❌ (multi-period, FX, perf-vs-benchmark,
> issue-level spread, LLM narrator) — see `docs/pnl-narrator-gaps.md` §11.

---

## 1.1 Pricing primitives for historical valuation

| Item | Status | Evidence |
| --- | --- | --- |
| Pricing function takes an explicit valuation date | ✅ | `convex-analytics/src/pricing.rs:53` `price_from_mark(bond, settlement, mark, curve, quote_frequency)`. `settlement` **is** the valuation date — there is no implicit "today". |
| Accrued / passage of time as of a specified date | ✅ | `pricing.rs:73` `bond.accrued_interest(settlement)`; clean/dirty split derived from it (`pricing.rs:130`). Cash-flow timing comes from the bond schedule relative to `settlement`. |
| No global state / no leakage across two valuations | ✅ | `price_from_mark` is a pure function of its arguments. No statics, no thread-locals, no "system clock" anywhere on the path (grep `Utc::now`/`SystemTime` in `convex-analytics` → none on the pricing path). |
| Curve carries its own valuation date | ✅ | `convex-curves/src/curves/discrete.rs:92` `DiscreteCurve::new(reference_date, …)`; `wrappers/rate_curve.rs:53` `RateCurve::reference_date()`. `curve_t0` and `curve_t1` are independent objects. |
| "Hold spread fixed, vary curve/date" reprice primitive | ✅ | `profile.rs:126` computes implied Z-spread, then `profile.rs:139` `ZSpreadCalculator::price_with_spread(bond, z_decimal, settlement)` reprices at that fixed spread against any (bumped) curve. This is exactly the kernel sequential repricing needs. |
| Price at an off-market spread on a govt curve (G/I) | ✅ | `pricing.rs:108` `SpreadType::ISpread | GSpread` path: `bond_yield = curve_par_rate@maturity + spread`. Lets us value a sovereign at a Bund-relative spread. |
| Spread-mark variants beyond Z/I/G/OAS | ⚠️ | `pricing.rs:120` rejects other `SpreadType`s. v1 uses Z (or G for sovereign-vs-Bund) — sufficient. |

**Synthesis.** Historical valuation is **orchestration, not missing math**.
The same bond can be priced at `(t0, curve_t0)` and `(t1, curve_t1)` in one
process with zero state leakage by calling `price_from_mark` (or the
spread-fixed `ZSpreadCalculator::price_with_spread`) twice with different
`settlement` + `curve` arguments. The hedge advisor already exercises exactly
this pattern inside `compute_position_risk` (price once, then reprice against
bumped curves at a held spread, `profile.rs:132-147`). Sequential repricing
for attribution is the same pattern stretched over two dates instead of ±1bp
bumps. **No pricing-core change required.**

The one genuine design question (not a gap): how to *split* the t0→t1 value
change into carry vs curve vs spread requires deciding which quantity is held
fixed in each intermediate reprice (e.g. price at `t1` with `curve_t0` and the
t0 spread isolates carry+roll; then swap in `curve_t1` isolates the curve
effect; then move to the t1 spread isolates the spread effect). The pricing
calls all exist; the *decomposition recipe* is the new logic (§1.3).

---

## 1.2 Curve loading from historical data

| Item | Status | Evidence |
| --- | --- | --- |
| Curve construction from a pillar set | ✅ | `convex-mcp/src/server.rs:164` `build_curve(spec)` → `DiscreteCurve::new(ref_date, tenors_years, rates_decimal, ZeroRate{Continuous,Act365}, MonotoneConvex)` → `RateCurve::new`. |
| Inline (stateless) curve input | ✅ | `server.rs:280` `CurveSpec { reference_date, tenors_years, zero_rates_pct }`; `server.rs:311` `CurveRef = untagged(Id(String) | Spec(CurveSpec))`. Inline spec is the recommended paste-block form. |
| Multiple curves identified by valuation date | ⚠️ | No "curve-by-date" registry. Not needed: each curve is passed inline carrying its own `reference_date`. Two-date attribution = two `CurveRef` arguments (`curve_t0`, `curve_t1`). |
| Demo data shape compatibility | ✅ | `demo/data/treasury-curve-live.json` is `{ "asOfDate", "curve": {tenor: rate_pct}, "tenorsYears": {…} }` — maps 1:1 onto `CurveSpec { reference_date, tenors_years, zero_rates_pct }`. The agent pastes two such blocks. |
| Read a zero rate at an arbitrary tenor | ✅ | `rate_curve.rs:141` `zero_rate_at_tenor(t, compounding)`; `:135` `zero_rate(date, …)`. Needed to sample `curve_t0`/`curve_t1` on a common grid for decomposition. |

**Synthesis.** Curve loading is fully solved by the existing
`CurveSpec`/`CurveRef` pattern the hedge advisor already uses. The PnL tool
takes **two** `CurveRef`s instead of one; no new loading mechanism, no
registry change, no file I/O in the core. The demo's pasted curve blocks
already match the schema.

---

## 1.3 Factor decomposition reuse

| Item | Status | Evidence |
| --- | --- | --- |
| Synthetic parallel shock | ✅ | `convex-curves/src/bumping/parallel.rs` `ParallelBump`; `bumping/scenario.rs:135` `ScenarioBump::parallel`. Zero-copy wrapper curves. |
| Synthetic slope (steepener/flattener) shock | ✅ | `scenario.rs:149/161` `ScenarioBump::steepener/flattener(short, long, pivot)`; linear-about-pivot shift in `shift_at` (`scenario.rs:217-259`). Precedent for the slope basis function. |
| Synthetic key-rate / custom shock, compositional | ✅ | `scenario.rs:171` `key_rate`, `:202` `custom(name, Fn(f64)->f64)`; `Scenario` applies bumps **additively** (`scenario.rs:402` `total_shift_at`). KRD bump used live in `profile.rs:137`. |
| Reprice under a curve shock | ✅ | `profile.rs:137-140` builds `RateCurve::new(KeyRateBump::new(t,bp).apply(&inner))` and reprices. Same trick works for any factor-component curve. |
| **Decompose an *observed* move (curve_t1 − curve_t0) → parallel/slope/curvature** | ❌ | Nothing decomposes a *realized* curve change into factor loadings. `scenario.rs` only goes the **forward** direction (apply a known shock). No least-squares / basis-projection of `{Δr(τ_i)}` onto {level, slope, curvature} exists in `convex-curves`, `convex-analytics`, or anywhere (grep `decompos`/`butterfly`/`twist` → only forward-shock and a doc comment). |
| Butterfly / curvature basis | ❌ | Steepener/flattener give a slope basis precedent; there is **no** curvature (butterfly) basis function anywhere. Small addition. |

**Synthesis.** The shock primitives are reusable and compositional, but they
only run **forward** (shock → reprice). The PnL narrator needs the **inverse**:
project the observed pillar-wise change `Δr(τ_i) = r_{t1}(τ_i) − r_{t0}(τ_i)`
onto a 3-factor basis (level / slope / curvature), then reprice each component
to attribute the curve PnL. This is the **central new piece of logic** — but
it is a *new function*, not a new module and certainly not a new crate. It
reuses `zero_rate_at_tenor` to sample both curves and `DiscreteCurve::new` to
materialise each component curve for repricing, and it should mirror the
steepener's linear-about-pivot slope shape so synthetic and decomposed factors
stay consistent.

---

## 1.4 Book / portfolio modeling

| Item | Status | Evidence |
| --- | --- | --- |
| Risk aggregation across positions | ✅ | `profile.rs:181` `aggregate_risk_profiles(&[RiskProfile], book_id)` — sums DV01/MV, unions KRD, DV01-weights durations. Risk roll-up, not a position container. |
| "Book on the wire" today | ⚠️ | The hedge advisor models a book as a **`Vec` of position-risk requests**: `server.rs:456` `AggregateBookRiskParams { positions: Vec<ComputePositionRiskParams> }` and `server.rs:508` `BookGroup { positions, curve, … }`. There is no named `Book` domain type. |
| Rich portfolio type | ⚠️ | `convex-portfolio::Portfolio { holdings: Vec<Holding>, base_currency, as_of_date }`. Does **not** derive `JsonSchema`, is **not** on the MCP wire; the hedge advisor deliberately avoided it (matches its investigation §1.2). Reusing it would break the established stateless MCP pattern. |
| Per-position contribution to a book | ✅ | `risk/hedging/contribution.rs:27` `position_contributions(&[RiskProfile])` → signed DV01 + gross share. Pattern to mirror for per-position PnL breakdown. |
| Prior-art factor attribution | ⚠️ | `convex-portfolio/src/contribution/attribution.rs` — CFA income/treasury/spread/residual, but **sensitivity-based** (duration×Δy), requires the caller to *supply* total return & yield change, operates on `Holding`, no `JsonSchema`. Good taxonomy reference (carry≈income, curve≈treasury, spread, residual); **not** reusable for full-revaluation two-date attribution. |
| Swap as a holdable position | ⚠️/❌ | `risk/hedging/types.rs:91` `InterestRateSwap` + `instruments.rs:117` `interest_rate_swap_risk` model the swap as a synthetic at-par fixed-leg `FixedRateBond` whose `issue_date = settlement` and `maturity = settlement + tenor_months` (`instruments.rs:182-215`). That is a **constant-maturity** model: it rebuilds a fresh par swap *at the valuation date*. Correct for a risk snapshot; **wrong for the value change of a static swap over a period** (a swap traded last week has a *fixed* maturity and an off-market fixed rate vs the new curve at t1). |

**Synthesis.** There is no `Book` type and the established pattern is "a book
is a `Vec` of position specs" (`AggregateBookRiskParams` / `BookGroup`). The
smallest reasonable extension is a thin `Book { positions, base_currency }`
whose entries mirror `ComputePositionRiskParams` **minus the per-call curve**
(PnL supplies two curves at the call level) **plus a second mark** for t1 —
new domain type in `convex-analytics`, new params struct in `convex-mcp`. No
new crate; no `convex-portfolio` dependency.

The **swap is a real gap**: `interest_rate_swap_risk` gives the right risk but
the wrong PnL because it re-issues the swap at each valuation date. PnL needs
the swap valued at a **fixed maturity and fixed rate** at both t0 and t1
(reuse the synthetic-fixed-leg idea, but pin maturity/rate at trade time;
pay-fixed swap PnL ≈ −Δ(fixed-leg PV), floating ≈ par at reset). This is the
single most important correctness gap and is the demo's hero moment.

---

## 1.5 Spread attribution mechanics

| Item | Status | Evidence |
| --- | --- | --- |
| Mark stored on the position | ❌ | No position type stores a `Mark`. `Holding` carries only `market_price: Decimal`. Marks enter analytics as a textual string at the call site (`server.rs:432` `ComputePositionRiskParams.mark: String`, parsed `Mark::from_str`). |
| Implied spread computed (not stored) | ✅ | `profile.rs:126` `ZSpreadCalculator::new(curve).calculate(bond, dirty, settlement)`. For two-date attribution: compute implied spread from (t0 mark, curve_t0) and from (t1 mark, curve_t1). The data shape is "two marks in, two spreads derived". |
| Spread types are typed enums | ✅ | `convex-core/src/types/mark.rs` `Mark::Spread { value: Spread, benchmark: String }`; `spread.rs` `SpreadType::{ZSpread,ISpread,GSpread,OAS,…}`. Numbers carry units (`Spread::as_bps`/`as_decimal`). |
| Benchmark as a typed reference | ❌ | `benchmark` is a free-form `String` id (e.g. `"DE.BUND.10Y"`). No typed benchmark registry; benchmarks (Bund/UST/swap) are just separate curves identified by id + the mark's benchmark string. **Sufficient for v1**: spread attribution keyed by the mark's benchmark *category* string. |
| Δspread → PnL primitive | ✅ | Two options, both exist: (a) `risk/duration/spread_duration.rs:20` `spread_duration` × Δspread (sensitivity), or (b) reprice at fixed curve with t0 spread vs t1 spread (full reval) via `ZSpreadCalculator::price_with_spread`. v1 should use the full-reval form for consistency with the curve leg. |
| Two spread snapshots at t0/t1 (BTP-Bund, OAT-Bund) | ⚠️ | No structured "spread snapshot" input type exists. The demo pastes per-benchmark spreads at t0 and t1; we model this as the per-position **t1 mark** (a spread mark) — no separate snapshot type needed if each position carries (t0 mark, t1 mark). |

**Synthesis.** Spread tracking is "two marks in → two implied spreads
derived", which the existing machinery supports cleanly. There is no stored
mark and no typed benchmark — both are fine for v1: attribute spread PnL per
the mark's `benchmark` string (BTP→Bund, OAT→Bund), computing the spread leg
by repricing at a fixed curve with the t0 vs t1 spread. The clean data shape
is **(t0 mark, t1 mark) per position**, which subsumes the "two spread
snapshots" the demo pastes.

---

## 1.6 Narrator pattern reuse

| Item | Status | Evidence |
| --- | --- | --- |
| Template narrator exists | ✅ | `risk/hedging/narrate.rs:18` `narrate(report: &ComparisonReport) -> String`. Pure, deterministic ("same input → same bytes", test `narrate.rs:168`). |
| Idioms to mirror | ✅ | `String::with_capacity(512)` + `std::fmt::Write`; currency via `Currency::code()`; bp `{:.2}`, currency `{:.0}`; explicit recommendation reason mapping (`narrate.rs:8`); cost-source disclosure line so heuristics aren't mistaken for live feeds (`narrate.rs:57`). |
| Shared formatting helpers | ❌ | None. Each narrator inlines its own `write!` calls — *that inlining is the pattern*. No `fmt_currency`/`fmt_bps` helper crate exists; matching the inline style is correct, not duplication to refactor. |
| Style enum | ⚠️ | `narrate` is single-style (no `NarrationStyle` param shipped). Prompt allows an optional style enum; v1 can ship one `TraderBrief`-equivalent and leave the enum extensible. |

**Synthesis.** The narrator pattern is directly reusable. `narrate_attribution`
should be a sibling module (e.g. `risk::pnl::narrate`) with the identical
shape — `narrate_attribution(&Attribution) -> String`, pure/deterministic,
same `write!` idioms, same provenance/heuristic-disclosure discipline. The
"hero moment" (the swap absorbing curve PnL) is a deterministic
`if swap_contribution …` clause in this function.

---

## 1.7 MCP tool patterns

| Item | Status | Evidence |
| --- | --- | --- |
| Tool registration | ✅ | `server.rs:773` `#[tool_router] impl ConvexMcpServer`; one `#[tool(description=…)] pub async fn` per tool; `Parameters<XxxParams>` in, `Self::json_result(&Out)` out (`server.rs:110`). Adding a tool = add a method. |
| Input idioms | ✅ | `BondRef`/`CurveRef` untagged (inline spec \| registry id); `settlement: String` (ISO-8601, `Date::parse`); `mark: String` (`Mark::from_str`); `notional_face: f64`; `#[derive(Deserialize, JsonSchema)]` on every params struct. |
| Output / provenance | ✅ | `Provenance { curves_used, cost_model, advisor_version, oas_volatility }` (`profile.rs:41`) echoed on every advisor output. Outputs are `serde_json` text (no MCP `outputSchema`, consistent with the rest of the surface). |
| Error envelope | ✅ | `convex-mcp/src/error.rs` `McpToolError::{InvalidInput,…}` → typed JSON-RPC. `McpToolError::from(AnalyticsError)` already wired. |
| "Book" through MCP | ✅ | Already done as `Vec<ComputePositionRiskParams>` (`AggregateBookRiskParams`, `BookGroup`). The PnL tool follows the same shape. |
| Schema-derived domain types | ✅ | Every wire type derives `JsonSchema` under the `schemars` feature (e.g. `RiskProfile` `profile.rs:54`, `HedgeInstrument` `types.rs:24`). New `Attribution`/`FactorBreakdown`/`CurveBreakdown` follow the same `#[cfg_attr(feature="schemars", derive(JsonSchema))]` + `schemars(with="f64")` for `Decimal` convention. |

**Synthesis.** Adding `attribute_pnl` and `narrate_attribution` is purely
additive and mechanically identical to how the four hedge-advisor tools were
added. Provenance discipline (curves used, factor model, conventions) plugs
into the existing `Provenance` echo pattern — extend it, don't reinvent it.

---

## 1.8 Demo data shape

| Item | Status | Evidence |
| --- | --- | --- |
| Two curves as input | ✅ | Two `CurveRef` (inline `CurveSpec`) — already the paste-block format (§1.2). |
| Book of mixed instruments | ⚠️ | Bonds fit `BondRef`. The **EUR swap does not** — it is not a `BondSpec`. Need a position spec that is a tagged enum `{ bond | swap }` (mirrors `HedgeInstrument`'s tagged-enum convention, `types.rs:25`). |
| Two marks / spread snapshots per position | ⚠️ | `ComputePositionRiskParams` carries one `mark`. PnL needs (t0 mark, t1 mark) per position. Additive field, not a new mechanism. |
| Dates on the wire | ⚠️ | The prompt says `t0: NaiveDate` / `t1: NaiveDate`, but the entire MCP surface uses `convex_core::types::Date` as an **ISO-8601 string** (`settlement: String`, `Date::parse`). Recommend matching the codebase (`"2026-05-07"` strings) rather than introducing `chrono::NaiveDate` into the schema — see *Contradictions to surface*. |
| A `MarketSnapshot`-style wrapper | ⚠️ | Not strictly needed: `attribute_pnl(book, t0, t1, curve_t0, curve_t1, [config])` with per-position (t0,t1) marks subsumes the "two curves + two spread snapshots" the demo pastes. A `MarketSnapshot` newtype is optional sugar, decided in Phase 2/3. |

**Synthesis.** The existing schema is ~80% there. The genuine extension is a
**book input whose positions are a tagged `{ bond | swap }` enum carrying two
marks**, plus two `CurveRef`s and two dates at the call level. Everything is
additive, lives in `convex-analytics` domain types + `convex-mcp` params, and
needs no new crate.

---

## Summary of gaps (prioritised for the PnL narrator build)

| # | Gap | Severity | Where it'll live |
| --- | --- | --- | --- |
| 1 | Curve-change decomposition `(curve_t1 − curve_t0) → {parallel, slope, curvature, residual}` (incl. a curvature/butterfly basis) | **Blocker** | `convex-curves::bumping` (new fn + butterfly basis) **or** `convex-analytics::risk::pnl` (new fn). No new module needed. |
| 2 | Sequential repricing engine (price each position at t0 & t1; split into carry / roll / curve-factors / spread / residual) | **Blocker** | `convex-analytics::risk::pnl` (new module in existing crate) |
| 3 | `Attribution`, `FactorBreakdown`, `CurveBreakdown` schema-derived output types | **Blocker** | `convex-analytics::risk::pnl::types` (new module) |
| 4 | `Book` input + tagged `{ bond \| swap }` position spec carrying (t0,t1) marks | **Blocker** | `convex-analytics::risk::pnl::types` + `convex-mcp` params |
| 5 | Fixed-maturity swap valuation for PnL (vs the constant-maturity `interest_rate_swap_risk`) | **Blocker** (the demo hero moment) | `convex-analytics::risk::pnl` (reuse synthetic-fixed-leg, pin maturity/rate) |
| 6 | `attribute_pnl` MCP tool | **Blocker** | `convex-mcp/src/server.rs` (extend `#[tool_router]`) |
| 7 | `narrate_attribution` template narrator + MCP tool | **Blocker** | `convex-analytics::risk::pnl::narrate` + `convex-mcp` |
| 8 | Provenance on attribution outputs (curves, factor model, conventions) | High | reuse/extend `Provenance` (`profile.rs:41`) |
| 9 | Benchmark for the sequential-repricing hot path | Medium | `convex-analytics/benches/` (extend `hedge_advisor.rs` or new `pnl.rs`) |
| 10 | Multi-period / FX / perf-vs-benchmark / issue-level spread | Low (deferred) | Out of v1 scope |

**Crate impact: zero new crates expected for v1.** All work fits inside
`convex-analytics` (new `risk::pnl` module tree), possibly one new function in
`convex-curves::bumping`, and `convex-mcp` (two new `#[tool]` methods) — the
exact footprint the hedge advisor used.

---

## Reusable primitives — the shopping list

- `convex_analytics::pricing::price_from_mark` — value any `Mark` at an
  arbitrary `(settlement, curve)`. The valuation-date primitive.
- `convex_analytics::spreads::ZSpreadCalculator::{calculate, price_with_spread}`
  — derive implied spread; reprice at a held spread against any curve. The
  carry/curve/spread isolation kernel.
- `convex_curves::bumping::{ParallelBump, ScenarioBump::{steepener,flattener,custom}, Scenario}`
  — forward factor shocks; the slope basis precedent.
- `convex_curves::RateCurve::{zero_rate_at_tenor, reference_date, inner}` +
  `DiscreteCurve::new` — sample two curves on a grid and materialise component
  curves for repricing.
- `convex_analytics::risk::profile::{compute_position_risk, RiskProfile, Provenance}`
  — per-position risk + the provenance echo pattern.
- `convex_analytics::risk::hedging::{InterestRateSwap, interest_rate_swap_risk, contribution::position_contributions}`
  — the swap spec to reuse (with a fixed-maturity tweak) and the
  per-position-breakdown pattern.
- `convex_analytics::risk::hedging::narrate::narrate` — the deterministic
  narrator pattern to clone for `narrate_attribution`.
- `convex_portfolio::contribution::attribution` — taxonomy reference only
  (income/treasury/spread/residual ≈ carry/curve/spread/residual); **not**
  code-reusable (sensitivity-based, `Holding`-bound, no `JsonSchema`).
- `convex_core::types::{Mark, Spread, SpreadType, Currency, Date, Frequency}`
  — type-safe inputs; `Date` is ISO-8601 on the wire.

---

## Contradictions to surface (per the prompt's "stop and surface" rule)

1. **`NaiveDate` vs `convex_core::types::Date`.** The prompt specifies
   `t0: NaiveDate`, `t1: NaiveDate`. The entire existing MCP/domain surface
   uses `convex_core::types::Date` (ISO-8601 *string* on the wire, parsed via
   `Date::parse`; `settlement: String` everywhere). Introducing `chrono::NaiveDate`
   into the schema would break the established pattern and the "match the hedge
   advisor's patterns" constraint. **Recommendation:** use `Date` / ISO-8601
   strings (`"2026-05-07"`). Flagging because it deviates from the prompt's
   literal type. Will confirm in Phase 2.

2. **The swap is a real correctness gap, not just plumbing.** The prompt's
   gap list frames the swap as "extend `Position` to an enum". The deeper
   issue (§1.4): the existing swap model (`interest_rate_swap_risk`) is
   **constant-maturity** — it re-issues the swap at the valuation date — so
   naively reusing it for two-date PnL would understate the swap's realised
   PnL and *miss the demo's hero moment*. PnL needs a **fixed-maturity,
   fixed-rate** swap valuation. This is the highest-risk item and is called
   out as Gap #5 with Blocker severity.

3. **Curve decomposition direction.** The prompt's gap #2 says decomposition
   "reuses the shock primitives but goes the other way". Confirmed: the shock
   primitives are forward-only; the inverse projection (and a curvature basis)
   genuinely does not exist and is the central new math (§1.3).

---

## Open questions to resolve in Phase 2

1. **Decomposition placement & method.** New fn in `convex-curves::bumping`
   (reusable, sits with the shock primitives) vs `convex-analytics::risk::pnl`
   (keeps curves crate pure-forward)? And: exact basis (level = mean Δr;
   slope = linear-about-pivot matching `steepener`; curvature = symmetric
   butterfly) + fit method (closed-form projection on 3 basis vectors vs
   least-squares; residual = unexplained pillar move).
2. **Carry / roll-down split.** Carry (coupon accrual + pull-to-par at the t0
   curve) vs roll-down (slide along the unchanged curve) — one "time" bucket
   or two? Recipe: price at `(t1, curve_t0, spread_t0)` isolates time;
   swapping in `curve_t1` isolates curve; moving to `spread_t1` isolates
   spread; the rest is residual.
3. **Swap PnL model.** Fixed-maturity synthetic fixed-leg bond (pin
   `issue/maturity/rate` at trade), pay-fixed PnL = −Δ(fixed-leg PV); floating
   ≈ par at reset. Confirm the v1 approximation and how the swap's "factor"
   breakdown (it's almost pure curve) is reported.
4. **Book input shape.** `Book { positions: Vec<PnlPositionSpec>, base_currency }`
   with `PnlPositionSpec = tagged { bond(BondRef, mark_t0, mark_t1, notional) |
   swap(InterestRateSwapPnlSpec) }`; vs an optional `MarketSnapshot` wrapper.
5. **Factor naming.** Align with the CFA prior art
   (`portfolio::contribution::attribution`): carry/income, curve/treasury
   (parallel/slope/curvature), spread (per benchmark), residual.

These get answered in `docs/pnl-narrator-gaps.md` (Phase 2).
