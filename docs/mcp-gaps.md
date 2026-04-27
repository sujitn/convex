# Convex MCP — Prioritized Gap Report

Companion document to `docs/mcp-audit.md`. Each gap is rated by **severity** (Blocker / High / Medium / Low) and **effort** (S = hours, M = 1–2 days, L = week+), and is anchored to one of the five strategic pillars:

1. **Trader-mark sovereignty** — the human owns the spread, the system owns the math.
2. **Type safety as correctness** — conventions are enums, not strings.
3. **Transparent provenance** — every number explains where it came from.
4. **Agent-native** — MCP-first, schema-first, deterministic tool surface.
5. **Open & embeddable** — Bloomberg can't ship this; QuantLib can't reach it.

Order: severity ↓, then effort ↑.

---

## Blockers

### G-1 · No `Mark` enum in pricing API

- **What:** There is no `Mark` enum in `convex-core` or `convex-engine`. Pricing is price-in / yield-out only; trader cannot input a spread or a yield as the canonical mark.
- **Why it matters:** Pillar 1 — trader-mark sovereignty is the architectural heart of the strategy. If the API can't accept "this bond is at +125 over the swap curve," the product narrative collapses to "yet another QuantLib wrapper."
- **Severity:** **Blocker**.
- **Effort:** **M**. Define enum + variants in `convex-core::types`, add `price_from_mark(bond, curve, mark, settlement) → PricingResult` in `convex-analytics` (delegating to existing yield/spread→price paths), wire one MCP tool. Existing callers continue using price-in helpers.
- **Suggested fix:**
  ```rust
  // convex-core::types::mark
  pub enum Mark {
      Spread { value: Spread, benchmark: BenchmarkRef },
      Price  { value: Decimal, kind: PriceKind /* Clean | Dirty */ },
      Yield  { value: Decimal, kind: YieldKind /* YTM | YTC | YTW */, frequency: Frequency },
  }
  ```
  Land this first, behind a new `price_bond` MCP tool. Refactor `calculate_yield` / `calculate_z_spread` later — don't break them on day one.

---

## High

### G-2 · MCP tools mutate hidden state via `bonds` / `curves` registries

- **What:** `ConvexMcpServer` holds `Arc<RwLock<HashMap>>` registries. `create_bond` / `create_curve` are write-only side-effects with no idempotency; analytic tools require those registries to be populated first.
- **Why it matters:** Pillar 4 — agent-native means stateless, idempotent, self-contained tool calls. Hidden state breaks reproducibility, complicates HTTP transport semantics (per-session state), and forces agents to maintain handle bookkeeping.
- **Severity:** High.
- **Effort:** M.
- **Suggested fix:** Make analytic tools accept inline `BondSpec` / `CurveSpec` objects. Keep registries as a convenience cache for sessions that want it, but every tool must work without prior `create_*` calls. Once that lands, deprecate the standalone `create_bond` / `create_curve` tools (or keep them for human-in-the-loop demos).

### G-3 · Hard-coded conventions in `create_bond`

- **What:** `create_bond` ignores user input for frequency, day count, currency, and face value. It always builds `SemiAnnual / Thirty360US / USD / 100`. No `DBR.10Y` (Annual / ActAct ICMA / EUR), no `JGB`, no `Bund`, no UK gilt can be created via MCP.
- **Why it matters:** Pillars 2 + 5. The whole crate has rich convention enums; the MCP surface throws them away. This is also a correctness landmine — the demo data uses ICMA day counts internally but `create_bond` will silently coerce.
- **Severity:** High.
- **Effort:** S.
- **Suggested fix:** Add `frequency: Frequency`, `day_count: DayCountConvention`, `currency: Currency`, `face_value: f64` (or default 100) to `CreateBondParams`, derive enum schemas via `schemars`. Pre-condition for any production use.

### G-4 · Outputs are stringly JSON, not typed structs with derived schemas

- **What:** Every tool returns `serde_json::json!({...})` blobs. No output type is declared, so clients can't type-check responses; LLMs can't introspect the structure either.
- **Why it matters:** Pillars 3 + 4. Schema-first means *both* sides of the contract — input *and* output. Stringly outputs preclude the structured-output features `rmcp 0.12` already supports.
- **Severity:** High.
- **Effort:** M.
- **Suggested fix:** Define `#[derive(Serialize, JsonSchema)]` output types per tool (e.g. `YieldResult`, `SpreadResult`, `PricingResult`). Return them via `Json<T>` content rather than `Content::text(json_string)`.

### G-5 · No curve / convention provenance on outputs

