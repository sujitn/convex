# ACT/ACT ICMA quotations

`workflow::PreparedIcmaQuotation` prices and quotes yields on fixed, stepped,
amortizing and legally permitted conditional workout cash flows under the
ACT/ACT ICMA convention. Its `quote_price` and `quote_yield` methods share the
same discount kernel and closed-domain inverse solver. The shared
`icma_quotation` operation reaches the generic Rust, JSON, C, MCP, WASM, Java and
Excel entry points. Java, Windows/Excel and wasm32/browser execution has not
been verified.

You pass in the reference context and the cash. This document does not define a
market Street formula, a government benchmark selection convention, option
exercise prediction or a stochastic OAS. The existing optional-model and
floating discount-margin guards remain. Floating coupons have no fixed-cash YTM.

## Reference context and coverage

`IcmaReferenceContext` carries a revision, the security reference, the normal
frequency and an ordered nominal determination mesh. Each period has an inclusive
start and an exclusive end. Validation checks the mesh against the anchored
schedule, frequency, EOM rule and every coupon reference period, including short
and long stubs. If delayed cash needs nominal periods beyond maturity, you must
supply them. Validation does not insert dates or extrapolate missing coverage.

The quotation time to each payment is the sum of the overlapping days, divided by
that nominal period's day count and multiplied by the explicit normal frequency.
Coupon accrual and quotation time share the core `icma_year_fraction`. The
coupon day count is supplied separately. Cash dates never replace the economic
reference dates.

The compounding frequency is also independent of the normal determination
frequency. The Simple, Annual, SemiAnnual, Quarterly, Monthly, Daily and
Continuous kernels remain available. The rate, domain and finite-factor checks
still apply.

`IcmaYieldConvention` version 1 names the reference-context and quotation-policy
revisions, and keeps the explicit frequency, compounding and reference work
ceiling. A different security, revision, policy or frequency fails. The shared
API resolves the payment, reset, observation and exercise calendar pins against
the calendar data you pass in. Rust embedders must bind that data and its
versions before building `IcmaQuotationInputs`.

Plain selection supplies settlement and same-day payment ownership. Workout
selection supplies the purchase, information and notice state and an exact
original maturity or provision date. The notice, protection, settlement and model guards
apply. Completed unpaid coupons and principal keep their dates, and scheduled
principal reductions come before early retirement.

Partial-exercise coupons keep the original profiles and reference denominators.
Purchase accrued, cash rounding and original face are independent of the
hypothetical exercise. Ambiguous or unmodeled terms fail.

The reference mesh must cover settlement and every payment used, including
independently dated principal, redemption settlements and delayed claims. An
empty, unordered, overlapping, conflicting or incomplete context fails before
pricing or the inverse endpoints. The prepared state is private and cannot be
deserialized from cash, times or accrued values supplied by the caller.

## Units, inverse and bounded work

Yields are annual decimals, and prices are per 100 **original face**. Signed
positions need the exact currency, Notional units and original-face reference
quantity. Price consideration is the computed dirty price times the signed
quantity divided by 100. Inverse consideration uses the **observed dirty target**,
or the clean target plus the purchase accrued. This differs from approximate
inverse repricing. No ledger rounding, posting or execution is involved.

The inverse keeps the closed domain, the parameter and price tolerances, max
iterations 1..256, the number of evaluations and the final residual. No root,
mixed-sign cash, non-identification, domain and exhausted-work outcomes are
reported explicitly. No bracket is expanded and no alternate convention is tried.

The 4,096-period and profile limits, the schedule span and the product bounds
still apply. Reference-entry preparation reserves
`(2 × distinct used payment dates + 3) × supplied periods + original coupon
reference entries`, under a positive ceiling you set, at most 16,777,216. This
covers the reference validation, comparison and map visits and the coverage and
arithmetic passes for each payment. Anchored schedule generation has its own
existing bound. Each payment time is prepared once, and the inverse evaluations
reuse those times. The reservation is not a measure of CPU operations or latency.

## Reproduce the checks

```sh
python3 tests/scripts/icma_reference.py
python3 tests/scripts/icma_qualification.py
python3 tests/scripts/icma_cash_components.py
python3 tests/scripts/icma_entitlement_reference.py
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test icma -- --test-threads=1
cargo run --locked -p convex --example icma_quotation
cargo run --locked -p convex --example icma_quotation --no-default-features
cargo run --locked -p convex --example icma_quotation --features schemars
python3 tests/scripts/icma_schema.py --binary target/debug/examples/icma_quotation
cargo build --locked -p convex-ffi
python3 tests/scripts/icma_parity.py --binary target/debug/examples/icma_quotation --library target/debug/libconvex_ffi.so
```

The Decimal60 controls were declared before the comparison, and they keep their
inputs, answers and bounds. Three raw controls failed and are kept as
diagnostics: option-bearing plain cash, combined-workout components, and the
accrual on a completed but unpaid purchase.

Separate plain, component and entitlement controls cover 147 requests: 116 full
successful quotations and 31 typed failures. The dirty, PV, cash, time, yield and
invoice answers and every numerical bound are unchanged. Six accrued and clean
boundary answers account for the unpaid receivable. Freshly captured native
outputs validate transport only.

Not yet covered: Street, model and market conventions, benchmark selection, the
full helper and risk coverage, the full operation matrix, and host and release
validation. This pure composition adds no package, service, provider, storage,
message queue, clock or runtime state. This work is partial and not certified.

## Sources and convention limits

[ICMA Appendix A5, January 2022](https://www.icmagroup.org/assets/ICMA-PMH-Circular-2022-01-FINAL.pdf)
describes splitting interest fractions at normal determination periods.
[QuantLib's schedule-based ActualActual implementation](https://github.com/lballabio/QuantLib/blob/master/ql/time/daycounters/actualactual.cpp)
also splits across its nominal dates and checks coverage. This review supports
the time kernel. It is not a market-price comparison run independently, and it
does not validate Street conventions. Market-specific yield treatment and
rounding need their own checks.

Related documents:

- [ICMA government spreads](ICMA-GOVERNMENT-SPREADS.md): spread and inverse
  composition for government securities, which uses this same code.
- [ICMA workouts](ICMA-WORKOUTS.md): path price and yield over every legal
  workout, and the worst-call composition. Every eligible maturity, call and put
  context is validated before any numerical valuation, and every path keeps its
  cash, sources, times and signed consideration.
- [ICMA cash helpers](ICMA-CASH-HELPERS.md): observed yield conversion and
  model-implied cash-helper repricing, calibration and dependent quote-risk
  graphs, and recipe and snapshot replay.
- [Direct ICMA yield risk](ICMA-YIELD-RISK.md): own-yield five-PV sensitivity and
  positive reference hedge columns.
