# PnL Narrator — Phase 2 Gap Report

Source: `docs/pnl-narrator-investigation.md`. Each gap is sized for the **v1
demo scope** (single currency, static 4-position book, two dates, full
revaluation + level/slope/curvature decomposition, template narrator).
Severity is from the demo's perspective; "Low" gaps are real but deferred.

**Top-line crate impact: zero new crates.** Every gap closes inside
`convex-analytics` (new `risk::pnl` module tree) and `convex-mcp` (two new
`#[tool]` methods). The case for/against a new crate is argued in §"Where a
new crate could be justified".

The three Phase-1 contradictions were **approved as recommended**:
ISO-8601 `Date` strings (not `NaiveDate`), a fixed-maturity swap model, and
decomposition as a new analytics function. These are recorded under *Resolved
design decisions*.

---

## Gap 0 — Historical valuation date in pricing — **NOT A GAP (closed)**

The prompt hypothesised this as *the* blocker: "if `price_bond` doesn't take
`valuation_date`, everything depends on it." Investigation §1.1 shows it is
already solved.

- **Status.** `convex-analytics/src/pricing.rs:53`
  `price_from_mark(bond, settlement, mark, curve, quote_frequency)` —
  `settlement` **is** the valuation date; accrued is computed at it
  (`pricing.rs:73`); there is no global "today"; the function is pure in its
  arguments. Curves carry their own `reference_date`
  (`discrete.rs:92`, `rate_curve.rs:53`).
- **Consequence.** The single largest hypothesised work item — adding a
  valuation-date parameter to the pricing core, with its signature churn and
  regression risk across FFI/Excel/MCP — **does not need to happen.** This
  removes the only change that would have touched the sub-microsecond pricing
  core. Sequential repricing is pure orchestration over an unchanged core.
- **Severity.** N/A. **Effort.** Zero. **Crate impact.** None.

This is the most important finding in the report: the prompt's primary risk is
retired before Phase 3.

---

## Gap 1 — Observed curve-change decomposition (parallel / slope / curvature / residual)

- **What is missing.** No code projects a *realised* pillar-wise curve change
  `Δr(τ_i) = r_{t1}(τ_i) − r_{t0}(τ_i)` onto a {level, slope, curvature}
  basis, and no curvature/butterfly basis function exists anywhere. The
  `convex-curves::bumping` primitives only run **forward** (apply a known
  shock); `ScenarioBump::steepener/flattener` (`scenario.rs:149/161`) is the
  only slope-shaped basis precedent.
- **Why it matters.** The curve factor is the dominant PnL driver in the demo
  (a €35mm long sovereign book over a one-day rate move). Splitting it into
  parallel/slope/curvature is the analytical core of the narrator's story.
