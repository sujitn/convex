# CDS recovery scenarios and a five-factor mixed hedge book

The existing `graph_hedge_risk`, `cash_basis_hedge_risk`, calibration scenario and
`hedge` operations now compose full CDS recovery risk with issuer cash, IRS and
rates-futures risk. No additional wire operation or financial pricer is introduced.

A CDS scenario resolves the exact survival-curve key and its explicit `Credit`
recovery assumption. `CdsContract::with_recovery_fraction` clones the complete
checked contract and changes that assumption only; the original premium periods,
calendar revisions, legal entity/seniority/document/restructuring identities,
notional, protection side, coupon, dates and initial-cash state survive. The same
original CDS pricer values the clone. The baseline is admitted and priced first;
a mismatched baseline recovery or legal reference cannot be repaired by a scenario.
A hypothetical assumption is distinct from a trade amendment or realized recovery.

`FrozenCurves` holds the actual survival probabilities and changes recovery in
[0,1]. `RebuildDependents` also supports supplied survival nodes: it changes the
credit role before rebuilding every dependent original helper. For a fitted CDS
curve, the full recipe role and every actual CDS helper receive the same changed
recovery before the original fit. Every dependent par-recovery cash recipe uses
that actual scenario recovery. Original native quote residuals and helper-domain
checks remain. Recovery one is valid with held survival; fitting a CDS survival
curve at full recovery remains rejected because the original fitted model cannot
identify it. Wrong entity, role, seniority, curve version, baseline/helper/model
recovery or calendar pins fail through the original owners.

The runnable consumer builds complete original cash-bond, buyer CDS, stepped IRS
and term-rates-futures columns for five common coordinates:

| Coordinate | Signed output unit |
| --- | --- |
| Collateral discount zero | Currency per basis point |
| Issuer hazard | Currency per annual hazard-rate fraction |
| Issuer cash basis | Currency per basis point |
| Term projection zero | Currency per basis point |
| Issuer recovery | Currency per recovery fraction |

Every column evaluates baseline, +h, -h, +h/2 and -h/2 through the complete original
product owners; even zero exposures are actually repriced. Curve keys, factor
identities, policies, snapshots and positive native reference quantities remain
explicit. Full coupon/principal profiles, known fixing revisions, actual calendar
holidays, CDS premium/default accrual and pending upfront cash enter the controls.
Independent Decimal60 controls compare complete CDS scenario diagnostics as well
as sensitivities; native output captures supply transport expectations only.

The book preserves separate current and pending quantities, native trade/holding/
liquidity intervals, exact contract lots, side costs and supplied same-snapshot
USD/EUR conversion. It requests four candidate trades against all five factors.
The independent synthetic target corresponds to cash -3,000 original face, CDS
+80,000 notional, IRS -200,000 notional and +2 futures contracts. The expected
cost is USD 7.84 or EUR 7.056. The solver retains proposal-only authority, complete
final constraint checks and bounded-work failure; it does not submit orders.

The solver reuses its original complete-pivot LU decomposition for at most three
residual corrections. Every correction is checked against the same original KKT
system and unchanged backward-error bound. Unresolved decomposition, nonfinite
values or remaining error still return `numerical_failure`; the original final
native quantity, factor and cost checks remain mandatory. This resolves roundoff
from independent hazard/recovery constraint rows without a pseudoinverse, ridge
or tolerance change. Original targets, factor scales/bands, solver defaults and
active-set/lot work counters remain. The extra correction work is bounded by
three small solves per active set. Raw failures and the unsuccessful separate
application-limit/rank-policy trials remain in private evidence. This bounded book
does not qualify every financial constraint scale or certify solver latency.

```bash
cargo run --locked -p convex --example recovery_hedge
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test recovery_hedge -- --test-threads=1
cargo test --locked --all-features -p convex --test recovery_graph -p convex-credit --test recovery_binding -- --test-threads=1
python3 tests/scripts/recovery_hedge_reference.py
python3 tests/scripts/recovery_hedge_admission.py
python3 tests/scripts/recovery_hedge_boundaries.py
```

Existing [graph columns](GRAPH-HEDGE-RISK.md), [issuer cash columns](CASH-BASIS-HEDGE-RISK.md)
and [constrained proposals](CONSTRAINED-HEDGES.md) retain their complete contracts.
The shared Rust/JSON/C/MCP/native-compiled WASM paths reuse the same implementation.
Java/JNI and Windows/.NET/Excel parity tests are wired into CI; actual execution
on those hosts and wasm32/browser remains unverified on the current macOS host.
CVX-063 remains partial and uncertified. Optional-model and all-member delivery
futures risk, the complete market/convention matrix and independent production/
latency qualification remain owning work. CVX-025/031 own authoritative server
limits, pending-state fencing and orders. Detailed local evidence and the next
coding-harness prompt are in `.ai/production-readiness/evidence/CVX-063-Recovery-Hedges`.
