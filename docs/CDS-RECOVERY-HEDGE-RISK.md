# CDS recovery scenarios and a five-factor mixed hedge book

The `graph_hedge_risk`, `cash_basis_hedge_risk`, calibration scenario and `hedge`
operations can now include CDS recovery risk alongside issuer cash, IRS and
rates-futures risk. This adds no new wire operation and no new pricer.

A CDS scenario looks up the exact survival-curve key and its explicit `Credit`
recovery assumption. `CdsContract::with_recovery_fraction` clones the validated
contract and changes only that assumption. The premium periods, calendar
revisions, legal entity, seniority, document and restructuring identities,
notional, protection side, coupon, dates and initial-cash state are kept. The
same CDS pricer values the clone.

The baseline is validated and priced first. A scenario cannot repair a baseline
whose recovery or legal reference does not match. A hypothetical assumption is
different from a trade amendment or a realized recovery.

`FrozenCurves` holds the survival probabilities and changes recovery in [0,1].
`RebuildDependents` also supports survival nodes you pass in. It changes the
credit role before rebuilding every dependent helper.

For a fitted CDS curve, the recipe role and every CDS helper get the same changed
recovery before the fit. Every dependent par-recovery cash recipe uses that
scenario recovery. Residuals in each quote's own unit and the helper-domain
checks still apply.

Recovery of one is valid with held survival. Fitting a CDS survival curve at full
recovery is rejected, because the fitted model cannot identify it. A wrong
entity, role, seniority, curve version, or baseline, helper, model or calendar
pin fails in the code that owns that check.

The runnable example builds cash-bond, buyer CDS, stepped IRS and term
rates-futures columns for five common coordinates:

| Coordinate | Signed output unit |
| --- | --- |
| Collateral discount zero | Currency per basis point |
| Issuer hazard | Currency per annual hazard-rate fraction |
| Issuer cash basis | Currency per basis point |
| Term projection zero | Currency per basis point |
| Issuer recovery | Currency per recovery fraction |

Every column evaluates the baseline, +h, -h, +h/2 and -h/2 through the full
product pricers. Even zero exposures are repriced. Curve keys, factor
identities, policies, snapshots and positive reference quantities in native
units stay explicit. The controls include full coupon and principal profiles,
known fixing revisions, calendar holidays, CDS premium and default accrual, and
pending upfront cash.

Independent Decimal60 controls compare the full CDS scenario diagnostics as well
as the sensitivities. The captured native outputs check transport only.

The book keeps current and pending quantities separate, with trade, holding and
liquidity intervals, exact contract lots, side costs and a same-snapshot USD/EUR
conversion you pass in. It requests four candidate trades against all five
factors. The independent synthetic target is:

- cash: -3,000 original face;
- CDS: +80,000 notional;
- IRS: -200,000 notional;
- futures: +2 contracts.

The expected cost is USD 7.84 or EUR 7.056. The solver only proposes trades. It
checks all constraints on the final result, fails when its work bound is
exceeded, and does not submit orders.

The solver reuses its complete-pivot LU decomposition for at most three residual
corrections. Each correction is checked against the same KKT system and the same
backward-error bound. An unresolved decomposition, non-finite values or remaining
error still return `numerical_failure`. The final checks on quantity, factor and
cost still apply.

This resolves roundoff from independent hazard and recovery constraint rows
without a pseudoinverse, ridge or tolerance change. Targets, factor scales and
bands, solver defaults and the active-set and lot work counters are unchanged.
The extra correction work is bounded by three small solves per active set. This
bounded book does not qualify every constraint scale and does not measure solver
latency.

```bash
cargo run --locked -p convex --example recovery_hedge
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test recovery_hedge -- --test-threads=1
cargo test --locked --all-features -p convex --test recovery_graph -p convex-credit --test recovery_binding -- --test-threads=1
python3 tests/scripts/recovery_hedge_reference.py
python3 tests/scripts/recovery_hedge_admission.py
python3 tests/scripts/recovery_hedge_boundaries.py
```

The [graph columns](GRAPH-HEDGE-RISK.md), [issuer cash columns](CASH-BASIS-HEDGE-RISK.md)
and [constrained proposals](CONSTRAINED-HEDGES.md) are documented separately.
The Rust, JSON, C, MCP and WASM paths share the same implementation.

Parity tests for Java/JNI and Windows/.NET/Excel are wired into CI. They have
not been run on those hosts, nor on wasm32/browser, from the current macOS
development host.

Not yet covered: optional-model and all-member delivery futures risk, the full
market and convention matrix, server limits, pending-state fencing and orders,
and independent latency measurement.