- **Severity.** **Blocker.**
- **Effort.** M. Sample both curves at a common analysis grid (union of pillar
  tenors), form `Δr`, project onto three basis vectors:
  - **level** `b_L(τ) = 1`
  - **slope** `b_S(τ)` = linear about a pivot, *matching the existing
    `ScenarioBump::steepener` shape* so synthetic and decomposed factors stay
    consistent
  - **curvature** `b_C(τ)` = symmetric butterfly (belly vs wings), the one
    new basis shape
  Solve the 3×3 normal equations for loadings `(a_L, a_S, a_C)` via
  `convex_math::solve_linear_system` (already used by `KeyRateFutures`);
  `residual(τ_i) = Δr_i − Σ a_k b_k(τ_i)` is the unexplained pillar move and
  is a first-class reported factor (matches the prompt's explicit "residual").
- **Placement.** `convex-analytics::risk::pnl::decompose` (new sub-module of
  the new `risk::pnl` module). It is an *analytic over two curves*, not curve
  infrastructure. Component curves for repricing are built with the **existing**
  `ScenarioBump::custom(name, move |t| loading * basis(t))` +
  `Scenario`/`RateCurve::new` — so **`convex-curves` is not modified at all**.
  No new ScenarioBump variant, no new module in `convex-curves`.
- **Crate justification.** No new crate. CLAUDE.md's "new analytic → add to
  existing crate" rule applies; `convex-curves` `custom` closure already
  expresses any basis shape, so the curvature basis lives as a closure in
  `convex-analytics`, not as new curve infra.
- **Performance.** Cheap: per `attribute_pnl` call, one sample of N≈10 pillars
  ×2 curves + one 3×3 solve. Negligible vs the repricing (Gap 2). The
  *repricing of the component curves* is the cost and is accounted there.

---

## Gap 2 — Sequential repricing engine (the heart of `attribute_pnl`)

- **What is missing.** The orchestration that values each position at t0 and
  t1 and splits the change into **carry → roll-down → curve
  (parallel/slope/curvature) → spread (per benchmark) → residual**, then rolls
  per-position results into book totals.
- **Why it matters.** This *is* `attribute_pnl`. Every other gap feeds it.
- **Severity.** **Blocker.**
- **Effort.** L (the single largest item: the waterfall recipe + per-instrument
  dispatch + book aggregation + provenance).
- **Recipe (locked; documented & path-order-explicit).** For each position
  carrying `(mark_t0, mark_t1, notional)`:
  1. `V0` = dirty value at `(t0, curve_t0, mark_t0)`; derive implied spread
     `s0` (Z, or G for a sovereign-vs-Bund mark) from `mark_t0 + curve_t0`.
  2. `V1` = dirty value at `(t1, curve_t1, mark_t1)`; derive `s1`.
  3. `coupon_cash` = coupons with pay date in `(t0, t1]`.
  4. **Total** = `(V1 − V0)·notional/100 + coupon_cash`.
  5. **Time effect** = value at `(t1, curve_t0, s0)` − `V0` + `coupon_cash`
     (date advances; curve & spread held). Split:
     - **carry** = `coupon_cash` + pull-to-par at the *constant t0 YTM*
       (one extra yield-mark reprice at `t1`),
     - **roll-down** = time effect − carry (slide along the unchanged t0
       curve as maturity shortens).
  6. **Curve effect** = value at `(t1, curve_t1, s0)` − value at
     `(t1, curve_t0, s0)`. Sub-attribute via Gap 1: reprice
     `curve_t0 + {parallel, slope, curvature}` component shifts (held spread,
     date t1); **residual_curve** = curve effect − Σ component effects.
  7. **Spread effect** = value at `(t1, curve_t1, s1)` − value at
     `(t1, curve_t1, s0)`, attributed to the mark's `benchmark` string
     (BTP→Bund, OAT→Bund).
  8. **Residual** = Total − carry − roll-down − Σ curve factors − spread.
     Closes the identity to ≈0 by construction (catches any path
     non-commutativity; reported, not hidden).
- **Pricer-call budget.** ≈ `V0` + `V1` + time + carry-yield + 3 curve
  components + curve-total + spread ≈ **8–9 reprices/position** + 2 implied
  spread root-finds (only on `s0`/`s1`, not on the held-spread reprices —
  reuse `ZSpreadCalculator::price_with_spread`, `profile.rs:139`). 4-position
  book ≈ **~35–40 reprices/call**. Each `FixedRateBond` price ≈ 1µs (hedge
  advisor baseline `risk_profile_apple_10y` ≈ 22µs for price + full risk +
  4-KRD). Expected `attribute_pnl` latency: **low-hundreds of µs**, well under
  interactive thresholds.
- **Placement.** `convex-analytics::risk::pnl::engine` (new module tree
  `risk/pnl/{mod,types,decompose,engine,narrate}.rs`), sibling of
  `risk::profile` / `risk::hedging`. **No new crate** (mirrors exactly how
  `risk::hedging` was added).
- **Performance.** The documented hot path. Mitigations: clone
  `curve.inner()` once per position, reuse the spread-fixed reprice kernel (no
  root-find on the 6+ held-spread reprices), `Vec::with_capacity` on factor
  vectors. Existing benches are on untouched code paths — regression bar met
  by construction, still verified per the 5% rule.

---

## Gap 3 — Attribution output types (`Attribution`, `FactorBreakdown`, `CurveBreakdown`)

- **What is missing.** `Attribution` (book-level totals + per-position),
  `PositionAttribution`, `FactorBreakdown` (carry / roll-down / curve / spread
  / residual, in ccy **and** bp), `CurveBreakdown`
  (parallel/slope/curvature/residual + loadings + pivot), `SpreadBreakdown`
  (per benchmark), and a provenance echo (Gap 9). None exist.
- **Why it matters.** These are the wire types the two MCP tools exchange and
  the contract the agent round-trips.
- **Severity.** **Blocker.** **Effort.** S.
- **Placement.** `convex-analytics::risk::pnl::types`. Conventions identical
  to the hedge advisor: `#[derive(Debug, Clone, PartialEq, Serialize,
  Deserialize)]` + `#[cfg_attr(feature = "schemars", derive(JsonSchema))]`,
  `#[cfg_attr(feature="schemars", schemars(with="f64"))]` for `Decimal`,
  units in field names (`carry_pnl_ccy`, `total_pnl_bps`,
  `parallel_pnl_ccy`, `slope_loading_bps`), factor model / convention as
  enums, `#[serde(default)]` on optional vectors for LLM round-trip
  resilience.
- **Crate justification.** No new crate — domain DTOs sit beside
  `risk::profile::RiskProfile` / `risk::hedging::types`, the documented home
  for analytics wire types.
- **Performance.** Cold path; owned `Vec`/`String`/`Decimal` for ergonomics.

---

## Gap 4 — Fixed-maturity swap valuation for PnL

- **What is missing.** A swap valuation that **pins maturity and fixed rate at
  trade time** and values the swap at t0 and t1. The existing
  `risk::hedging::instruments.rs:117` `interest_rate_swap_risk` builds the
  synthetic fixed leg with `issue_date = settlement`,
  `maturity = settlement + tenor_months` (`instruments.rs:182-215`) — a
  **constant-maturity** model that re-issues a fresh par swap at the
  valuation date.
- **Why it matters.** The demo's hero moment is last week's **pay-fixed EUR
  swap absorbing this week's curve move** ("working as designed"). A
  constant-maturity model would value a *different* swap each day and **miss
  exactly the PnL the narrator must highlight**. This is the closed-loop
  payoff of the whole series.
- **Severity.** **Blocker** (and the highest-risk correctness item).
- **Effort.** M. Reuse the proven synthetic-fixed-leg idea but pin
  `issue_date`/`maturity`/`fixed_rate` at trade: build a `FixedRateBond` with
  fixed dates, value its fixed leg via the spread-fixed kernel at
  `(t0, curve_t0)` and `(t1, curve_t1)`. Floating leg ≈ par at last reset —
  the **same documented post-LIBOR≈0 approximation** the hedge advisor
  already ships (`instruments.rs:116`), so it is consistent, not a new
  assumption. Pay-fixed PnL ≈ `−Δ(fixed-leg PV)`. Its factor breakdown is
  almost pure **curve** (no credit spread → spread bucket 0; carry =
  fixed-vs-float accrual differential), which is precisely why it offsets the
  bonds' curve PnL in the narration.
- **Placement.** `convex-analytics::risk::pnl::engine` reuses
  `risk::hedging::types::InterestRateSwap` (extended with trade/maturity dates
  — see Gap 6) + a new fixed-maturity valuation helper co-located in
  `risk::pnl`. **No new crate**; reuses `convex-bonds::FixedRateBond` +
  `price_from_mark`.
- **Performance.** Identical to a bond reprice (synthetic bond under the
  hood): same ≈8 reprices/position budget as Gap 2.

---

## Gap 5 — Spread tracking at two dates

- **What is missing.** No position type carries a `Mark`, and there is no
  structured place for a t0/t1 spread pair (investigation §1.5). No typed
  benchmark registry — `benchmark` is a free `String` on `Mark::Spread`.
- **Why it matters.** The spread leg (BTP-Bund, OAT-Bund widening) needs an
  implied spread at both dates.
- **Severity.** **Blocker** (for the spread factor). **Effort.** S.
- **Resolution (smallest change).** Carry `(mark_t0, mark_t1)` as two textual
  mark fields on the new per-position spec (Gap 6), parsed via
  `Mark::from_str` exactly like `ComputePositionRiskParams.mark`
  (`server.rs:432`). Implied spreads `s0`/`s1` are *derived* in the engine
  (`ZSpreadCalculator::calculate`, `profile.rs:126`) — nothing stored,
  matching the established "mark at call site" pattern. Spread PnL is keyed by
  the mark's `benchmark` category string — sufficient for v1 (issue-level
  spread attribution is explicitly deferred).
