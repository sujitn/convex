# Bounded Rust and model assurance

Convex applies a small, enforceable subset of the NASA-inspired production
rules. This is an engineering assurance baseline, not NASA certification,
product certification, a latency guarantee, or a completed production gate.
The rules live in ordinary Rust attributes, CI, and `tools/assurance`; no
runtime policy framework is added to pricing.

## Scope and dependency direction

The pure calculation crates are `convex`, `convex-core`, `convex-math`,
`convex-curves`, `convex-bonds`, `convex-analytics`, and `convex-portfolio`.
Each forbids unsafe code and denies unchecked `must_use` results. Math and
FFI production builds additionally deny Clippy's `unwrap_used`, `expect_used`,
and `panic`; core contracts and global curve calibration apply the same
scoped rules. Test assertions remain available. FFI denies undocumented
unsafe blocks, missing safety documentation, and implicit unsafe operations.

The dependency checker traverses all-feature, target-specific **normal**
dependency edges using `cargo metadata --locked`. It rejects network,
database, async-runtime, vendor-SDK and application adapter packages, plus
any normal package absent from the reviewed allowlist. Dev-only property
tools and build-only header generation do not enter the calculation graph.
The lockfile, manifests, source inventory and CI are change-review protected.
Legacy calendar file conveniences are inventoried separately; they do not
make filesystem access part of the production calculation flow.

`inventory.json` records each critical source hash, invariants, termination
bounds, failure contract, tests and applicable exceptions. The inventory
covers math, FFI, checked dates/contracts, selected calendar/day-count paths,
curve storage/calibration, HW1F and the existing graph/runtime boundary.
It is deliberately scoped: it does not assert every product or every pure
API is already complete. Its C export list detects ABI inventory drift.

## Numerical contracts and bounds

| Path | Enforced bound | Success/failure contract |
|---|---|---|
| Root solvers | 1–10,000 iterations per configured phase | Finite configuration, input, evaluations and iterates; successful residual below the requested tolerance; stagnation/nonconvergence is an error. |
| Hybrid solver | Up to 20 Newton steps, 50 bracket expansion steps, then the configured Brent budget | Normal zero-derivative/divergence recovery is allowed; nonfinite arithmetic is an error. Caller callbacks must terminate and must not panic. |
| Interpolation/tridiagonal | At most 65,536 points | Matched dimensions and finite values; strictly increasing finite interpolation nodes; invalid query/output is an error. Empty tridiagonal systems return the empty solution. |
| Dense LU | At most 256 × 256 | Finite arithmetic and explicit singular-system failure; existing pivot threshold retained. |
| Global calibration | At most 128 helpers; 1–10,000 LM iterations | Helper failures propagate, no zero residual or zero step substitutes; final finite RMS must satisfy the requested tolerance. |
| Piecewise calibration | At most 128 helpers, configured solver bound for each pillar, at most 32 full-curve refinement sweeps | Failed solves cannot become quote values. Full-curve repricing satisfies the requested tolerance or returns an error. |
| HW1F calibration | At most 128 helpers; combined expiry/tail ≤150 years, 200 minimizer iterations | Finite valid inputs and positive market values; missing curve values and failed Jamshidian roots return errors. |
| Checked HW1F swaption | At most 150 annual payments and 100 root iterations | Finite price or typed error; no zero-rate fallback. |
| Checked date arithmetic | Chrono's representable calendar; checked day/month/year addition | Overflow/out-of-range returns an error; signed month arithmetic uses Euclidean division. |

Solver defaults remain `1e-10` and 100 iterations. Piecewise defaults remain
`1e-12` and 100 iterations. HW1F retains its existing minimizer bracket,
`1e-9` width tolerance and 200 iterations. The existing financial fixtures
and test tolerances are not widened. Root success now requires a residual
postcondition; a narrow bracket alone no longer certifies a root.

These are resource ceilings, not measured capacity or latency targets.
Caller allocations, arbitrary custom closures and allocator failure cannot
be made safe by an iteration count. Product-specific convergence studies
and comparator coverage remain separate work.

For externally supplied input use `Date::checked_add_days`,
`UfrConvergence::try_new`, `Extrapolator::try_extrapolate`, parametric
`try_forward_rate`, `golden_section_checked`, and `try_payer_swaption_hw1f`.
Older infallible signatures remain compatible under registered preconditions;
golden-section and swaption compatibility functions return NaN on failure,
never a plausible last iterate or invented rate. Do not consume those NaNs
as successful analytics.

Negative rates remain valid for the supported rate/log-discount paths;
discount factors above one are allowed. The existing monotone-convex model
has a nonnegative-forward constraint. UFR convergence remains a heuristic,
not Smith-Wilson. The approximate `SequentialBootstrapper` builds a quote
curve for diagnostics; its historical completion flag does not attest
repricing. Use the checked piecewise/global fitter in production ingress.
These limitations are explicit, time-bounded inventory exceptions.

## Host ABI

Build host libraries with `cargo build --locked --profile excel -p convex-ffi`.
The `excel` profile uses `panic=unwind`; an abort build of the FFI crate is
rejected at compile time. Invalid input uses normal errors. `catch_unwind`
is an unexpected-panic backstop, not an input validator and not protection
against abort, allocator termination or undefined behavior.

All JSON constructors and RPCs validate input before building or dispatching:

- At most 2 MiB of UTF-8 input, depth 64, 32,768 nodes, 4,096 entries per
  array/object, and 8,192 bytes per key/string. Duplicate fields at any depth
  and trailing tokens are rejected.
