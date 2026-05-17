# PnL Narrator — Claude Desktop Smoke Test

End-to-end check that `attribute_pnl` + `narrate_attribution` work when an
LLM (not just `cargo test`) drives them. Run this whenever the PnL surface
changes; the goal is to catch *agent-experience* regressions unit tests can't.

Offline equivalent: `cargo test -p convex-mcp --lib pnl_narrator_e2e`.

## Setup

Same server as the hedge advisor — if that demo is already registered, skip
to Test 1.

1. **Build** (one-time per code change):
   ```
   cargo build -p convex-mcp --release
   ```
   Binary: `target/release/convex-mcp-server.exe`.
2. **Register** in `%APPDATA%\Claude\claude_desktop_config.json`:
   ```json
   "mcpServers": {
     "convex": {
       "command": "C:\\Users\\sujit\\source\\convex\\target\\release\\convex-mcp-server.exe"
     }
   }
   ```
3. **Restart Claude Desktop** (full quit + relaunch — config is read at startup).
4. **Verify** the 🔌 / "convex" tool list shows in a fresh chat.

Curve data for the canonical scenario is in
`demo/data/eur-govt-curve-2026-05-07.json` and `…-05-08.json` (illustrative,
not live).

## Test 1 — OAT/BTP/Bund + the swap (canonical, closed-loop)

**Prompt** (paste verbatim into a fresh chat):

> EUR sovereign book, attribute yesterday→today PnL (t0 = 2026-05-07,
> t1 = 2026-05-08). Positions:
> - long €10mm OAT 2.75% 2034-05-25, marked +12 bp t0 / +14 bp t1 vs Bund;
> - long €5mm BTP 4.0% 2035-02-01, marked +135 bp / +141 bp vs Bund;
> - long €10mm Bund 2.5% 2034-08-15, flat to its own curve;
> - pay-fixed €10mm 10Y EUR swap, 2.65% fixed annual 30E/360, traded
>   2026-05-01, maturing 2036-05-01 — the hedge from last week.
>
> EUR govt zero curve t0: 6M 2.40, 1Y 2.50, 2Y 2.60, 5Y 2.80, 10Y 3.00,
> 30Y 3.30. t1 (bear steepener): 6M 2.44, 1Y 2.55, 2Y 2.66, 5Y 2.88,
> 10Y 3.10, 30Y 3.42.
>
> Attribute the book PnL, then narrate it.

**What the agent should do** (two calls):

1. **`attribute_pnl`** with `base_currency = "EUR"`, `t0`/`t1` ISO strings,
   inline `curve_t0`/`curve_t1` (`CurveSpec` from the numbers above), and
   `positions`: three `kind:"bond"` (inline `BondSpec`, signed
   `notional_face`, textual `mark_t0`/`mark_t1` e.g. `"+12bps@FR.OAT-DE.BUND"`,
   Bund e.g. `"+1bps@DE.BUND"` both dates) + one `kind:"swap"`
   (`InterestRateSwapPnlSpec`, `side:"pay_fixed"`).
2. **`narrate_attribution`** on the returned `Attribution`.

## Verification checklist

### `attribute_pnl`
- [ ] Output parses cleanly into `Attribution`; `positions.len() == 4`.
- [ ] `provenance.factor_model == "level_slope_curv_v1"`; `curve_t0_id` /
      `curve_t1_id` present (`"<inline_t0>"` / `"<inline_t1>"` for inline specs).
- [ ] Σ `factors[*].pnl_ccy` ≈ `total_pnl_ccy` (identity closes, < 1 ccy).
- [ ] OAT / BTP / Bund `total_pnl_ccy` all **negative** (long, rates up).
- [ ] Swap `total_pnl_ccy` **positive**; `total_pnl_ccy` (book) is greater
      than the sum of the three bond totals (the swap offsets).
- [ ] Swap's `spread` factor ≈ 0 (no credit); Bund's spread factor ≈ 0
      (`s0 == s1`).
- [ ] Largest book factor is `curve_parallel`; `curve_slope` is non-trivial
      and signed with the steepener; `curve_residual` small.
- [ ] Each position's `curve` block has the same `parallel_bps` /
      `slope_bps` / `pivot_tenor_years` (the move is decomposed once).

### `narrate_attribution`
- [ ] Single deterministic paragraph; starts with `"Book PnL …"`.
- [ ] States the biggest driver and the curve decomposition.
- [ ] One `"<benchmark> contributed …"` clause per non-zero spread benchmark
      (BTP, OAT); **no** `"widened"`/`"tightened"` (direction is not inferred).
- [ ] `"Swap positions contributed … offsetting …% of the bonds' … total
      move."` — and **no** intent claims (`"working as designed"`,
      `"last week"`).
- [ ] Ends with the `(curves … → …; factor model …)` provenance tail.

### Agent experience (qualitative)
- [ ] Agent picks `attribute_pnl` then `narrate_attribution` without heavy
      prompting; passes ISO date strings and textual marks without errors.
- [ ] It can explain the result in its own words from the structured output
      (e.g. "the swap clawed back ~X% of the rate loss").

## Test 2 — Price / yield marks (residual is not machine-zero)

Same book, but mark the OAT with a clean price (`"99.10C"`) at t0 and a
yield (`"3.05%@A"`) at t1. **Expected:** still parses and attributes; the
OAT's `residual` factor is no longer ~0 but small (the held-spread fall-back
solve carries Brent-tolerance error, by design — documented in
`risk::pnl::engine`). Total still closes.

## Test 3 — Callable position is rejected

Add a position whose `BondSpec` carries `make_whole_spread_bps`. **Expected:**
`attribute_pnl` returns `invalid_input` — *"requires a fixed-coupon
(non-callable) bond"*. v1 does not silently price a callable as its bullet
(the OAS path is the hedge advisor's `compute_position_risk`).

## Test 4 — The closed loop

Run the **hedge advisor** demo first (it recommends the pay-fixed €10mm 10Y
EUR swap), then run Test 1 a "day later" with that swap in the book. The
narrator should show the swap offsetting the curve loss — last week's
recommendation surfacing in this week's PnL. This is the demo's point;
verify the swap contribution and offset percentage read sensibly.

## Triage notes

| Symptom | Likely cause |
| --- | --- |
| "convex" not in tools | Config path wrong, or Claude Desktop not restarted. |
| `mark` parse error | Unsupported form. Grammar: `crates/convex-core/src/types/mark.rs::FromStr` (e.g. `+12bps@FR.OAT-DE.BUND`, `99.1C`, `3.05%@A`). |
| `t0`/`t1` parse error | Non-ISO date. Use `YYYY-MM-DD`. |
| "requires a fixed-coupon (non-callable) bond" | Position resolved to a Callable (a `make_whole_spread_bps` was set) — expected (Test 3). |
| "need ≥3 distinct analysis tenors" / "pivot … must lie strictly inside" | Curve has < 3 distinct pillars, or `config.pivot_tenor_years` is on/outside the pillar span. |
| Large `residual` factor | A price/yield mark (Test 2) — Brent-tolerance held-spread gap, expected; or a genuinely non-decomposable curve move (check `fit_residual_l1_bps`). |
| Swap PnL has the wrong sign | `side` mis-set; pay-fixed must gain when rates rise. |

## When the test passes

Note any agent-experience friction in the PR (suboptimal tool descriptions,
confusion points) and consider small tool-description tweaks if they would
obviously help the LLM.
