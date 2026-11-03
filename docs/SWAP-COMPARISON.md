# Swap comparison against QuantLib

This document describes how the swap pricer in `convex` is compared with
QuantLib 1.40, and how to rerun the comparison. The inputs, scripts and captured
outputs are under `tests/fixtures/comparisons/` and `tests/scripts/`.

The comparison passed on 6 October 2026 using a QuantLib 1.40 runtime that was
verified against its publisher wheel and installed payloads. All ten contracts
and 973 numeric values match within bounds declared beforehand, with exact
coupon, payment, quotation and fixing lineage. The largest absolute cash
difference is `2.375200289316311e-9` USD. The
[pinned capture](../tests/fixtures/comparisons/swap-quantlib-1.40/manifest.json)
keeps the raw outputs, commands, timestamps, build records and source/runtime
hashes. Earlier download and capture failures are kept as history.

This covers the ten contracts only. It does not cover market/model validation,
lifecycle, risk, operations or latency. The earlier
[twenty manual references](SWAP-VALUATION.md) are a separate check.

## Run it locally

From the repository root:

```sh
python3 tests/scripts/swap_quantlib_compare.py check
python3 -m unittest discover -s tests/scripts -p test_swap_quantlib_compare.py -v
cargo run --locked -p convex --example swap_comparison
```

Build the current Rust example and compare it against the captured reference,
without installing QuantLib or using the network:

```sh
cargo build --locked -p convex --example swap_comparison
python3 tests/scripts/swap_quantlib_compare.py offline \
  --candidate-binary target/debug/examples/swap_comparison \
  --report /private/tmp/convex-swap-comparison-report.json
```

`offline` checks every captured artifact against its numerical-policy pin,
validates the original successful execution, then runs the binary you pass in
against the fixed QuantLib outputs. It records that binary's hash, the current
source hashes, the command, clocks and raw outputs. Build the binary first: this
mode does not attest that an arbitrary executable was built from the current
checkout. CI runs the build and the comparison in sequence and uploads the
report and raw candidate outputs. It does not need QuantLib.

The Rust example also runs with `--no-default-features` and
`--features schemars`. It adds no production dependency, server route or adapter
requirement. `replay` checks a captured run against the exact calculation and
protocol sources it was made with, and refuses source changes. To compare a new
Rust build with the unchanged reference, use `offline`.

## Install QuantLib and capture