- **Placement.** `risk::pnl::types` + `convex-mcp` params. No new crate. No
  perf impact.

---

## Gap 6 — `Book` input + tagged `{ bond | swap }` position spec

- **What is missing.** No `Book` type; the wire convention is `Vec` of
  position specs (`AggregateBookRiskParams`, `BookGroup`). The EUR swap does
  not fit `BondRef` (it is not a `BondSpec`).
- **Why it matters.** The demo book is 3 bonds **+ a swap**; the input must
  carry both kinds plus per-position (t0, t1) marks.
- **Severity.** **Blocker.** **Effort.** S–M.
- **Design.** `Book { positions: Vec<PnlPositionSpec>, base_currency: Currency }`.
  `PnlPositionSpec` is a **tagged enum**, mirroring the existing
  `HedgeInstrument` convention (`types.rs:25`,
  `#[serde(tag="instrument", rename_all="snake_case")]`):
  - `bond { bond: BondRef, notional_face, mark_t0, mark_t1, position_id? }`
  - `swap { spec: InterestRateSwap-with-trade/maturity, mark? , position_id? }`
  This is the open-extensibility seam (future: `cash`, `future`) the hedge
  advisor's tagged enum established — adding a kind = add a variant, no engine
  rewrite.
- **Placement.** Domain `Book`/`PnlPositionSpec` in
  `convex-analytics::risk::pnl::types`; `AttributePnlParams` wrapper in
  `convex-mcp/src/server.rs`. **No new crate**; no `convex-portfolio`
  dependency (its `Portfolio` lacks `JsonSchema` and is off the MCP wire —
  reusing it would break the stateless pattern, investigation §1.4).
