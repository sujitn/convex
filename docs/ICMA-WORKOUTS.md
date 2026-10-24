# Complete contextual ICMA workout quotations

`workflow::quote_icma_workout_prices` and `quote_icma_workout_worst` compose
the original legal-path enumeration with the same checked
`PreparedIcmaQuotation` used by [single-path quotations](ICMA-QUOTATIONS.md).
The shared `icma_workout` operation carries the complete original security,
independently versioned calendars, nominal reference mesh, quotation policy,
signed original-face position, yield convention and purchase/notice context.
It reaches the existing generic native/JSON/C/Java/Excel/MCP/WASM dispatchers.
Actual Java/JNI, Windows/.NET/Excel and wasm32/browser execution remains a
separate qualification gate.

| `action.kind` | Inputs | Result |
|---|---|---|
| `price_paths` | Annual decimal `decimal_yield`, `IcmaWorkoutSettings` | Full price, cash, time, sources and invoice for every eligible maturity/call/put path |
| `worst` | Original clean/dirty per100 `target`, `IcmaWorkoutSolveSettings` | Full same-pricer yield for every path, plus the defined worst-call `selected_index` |

For one explicitly selected path, retain `icma_quotation` with a workout cash
selection. Context-free dated yields retain `workout`. These operations have
different explicit time inputs; no default convention or compatibility alias
converts between them.

## Original economics and all-path admission

Original purchase settlement must reproduce the contractual lag, adjustment
and actual payment calendar. Information date, trade date, settlement and
notice facts stay separate. Enumeration uses the original European dates,
Bermudan dates and every future calendar date in an American window. Protection
and earliest-new-notice exclusions retain their original meaning. Output order
is maturity first, then original provision order and actual date order. The
examined-date count includes excluded protection and notice dates.

An actual timely notice forces its exact one path, including a supplied put.
Otherwise the minimum is selected among maturity and issuer calls. Every put
is still fully prepared and inverted. A lower put yield does not reduce the
worst-call measure. Exact numerical ties retain the first candidate in the
original order; no tolerance-based tie or alternate ordering is invented.

Every eligible path is constructed and its full ICMA time coverage admitted
before any numerical price or inverse endpoint. A missing reference, unsupported
partial ownership, invalid forced notice, failed unselected put or exhausted
request bound returns an error with no accepted subset. Each inverse uses the
original closed domain, identification and mixed-sign guards, bounded solver
and final native price residual. A bad inverse never disappears from selection.

Cash construction remains in the original bond contract. Same-day scheduled
principal reductions precede early retirement; completed unpaid coupons and
principal retain their independent actual payment dates, even after early
redemption. Partial coupons retain original steps, nominal denominators and
rounding. Included-accrued redemption does not invent a partial coupon. Original
purchase accrued and ex-coupon rebate remain separate from exercise accrued.
Every result retains both original unexercised cash and the complete used path.

The supplied nominal mesh must cover every original economic coupon reference
and every actual consumed payment or delayed exercise settlement on every path.
It is not shortened to the selected call. Quotation compounding stays independent
of normal reference frequency and contractual coupon day count. No periods,
calendar versions, histories or quotation policy are inferred.

## Units and bounded work

Yield is an annual decimal; clean/dirty/accrued are per100 original face.
Signed invoice consideration uses original-face quantity and exact currency.
Price paths use computed dirty price. Every inverse uses the **original observed
dirty consideration**, or original observed clean plus original purchase accrued,
rather than the inverse's approximate repricing. No payment, ledger or order
authority is inferred.

`IcmaWorkoutSettings` has no defaults. Supply `max_dates` in `1..=16384`,
`max_reference_evaluations` in `1..=16777216` and `max_cash_evaluations` in
`1..=67108864`. Original per-path reference-work controls also apply.
The total reference reservation is the sum of each checked path's original
one-time ICMA preparation reservation. Cash reserves each eligible path's
original unpaid source cash count plus at most two exercise components,
multiplied by one price evaluation or the unchanged inverse iteration ceiling
plus eight. The request checks these reservations before numerical valuation;
actual cash discounts must fit the reserved total. No truncated enumeration,
relaxed retry or expanded domain is provided.

`IcmaWorkoutSolveSettings` contains `inverse` and `paths`; price controls contain
only path bounds. Unused inverse or target fields on a price request are rejected.
All full quotations are retained, so this is bounded batch work. Native resource
ceilings and shared JSON ingress/output bounds are not allocation, capacity or
colocation latency certificates.

## Reproduction and qualification

```sh
python3 tests/scripts/icma_workout_reference.py
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test icma_workout -- --test-threads=1
cargo run --locked -p convex --example icma_workout
cargo run --locked -p convex --example icma_workout --no-default-features
cargo run --locked -p convex --example icma_workout --features schemars
python3 tests/scripts/icma_workout_schema.py --binary target/debug/examples/icma_workout
cargo build --locked -p convex-ffi
python3 tests/scripts/icma_workout_parity.py --binary target/debug/examples/icma_workout --library target/debug/libconvex_ffi.so
```

Predeclared Decimal60 controls cover full cash, contextual times, accrued,
clean/dirty, PV, every path's yield, original signed invoice, legal ordering and
selection. Original answers, controls and failed commands remain immutable.
Fresh native envelopes check transport only. No independently executed Street
comparator, human quantitative approval or complete market/model/library/host/
production/latency certificate is implied. CVX-063 remains partial and unaccepted.

[ICMA Appendix A5](https://www.icmagroup.org/assets/ICMA-PMH-Circular-2022-01-FINAL.pdf)
supports explicit nominal-period interest fractions.
[MSRB Rule G-15](https://www.msrb.org/Rules-and-Interpretations/MSRB-Rules/General/Rule-G-15)
distinguishes issuer call/maturity quotations, put-date transactions and supplied
notice considerations. Those distinctions inform the composition; this supplied
contextual yield is not municipal confirmation compliance, exercise prediction
or option value. Floating conditional cash retains its separate frozen-projection
DM owner; stochastic OAS, make-whole/triggered terms and full Street conventions
remain separate owning work.
