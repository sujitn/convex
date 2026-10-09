# Explicit contextual ACT/ACT ICMA quotations

`workflow::PreparedIcmaQuotation` composes complete original fixed, stepped,
amortizing and legally permitted conditional workout cash. Its `quote_price`
and `quote_yield` methods share the original native discount kernel and closed
inverse solver. The strict shared `icma_quotation` operation reaches the generic
native/JSON/C/MCP/WASM/Java/Excel entry points. Actual Java/Windows/Excel and
wasm32/browser execution remains a separate qualification gate.

This is a supplied contextual cash quotation convention. It does not establish
a particular market Street formula, government benchmark selection convention,
option exercise prediction or stochastic OAS. Existing optional-model and
floating DM guards remain. Floating coupons have no contractual fixed-cash YTM.

## Original context and coverage

`IcmaReferenceContext` carries an independent revision, exact security reference,
normal frequency and a complete ordered nominal determination mesh. Each period
has an explicit inclusive start and exclusive end. Native admission checks this
mesh against the original anchored schedule, frequency and EOM rule and every
original coupon reference period, including short and long stubs. Additional
nominal periods beyond maturity must be supplied when actual delayed cash needs
them. Validation does not insert dates or extrapolate missing coverage.

Quotation time to each actual payment is the sum of actual overlapping days
divided by that nominal period's actual day count times the explicit normal
frequency. Coupon accrual and quotation time share core `icma_year_fraction`;
contractual coupon day count remains independently supplied. Cash dates never
replace economic reference dates. Compounding frequency is also independent
of the normal determination frequency. Original native Simple, Annual,
SemiAnnual, Quarterly, Monthly, Daily and Continuous kernels remain available.
Native rate/domain/finite-factor checks still apply.

`IcmaYieldConvention` version1 names the exact reference-context and quotation
policy revisions and retains the explicit frequency, compounding and reference
work ceiling. A different security, revision, policy or frequency fails. The
shared API resolves the original independent payment/reset/observation/exercise
calendar pins against actual calendar data; native embedders must bind those
data and versions before constructing `IcmaQuotationInputs`.

Plain selection supplies settlement and same-day payment ownership. Workout
selection supplies full original purchase/information/notice state and an exact
original maturity or provision/date. Native original notice, protection,
settlement and model guards apply. Completed unpaid coupons/principal keep their
actual dates; scheduled principal reductions precede early retirement. Partial
exercise coupons retain the original profiles and reference denominators.
Original purchase accrued, cash rounding and original face remain independent
of the hypothetical exercise. Ambiguous/unmodeled terms fail.

The full original reference mesh must cover settlement and every actual used
payment, including independently dated principal, redemption settlements and
delayed claims. Empty, unordered, overlapping, conflicting or incomplete context
fails before pricing or inverse endpoints. Checked prepared state is private
and cannot be deserialized from caller-created cash/times/accrued values.

## Units, inverse and bounded work

Yields are annual decimals; prices are per100 **original face**. Signed positions
require exact currency, Notional units and original-face reference quantity.
Price consideration uses computed dirty price times signed quantity divided
by100. Inverse consideration uses the **original observed dirty target**, or
original clean target plus original purchase accrued, preserving the distinction
from approximate inverse repricing. No ledger rounding, posting or execution
authority is granted.

The inverse retains the original closed domain, parameter and native price
tolerances, max iterations1..256, actual evaluations and final native residual.
No root, mixed-sign cash, non-identification, domain and exhausted-work outcomes
remain explicit; no expanded bracket or alternate convention is tried.

The original 4096-period/profile, schedule span and product bounds remain.
Reference-entry preparation reserves
`(2 × distinct used payment dates + 3) × supplied periods + original coupon
reference entries`, under the caller's positive ceiling at most16777216.
This covers reference validation/comparison/map visits and the coverage/arithmetic
passes for each payment. Primitive anchored schedule generation keeps its own
existing bound. Each payment time is prepared once; inverse evaluations reuse
those exact times. This reservation is not a CPU-operation or latency certificate.

## Reproduce the supplied controls

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

The original predeclared independent Decimal60 controls retain their physical
inputs, answers and bounds. Failed raw option-bearing plain, combined-workout
component and completed-but-unpaid purchase accrual controls remain diagnostics.
Separate predeclared plain, component and entitlement controls admit147requests:
116 full successful quotations and31typedfailures. Original dirty/PV/cash/time/
yield/invoice answers and every numerical bound remain; six explicit accrued/
clean boundary answers account for the original unpaid receivable. Fresh native envelopes validate transport only. CVX-063 stays
partial, unaccepted and uncertified; remaining Street/model/market, benchmark
context integration, full operation matrix and host/release certificates remain
open. This pure composition adds no package, service, provider, storage, MQ,
clock, runtime state or executable authority.

## Sources and convention limits

[ICMA Appendix A5, January2022](https://www.icmagroup.org/assets/ICMA-PMH-Circular-2022-01-FINAL.pdf)
describes splitting interest fractions at normal determination periods.
[QuantLib's original schedule-based ActualActual implementation](https://github.com/lballabio/QuantLib/blob/master/ql/time/daycounters/actualactual.cpp)
also splits across its supplied nominal dates and checks coverage. This review
supports the time kernel, not an independently executed market-price comparator
or a blanket Street certificate. Original market-specific yield treatment and
rounding retain their separate qualification requirements.
