# Convex canonical analytics contracts

The `convex_core::contracts` module supplies validated identities, units, time, signed factor labels and result/error records for new library and application boundaries. The facade exposes the same module as `convex::contracts`. These are pure data contracts; they do not resolve records, obtain market data, calculate risk or certify a product model. Existing types and JSON/FFI calls remain available with their current behavior.

## Identities and time

Internal instrument, trade, curve, index and entity IDs are distinct Rust types with private fields. Constructors and deserializers reject empty, oversized or malformed tokens. An ID contains 1–128 ASCII characters, starts with a letter/digit, and then permits letters/digits or `_ . : / -`. IDs retain case and are not generated from supplier identifiers. A vendor remap is an application mapping change, not a new internal identity. Explicit `TryFrom` bridges validate the public InstrumentId/CurveId before admitting them into a canonical record.

`Version` requires a positive integer. `SchemaVersion` accepts only `"2.0"`; it never means the latest schema. `ValuationSubject` pins an instrument or trade version. Full reference history and mapping resolution remain repository responsibilities.

`BusinessDate` requires valid YYYY-MM-DD in years 0001–9999. `UtcTimestamp` requires UTC `Z` notation with zero to nine fractional second digits; offsets, missing time zones, excess precision and leap seconds are rejected. Native constructors accept supplied dates/instants, never the current clock. Valuation time, known-at cutoff and calculation completion time are separate provenance fields. A future economic valuation time is permitted, while a known-at cutoff after calculation completion is invalid.

## Units and precision

| Type | Wire value | Required unit | Meaning |
|---|---|---|---|
| PricePer100 | `"98.50"` | `price_per_100` | Price or accrued per 100 face |
| DecimalRate | `"0.05"` | `decimal_rate` | 5%, including negative rates |
| BasisPoints | `"125"` | `bp` | 125 basis points |
| FaceAmount | `"1000000.00"` | `face_amount` | Exact nonnegative face; trade direction is separate |
| FuturesContracts | `"-12"` | `futures_contracts` | Signed integer position, no fractional contracts |
| Money | `"-425.00"` | `currency` | Exact signed cash; currency is mandatory |

Every wire value above is an object, such as `{"value":"0.05","unit":"decimal_rate"}`. Units are mandatory and unknown fields/units fail. Decimal and integer strings preserve precision in clients that use binary floating-point JSON numbers. Decimal ingress uses the existing 96-bit rust_decimal coefficient and maximum 28-place scale; excess precision/overflow fails instead of rounding. Plain notation is required, without exponent notation, separators or whitespace. Futures counts are bounded signed 64-bit integers, encoded as strings. Native types cannot be substituted for each other.

`ModelPricePer100`, `ModelDecimalRate`, `ModelBasisPoints` and currency-qualified `ModelMoney` instead use JSON numbers validated by `FiniteF64`. They preserve finite model values without posting-time rounding. NaN/infinity are rejected at native and serde ingress. `MeasureValue` distinguishes exact and model representations by their value type while retaining the same explicit economic unit. Negative rates, spread signs and monetary PVs are valid.

`BasisPoints::to_decimal_rate` and the reverse conversion are explicit and reject overflow or loss of exactness. `ExactDecimal::to_model` declares the transition to binary model precision. The reverse requires `DecimalRounding` with scale and named half-even, half-away-from-zero or toward-zero rule; overflow and representation underflow fail. Currency never selects a rounding rule implicitly. Ledger posting/rounding residual policies remain the ledger task's responsibility.

## Signed risk and results

`RiskFactorKey` includes family, structured target, quote currency, tenor, quote type, bump/recalibration basis, convention version and model version. All dimensions participate in equality/hash. Yield, zero-node and recalibrated market-quote risk cannot share an unqualified bucket. `RiskSensitivity` carries reporting currency, snapshot, actual positive bump and method. It validates family/measure compatibility. Futures-price, upfront, hazard and recovery sensitivities cannot be relabelled as a one-bp rate/spread measure.

`pv01_signed = (PV(q+0.0001)-PV(q-0.0001))/2`, normalized to a +1 bp quote move. `dv01_loss = -pv01_signed`; `cs01_signed` uses the analogous credit spread quote move. Multiply these already normalized values directly by a move expressed in bp. Constructing the record does not calculate or validate the quantitative sensitivity; pricing and recalibration implementations must prove those semantics in their owning tasks.

`ResultEnvelope` pins schema, request, snapshot, configuration and full caller-supplied provenance. Complete requires successful measures and no errors; Partial requires both; Failed requires errors and no numerical outputs. Cross-field ingress rejects snapshot/configuration disagreement, mixed-snapshot risk and duplicate consumed definition records. Results/errors/manifests have explicit count bounds. Requested-measure coverage is still checked by the calculating use case. Completeness is independent of quality and does not authorize execution. Individual measures carry method, quality and an actual observation reference when applicable; snapshot manifests supply full source lineage.

Errors have stable enum codes, field, message and retryability. UnknownOutcome requires reconciliation and cannot be marked safely retryable. Failure has no zero placeholder. Wire examples are in [the canonical fixtures](tests/fixtures/canonical_partial_result.json) and [risk fixture](tests/fixtures/canonical_risk.json).

## Compatibility and schemas

`convex_analytics::contracts::ValidatedBondSpec` retains every existing BondSpec field and default, while validating issue/maturity/face and finite optional model inputs. It exposes coupon as decimal and floating spread as bp. It is a validated DTO boundary, not a new complete instrument specification or a strict v2 product ingress schema. Full schedule/option/convention validation remains with product construction. `CanonicalPricingValues` performs finite typed conversion and preserves native f64 bits; it never divides an existing decimal yield by 100. `ValidatedRiskResponse` preserves the original risk signs and bases; it cannot invent the factor, quantity, currency, bump or snapshot needed for canonical PV01.

Port methods `BondReferenceData::canonical_boundary` and `BondQuoteOutput::canonical_mid_values` use the same units. Reference identity stays independent of supplier fields. Quote adaptation leaves missing prices/yields/accrued absent. Dirty price needs both clean and accrued, and exact addition must not overflow or round. The authoritative reference repository validates this boundary and persists exact decimal strings; see [the reference service contract](../convex-server/README.md#authoritative-bond-reference-data). Publishing helpers retain their existing behavior.

Enable `schemars` to call `contracts::json_schemas()`. The checked-in [schema bundle](schemas/canonical-contracts.json) is generated from the Rust wire definitions and checked by a snapshot test. To regenerate it:

```sh
cargo run --locked --quiet -p convex-core --features schemars --example export_contract_schemas > crates/convex-core/schemas/canonical-contracts.json
```

Schemas cover structure, units, scalar bounds, formats and envelope status/count rules. Calendar validity, decimal coefficient bounds, factor compatibility, chronology and version equality still require checked deserialization. JSON Schema alone is not a quantitative or data-quality certificate. The v2 service/OpenAPI and expanded shared FFI dispatch remain later tasks; no endpoint or C symbol was added here.