- **What:** `calculate_z_spread` echoes `curve_id` but no reference date, build method, day count, interpolation, or curve-as-of timestamp. `calculate_yield` returns no curve, no day count, no compounding.
- **Why it matters:** Pillar 3. Without provenance, an agent's "the bond's Z-spread is 95 bps" is unfalsifiable. A trader needs to know which curve, which date, which conventions produced the number.
- **Severity:** High.
- **Effort:** S (once G-4 lands — it's just adding fields to the new output structs).
- **Suggested fix:** Every analytic output struct carries `curve_provenance: CurveProvenance { id, reference_date, day_count, interpolation, build_method }` and `bond_provenance: BondProvenance { id, currency, day_count, frequency, settlement_date }`.

### G-6 · Missing core tools (`price_bond`, `compute_spread`, `shock_curve`, `attribute_pnl`, `parse_term_sheet`, `build_curve` from instruments)

- **What:** Only `calculate_yield` and `calculate_z_spread` exist. The other five spread methods, OAS, FRN discount margin, key-rate duration, scenario bumps, P&L attribution, and term-sheet parsing — none are MCP tools, despite all but the last existing in the underlying crates.
- **Why it matters:** Pillars 4 + 5. The differentiator vs. a generic library wrapper is that an LLM can survey *all* the standard fixed-income calls and pick the right one. Today the LLM sees a slice so small it has nothing to choose from.
- **Severity:** High (per missing tool).
- **Effort:** S each for stubs with full schemas; M each for full implementations on top of existing engines (parse_term_sheet is L and is explicitly out of scope for this session — stub only).
- **Suggested fix:** Land all stubs in one PR after G-1 to G-5 are in place. Stubs return `Err(NotImplemented)` so the tool list is complete from day one and we can fill them in incrementally without breaking the surface.

### G-7 · `calculate_yield` requires prior `create_bond` — bond can't be specified inline

- **What:** Same root cause as G-2, but it shows up most painfully on the most basic tool. To get a yield, an agent must first call `create_bond` (with hard-coded conventions, see G-3), get a handle, then call `calculate_yield` with the handle.
- **Why it matters:** Pillar 4. Agents pay token cost per round-trip; forcing two round-trips for one calculation is wrong.
- **Severity:** High.
- **Effort:** S (subset of G-2).
- **Suggested fix:** Allow `BondSpec` *or* `bond_id` as alternatives in the input schema (one-of). Same pattern for `CurveSpec` / `curve_id`.

---

## Medium

### G-8 · No reference-verification harness wired into MCP / CI

- **What:** `tools/reconcile_bench` and `reconciliation/` exist (milestone 1 done), and `tests/fixtures/quantlib_reference_tests.json` is checked in, but nothing exercises them through MCP. The MCP crate has no `tests/` directory at all.
- **Why it matters:** Pillars 3 + 5. "Bloomberg-YAS parity" is a brand promise; until each MCP tool has a test that calls it and compares to a pinned reference, the promise is rhetoric. Also gates Pillar 4: agents trust deterministic, reference-verified tools more.
- **Severity:** Medium.
- **Effort:** M.
- **Suggested fix:** Add `crates/convex-mcp/tests/smoke.rs` that spins up `ConvexMcpServer`, calls each tool with a canonical fixture, and asserts schema-shape + numeric tolerance against a reference (start with the demo bonds vs. the existing QL fixtures). Run in CI.

### G-9 · MCP bypasses `convex-traits` / `convex-engine` hex layer

- **What:** `convex-mcp` reaches into `convex-bonds` and `convex-analytics` directly instead of consuming the trait-based pricing engine in `convex-engine`. The richer `PricingInput` / `BondQuoteOutput` surface (bid/ask, OAS, multi-curve, key-rate dur) is invisible to MCP.
- **Why it matters:** Pillars 1 + 5. The hex layer is exactly where the `Mark` enum will live; if MCP doesn't consume it, every new feature has to be re-plumbed twice. Also: MCP and the Excel/server adapters drifting apart is a long-term maintenance hazard.
- **Severity:** Medium (but interacts with G-1: doing them together is cheaper).
- **Effort:** M.
- **Suggested fix:** When `Mark` is added, place it in `convex-core` and add a new method on the `convex-engine::PricingRouter` (or a new free function in `convex-analytics`) that consumes it. MCP calls that, not the bare analytics functions.

### G-10 · No `parse_term_sheet` stub

- **What:** Listed for clarity only — fully out of scope for this session per the prompt. But a stub returning `Err(NotImplemented)` with a real input schema *is* useful: it makes the gap visible to agents and pins the eventual API shape.
- **Why it matters:** Pillar 4 (advertised tool surface) + roadmap signaling.
- **Severity:** Medium.
- **Effort:** S.
- **Suggested fix:** Stub only. Schema: `{ pdf_bytes: Bytes, hint: Option<String> } → Result<BondSpec, ReviewRequired>`.

### G-11 · `coupon_rate`, `clean_price`, `tenor`, `rates` lack unit suffixes in input field names

- **What:** Input field names like `coupon_rate` (described as percentage), `clean_price` (units undocumented), `tenor` (years), `rates` (percentages) violate the project's own output convention (`ytm_pct`, `zero_rate_pct`, `z_spread_bps`).
- **Why it matters:** Pillar 2/3. LLM-callable APIs can't recover from ambiguous units after the fact — they propagate quietly. The audit's CLAUDE.md explicitly notes a long-standing percentage-vs-decimal convention split between FFI and Excel layers.
- **Severity:** Medium.
- **Effort:** S.
- **Suggested fix:** Rename to `coupon_rate_pct`, `clean_price_per_100`, `tenor_years`, `zero_rates_pct`, `face_value_per_100`. Doc-comments stay, but the name carries the unit.

### G-12 · No structured error variants — all errors are stringly

- **What:** Every error funnels through `McpError::invalid_params(String, None)` or `internal_error(String, None)`. Agents can't program against `BondNotFound` vs. `ConvergenceFailure` vs. `InvalidDate`.
- **Why it matters:** Pillar 4. Typed errors let agents recover (e.g. "bond not found → call `create_bond`"). Stringly errors force the agent to do natural-language pattern-matching.
- **Severity:** Medium.
- **Effort:** S.
- **Suggested fix:** Add a `McpToolError` enum in `convex-mcp` with named variants and an `Into<McpError>` conversion that fills `code` + `data` per MCP spec.

### G-13 · `bootstrap_curve` not exposed (only raw-zero `create_curve`)

- **What:** `convex-curves::calibration` supports Deposit / FRA / Swap / OIS bootstrapping (Global Fit + piecewise) and the `convex-ffi` / Excel layers expose this fully. MCP ignores it. Only raw `(tenors, rates)` zero curves can be built.
- **Why it matters:** Pillar 5. A trader hands an agent today's deposit + swap rates; the agent should be able to bootstrap a curve directly. Forcing pre-bootstrapped zeros pushes work onto the agent that the library already does.
- **Severity:** Medium.
- **Effort:** S (the engine exists; just wrap it).
- **Suggested fix:** New tool `bootstrap_curve(instruments: Vec<Instrument>, valuation_date: Date, interpolation, day_count, method) → CurveSpec`. Subsumes the target `build_curve`.

---

## Low

### G-14 · stdio is the only default transport advertised in client configs

- **What:** The `http` feature exists and works; the README and integration guide both bury it. Remote-agent use cases (Claude on the web, agents-as-a-service) need HTTP foregrounded.
- **Why it matters:** Pillars 4 + 5.
- **Severity:** Low (the code works; it's a docs gap).
- **Effort:** S.
- **Suggested fix:** Add a "Remote / hosted" section to the README with a `curl` smoke test and a `fly.io` config (one already exists at workspace root — wire it up in docs).

### G-15 · No `tracing` instrumentation on tool handlers

- **What:** The crate uses `tracing` at startup but tool handlers are uninstrumented — no per-tool spans, no input/output debug logs.
- **Why it matters:** Pillar 3 (debuggability of agent workflows).
- **Severity:** Low.
- **Effort:** S.
- **Suggested fix:** `#[tracing::instrument(skip(self))]` on each handler with structured fields for tool name + key inputs.

### G-16 · `create_bond` / `create_curve` silently overwrite existing IDs

- **What:** Storing under an existing ID replaces the prior value with no warning.
- **Why it matters:** Pillar 4 (determinism).
- **Severity:** Low.
- **Effort:** S.
- **Suggested fix:** Either reject duplicates with `BondAlreadyExists` (typed error) or treat the registry as content-addressable (hash the spec, return the hash as the ID). Latter is cleaner.

---

## Recommended ordering for Part 3

If approved, work them in this order — each builds on the prior:

1. **G-1** (`Mark` enum) + **G-3** (convention enums in `create_bond`) — foundational. **M+S.**
2. **G-4** (typed outputs) + **G-5** (provenance) + **G-11** (unit suffixes) — shape the new tool contracts before adding more tools. **M+S+S.**
3. **G-7** + **G-2** (inline specs, deprecate registry coupling) — relies on G-3/G-4. **S+M.**
4. **G-12** (typed errors) — clean prerequisite for stub work. **S.**
5. **G-6** (add tool stubs for the missing target surface, + **G-10** parse_term_sheet stub, + **G-13** bootstrap_curve real implementation). **S each + M for bootstrap_curve.**
6. **G-8** (smoke-test harness in CI). **M.**
7. **G-9** (route MCP through `convex-engine`) — opportunistic, do alongside whichever step it most simplifies.
8. **G-14**, **G-15**, **G-16** — polish.

Steps 1 and 2 are the only ones that change semantics; everything after is additive.
