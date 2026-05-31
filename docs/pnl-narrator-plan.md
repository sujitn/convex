# PnL Narrator — Phase 3 Fix Plan

Source: `docs/pnl-narrator-investigation.md` + `docs/pnl-narrator-gaps.md`.

**Top-line.** Zero new crates. **Six commits**, smallest first. Pricing core
**untouched** (Gap 0: `price_from_mark` already takes the valuation date — the
prompt's hypothesised blocker does not exist). One new module tree
`convex-analytics::risk::pnl`, two `convex-mcp` tools. Estimated ~M total.

This plan was written with an explicit anti-slop / anti-overengineering pass
(§3.6) — what was deliberately *cut* is as important as what's built.

---

## 3.1 Touched files and modules

### New files

| File | Why it's a separate file (not slop) |
| --- | --- |
| `crates/convex-analytics/src/risk/pnl/mod.rs` | Module declaration + re-exports. Mirrors `risk/hedging/mod.rs`. |
| `crates/convex-analytics/src/risk/pnl/types.rs` | Wire DTOs + `AttributionConfig` + `AttributionProvenance`. (Config/provenance live *here*, not in their own files — see §3.6.) |
| `crates/convex-analytics/src/risk/pnl/decompose.rs` | Pure curve-move → {parallel,slope,curvature,residual} math. Separate because it is independently unit-testable with hand-computed fixtures and has no pricing dependency. |
| `crates/convex-analytics/src/risk/pnl/engine.rs` | The sequential-repricing waterfall + book aggregation. |
| `crates/convex-analytics/src/risk/pnl/narrate.rs` | Deterministic template narrator. Separate for the same reason `hedging/narrate.rs` is. |

This is the **same shape as `risk/hedging/`** (`types`/`strategies`/`compare`/
`narrate`), i.e. the house pattern — not gratuitous fragmentation. No
`config.rs`, no `provenance.rs`, no `snapshot.rs`.

### Modified files

| File | Change |
| --- | --- |
| `crates/convex-analytics/src/risk/mod.rs` | `pub mod pnl;` + targeted re-exports (`attribute_pnl`, `narrate_attribution`, `Attribution`, `Book`, `PnlPositionSpec`, `AttributionConfig`). |
| `crates/convex-analytics/src/lib.rs` | Add the same names to the `risk` prelude block (matches how hedging types are exported). |
| `crates/convex-mcp/src/server.rs` | Two `#[tool]` methods + `AttributePnlParams` / `NarrateAttributionParams` / output structs on the existing `#[tool_router] impl`. |
| `crates/convex-analytics/benches/hedge_advisor.rs` | Add one `pnl` Criterion group (extend, don't create a new bench file — one fewer `[[bench]]` entry to maintain). |
| `README.md` | "PnL Narrator" section, same format as the "Hedge Advisor" section. |
| `docs/pnl-narrator-investigation.md` | Flip ❌→✅ as gaps close. |
| `docs/perf-baselines.md` | Record the `pnl` bench numbers; re-affirm existing benches unaffected. |

**`convex-curves` and `convex-portfolio`: not touched.** Component curves use
the existing `ScenarioBump::custom` closure (`scenario.rs:202`); the curvature
basis is a closure, not new curve infrastructure.

---

## 3.2 Performance plan

### Hot path: the repricing waterfall

Per position, the locked recipe (§3.3 commit 3) costs **9 reprices + 2 implied-spread
solves**:

| Reprice | Holds | Isolates |
| --- | --- | --- |
| `V0 = V(t0, curve_t0, s0)` | — | base |
| `V(t1, curve_t0, s0)` | curve, spread | time (carry+roll) |
| `V(t1, curve_t0, const-t0-yield)` | yield flat | carry (→ roll = time − carry) |
| `V(t1, curve_t0+Δparallel, s0)` | spread | parallel |
| `V(t1, curve_t0+Δslope, s0)` | spread | slope |
| `V(t1, curve_t0+Δcurv, s0)` | spread | curvature |
| `V(t1, curve_t1, s0)` | spread | curve total (→ residual_curve) |
| `V(t1, curve_t1, s1)` | — | spread (→ residual = total − Σ) |
| `V1 = V(t1, curve_t1, mark_t1)` | — | cross-check vs spread reprice |

Held-spread reprices use `ZSpreadCalculator::price_with_spread`
(`profile.rs:139`) — **no root-find**. Only `s0` and `s1` cost a Z-spread
solve (2/position). 4-position demo book ≈ **~36 reprices + 8 solves/call**.

Reference: hedge-advisor `risk_profile_apple_10y` ≈ **22 µs** for one
`price_from_mark` + full risk + 4-tenor KRD (`docs/perf-baselines.md`). A bare
reprice is ≈1–2 µs. Estimated `attribute_pnl` ≈ **low-hundreds of µs** for the
4-position book — well under interactive thresholds, no streaming needed.

### Regression bar

Existing pricing/curve/bond code paths are **not modified** (Gap 0 stands:
no valuation-date parameter is threaded through `price_from_mark`). Therefore
existing benches *cannot* regress by construction. Still re-run per the 5%
rule:

- `cargo bench -p convex-analytics --bench hedge_advisor` (baseline + new `pnl` group)
- `cargo bench -p convex-bonds --bench trinomial_tree`
- `cargo bench -p convex-engine`

Record `pnl` numbers in `docs/perf-baselines.md`.

### Allocation discipline

- Clone `curve_t0.inner()` / `curve_t1.inner()` **once per position**, not per
  reprice.
- `Vec::with_capacity` on the factor list (fixed length: 5 factors) and
  per-position vector (book size known).
- No `Box<dyn …>` in the engine (no trait objects — see §3.6).

---

## 3.3 Implementation sequence

Six commits, each reviewable and test-first where TDD applies. **Stop and
surface anything unexpected.**

### Commit 1 — domain types + config + provenance (S)

**Files:** `risk/pnl/mod.rs`, `risk/pnl/types.rs`; modify `risk/mod.rs`,
`lib.rs`.

**Surface (locked names — confirm at the gate, names are hard to change):**

```rust
// Book input. base_currency is single (v1 scope).
pub struct Book { pub positions: Vec<PnlPositionSpec>, pub base_currency: Currency }

// Tagged enum — mirrors HedgeInstrument's #[serde(tag=…, rename_all="snake_case")].
pub enum PnlPositionSpec {
    Bond  { position_id: Option<String>, /* bond fields via convex-mcp BondRef at the tool layer */
            notional_face: Decimal, mark_t0: String, mark_t1: String },
    Swap  { position_id: Option<String>, spec: SwapPnlSpec },
}

// Fixed-maturity swap (Gap 4): maturity & rate pinned at trade, NOT re-derived.
pub struct SwapPnlSpec {
    pub trade_date: Date, pub maturity: Date,
    pub fixed_rate_decimal: f64, pub fixed_frequency: Frequency,
    pub fixed_day_count: DayCountConvention,
    pub side: SwapSide,          // reuse risk::hedging::types::SwapSide
    pub notional: Decimal, pub currency: Currency,
}

pub enum PnlFactor { Carry, RollDown, CurveParallel, CurveSlope, CurveCurvature,
                     CurveResidual, Spread, Residual }   // enum, not strings

pub struct FactorPnl { pub factor: PnlFactor, pub pnl_ccy: Decimal, pub pnl_bps: f64,
                       pub benchmark: Option<String> }    // benchmark Some only for Spread

pub struct CurveBreakdown {                                // loadings in bp, for the narrator
    pub parallel_bps: f64, pub slope_bps: f64, pub curvature_bps: f64,
    pub pivot_tenor_years: f64, pub fit_residual_l1_bps: f64 }

pub struct PositionAttribution {
    pub position_id: Option<String>, pub kind: &'static str,  // "bond" | "swap"
    pub total_pnl_ccy: Decimal, pub total_pnl_bps: f64,
    pub factors: Vec<FactorPnl>, pub curve: CurveBreakdown }

pub struct Attribution {
    pub currency: Currency, pub t0: Date, pub t1: Date,
    pub book_market_value_t0: Decimal,
    pub total_pnl_ccy: Decimal, pub total_pnl_bps: f64,
    pub factors: Vec<FactorPnl>,                 // book-level, factor-summed
    pub positions: Vec<PositionAttribution>,
    pub provenance: AttributionProvenance }

pub struct AttributionConfig {                   // exactly two knobs (see §3.6)
    pub pivot_tenor_years: Option<f64>,          // default 2.0
    pub analysis_tenors: Option<Vec<f64>> }      // default = curve_t0 pillars

pub struct AttributionProvenance {               // dedicated, minimal, deterministic
    pub curve_t0_id: String, pub curve_t1_id: String,
    pub factor_model: String,                    // "level_slope_curv_v1"
    pub pivot_tenor_years: f64,
    pub tool_version: String }                   // env!(CARGO_PKG_VERSION); NO timestamp
```

Derives on every type: `Debug, Clone, PartialEq, Serialize, Deserialize` +
`#[cfg_attr(feature="schemars", derive(JsonSchema))]`, `schemars(with="f64")`
on every `Decimal`, `#[serde(default)]` on optional vectors (LLM round-trip
resilience — established hedge-advisor convention).

**Tests:** serde round-trip per type; `#[cfg(feature="schemars")]` schema
non-empty for `Attribution` and `Book`.

**Effort:** S. **Gate:** present this surface for confirmation before wiring.

### Commit 2 — curve-move decomposition (S/M)

**File:** `risk/pnl/decompose.rs`.

```rust
pub struct CurveDecomposition {
    pub parallel_bps: f64, pub slope_bps: f64, pub curvature_bps: f64,
    pub pivot_tenor_years: f64,
    pub residual_by_tenor: Vec<(f64, f64)>,   // unexplained Δr per analysis tenor, bps
}
pub fn decompose_curve_move(
    curve_t0: &RateCurve<DiscreteCurve>, curve_t1: &RateCurve<DiscreteCurve>,
    analysis_tenors: &[f64], pivot_tenor_years: f64,
) -> AnalyticsResult<CurveDecomposition>;
```

Math: sample `Δr_i = zero_rate_at_tenor(τ_i)` on both curves (continuous,
Act365 — the stored convention). Basis vectors, **normalized so loadings read
in bp**:
- `b_L(τ)=1` (parallel)
- `b_S(τ)=(τ−pivot)/span` (slope) — *same linear-about-pivot shape as
  `ScenarioBump::steepener`* so synthetic and decomposed slope agree (tested)
- `b_C(τ)= symmetric butterfly` (belly +1, wings −0.5; mean-zero, slope-zero)

Solve the 3×3 normal equations `(BᵀB)a = BᵀΔr` via
`convex_math::linear_algebra::solve_linear_system` with `nalgebra::{DMatrix,
DVector}` — the **exact pattern `key_rate_futures` already uses**
(`strategies.rs:632-644`); residual = `Δr − Σ aₖbₖ`.

**Tests (hand-computed, no pricing):**
- pure +10bp parallel move → `parallel≈10, slope≈0, curvature≈0, residual≈0`.
- pure steepener built via `ScenarioBump::steepener` → recovered `slope` matches
  the input, `parallel≈0` (the consistency property).
- identical curves → all factors 0, residual 0 (zero-move edge case).
- a kinked move → non-zero `residual_by_tenor` (residual is real, not hidden).

**Effort:** S/M.

### Commit 3 — sequential repricing engine (M/L) — the heart

**File:** `risk/pnl/engine.rs`.

```rust
pub fn attribute_pnl(
    book: &ResolvedBook,            // bonds/swaps already resolved (tool layer resolves refs)
    t0: Date, t1: Date,
    curve_t0: &RateCurve<DiscreteCurve>, curve_t0_id: &str,
    curve_t1: &RateCurve<DiscreteCurve>, curve_t1_id: &str,
    config: &AttributionConfig,
) -> AnalyticsResult<Attribution>;
```

Per position, the **path-ordered waterfall** (documented; residual closes the
identity):

1. `s0` ← implied spread from `mark_t0` + `curve_t0` (`ZSpreadCalculator::calculate`);
   `s1` ← from `mark_t1` + `curve_t1`. `V0`, `V1`.
2. `coupon_cash` ← `bond.cash_flows(t0)` filtered to `t0 < date ≤ t1`, coupon
   types (`CashFlowType::Coupon|CouponAndPrincipal`). (Usually 0 for the
   May-7→8 demo; handled generally.)
3. **Time** = `V(t1,curve_t0,s0) − V0 + coupon_cash`.
   **carry** = `coupon_cash + [V(t1,curve_t0, const-t0-YTM) − V0]`;
   **roll-down** = Time − carry.
4. **Curve total** = `V(t1,curve_t1,s0) − V(t1,curve_t0,s0)`. Decompose via
   commit 2; reprice `curve_t0 + ScenarioBump::custom(|t| loadingₖ_decimal·bₖ(t))`
   for k∈{parallel,slope,curvature}; **curve_residual** = curve_total − Σ.
5. **Spread** = `V(t1,curve_t1,s1) − V(t1,curve_t1,s0)`, tagged with the mark's
   `benchmark` string (BTP→Bund, OAT→Bund; Bund≈0).
6. **Residual** = Total − carry − roll − Σcurve − spread (absorbs
   second-order cross terms; small for a 1-day move; **reported**).

Swap positions (Gap 4): build a fixed-maturity `FixedRateBond`
(`issue=trade_date`, `maturity=spec.maturity`, `coupon=fixed_rate`) once;
value its fixed leg at `(t0,curve_t0)` / `(t1,curve_t1)` with `s≡0` (Z-flat,
matching `interest_rate_swap_risk`); pay-fixed PnL = `−Δ(fixed-leg PV)·sign`,
floating ≈ par at reset (the **same documented post-LIBOR≈0 approximation**
the hedge advisor ships). Swap spread factor ≡ 0; its PnL is ~pure curve —
*that is why it offsets the bonds.*

Book level: sum `FactorPnl` by `PnlFactor`; `positions` preserves input order;
`total_pnl_bps` is on `book_market_value_t0`.

**Tests:**
- **zero-move**: `curve_t1==curve_t0`, `mark_t1==mark_t0`, `t1==t0` → every
  factor and total ≈ 0 (the prompt's mandated edge case).
- **identity closure**: `|total − Σ factors| < 1e-6` ccy on the demo book.
- **known-answer parallel**: flat curve +10bp, one bond → curve PnL ≈
  `−DV01·10` within 1%; parallel factor ≈ curve total; slope/curv ≈ 0.
- **swap sign (the hero-moment guard)**: rates **rise** → long bond PnL < 0
  **and** pay-fixed swap PnL > 0 (partial offset). Explicit sign assertion.
- **per-position sums to book** for every factor.

**Effort:** M/L. **Gate:** present a sample `Attribution` JSON for the demo
book before MCP wiring.

### Commit 4 — template narrator (S)

**File:** `risk/pnl/narrate.rs`. Clone `hedging/narrate.rs` idioms verbatim:
`String::with_capacity`, `std::fmt::Write`, `Currency::code()`, bp `{:.2}` /
ccy `{:.0}`, deterministic.

```rust
pub fn narrate_attribution(a: &Attribution) -> String;
```

Must state, in order: total PnL (bp **and** ccy); the **largest-magnitude
factor** by name; the BTP-Bund (and OAT-Bund) **spread move**; an explicit
**swap clause** — e.g. *"the pay-fixed EUR swap contributed €X, absorbing N%
of the book's curve move — the hedge from last week working as designed."*
The swap clause fires whenever the book contains a swap whose curve PnL sign
opposes the bonds' (deterministic `if`, no heuristics).

**Tests:** mentions every position's kind; mentions "swap" + the offset when a
swap is present; deterministic (same input → identical bytes); handles a
bonds-only book (no swap clause, no panic).

**Effort:** S.

### Commit 5 — two MCP tools (S)

**File:** `crates/convex-mcp/src/server.rs`.

```rust
pub struct PnlPositionParams {                 // tool-layer: BondRef instead of resolved bond
    #[serde(flatten)] kind: PnlPositionKind }   // tagged: bond{bond:BondRef,…} | swap{spec}
pub struct AttributePnlParams {
    book: Vec<PnlPositionParams>, base_currency: Currency,
    t0: String, t1: String,                    // ISO-8601 (Date::parse) — NOT NaiveDate
    curve_t0: CurveRef, curve_t1: CurveRef,
    #[serde(default)] config: Option<AttributionConfig> }
pub struct NarrateAttributionParams { attribution: Attribution }
```

`attribute_pnl` resolves bonds/curves via the existing
`resolve_bond`/`resolve_curve` (`server.rs:188/200`), builds `ResolvedBook`,
calls the engine, `Self::json_result`. `narrate_attribution` →
`{ text: String }`. Errors via `McpToolError` (existing `From<AnalyticsError>`).

**Tests:** `#[tokio::test] pnl_narrator_e2e_oat_book` — the 4-position demo
book through `attribute_pnl` → `narrate_attribution`; assert provenance present
(both curve ids, factor model), per-position + book factors present, swap
contribution non-zero and sign-correct, narration mentions the swap.

**Effort:** S.

### Commit 6 — bench + docs + investigation flip (S)

**Files:** extend `benches/hedge_advisor.rs` (`pnl` group: the 4-position book,
2 dates); `README.md` ("PnL Narrator" section, mirrors "Hedge Advisor");
`docs/perf-baselines.md` (record `pnl`, re-affirm others flat);
`docs/pnl-narrator-investigation.md` (❌→✅).

**Benchmarks run:** `pnl` group + regression re-run of
`convex-bonds`/`convex-engine`. **Effort:** S.

### Effort

| Commit | Effort |
| --- | --- |
| 1 types/config/provenance | S |
| 2 decomposition | S/M |
| 3 engine (heart) | M/L |
| 4 narrator | S |
| 5 MCP tools | S |
| 6 bench + docs | S |
| **Total** | **~M** (smaller than hedge advisor: no strategies, no cost model, no CTD) |

---

## 3.4 Demo plan

**Book** (same as hedge advisor + the swap; EUR, single currency):

| Position | Instrument | Notional | Mark t0 / t1 |
| --- | --- | --- | --- |
| OAT 10Y | long bond | €10mm | spread to EUR curve |
| BTP 4.0% Feb-2035 | long bond | €5mm | G/Z-spread (BTP-Bund) |
| Bund 2.5% Aug-2034 | long bond | €10mm | spread ≈ 0 (it *is* the benchmark) |
| Pay-fixed €10mm 10Y EUR swap | swap (fixed maturity) | €10mm | n/a (curve-priced) |

**Curves:** two pasted `CurveSpec` blocks, `reference_date` 2026-05-07 and
2026-05-08 (same paste format as the hedge advisor / `demo/data` JSON).

**Agent flow:**
1. User pastes book + curve_t0 + curve_t1 + the two spread marks per bond.
2. `attribute_pnl(book, "2026-05-07", "2026-05-08", curve_t0, curve_t1)` →
   `Attribution` (book totals + per-position + per-factor + provenance).
3. `narrate_attribution(attribution)` → paragraph.
4. Follow-up answered from the structured output.

**Expected `Attribution` shape:** book `total_pnl_ccy`/`_bps`; `factors` =
[carry, roll-down, curve_parallel, curve_slope, curve_curvature,
curve_residual, spread{Bund≈0, BTP, OAT}, residual]; `positions[*]` same
decomposition; the swap's curve factor opposing the bonds'.

**Narration verifies:** total (bp+ccy); biggest driver (curve); BTP-Bund
widening; **the swap absorbing €X of the curve move** — the hero moment.

---

## 3.5 Risks and tradeoffs

| # | Risk | Mitigation |
| --- | --- | --- |
| 1 | **Swap PnL sign flip** — would invert the hero moment. | Dedicated test: rates↑ ⇒ long-bond PnL<0 *and* pay-fixed swap PnL>0. Reuse the proven `SwapSide` sign (`instruments.rs:164`). Highest-priority test. |
| 2 | Path-ordered waterfall is not unique; residual absorbs cross terms. | Order fixed & documented; residual is a first-class reported factor; identity-closure test `<1e-6`. Honest, not hidden. |
| 3 | Constant-maturity swap trap (Gap 4) if `interest_rate_swap_risk` were reused naively. | Fixed-maturity model: `issue=trade_date`, `maturity=spec.maturity` pinned. Explicit test that swap maturity does not move with the valuation date. |
| 4 | Decomposition basis arbitrariness (slope/curvature shapes). | Slope basis = `ScenarioBump::steepener` shape (consistency tested); curvature mean-/slope-zero; loadings in bp; residual surfaced. Pivot configurable, default 2Y. |
| 5 | Curve grids differ between t0/t1 paste. | Sample both on `analysis_tenors` (default curve_t0 pillars) via `zero_rate_at_tenor`; document that t0 pillars define the analysis grid. |
| 6 | Floating-leg ≈ par approximation on the swap. | Same documented post-LIBOR≈0 assumption already shipped in the hedge advisor; magnitude < fixed-leg DV01 between resets; stated in provenance/README. |

**Deferred to v2 (explicitly not built):** multi-period chained attribution;
position changes mid-period; FX attribution; performance vs benchmark;
issue-level spread beyond benchmark category; LLM narrator; optimisation
passes beyond the regression check.

---

## 3.6 AI-slop / overengineering review (what was deliberately cut)

A senior-engineer pass over the design. Each item below was considered and
**rejected** to keep v1 tight:

- **No `HedgeLeg`-style trait or trait objects.** PnL has exactly two
  instrument kinds and both reduce to "value a `FixedRateBond` at
  `(date, curve, held-spread)`". A trait + `Box<dyn>` dispatch would add a
  v-table and a generic surface for **zero** present benefit. The engine is
  plain functions with a `match` on `PnlPositionSpec`. Add a kind = add a
  match arm (the same extensibility the hedge advisor's tagged enum gives,
  without the trait).
- **No `MarketSnapshot` wrapper type.** The prompt floated it as *optional*.
  Two `CurveRef` + two date strings + per-position `(mark_t0, mark_t1)` fully
  express the demo. A wrapper would be a type for its own sake.
- **No timestamp in provenance.** The hedge advisor's shipped `Provenance`
  has none; a `computed_at: DateTime` would make outputs non-deterministic and
  break the "same input → same bytes" property the narrator relies on for
  reproducible demos. Provenance is the inputs that determine the result,
  not wall-clock.
- **Dedicated 5-field `AttributionProvenance`, not the shared hedge
  `Provenance`.** Reusing the hedge `Provenance` would drag `cost_model` /
  `oas_volatility` (meaningless for PnL) into every attribution output.
  *Fewer* fields and *less* coupling — the minimal type is the non-slop choice
  here, not the extra one.
- **`AttributionConfig` has exactly two knobs** (`pivot_tenor_years`,
  `analysis_tenors`). No compounding toggle, no "include_residual" flag, no
  factor on/off switches, no rounding policy. Defaults cover the demo.
- **No generic/pluggable factor framework.** Factors are a fixed `enum`, basis
  functions are three hard-coded closures. A `Vec<Box<dyn BasisFn>>` registry
  is precisely the speculative generality to avoid.
- **No new bench file.** Extend `hedge_advisor.rs` with a `pnl` group — one
  fewer `[[bench]]` entry and `Cargo.toml` edit.
- **No `convex-portfolio` reuse.** Its `Portfolio`/`Holding` lack `JsonSchema`
  and are off the MCP wire; bending the stateless tool pattern to reuse them
  would be more code and more coupling than a thin `Book`.
- **Carry/roll-down split kept but minimal** — one extra constant-yield
  reprice, not a term-structure-of-carry model. The prompt requires both
  fields; this is the cheapest correct split.
- **Six commits, not eleven.** The hedge advisor needed 11 for 5 strategies +
  cost model + CTD + 4 tools. PnL has none of those. Padding the commit count
  to look thorough would be slop.

**Net:** one module tree, five small files, two tools, zero new crates, zero
core changes, six commits. Every type that exists is on the wire or directly
testable; nothing is built "for v2".

---

## Stop-and-confirm gates during Phase 4

- **After Commit 1 (types):** present the type surface — names + units +
  derives. Names are hard to change post-wire.
- **After Commit 3 (engine):** present a sample `Attribution` JSON for the
  demo book. Sanity-check factor signs (esp. the swap) before MCP wiring.
- **After Commit 6 (bench + docs):** present bench numbers + the regression
  re-run. End-of-Phase-4 review.

Anything unexpected mid-implementation → stop and surface, per the prompt's
reporting cadence.
