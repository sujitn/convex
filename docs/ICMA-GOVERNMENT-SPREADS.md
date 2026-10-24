# Contextual ICMA actual-security G-spread

`cash_quotation::icma_government_spread` and `price_icma_government_spread`
compose the original checked `workflow::PreparedIcmaQuotation` with the
existing government-security admission and exact/closed-linear actual
contractual maturity selection. Shared `icma_government_spread` and
`icma_government_price` delegate through existing generic JSON/C/MCP/WASM/
Java/Excel calls. Provider, database, MQ, clock and runtime authority remain
outside pricing. The correct context-free government routes remain available.

Every target and actual benchmark supplies its complete original terms,
settlement/entitlement, independently versioned nominal reference mesh,
normal coupon frequency, quotation policy and independent calendar pins.
Observed sources retain the full `CashBondHelper`, original quote meaning/
unit/version and separate security reference. Original `convert_cash_quote`
owns observation conversion; an unsupported input yield cannot become a
guessed price. Common settings require an explicitly supplied annual-decimal
policy and compounding on every source. Coupon frequencies may differ: each
source is independently inverted on its own full cash/reference denominators
under that same output representation. Missing dates and contexts are never
inferred, and a shared target mesh or zero-curve proxy cannot substitute.

All source conversion, calendar/context/source guards and every actual payment
time are prepared before government quotation inverses, including unselected
securities. Original plain fixed, optional/floating, settlement, informative
cash and delayed-pillar guards remain. Same-day ownership, ex-coupon rebate,
completed unpaid accrued, economic steps, separate principal allocations and
delayed cash retain their original native owners.

Each `IcmaSecurityContext` carries a supplied signed Notional position with
exact currency and **original face** reference quantity. Output quote pins
view the original helper identity/version pair; the reverse target retains
the original target identity and independent spread quote version. No quantity
or revision is generated. Native callers bind actual calendar data/versions;
shared dispatch resolves exact pins against supplied calendars before pricing.

Original government membership, unique securities/maturities, common currency,
settlement and entitlement are checked. There are 1..128 benchmarks; exact
selection requires one. Linear selection admits only the closed actual
contractual maturity interval. Original order, indices, actual maturity dates
and calendar-day weights survive. Delayed payment dates never replace maturity.
Each source retains its original [ICMA work reservation](ICMA-QUOTATIONS.md)
and independent ceiling. Common `max_reference_evaluations` caps their sum,
including the target, at a positive maximum of16777216. Original product and
inverse bounds remain; this is not a measured CPU or latency certificate.

Forward output retains full original observed conversion, contextual inverse,
source terms/versions, cash/times and signed invoice for every source. Spread
is target minus selected benchmark annual yield; basis points are that value
times10000 without rounding. Invoices retain **original observed dirty
consideration**, independently of approximate inverse repricing.

The reverse target supplies full `GovernmentSpreadTarget` and context without
an invented observed price. `GovernmentSpreadQuote` retains original version
and DecimalRate/BasisPoints units. The selected benchmark yield plus requested
spread prices all original target cash. The same prepared cash is inverted
against computed dirty price within the unchanged closed domain and work
controls. Both the native price residual and separately supplied annual-decimal
spread residual in (0,1e-6] must pass. Original computed dirty consideration
survives the roundtrip. No-root, nonunique, mixed-sign, stale/missing/incomplete,
unsupported or exhausted-work outcomes return no accepted subset or instruction.

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

Independent Decimal60 full controls precede candidate execution. Raw requests
and error expectations remain separately preserved; explicit qualification
distinguishes consistently rebound revisions from stale pins and an excluded
quote from a genuinely non-identifying bracket. Native envelopes prove
transport agreement only; independent financial answers remain separate.

[CFA Institute's yield discussion](https://www.cfainstitute.org/insights/professional-learning/refresher-readings/2026/yield-and-yield-spread-measures-for-fixed-rate-bonds)
supports explicit compounding and separate benchmark yields/spreads.
[ICMA Appendix A5](https://www.icmagroup.org/assets/ICMA-PMH-Circular-2022-01-FINAL.pdf)
and [QuantLib's schedule-based ActualActual](https://github.com/lballabio/QuantLib/blob/master/ql/time/daycounters/actualactual.cpp)
support the existing time owner. This supplied composition and maturity
selection are API choices. No independently executed new market comparator,
blanket Street convention or certificate is claimed. Unavailable Java/JNI,
Windows/.NET/Excel and wasm32/browser remain separate actual-host gates.
CVX-063 is partial, unaccepted and uncertified; mandatory records stay Planned.
