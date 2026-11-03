# Executing complete Java host checks

The Java shared API passes exact decimal strings and the complete supplied
`LibraryRequest` to `convex_library` through FFM. Native envelopes establish
transport expectations; independent financial controls establish pricing and risk
answers. Host parity does not admit additional product/convention/model terms or
certify the library.

Build a fresh native library and the complete transport consumer:

```sh
cargo build --locked -p convex-ffi -p convex --examples --features convex/schemars
PYTHONDONTWRITEBYTECODE=1 python3 tests/scripts/library_host_census.py \
  --binary target/debug/examples/library_host_census \
  --output-dir bindings/java/target/library-host-census
```

Stage that current C library under the applicable Java resource platform path,
as in `.github/workflows/java.yml`, then run JDK 22 or later:

```sh
mvn -B -f bindings/java/pom.xml -Dtest=LibraryCensusTest test
```

The census includes `cases.json` and every supplemental corpus in the current
capability matrix. The native consumer checks direct Rust/JSON results and the
declared success/error category, then retains every complete request and native
envelope in gzip JSON-lines files. The preparer rejects missing/extra/duplicate
requests and changed original terms. Java reads one complete request at a time,
calls actual FFM, retains the full actual envelope, and compares every field.
No numerical tolerance, sampling or response truncation is applied. The output
directory is new for each execution; existing captures are never overwritten.

`target/library-host-actual/summary.json` reports executed counts. Full actual
envelopes remain in the accompanying gzip files. Runtime, C/native binary and
original source hashes must accompany a qualification record. A complete Maven
suite also needs the original per-consumer parity files; the Java CI workflow
generates these before `verify`, including the monotone interpolation consumer.
Passing one census is scoped evidence for that actual JDK/platform/build. Other
JDKs, platforms, native build profiles and retained Java conveniences keep their
own qualification requirements. Windows/.NET/Excel and WASM/browser execution
are separate gates.