- **Performance.** Cold path; parsed once per call.

---

## Gap 7 — `attribute_pnl` MCP tool

- **What is missing.** Unregistered tool:
  `attribute_pnl(book, t0, t1, curve_t0, curve_t1, [config]) -> Attribution`.
- **Why it matters.** The external surface of the engine.
- **Severity.** **Blocker.** **Effort.** S (≈one `#[tool]` method + params
  struct; the macro derives schema + router, exactly like the four advisor
  tools).
- **Inputs.** `book: Book`, `t0: String` / `t1: String` (ISO-8601, parsed via
  `Date::parse` — *approved* over `NaiveDate*), `curve_t0: CurveRef`,
  `curve_t1: CurveRef`, `config: Option<AttributionConfig>` (pivot tenor,
  analysis grid, factor model). Resolve bonds/curves with the existing
  `resolve_bond`/`resolve_curve` helpers (`server.rs:188/200`).
- **Placement.** `convex-mcp/src/server.rs` (extend the existing
  `#[tool_router] impl ConvexMcpServer`). **No new crate.**
- **Performance.** Tool dispatch overhead is µs; the math is Gap 2.

---

## Gap 8 — `narrate_attribution` MCP tool + template narrator

- **What is missing.** A deterministic `narrate_attribution(&Attribution,
  [style]) -> String` and its MCP tool. No LLM call.
- **Why it matters.** v1 deliverable; carries the demo's narrative,
  including the **hero-moment swap clause**.
- **Severity.** **Blocker.** **Effort.** S.
- **Design.** Clone the `risk::hedging::narrate::narrate` pattern verbatim
  (`narrate.rs:18`): pure, deterministic ("same input → same bytes"),
  `String::with_capacity` + `std::fmt::Write`, `Currency::code()`, bp
  `{:.2}` / ccy `{:.0}`, provenance/heuristic disclosure. Must state: total
  PnL (bp **and** ccy), the biggest factor driver, the BTP-Bund spread move,
  and an explicit clause for the **swap's curve-PnL contribution** ("the
  pay-fixed swap absorbed €X of the curve move"). v1 ships one style
  (`TraderBrief`-equivalent) behind an extensible `NarrationStyle` enum.
- **Placement.** `convex-analytics::risk::pnl::narrate` +
  `convex-mcp/src/server.rs`. **No new crate** — same argument as hedge
  advisor gaps §7: a v2 LLM narrator (HTTP + API key) would justify a
  `convex-narrator` crate or live in `convex-mcp`; v1 template stays in
  `convex-analytics`.
- **Performance.** Cold path.

---

## Gap 9 — Provenance on attribution outputs

- **What is missing.** A provenance echo: both curve ids, the factor model
  name + pivot tenor, conventions, tool/advisor version. Upstream
  `PricingResult` carries no provenance.
- **Why it matters.** Transparent provenance is a stated pillar — the trader
  must see *which* curves, *which* factor model, *which* conventions produced
  the attribution, especially since the waterfall is path-ordered.
- **Severity.** **High.** **Effort.** S.
- **Design.** Reuse the shape of `risk::profile::Provenance` (`profile.rs:41`,
  already `Default` + `#[serde(default)]` + `JsonSchema`). Either embed it and
  add `factor_model: String` + `pivot_tenor_years: f64` + a `conventions`
  snapshot, or define a thin `PnlProvenance` with the same conventions. Echoed
  on `Attribution` and on each `PositionAttribution`.
- **Placement.** `convex-analytics::risk::pnl::types`. **No new crate.**
- **Performance.** Cold path; tiny struct.

---

## Gap 10 — Sequential-repricing benchmark

- **What is missing.** No bench covers the t0→t1 attribution path. The 5%
  regression bar needs a baseline.
- **Why it matters.** Sequential repricing is the documented hot path
  (~35–40 reprices/call); we must record a baseline and prove existing benches
  don't regress.
- **Severity.** **Medium.** **Effort.** S.
- **Design.** Add a `pnl` Criterion group (extend
  `convex-analytics/benches/hedge_advisor.rs` or a new `benches/pnl.rs` with a
  `[[bench]]` entry mirroring the existing one, `Cargo.toml:60`): a group
  attributing the 4-position demo book over one day. Record in
  `docs/perf-baselines.md`. Re-run `convex-bonds`/`convex-engine` benches
  before/after (untouched paths → expected flat, still verified).
- **Placement.** `convex-analytics/benches/`. **No new crate.**

---

## Gap 11 — Deferred (Low / out of v1 scope)

| Item | Why deferred |
| --- | --- |
| Multi-period chained attribution | v1 is exactly two dates. |
| Position changes during the period | v1 assumes a static book. |
| FX attribution | v1 single currency (EUR book). |
| Performance vs benchmark | Not in the demo's narrative. |
| Issue-level spread beyond benchmark category | Benchmark-keyed spread suffices for BTP/OAT-vs-Bund. |
| LLM-based narrator | Template only in v1 (would justify a new crate — see below). |
| Real-time / streaming, optimisation passes beyond regression | Out of scope. |

---

## Where a new crate could be justified — and why we still aren't proposing one

The only candidate is the **v2 LLM narrator** (identical to hedge advisor gaps
§7): once narration calls an LLM over HTTP it brings `reqwest`/an SDK + an
API-key env dependency, which does not belong in `convex-analytics` (pure
math + bonds + curves). At that point a `convex-narrator` crate, or placement
inside `convex-mcp` (which already owns transport), is correct.

**For v1 we are not building that.** The template narrator is deterministic
and dependency-free in `convex-analytics::risk::pnl::narrate`. A new crate now
would be premature and would contradict the hedge advisor precedent.

---

## Crate impact summary (v1)

| Crate | Change |
| --- | --- |
| `convex-analytics` | New module tree `risk::pnl::{mod, types, decompose, engine, narrate}`. Curvature basis is a closure over the existing `ScenarioBump::custom`. New `[[bench]]` (or extend `hedge_advisor.rs`). Re-export from `risk::mod.rs` + prelude. |
| `convex-mcp` | Two `#[tool]` methods (`attribute_pnl`, `narrate_attribution`) + their `Parameters<…>` / output structs on the existing `#[tool_router] impl`. New schema-bearing types arrive via the `convex` umbrella (`features = ["schemars"]`). |
| `convex-curves` | **Untouched** (`ScenarioBump::custom` already expresses any basis). |
| `convex-portfolio` | **Untouched** (no dependency added). |
| All other crates | Untouched. |
| **New crates** | **None.** |

This is the *same footprint* as the hedge advisor (new modules in
`convex-analytics`, new `#[tool]` methods in `convex-mcp`), reinforcing
architectural continuity.

---

## Performance bar

- **No regression > 5%** on existing `convex-bonds` / `convex-engine` /
  `convex-analytics` benches — guaranteed by construction (no existing code
  path is modified; the pricing core gains **no** valuation-date parameter,
  Gap 0), still re-run and recorded.
- **`attribute_pnl` (4-position book, 2 dates, 3 curve factors)** target:
  well under 1 ms (estimated low-hundreds of µs from the ≈35–40-reprice
  budget vs the 22 µs hedge-advisor single-position baseline).
- **No avoidable allocations** in the repricing loop: clone `curve.inner()`
  once per position, reuse the spread-fixed reprice kernel (root-find only for
  `s0`/`s1`), `with_capacity` factor vectors. JSON serialization may allocate.

---

## Resolved design decisions (carried into Phase 3)

1. **ISO-8601 `Date` strings, not `NaiveDate`.** `t0`/`t1` are `String`
   (parsed via `Date::parse`), matching `settlement: String` across the MCP
   surface and the "match the hedge advisor's patterns" constraint. *(Approved
   Phase-1 contradiction #1.)*

2. **Fixed-maturity swap model for PnL.** PnL values the swap at a maturity &
   fixed rate pinned at trade — *not* the constant-maturity
   `interest_rate_swap_risk`. This is what makes the demo's hero moment
   correct. *(Approved Phase-1 contradiction #2; Gap 4 Blocker.)*

3. **Decomposition is a new analytics function**, in
   `convex-analytics::risk::pnl::decompose`, reusing `ScenarioBump::custom`
   closures for component curves — `convex-curves` is not modified.
   *(Approved Phase-1 contradiction #3; Gap 1.)*

4. **Path-ordered waterfall, residual reported.** Attribution order is
   carry → roll-down → curve(parallel/slope/curvature) → spread → residual;
   the residual closes the identity and is surfaced (not hidden) so any path
   non-commutativity is visible — a trader-sovereignty/provenance choice.

5. **`risk::pnl` is a risk-module citizen**, sibling of `risk::profile` and
   `risk::hedging`, consuming their primitives. It does not live under
   `hedging` (attribution is not hedging) — mirrors the Phase-2 decision that
   put `compute_position_risk` in `risk::profile`, not `risk::hedging`.

These feed directly into `docs/pnl-narrator-plan.md` (Phase 3).
