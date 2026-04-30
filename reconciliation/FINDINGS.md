# Reconciliation Findings

Convex ↔ QuantLib 1.40 fixed-income book across 2025-12-31 and 2025-06-30
snapshots. Bit-for-bit match within `reconcile.py` tolerances.

| Milestone | Result | Scope |
|---|---|---|
| M2–M5 | 113 / 113 | Vanilla bullets across UST/Gilt/Bund/JGB; ICMA, EOM, calendar fixes |
| post-M5 | 121 / 121 | + corporate SOFR FRN on SOFR OIS curve |
| A1–A4 | 174 / 174 | + zero, sinker, make-whole, Bermudan puttable |
| A polish | 172 / 172 | MW wired through FFI/MCP/Excel; bond day-count for MW; Optional CallSchedule |
| B1 | 178 / 178 | + callable SOFR FRN, workout-bullet DM-to-first-call |
| B1 polish | 188 / 188 | Mid-month re-anchor; ARRC × workout-bullet; per-call DM + DM-to-worst |

Commit history holds the per-milestone bug detail; the running snapshot below
is the only piece worth keeping in-tree.

## Current snapshot

`SYNTH_CALLABLE_SOFR_FRN` exercises ARRC compound-in-arrears × workout-bullet
truncation on both snapshots: re-anchored to 2024-11-15 / 2029-11-15 with
annual NC2 calls so settlement always lands mid-period. Per-call DM rows
(`dm_to_call_<yyyymmdd>_bps`) and `dm_to_worst_*` mirror the puttable's YTW
pattern. `DiscountMarginCalculator` carries an optional in-progress coupon
override; the bench supplies a `compound_in_arrears` closure and reuses the
library's workout-bullet PV instead of duplicating it. 16 of the 188 rows
are HW1F trinomial OAS or σ-calibration metrics on the two callable
fixed-rate bonds; σ is calibrated independently on each side against the
same ATM SOFR co-terminal strip with `a = 0.03` fixed.

See `NEXT_STEPS.md` for open items.