The runtime pin targets a macOS ARM64 development host. Use Python 3.11+ in an
isolated environment. Download this exact publisher file
from the [official QuantLib 1.40 release](https://pypi.org/project/QuantLib/1.40/#files):

```text
quantlib-1.40-cp38-abi3-macosx_11_0_arm64.whl
SHA256 fefb04e34a4f6a529e8d3a0506de6cd443d52ba85fbc13b4c30b7865ce5fa4ce
```

Its [publisher build identity](https://github.com/lballabio/quantlib-wheels/tree/fa275d5ea06e13d0dd2f475a3ceb857b8f388443)
is pinned in the input fixture. For another OS or architecture, add its
verified publisher artifact as a separately reviewed protocol change; a version
string or arbitrary package is not enough. This pin applies to the test runtime
only and does not limit where Convex runs.

After placing the checksum-verified wheel at `/private/tmp/cvx019-wheel/`:

```sh
shasum -a 256 /private/tmp/cvx019-wheel/quantlib-1.40-cp38-abi3-macosx_11_0_arm64.whl
python3 -m venv /private/tmp/cvx019-quantlib
/private/tmp/cvx019-quantlib/bin/python -m pip install --no-deps /private/tmp/cvx019-wheel/quantlib-1.40-cp38-abi3-macosx_11_0_arm64.whl
python3 tests/scripts/swap_quantlib_compare.py run \
  --quantlib-python /private/tmp/cvx019-quantlib/bin/python \
  --wheel /private/tmp/cvx019-wheel/quantlib-1.40-cp38-abi3-macosx_11_0_arm64.whl \
  --output /private/tmp/cvx019-quantlib-run-001
python3 tests/scripts/swap_quantlib_compare.py replay /private/tmp/cvx019-quantlib-run-001
```

Compare the `shasum` output with the declared SHA256 before installing. The
runner repeats the hash check and verifies the installed publisher payloads
(the Python wrapper, native extension, metadata and relocated publisher license
in your virtual environment) **before importing QuantLib**. It also checks the
QuantLib engine version. It never downloads or installs packages. A missing or
wrong runtime is a failure.

Use a fresh output directory for every attempt; existing output directories are
never overwritten. A failed engine run keeps its command, timestamps, exit
status and raw stdout/stderr, and cannot replay successfully. QuantLib runs in a
separate Python process, so its evaluation-date state never enters the Convex
library or server. The runner restores settings between cases.

## Matched input scope

The [input fixture](../tests/fixtures/comparisons/swap-quantlib-inputs.json)
contains the full terms and no expected outputs. Its SHA256 is
`ba61375e251884208f70585cb0aeac034324016cc91623ec7abd0e0020b2211a`.

| Cases | Economics exercised |
|---|---|
| IRS seasoned, receiver and forward | Three-month indexed coupons, independent coupon/payment calendars, eligible resets, separate discount/projection curves and trade direction. |
| IRS amortizing/stepped | Coupon-boundary notional, fixed-rate, spread and gearing changes with published and projected observations. |
| IRS negative | Negative history, projection and discount rates; negative coupons remain allowed. |
| OIS seasoned and amortizing/stepped | Holiday/day weights, current coupon accrued, future compounding and coupon-boundary economic changes. |
| OIS lookback, observation shift and lockout | Explicit two-business-day lookback, shifted weights and three-business-day reuse, on the pinned observation calendar. |

All ten use ACT/360 coupons, unrounded cash, quarterly aligned dates and a
two-business-day payment delay. The calendars are synthetic resources at version
1, with Saturday/Sunday weekends and explicit holiday dates. Final, varying
fixing observations include publication/known-at timestamps and revision 1.
Publication is declared at 08:00 UTC; valuations use an end-of-day information
cutoff. Future records are not loaded into QuantLib as known fixings.

The discount and index curves are separate, with exact versions and dates,
ACT/365F linear continuous zeros and extrapolation disabled. Their reference
date precedes valuation, so payment discount ratios must normalize at valuation.
There are no principal exchanges. Rates are annualized decimals; cash, PV and
accrued are absolute USD amounts at an initial notional of 1,000,000.

The IRS quotation interval is `IndexTenor` with three months. This does not
reinterpret the earlier monthly `ResetInterval` fixture as a standard three-month
index coupon.

Outside the scope of this comparison:

- stubs;
- multiple resets within a coupon;
- profile changes inside a coupon;
- rounding;
- contextual day counts;
- calendars other than those in this suite;
- other host and model routes.

The broader Rust and manual tests still apply but have no external comparison.

The QuantLib side builds its schedules from the terms and its own calendars, then
constructs real `FixedRateCoupon`, indexed `IborCoupon` and
`OvernightIndexedCoupon` objects. It uses QuantLib index forecasts, historical
fixings, coupon accrued and `DiscountingSwapEngine`. It reads no Convex or
manual expected output to compute its reference. The full cash and par results
are compared only after both engine runs succeed.

The API mapping is based on the pinned
[coupon bindings](https://github.com/lballabio/QuantLib-SWIG/blob/v1.40/SWIG/cashflows.i),
[swap engine bindings](https://github.com/lballabio/QuantLib-SWIG/blob/v1.40/SWIG/swap.i)
and [zero-curve bindings](https://github.com/lballabio/QuantLib-SWIG/blob/v1.40/SWIG/zerocurve.i).
The pinned runtime executed these bindings successfully. When recapturing,
resolve API or economic differences without changing the declared bounds.

## Evidence and tolerances

The bounds were declared before any comparison run: absolute currency tolerance
`1e-7`, annualized rate tolerance `1e-12`, discount-ratio and year-fraction
tolerance `1e-13`, zero relative tolerance and exact date/identity matches.
There is no percentage or scale-based relaxation. The saved report lists every
mismatch and the maximum error by unit. Existing numerical artifacts, tolerances
and solver defaults are unchanged.

A run keeps the inputs, the QuantLib and Rust raw stdout/stderr, the commands
and clocks, Rust/Cargo version outputs, the Cargo build record, the executable
SHA256, build features/profile/flags, the reviewed calculation-source hashes, the
installed QuantLib payload hashes and the artifact manifest. Replay checks those
bytes and bindings against the unchanged source and protocol, rejects incomplete
execution, and recomputes the comparisons offline. SHA256 bindings detect
changes; they do not authenticate who produced the execution records.

The comparison tool always reports `accepted_task: false` and `certified: false`;
those fields are fixed and do not record a review outcome. Do not replace
mismatching captures, relabel manual references, remove cases or widen bounds.
Changes to pinned numerical results need independent review under the
[assurance policy](internal/ASSURANCE.md). Not yet covered: other hosts,
market/model validation, operations and latency.
