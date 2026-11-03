# ICMA workout quotations

This document covers yield and price quotations across all call, put and
maturity paths of a bond (workouts) under the ICMA convention. The code is
`workflow::quote_icma_workout_prices` and `quote_icma_workout_worst`. They
combine the legal-path enumeration with the same validated
`PreparedIcmaQuotation` used by [single-path quotations](ICMA-QUOTATIONS.md).

The shared `icma_workout` operation carries:

- the security and independently versioned calendars;
- the nominal reference mesh and the quotation policy;
- a signed face position and the yield convention;
- the purchase and notice context.

It reaches the generic Rust, JSON, C, Java, Excel, MCP and WASM dispatchers.
Java/JNI, Windows/.NET/Excel and wasm32/browser execution have not been run.

| `action.kind` | Inputs | Result |
|---|---|---|
| `price_paths` | Annual decimal `decimal_yield`, `IcmaWorkoutSettings` | Full price, cash, time, sources and invoice for every eligible maturity/call/put path |
| `worst` | Clean/dirty per100 `target`, `IcmaWorkoutSolveSettings` | Full same-pricer yield for every path, plus the defined worst-call `selected_index` |

For one chosen path, use `icma_quotation` with a workout cash selection. For
context-free dated yields, use `workout`. These operations take different
explicit time inputs. There is no default convention or alias that converts
between them.

## Paths and validation

Purchase settlement must reproduce the contractual lag, adjustment and payment
calendar. Information date, trade date, settlement and notice facts stay
separate.

Enumeration uses the European dates, the Bermudan dates and every future
calendar date in an American window. Protection and earliest-new-notice
exclusions keep their meaning. Output is ordered by maturity first, then by
provision order, then by date. The examined-date count includes excluded
protection and notice dates.

A timely notice forces its one path, including a put you supply. Otherwise the
minimum is selected among maturity and issuer calls. Every put is still fully
prepared and inverted, and a lower put yield does not reduce the worst-call
measure. Exact numerical ties keep the first candidate in the original order.
No tolerance-based tie or alternate ordering is invented.

Every eligible path is built, and its ICMA time coverage validated, before any
price or inverse is computed. These return an error with no partial result:

- a missing reference;
- unsupported partial ownership;
- an invalid forced notice;
- a failed unselected put;
- an exhausted request bound.

Each inverse uses the closed domain, the identification and mixed-sign guards,
a bounded solver and a final price residual check. A bad inverse never drops
out of the selection.

Cash construction stays in the bond contract:

- Scheduled principal reductions on the same day come before early retirement.
- Completed unpaid coupons and principal keep their own payment dates, even
  after early redemption.
- Partial coupons keep their steps, nominal denominators and rounding.
- Redemption that includes accrued interest does not invent a partial coupon.
- Purchase accrued interest and the ex-coupon rebate stay separate from accrued
  interest at exercise.
- Every result keeps both the unexercised cash and the full path used.

The nominal mesh you pass in must cover every economic coupon reference and
every payment or delayed exercise settlement on every path. It is not shortened
to the selected call. Quotation compounding is independent of the normal
reference frequency and the coupon day count. No periods, calendar versions,
histories or quotation policy are inferred.

## Units and bounded work

Yield is an annual decimal. Clean, dirty and accrued are per100 of face. The
signed invoice consideration uses the face quantity and the exact currency.

Price paths use the computed dirty price. Every inverse targets the observed
dirty consideration, or the observed clean price plus the purchase accrued
interest, and not the inverse's approximate repricing. No payment, ledger or
order authority is inferred.

`IcmaWorkoutSettings` has no defaults. You supply:

- `max_dates` in `1..=16384`;
- `max_reference_evaluations` in `1..=16777216`;
- `max_cash_evaluations` in `1..=67108864`.

The per-path reference-work controls also apply.

The total reference reservation is the sum of each path's one-time ICMA
preparation reservation. Cash reserves, for each eligible path, the unpaid
source cash count plus at most two exercise components. That is multiplied by
one price evaluation, or by the inverse iteration ceiling plus eight. The
request checks these reservations before valuation, and the actual cash
discounts must fit the reserved total. There is no truncated enumeration, no
relaxed retry and no widened domain.

`IcmaWorkoutSolveSettings` contains `inverse` and `paths`. Price controls
contain only path bounds. Unused inverse or target fields on a price request
are rejected.

All full quotations are kept, so this is bounded batch work. The resource
ceilings and the shared JSON input and output bounds are not allocation,
capacity or colocation latency guarantees.

## Reproduction

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

Decimal60 controls, declared in advance, cover cash, contextual times, accrued
interest, clean/dirty, PV, the yield of every path, the signed invoice, legal
ordering and selection. The original answers and controls are unchanged. The
transport check only compares Rust output across hosts.

Not yet covered: an independently run Street comparator and quantitative
review, and complete market, model and host coverage.

[ICMA Appendix A5](https://www.icmagroup.org/assets/ICMA-PMH-Circular-2022-01-FINAL.pdf)
supports explicit nominal-period interest fractions.
[MSRB Rule G-15](https://www.msrb.org/Rules-and-Interpretations/MSRB-Rules/General/Rule-G-15)
distinguishes issuer call and maturity quotations, put-date transactions and
notice considerations. Those distinctions informed the design. The yields here
are not municipal confirmation compliance, exercise prediction or option value.

Floating conditional cash has its own frozen-projection DM calculation.
Stochastic OAS, make-whole and triggered terms, and full Street conventions are
not covered.
