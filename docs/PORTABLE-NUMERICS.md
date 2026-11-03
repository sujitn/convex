# Portable numerical kernels

Rust and WebAssembly builds use the same locked, pure Rust `libm` kernels for
exponentials, logarithms, powers and stable two-component norms. The workspace
enables `libm`'s
`force-soft-floats` and nalgebra's `libm-force` feature. That includes the scalar
operations inside the bounded SVD calibration and hedge solves. It adds no
external package or version to the locked dependency graph.

Each calculation keeps its financial domains, curve roles, cash units, residuals,
bounds and work limits. The low-level IEEE functions can return infinity or NaN.
The financial callers validate their input and apply their own rejection rules.
Results are not rounded or filtered to force the hosts to agree. The independent
financial reference values and tolerances are unchanged.

The primitive controls compute their expected answers from the exact binary64
input bits using Python Decimal arithmetic. They include cancellation, subnormal
tails and overflow. Their four-ULP bound is separate from each instrument's
financial tolerance.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/scripts/portable_math_reference.py check
cargo test --locked -p convex --test portable_math
cargo run --locked -p convex --example portable_math
```

`library_host_census.py` captures every complete current library request.
`library_wasm_census.py` runs those requests through wasm32 under Node and keeps
all outputs and diagnostics. It compares every field to the current Rust
envelope with no numeric tolerance. This shows that the hosts agree. The
independent financial controls remain the reference for the values themselves.

This addresses platform-dependent numerical drift on the hosts available so far.
Not yet covered: Windows/Excel checks, full instrument and convention coverage,
any latency target, and independent certification.

Rust documents platform-dependent transcendental precision in its
[floating-point API](https://doc.rust-lang.org/std/primitive.f64.html#method.exp).
The locked [nalgebra features](https://docs.rs/crate/nalgebra/0.34.1/features)
provide the explicit portable linear-algebra selection.
