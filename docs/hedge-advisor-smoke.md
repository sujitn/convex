# Hedge Advisor — Claude Desktop Smoke Test

End-to-end check that the four advisor MCP tools work correctly when an
LLM (not just `cargo test`) drives them. Run this whenever the advisor
surface changes; the goal is to catch *agent-experience* regressions
that unit tests can't.

## Setup

1. **Build the server** (one-time per code change):
   ```
   cargo build -p convex-mcp --release
   ```
   Binary: `target/release/convex-mcp-server.exe`.

2. **Configure Claude Desktop**. Edit
   `%APPDATA%\Claude\claude_desktop_config.json` to add:
   ```json
   "mcpServers": {
     "convex": {
       "command": "C:\\Users\\sujit\\source\\convex\\target\\release\\convex-mcp-server.exe"
     }
   }
   ```

3. **Restart Claude Desktop** (full quit + relaunch — config is read at startup).

4. **Verify the server is connected**: in a new chat, look for the 🔌
   icon / "convex" listed under available tools.

## Test 1 — Apple 10Y demo (canonical scenario)

**Prompt** (paste verbatim into a fresh chat):

> I'm long $10mm of an Apple-like corporate bond: 4.85% coupon, matures
> 2034-05-10, semi-annual 30/360 US. Today's settlement is 2026-01-15.
> I'm marked at +85 bps over a flat 4.5% USD SOFR curve. Walk me through
> hedging this position with the convex tools.

**What the agent should do** (tool-by-tool):

1. **`create_bond`** with id `AAPL.10Y` (or similar) and the spec from
   the prompt.
2. **`create_curve`** with id `usd_sofr` (flat 4.5% across standard tenors).
3. **`compute_position_risk`** with `mark = "+85bps@USD.SOFR"` (or
   equivalent textual mark) and `notional_face = 10_000_000`.
4. **`propose_hedges`** against the resulting `RiskProfile`.
5. **`compare_hedges`** against the two proposals.
6. **`narrate_recommendation`** on the comparison.

## Verification checklist

For each tool call, confirm:

### `compute_position_risk`
- [ ] Output JSON parses cleanly into `RiskProfile`.
- [ ] `dv01` is positive (long bond).
- [ ] `key_rate_buckets` has 4 entries (default tenors `[2, 5, 10, 30]`)
      sorted ascending.
- [ ] `provenance.curves_used` includes `"usd_sofr"`.
- [ ] `provenance.cost_model == "heuristic_v1"`.
- [ ] `modified_duration_years` is in the 6.5–7.5 range (10Y bullet at 5%).

### `propose_hedges`
- [ ] Returns exactly two proposals: `DurationFutures` and `InterestRateSwap`.
- [ ] Each proposal has `residual.residual_dv01.abs() / position.dv01 < 0.001`
      (DV01-neutral within 0.1%).
- [ ] `DurationFutures` proposal references contract `"TY"` (10-year UST future).
- [ ] `InterestRateSwap` proposal has `side == "pay_fixed"` (long bond → pay fixed).
- [ ] Both proposals carry `provenance.cost_model == "heuristic_v1"`.

### `compare_hedges`
- [ ] `rows.len() == 2`.
- [ ] `recommendation.strategy == "DurationFutures"` (lower cost, ~0.25 bp
      vs ~0.6 bp for the swap).
- [ ] `recommendation.reasons` includes `"lowest_cost"`.

### `narrate_recommendation`
- [ ] Single paragraph, deterministic.
- [ ] Mentions both `DurationFutures` and `InterestRateSwap`.
- [ ] Ends with `"Recommend DurationFutures (...)"`.
- [ ] Costs and residual DV01s are stated in dollars / bps.

### Agent experience (qualitative)
- [ ] The agent picks the right tools without significant prompting.
- [ ] String-typed `settlement` (`"2026-01-15"`) and `mark`
      (`"+85bps@USD.SOFR"` etc.) round-trip without errors.
- [ ] The agent can explain the recommendation in its own words after
      receiving the narration.
- [ ] No tool call returns an `invalid_input` error on a reasonable input.

## Test 2 — Constraint that filters strategies

Same setup, but ask:

> Same bond, but I only want to consider the bond-future hedge — show
> me only that proposal.

**Expected behaviour**: agent calls `compare_hedges` with
`constraints.allowed_strategies = ["DurationFutures"]`. Recommendation
is `DurationFutures`; if the agent passes `["CashBondPair"]` (typo or
hallucination), the tool errors with
`"no proposed strategy matches allowed_strategies"`.

## Test 3 — Yield mark instead of spread mark

Same setup, but:

> I'm marked at 5.35% YTM (semi-annual). Hedge it.

**Expected behaviour**: `mark = "5.35%@SA"` parses; `compute_position_risk`
returns a profile whose `dv01` is within 1% of the spread-mark version
(same bond, equivalent yield).

## Triage notes

| Symptom | Likely cause |
| --- | --- |
| "convex" not listed in tools | Config path wrong, or Claude Desktop not restarted. |
| "tool call failed: spawn ENOENT" | Binary path wrong in config. |
| `mark` parse error | Agent passed an unsupported form. Check `crates/convex-core/src/types/mark.rs::FromStr` for the grammar. |
| `settlement` parse error | Agent passed non-ISO format. Should be `YYYY-MM-DD`. |
| `compute_position_risk requires Fixed/Callable` | Agent created a Floating or Zero bond. |
| Compare/narrate succeed but recommendation is wrong | Cost model or recommendation rule has drifted. Check `compare.rs::recommend`. |

## When the test passes

Document any agent-experience friction in the PR description (suboptimal
tool descriptions, unexpected confusion points, etc.) and consider
small tool-description tweaks if they would obviously help the LLM.
