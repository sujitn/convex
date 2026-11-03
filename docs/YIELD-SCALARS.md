# Yield scalar measures

Current yield, arithmetic simple-yield estimates and scalar yield reporting live in
`convex_bonds::pricing::yield_scalars`. The functions in `convex-bonds` and the
convenience functions in `convex-analytics` delegate to that module. Each one
validates its input and returns an error instead of guessing.

Percentage and Decimal methods on `YieldResult` and `YieldEngineResult` return
`Result`; propagate errors with `?`. Analytics reexports the `convex-bonds` engine
result instead of keeping a second result definition. Solver, price and accrued
conventions are separate.

Nonfinite values, nonpositive price/time/face, overflow and loss of a nonzero value
to zero fail. Signed coupon/yield values and true zeros remain valid. Decimal
reporting retains binary64; it does not reconstruct the decimal quote. Checked
Decimal current/simple calculations keep the same operator order, units and
rounding as before on valid inputs, and report arithmetic failure rather than
panicking.

A price per 100 must be paired with annual coupon cash per 100. Both from-bond
helpers used to divide face-sized coupon cash by that quote: a 5% bond with face
5000 at price 100 returned decimal 2.5, or 250%. The constant-coupon functions now
validate the positive face and return decimal 0.05, or 5%, just as for face 100.
A current or simple yield is not a complete stepped or floating bond valuation, an
instrument yield, a market convention or a hedge instruction.

The strict shared `yield_scalar` operation takes explicit coupon and price inputs,
the original face where applicable, and returns distinct decimal-rate and percentage
outputs. Outputs carry `unit`, `representation` and `value`; Decimal inputs are
strings. Use `cargo run --locked -p convex --example yield_scalars` for the scalar
and failure corpus, or add `--features schemars -- --emit-schema` to emit the
request schema. The C, Java, Excel, MCP and WASM bindings share the Rust JSON route.

Not yet covered: the general yield engines, contextual ICMA time preparation,
accrued and ex-coupon shortcuts, and the remaining price helpers that cannot
return an error. Testing in Windows and Excel is also pending, and no market,
model, library, production or latency certification follows from these checks.
