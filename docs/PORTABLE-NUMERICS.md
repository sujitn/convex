# Portable numerical kernels

Native and WebAssembly valuation use the same locked pure Rust `libm` kernels
for exponentials, logarithms, powers and stable two-component norms. The workspace
enables `libm`'s `force-soft-floats` and nalgebra's `libm-force` feature, including
the scalar operations inside bounded SVD calibration and hedge solves. This adds
no external package or version to the existing locked dependency graph.

Calculation owners keep their original financial domains, curve roles, cash
units, residuals, bounds and work limits. The low-level IEEE functions can return
infinity or NaN; the checked financial callers retain their own rejection rules.
Results are not rounded or filtered to manufacture transport parity. Existing
independent financial reference values and tolerances remain unchanged.

The primitive controls derive answers from exact binary64 input bits using Python
Decimal arithmetic, including cancellation, subnormal tails and overflow. Their
four-ULP bound is separate from each instrument's original financial tolerance.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/scripts/portable_math_reference.py check
cargo test --locked -p convex --test portable_math
cargo run --locked -p convex --example portable_math
```

`library_host_census.py` captures every complete current supplied-input request.
`library_wasm_census.py` executes those requests through actual wasm32/Node and
retains all outputs and diagnostics. It compares every field to the current
native envelope without a numeric tolerance. Native envelopes establish transport
parity; independent financial controls remain the valuation reference.

This addresses platform-dependent numerical drift in the available host. Full
required instrument/convention/model coverage, advertised convenience APIs,
unavailable Windows/Excel checks and independent certification retain their own
completion gates. It does not establish a production latency target.

Rust documents platform-dependent transcendental precision in its
[floating-point API](https://doc.rust-lang.org/std/primitive.f64.html#method.exp).
The locked [nalgebra features](https://docs.rs/crate/nalgebra/0.34.1/features)
provide the explicit portable linear-algebra selection.
