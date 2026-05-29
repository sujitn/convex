# Convex Java

JVM bindings for the [Convex](../../README.md) fixed-income analytics library.

Java users get the same analytics as the Rust API — bonds, curves, pricing,
risk, spreads, and the hedge advisor — through an idiomatic typed API
(builders, `BigDecimal`/`LocalDate`, immutable result records, `AutoCloseable`
handles). The JSON-RPC boundary to the native library is fully hidden.

## How it works

```
com.convex            ← typed public API (builders, ConvexAnalytics, HedgeAdvisor)
com.convex.internal   ← Panama FFM binding + Jackson envelope codec (hidden)
crates/convex-ffi     ← the native cdylib (C ABI), built from the Rust workspace
```

The binding calls the native library through Java's **Foreign Function & Memory
API** (Project Panama). There is **no hand-written JNI / C shim** — Java binds
directly to the cdylib's `char*`-in / `char*`-out C functions by symbol name.

## Requirements

- **JDK 22 or newer** (the FFM API is finalized in 22).
- At runtime, native access must be granted:
  `--enable-native-access=ALL-UNNAMED` (the Maven build sets this for tests).
- The native `convex-ffi` cdylib for your platform, bundled under
  `src/main/resources/native/<classifier>/` (see below). Versions track the Rust
  workspace (currently `0.13.0`).

## Building

```sh
# 1. Build + stage the native library for your platform classifier
#    (linux-x86_64, linux-aarch64, darwin-x86_64, darwin-aarch64, windows-x86_64)
cargo build -p convex-ffi --release
mkdir -p bindings/java/src/main/resources/native/linux-x86_64
cp target/release/libconvex_ffi.so \
   bindings/java/src/main/resources/native/linux-x86_64/

# 2. Build + test the jar
mvn -f bindings/java/pom.xml verify
```

`NativeLoader` extracts the cdylib matching the running OS/arch from the jar at
startup, so one published artifact carries every platform.

## Usage

```java
import com.convex.*;
import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.List;

var settle = LocalDate.of(2025, 4, 15);

try (Bond bond = FixedRateBond.builder()
        .cusip("037833100")
        .couponRate(new BigDecimal("0.05"))
        .frequency(Frequency.SEMI_ANNUAL)
        .issue(LocalDate.of(2025, 1, 15))
        .maturity(LocalDate.of(2035, 1, 15))
        .dayCount(DayCount.THIRTY_360_US)
        .currency(Currency.USD)
        .build();
     YieldCurve curve = YieldCurve.discrete()
        .referenceDate(LocalDate.of(2025, 1, 15))
        .point(2.0, 0.040).point(5.0, 0.042).point(10.0, 0.045)
        .interpolation(Interpolation.MONOTONE_CONVEX)
        .build()) {

    // Pricing & risk
    var price = ConvexAnalytics.price(bond, settle, Mark.cleanPrice(new BigDecimal("99.5")));
    var risk  = ConvexAnalytics.risk(bond, settle, Mark.cleanPrice(new BigDecimal("99.5")), curve, 2, 5, 10, 30);
    System.out.printf("YTM %.4f, DV01 %.2f%n", price.ytmDecimal(), risk.dv01());

    // Hedge advisor
    RiskProfile pos = HedgeAdvisor.positionRisk()
            .bond(bond).settlement(settle).mark(Mark.cleanPrice(new BigDecimal("99.5")))
            .notionalFace(new BigDecimal("10000000")).curve(curve).curveId("USD.SOFR")
            .keyRateTenors(2, 5, 10, 30)
            .compute();

    HedgeProposal futures = HedgeAdvisor.durationFutures(pos, curve, settle);
    HedgeProposal swap    = HedgeAdvisor.swap(pos, curve, settle, Constraints.none());
    ComparisonReport report = HedgeAdvisor.compare(pos, List.of(futures, swap), Constraints.none(), true);
    System.out.println(report.narrative().orElse(""));
}
```

## Thread-safety

The native registry and analytics are safe for concurrent use, and the FFM
method handles are immutable, so the API may be called from multiple threads.
(The `convex-ffi` registry clones objects out under the lock before running
analytics, so concurrent calls never deadlock.)

## Status / not yet wired

The native FFI does not yet expose every Rust capability. Currently missing
(same DTO + dispatch pattern when added — see
`crates/convex-analytics/src/dto.rs`):

- Convention/calendar introspection (`get_convention_options` analog)
- YAS settlement invoices, VaR, Hull-White calibration, piecewise bootstrap
- FRN projection / cap-floor analytics

Packaging note: the binding ships as an automatic module (works on the
classpath). A `module-info.java` is intentionally omitted for now; named-module
consumers should grant `--enable-native-access` to the unnamed module.

## Releasing to Maven Central

Publishing is driven by the `release` Maven profile (sources + javadoc jars,
GPG signing, and the Sonatype Central Portal plugin) and the
`.github/workflows/java-release.yml` workflow, triggered by a `java-v*` tag.

The workflow builds the cdylib for every platform classifier in a matrix,
stages them all under `src/main/resources/native/`, then runs
`mvn -Prelease deploy` so the single jar carries every platform.

Required repository secrets:

| Secret | Purpose |
|---|---|
| `MAVEN_CENTRAL_USERNAME` / `MAVEN_CENTRAL_PASSWORD` | Central Portal user token |
| `MAVEN_GPG_PRIVATE_KEY` | ASCII-armored signing key |
| `MAVEN_GPG_PASSPHRASE` | passphrase for that key |

To cut a release: `git tag java-v0.13.0 && git push origin java-v0.13.0`.
