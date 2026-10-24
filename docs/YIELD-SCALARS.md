# Checked yield scalar measures

Current yield, arithmetic simple-yield estimates and scalar yield reporting now
use checked native operations in `convex_bonds::pricing::yield_scalars`. The native
and analytics convenience functions delegate to that owner. Percentage/Decimal
methods on `YieldResult` and `YieldEngineResult` return `Result`; propagate errors
with `?`. Analytics reexports the native engine result instead of maintaining a
second result definition. Existing solver/price/accrued conventions are separate.

Nonfinite values, nonpositive price/time/face, overflow and loss of a nonzero value
to zero fail. Signed coupon/yield values and true zeros remain valid. Decimal
reporting retains binary64; it does not reconstruct an original decimal quote.
Checked Decimal current/simple calculations preserve the original operator order,
units and rounding on admitted inputs, and report arithmetic failure rather than
panicking. The original independently tested financial cases and bounds remain.

A price per100 must be paired with annual coupon cash per100. Both original
from-bond helpers previously divided face-sized coupon cash by that quote: a5%
bond with face5000 at price100 returned decimal2.5 or250%. The checked constant-
coupon owners validate actual positive face and return decimal0.05 or5%, just as
for face100. A current/simple scalar is not a complete stepped or floating bond
valuation, instrument yield, market convention or executable hedge instruction.

The strict shared `yield_scalar` operation exposes explicit coupon/price inputs,
original face where applicable, and distinct decimal-rate/percentage outputs.
Outputs retain `unit`, `representation` and `value`; Decimal inputs are strings.
Use `cargo run --locked -p convex --example yield_scalars` for the independent
scalar and failure corpus, or add `--features schemars -- --emit-schema` to emit
the complete request schema. C/Java/Excel/MCP/WASM share the native JSON route;
actual source-bound host qualification remains separate from capability claims.

The raw general yield engines, contextual ICMA time preparation, accrued/ex-coupon
shortcuts and remaining infallible price helpers are still unqualified work.
CVX-063 stays open. Windows/Excel and independentCVX064/065 require their own
execution/approval; no market/model/library/production/latency certificate follows.
