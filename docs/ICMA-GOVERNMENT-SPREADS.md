# ICMA government spread (G-spread)

`cash_quotation::icma_government_spread` and `price_icma_government_spread` compute
the G-spread of a government bond using ICMA quotation conventions. They build on
`workflow::PreparedIcmaQuotation`, the existing government-security validation and
the choice of benchmark by exact or linear interpolation on contractual maturity.
The shared operations `icma_government_spread` and `icma_government_price` are
reached through the generic JSON, C, MCP, WASM, Java and Excel calls. Pricing does
not depend on a provider, database, message queue or clock. The context-free
government routes remain available.

The target and every benchmark supply their full terms, settlement and entitlement,
a nominal reference mesh with its own version, the normal coupon frequency, the
quotation policy and calendar pins. Observed sources keep the full
`CashBondHelper`, the quote meaning, unit and version, and a separate security
reference. `convert_cash_quote` converts observations; an unsupported input yield
cannot become a guessed price.

Common settings require an annual-decimal policy and compounding on every source.
Coupon frequencies may differ: each source is inverted on its own cash and
reference denominators under that same output representation. Missing dates and
contexts are never inferred, and neither a shared target mesh nor a zero-curve
proxy can stand in for them.

All source conversion, calendar, context and source checks, and every payment time,
are prepared before the government quotation inverses, including for unselected
securities. The existing guards for plain fixed coupons, optional and floating
bonds, settlement, informative cash and delayed pillars still apply. Same-day
ownership, ex-coupon rebate, completed unpaid accrued, economic steps, separate
principal allocations and delayed cash are handled by the same code as before.

Each `IcmaSecurityContext` carries a signed Notional position with its currency
and a face reference quantity. Output quote pins use the helper identity and
version pair; the reverse target keeps its own identity and a separate spread
quote version. No quantity or revision is generated. Rust callers bind the
calendar data and versions; the shared dispatcher resolves exact pins against the
calendars you pass in before pricing.

Government membership, unique securities and maturities, common currency,
settlement and entitlement are checked. There are 1..128 benchmarks; exact
selection requires one. Linear selection accepts only the closed interval of
contractual maturities. Order, indices, maturity dates and calendar-day weights
are kept, and delayed payment dates never replace maturity.

Each source keeps its [ICMA work reservation](ICMA-QUOTATIONS.md) and its own
ceiling. A common `max_reference_evaluations` caps their sum, including the
target, at a positive maximum of 16777216. The product and inverse bounds still
apply. These are not measured CPU or latency figures.

Forward output keeps, for every source, the observed conversion, the contextual
inverse, the terms and versions, the cash and times, and the signed invoice. The
spread is the target annual yield minus the selected benchmark annual yield; basis
points are that value times 10000 without rounding. Invoices keep the **observed
dirty consideration**, independently of the approximate inverse repricing.

The reverse target supplies a full `GovernmentSpreadTarget` and context, without an
invented observed price. `GovernmentSpreadQuote` keeps its version and its
DecimalRate or BasisPoints unit. The selected benchmark yield plus the requested
spread prices all of the target's cash. That cash is then inverted against the
computed dirty price within the same closed domain and work limits. Both the price
residual and the annual-decimal spread residual, which you supply in (0,1e-6],
must pass. The computed dirty consideration survives the roundtrip. No root, a
nonunique root, mixed signs, stale, missing or incomplete inputs, unsupported
cases and exhausted work all return an error with no partial result.

```sh
python3 tests/scripts/icma_government_reference.py
python3 tests/scripts/icma_government_qualification.py
python3 tests/scripts/icma_government_api_corpus.py
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test icma_government -- --test-threads=1
cargo run --locked -p convex --example icma_government
cargo run --locked -p convex --example icma_government --no-default-features
cargo run --locked -p convex --example icma_government --features schemars
python3 tests/scripts/icma_government_schema.py --binary target/debug/examples/icma_government
cargo build --locked -p convex-ffi --lib
python3 tests/scripts/icma_government_parity.py --binary target/debug/examples/icma_government --library target/debug/libconvex_ffi.so
```

The independent Decimal60 controls were written before the code was run. The raw
requests and expected errors are preserved separately. Qualification cases
distinguish consistently rebound revisions from stale pins, and an excluded quote
from a bracket that does not identify a root. The Rust envelopes show agreement
across transports only; the financial answers come from the independent controls.

[CFA Institute's yield discussion](https://www.cfainstitute.org/insights/professional-learning/refresher-readings/2026/yield-and-yield-spread-measures-for-fixed-rate-bonds)
supports explicit compounding and separate benchmark yields/spreads.
[ICMA Appendix A5](https://www.icmagroup.org/assets/ICMA-PMH-Circular-2022-01-FINAL.pdf)
and [QuantLib's schedule-based ActualActual](https://github.com/lballabio/QuantLib/blob/master/ql/time/daycounters/actualactual.cpp)
support the existing time code. The composition and the maturity selection are API
choices. No new market comparison has been run, and no Street convention is
assumed. Execution on Java/JNI, Windows/.NET/Excel and wasm32/browser has not been
tested.