- At most 128 calibration instruments, 64 key-rate/scenario/proposal entries
  and 32 futures basket overrides. Numeric magnitudes are bounded by `1e18`;
  tenors by 150 years, date years by 1–9999, instrument spans by 150 × 366 days,
  rates/coupons by −10 to 10 and volatility by 0 to 10.
- At most 32 simultaneous constructor/RPC calls and 16,384 registered objects.
  Overload returns an error. Admission is released during unwinding. A registry
  read clones an `Arc` before analytics runs, so eviction does not invalidate
  an in-flight calculation and nested lookup cannot retain a registry lock.

Exact decimal strings pass to the original DTO parser after bounds checking;
the check does not rewrite financial digits. Simple/Daily compounding for
zero-coupon construction is explicitly unsupported, rather than substituted
with another convention. Poisoned registry locks fail closed. An unavailable
object count is `-1`, not a false empty registry. Unexpected panic envelopes
contain a fixed diagnostic; the host's Rust panic hook may still log locally.

Every input pointer must reference an immutable, NUL-terminated allocation
valid for the entire call. The bounded scan prevents unbounded valid input;
it cannot validate dangling pointers or reads outside a caller allocation.
Free Rust-owned outputs exactly once with `convex_string_free`; never free
borrowed diagnostics/static version pointers. Safety contracts on each export
and unsafe block follow Rust's FFI and C-string requirements.

## Review and exceptions

`exceptions.json` requires scope, rationale, mitigations, owner, review date,
expiration and tracking task. An exception lasts at most 90 days. An expired,
future, malformed or duplicate exception rejects assurance. Exceptions do
not allow network/database runtimes in pure crates, invented financial
results, wider financial tolerances, or skipped required checks. Owners are
declared maintenance assignments, not evidence of independent human approval.

`numeric-evidence.json` pins comparator code/workflow, fixtures and the
existing numerical test modules. Changes to those bytes fail the normal
check. Refreshing/removing an existing pin requires a version increase and
a declared independent quantitative review against the PR base. Changing
an existing pinned fixture also requires that review, even if its hash has
been refreshed. New product evidence must describe units, settings, reference
version and justification; a hash is not proof of quantitative accuracy.

`change-reviews.json` ties engineering impact to a base commit and candidate
file hashes, with actor, rationale, review date and test evidence. It includes
untracked candidate modules and detects changes after the review. An old
record cannot authorize a new candidate. For each coding task:

1. Select the actual PR base and review the diff and relevant inventory entries.
2. Record the engineering impact and exact candidate hashes after the final edit.
   Update affected inventory entries deliberately; normal checks never refresh
   hashes. If an existing quantitative artifact changes, obtain independent
   quantitative review and record its evidence; an agent cannot attest that
   human approval occurred.
3. Run both `python3 tools/assurance/check.py` and
   `python3 tools/assurance/check.py --base <base-sha>`, plus the applicable
   checks below. Keep actual failures and unexecuted checks in task evidence.

The initial impact record covers CVX-039 against its immediate parent commit.
Before opening a PR for the whole extension branch, add an impact record against
that PR's actual base; earlier branch changes are not implicitly authorized by
the task record.

The workflow validator checks the committed CI structure and exact required
commands, rejects conditional/skipped checks and ignored exits, and permits
only the PR change-impact step to be conditional. It is a narrow structure
check, not a general YAML parser or an authorization system. Repository files
cannot authenticate declared reviewer identities or configure GitHub branch
protection. Administrators must require these checks and independent review
on protected branches; this task does not claim that remote policy is active.

## Executable verification

```sh
python3 tools/assurance/check.py
python3 tools/assurance/check.py --base <base-sha>
python3 -m unittest discover -s tools/assurance -p 'test_*.py'
cargo fmt --all -- --check
cargo test --locked -p convex-math -p convex-core -p convex-ffi
cargo test --locked -p convex-curves -p convex-bonds -p convex-analytics
cargo clippy --locked --all-features --workspace -- -D warnings
cargo check --locked -p convex -p convex-core -p convex-math -p convex-curves -p convex-bonds -p convex-analytics -p convex-portfolio --no-default-features
cargo test --locked -p convex-math -p convex-core -p convex-ffi --all-features
cargo test --locked -p convex-engine --test graph_regressions
cargo build --locked --profile excel -p convex-ffi
python3 tools/assurance/abi_smoke.py
```

The assurance tests include deterministic 512-case root/tridiagonal/date
properties, arbitrary floating-point patterns, arbitrary bytes and JSON
mutations through real FFI exports, and synchronized registry/admission
concurrency. The saved proptest seed reproduces a premature root-stop defect.
The ABI smoke script loads the optimized cdylib in its own consumer process.
These are bounded fuzz/property campaigns, not coverage-guided fuzzing or a
soak certificate. CI also retains full workspace tests, docs, strict Clippy,
format and the deliberately enforced production probes.

Where supported, an additional interpreter target is:

```sh
cargo +nightly miri test -p convex-ffi --lib registry::tests
```

This requires a Miri-enabled nightly and a compatible build-script setup;
an unavailable command is not a pass. Sanitizers, longer coverage-guided fuzz
campaigns, independent comparator refresh, distributed state exploration,
allocation profiling and live host/transport certification require separate
recorded runs. None is inferred from the default unit suite.

References: [Cargo metadata](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html),
[Clippy unwrap scope](https://rust-lang.github.io/rust-clippy/master/index.html#unwrap_used),
[Rust FFI](https://doc.rust-lang.org/nomicon/ffi.html),
[C string safety](https://doc.rust-lang.org/std/ffi/struct.CStr.html).
