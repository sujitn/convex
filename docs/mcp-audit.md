# Convex MCP Audit

Audit of the `convex-mcp` crate as a foundation for the strategic vision: an open, embeddable, agent-native fixed-income analytics MCP server with trader-mark sovereignty.

Audit date: 2026-04-26
Audited revision: branch `main`, head `1ab5899`
Crate version: `0.12.0`

Legend: ✅ done · ⚠️ partial / risk · ❌ gap

---

## 1.1 Crate inventory

**Workspace crates (15):**

| Crate | Role |
|---|---|
| `convex-core` | Core domain types (`Date`, `Price`, `Yield`, `Spread`, `SpreadType`, `Currency`, `Frequency`, `Compounding`, `DayCountConvention`), core traits (`YieldCurve`, `PricingEngine`, `RiskCalculator`, `SpreadCalculator`, `Discountable`). |
| `convex-math` | Solvers, root-finding, numerical helpers. |
| `convex-curves` | Curve construction (`DiscreteCurve`, `RateCurve`), bootstrapping (`GlobalFitter`, OIS, mixed instruments), interpolation, bumping, scenarios. |
| `convex-bonds` | Bond instruments (`FixedRateBond`, `ZeroCouponBond`, `FloatingRateNote`, `CallableBond`), pricing engines, ARRC FRN, options/trinomial tree. |
| `convex-analytics` | Free-function analytics (`yield_to_maturity`, `modified_duration`, `convexity`, `dv01`), spread calculators (Z, I, G, ASW, OAS, DM), YAS calculator. |
| `convex-portfolio` | Portfolio/book aggregation, ETF/SEC reporting, benchmarking. |
| `convex-ffi` | C ABI for Excel-DNA. |
| `convex-wasm` | WASM bindings. |
| `convex-mcp` | **MCP server (this audit's subject).** |
| `convex-traits` | Hexagonal port traits: storage, transport, market_data, reference_data, ids, output, config. |
| `convex-engine` | Pricing-engine adapter built on traits — `PricingInput`, `PricingRouter`, `BondQuoteOutput`, calc graph, reactive market-data listener. |
| `convex-ext-redb` / `-file` / `-json` | Trait adapters (storage / file / json). |
| `convex-server` | Axum-based pricing server (separate from MCP). |
| `tools/reconcile_bench` | QuantLib reconciliation bench (binary `convex_bench`). |

**`convex-mcp` dependencies (downstream):**
- `convex-core`, `convex-curves`, `convex-bonds`, `convex-analytics`. ✅

**Reverse dependencies on `convex-mcp`:** none (only the workspace root). ✅

**Hexagonal boundary:**
- `convex-mcp` does **not** depend on `convex-traits` or `convex-engine` — it bypasses the trait-based hexagonal layer entirely and reaches directly into `convex-bonds` instruments and `convex-analytics` functions. ⚠️
- It is, however, a leaf adapter (no reverse deps), so the boundary is preserved in the dependency-graph sense. ✅
- The richer `PricingInput` / `PricingRouter` / `BondQuoteOutput` surface in `convex-engine` (with bid/mid/ask, multiple curves, OAS, FRN DM, key-rate durations) is **not used by MCP**. The MCP server reimplements a much thinner slice of that surface against the lower-level analytics functions. ⚠️

**Infrastructure leakage into the calc engine:** none observed. The calc engine is clean. ✅

---

## 1.2 MCP server implementation

| Item | Status | Detail |
|---|---|---|
| MCP SDK | ✅ | `rmcp 0.12` with `server` + `macros` features. |
| Transport — stdio | ✅ | Default feature `stdio` → `rmcp/transport-io`. Handler in `main.rs::run_stdio_server`. |
| Transport — streamable HTTP | ✅ | Optional feature `http` → `rmcp/transport-streamable-http-server` + `axum`. Handler in `main.rs::run_http_server` mounts `/mcp` with CORS and a `/health` endpoint. |
| Transport — SSE / WebSocket | ❌ | Not configured. |
| Statefulness | ⚠️ | Server holds two `Arc<RwLock<HashMap<String, _>>>` — `bonds` and `curves` — keyed by user-supplied IDs. Tool calls are **not** stateless: `create_bond` mutates state, `calculate_yield` / `calculate_z_spread` rely on prior state. This is a problem for stateless agents and per-request HTTP semantics. Each HTTP session gets a fresh server (good), but within a session there's hidden mutable state. |
| Error → MCP error conversion | ⚠️ | Uses `McpError::invalid_params(msg, None)` and `McpError::internal_error(msg, None)` with stringified causes. No typed error variants reach the wire — agents see free-text error reasons. |
| Logging / tracing | ✅ | `tracing` + `tracing-subscriber` with `EnvFilter`. Log routing is correct: stderr-only when stdio (so it doesn't corrupt the protocol). |
| Server info / capabilities | ✅ | `ServerInfo` populated (name, version, title, website_url) and tool capability advertised. Demo-mode-aware instructions. |

**Server identity:**
```rust
pub struct ConvexMcpServer {
    pub bonds: Arc<RwLock<HashMap<String, StoredBond>>>,
    pub curves: Arc<RwLock<HashMap<String, StoredCurve>>>,
    demo_mode: bool,
    demo_data: Option<Arc<DemoData>>,
    tool_router: ToolRouter<Self>,
}
```

The mutable `HashMap<String, _>` registries are the architectural issue: they push the server toward "REPL-with-handles" rather than "stateless function-call surface." This conflicts with the agent-native goal — every interaction should ideally be self-contained.

---

## 1.3 Tool surface

**Tools currently exposed (10 total):**

| Tool | Description (verbatim) | Stateless? |
|---|---|---|
| `create_bond` | Create a new fixed rate bond. Coupon rate is in percentage (e.g., 5.0 for 5%). | ❌ writes registry |
| `calculate_yield` | Calculate yield to maturity (YTM) from clean price. Returns yield as percentage. | ❌ reads registry |
| `create_curve` | Create a yield curve from zero rate points. Rates should be in percentage. | ❌ writes registry |
| `get_zero_rate` | Get zero rate at a specific tenor. Returns rate as percentage. | ❌ reads registry |
| `calculate_z_spread` | Calculate Z-spread for a bond. Returns spread in basis points. | ❌ reads registry |
| `list_all_bonds` | List all bonds currently stored in the system. | ❌ reads registry |
| `list_all_curves` | List all curves currently stored in the system. | ❌ reads registry |
| `get_market_snapshot` | Get the demo market snapshot with key rates. Only in demo mode. | ✅ |
| `list_demo_bonds` | List all demo bonds with details. Only in demo mode. | ✅ |
| `list_demo_curves` | List all demo curves with details. Only in demo mode. | ✅ |

**Schema derivation:** parameter structs derive `JsonSchema` via `rmcp::schemars` (not the workspace `schemars` directly — they're aliased via the SDK re-export). ✅

**Output shape:** every tool returns ad-hoc `serde_json::json!({...})` objects. Outputs are **not** typed structs and have **no derived JSON schema**. ⚠️

**Idempotency:** `create_bond` and `create_curve` silently overwrite existing entries with the same ID. ⚠️

**Comparison vs. target tool surface:**

| Target tool | Present? | Notes |
|---|---|---|
| `parse_term_sheet(pdf_bytes) → Bond \| ReviewRequired` | ❌ | Not present, not even as a stub. |
| `build_curve(instruments, valuation_date) → Curve` | ⚠️ | `create_curve` exists but only consumes raw zero rates. Bootstrap-from-instruments (deposits/swaps/OIS/FRA) exists in `convex-curves::calibration` and is exposed as Excel UDFs but not as MCP tools. |
| `price_bond(bond, curve, mark) → PricingResult` | ❌ | No `price_bond` tool. There is `calculate_yield` (price-in → yield-out) but no price-out tool, no `Mark` input, no PV/PnL output, no curve provenance. |
| `compute_spread(bond, curve, kind) → Spread` | ⚠️ | Only `calculate_z_spread`. `convex-analytics::spreads` already implements I-spread, G-spread, ASW (par + proceeds), OAS, discount margin — none are MCP tools. |
| `attribute_pnl(book, t0, t1) → Attribution` | ❌ | Not present. |
| `shock_curve(curve, shock) → Curve` | ❌ | Not present (despite `convex-curves::bumping` providing parallel and key-rate bumps). |

**Coverage gap summary:** ~2 of 6 target tools are partially covered; 4 are missing entirely. None of the existing tools accept a trader mark. Several rich underlying engines (OAS, FRN DM, key-rate duration, callable trinomial pricing, ARRC FRN, YAS, bootstrapping, scenario bumping) are completely invisible via MCP.

---

## 1.4 The `Mark` contract — CRITICAL

**Result: ❌ no `Mark` enum exists.**

Searched: `Mark`, `enum Mark`, `trader_mark`, `TraderMark`. Only `Spread` (in `convex-core::types::spread`) and `Yield` (in `convex-core::types::yield_type`) exist as standalone value types — neither is unioned into an explicit pricing input.

**How pricing currently works:**

- `convex-core::traits::PricingEngine::price(bond, curve, settlement_date) → Price` — discounts cash flows off the curve. No mark.
- `convex-core::traits::PricingEngine::yield_to_maturity(bond, price, settlement_date) → Yield` — price-in, yield-out. Implicit mark = price.
- `convex-engine::pricing_router::PricingInput` carries `market_price_bid/mid/ask: Option<Decimal>` (clean price) plus discount/benchmark/government/volatility curves. The mark is implicitly **clean price (bid/mid/ask)** — there is no way to mark a bond by spread or yield through this API.
- The MCP `calculate_yield` tool takes `clean_price: f64`. The MCP `calculate_z_spread` tool takes `clean_price: f64`. The MCP `create_bond` tool takes no mark at all (it just stores the instrument).

**Impact:** the trader cannot say "this bond is at +125 over the curve" or "this bond yields 5.20%" and ask for the implied price. They must back into the price first. This violates the "trader-mark sovereignty" pillar of the strategy — the human's mark is not a first-class input.

**This is the highest-priority architectural gap.**

---

## 1.5 Schema derivation

| Item | Status | Detail |
|---|---|---|
| Input schemas via `schemars` | ✅ | All 5 parameter structs derive `JsonSchema` (via `rmcp::schemars`). |
| Output schemas via `schemars` | ❌ | Outputs are hand-rolled `serde_json::json!({...})` blobs — no derived schema, no type contract surfaced to the client. |
| Domain enums exposed as enums | ❌ | None of the user-facing domain enums (`DayCount`, `BusinessDayConvention`, `Frequency`, `Compounding`, `Currency`, `Calendar`) are in the schema at all — they're hard-coded in `create_bond`'s body to `Frequency::SemiAnnual`, `DayCountConvention::Thirty360US`, `Currency::USD`, `face_value=100`. The trader has zero control over conventions. |
| Units in field names / descriptions | ⚠️ | Mixed. Examples: `coupon_rate` (described as "percentage, e.g., 5.0 for 5%" — should be `coupon_rate_pct`), `clean_price` (no unit suffix; ambiguous between dollars and per-100), `tenor` (described as "years"; should be `tenor_years`), `rates` (described as "percentages"). The output side does carry suffixes: `ytm_pct`, `zero_rate_pct`, `z_spread_bps`. ⚠️ |

The hard-coded conventions are by far the worst symptom — any non-USD, non-30/360, non-semi-annual bond is unrepresentable through `create_bond`.

---

## 1.6 Structured outputs and provenance

| Item | Status | Detail |
|---|---|---|
| Numeric values carry units | ⚠️ | Output JSON keys mostly do (`ytm_pct`, `z_spread_bps`, `zero_rate_pct`); inputs mostly don't. |
| Currency in outputs | ❌ | Never returned. |
| Day count / frequency in outputs | ❌ | Never returned. |
| Curve provenance on pricing outputs | ❌ | `calculate_yield` does not reference any curve. `calculate_z_spread` echoes `curve_id` but not the curve's reference date, day count, interpolation, or build method. |
| Cash flows | ❌ | No tool returns cash flow schedules. The underlying `Bond` trait has them, but nothing exposes them. |
| Errors typed | ❌ | All errors are `McpError::invalid_params(String, None)` or `internal_error(String, None)`. No structured error variants (e.g. `BondNotFound`, `InvalidDate`, `ConvergenceFailure`). |

---

## 1.7 Determinism and verification

| Item | Status | Detail |
|---|---|---|
| Tool calls deterministic given same inputs | ⚠️ | The math is deterministic, but tool calls are **state-dependent** (must `create_bond` before `calculate_yield`). Same inputs to `calculate_yield` produce the same output only if the bond registry is identical. |
| Reference verification harness | ⚠️ | `tools/reconcile_bench` exists at workspace level and a static `tests/fixtures/quantlib_reference_tests.json` exists. The reconciliation framework is scaffolded (`reconciliation/README.md`, milestone 1 done) but milestones 2–4 (run, triage, CI) are not done. None of this is wired into MCP — there is no test that says "MCP `calculate_z_spread` matches QL within tolerance." |
| Tests in CI | ❌ | `convex-mcp` has **no `tests/` directory and no `benches/` directory**. The only tests in the crate are 3 unit tests in `demo.rs` covering demo data construction. Tool handlers themselves are entirely untested. |

---

## 1.8 Documentation

| Item | Status | Detail |
|---|---|---|
| README — how to start the server | ✅ | `crates/convex-mcp/README.md` covers stdio + HTTP launch, demo mode, and a tool table. |
| Client integration examples | ✅ | `crates/convex-mcp/docs/integration-guide.md` covers Claude Desktop, Claude Code, Cursor, Cline, Continue.dev, Zed, with config snippets. |
| Example client invocations (curl, Python) | ❌ | No raw-protocol or HTTP client examples for the streamable-HTTP transport. |
| Tool descriptions LLM-friendly | ⚠️ | Descriptions are short and clear but several are jargon-heavy without context (e.g. "Calculate Z-spread for a bond" — what is Z-spread? what curve does it use?). The `instructions` field on `ServerInfo` is generic. |
| Per-tool example input/output JSON | ❌ | Not in the README or anywhere. |
| Doc comments on parameter fields | ✅ | Every `JsonSchema`-derived field has a `///` doc comment that becomes the schema description. |

---

## Summary table

| Section | Status | One-line takeaway |
|---|---|---|
| 1.1 Crate inventory | ⚠️ | Hex boundary intact dependency-wise, but MCP bypasses `convex-traits` / `convex-engine` and reaches into analytics directly. |
| 1.2 Server impl | ⚠️ | rmcp + stdio + HTTP all good; mutable bond/curve registries are an anti-pattern for an agent-native server. |
| 1.3 Tool surface | ❌ | 4 of 6 target tools missing. Rich underlying engines (OAS, KRD, bootstrap, FRN DM, scenario, term-sheet, P&L attribution) entirely unexposed. |
| 1.4 `Mark` contract | ❌ | **No `Mark` enum. Trader cannot mark by spread or yield. Highest-priority gap.** |
| 1.5 Schema derivation | ⚠️ | Inputs `schemars`-derived; outputs are stringly JSON blobs; conventions hard-coded; some inputs lack unit suffixes. |
| 1.6 Outputs & provenance | ❌ | No curve provenance, no cash flows, no currency/day-count, no typed errors. |
| 1.7 Determinism / verification | ❌ | No MCP-level tests, no QL reconciliation hook, hidden state breaks pure-function semantics. |
| 1.8 Documentation | ⚠️ | Setup docs solid; per-tool examples missing; tool descriptions could be more LLM-friendly. |
