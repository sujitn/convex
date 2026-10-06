# CVX-019 external swap comparison

CVX-019 AC2 passed on 6 October 2026 using an actually executed QuantLib
1.40 runtime verified against its publisher wheel and installed payloads. All
ten supplied contracts and 973 numeric values match within the original
predeclared bounds, with exact coupon, payment, quotation and fixing lineage.
The largest absolute cash difference is `2.375200289316311e-9` USD. The
[complete pinned capture](../tests/fixtures/comparisons/swap-quantlib-1.40/manifest.json)
retains raw outputs, commands, timestamps, build records and source/runtime
hashes. Earlier download and capture failures remain historical evidence.

CVX-019 is accepted as scoped engineering work. Market/model approval,
complete library/host coverage, lifecycle, risk, operations and latency
certificates remain separate requirements. The earlier
[twenty manual references](SWAP-VALUATION.md) remain separately qualified.

## Start locally

From the repository root on `codex/production-readiness`:

```sh
python3 tests/scripts/swap_quantlib_compare.py check
python3 -m unittest discover -s tests/scripts -p test_swap_quantlib_compare.py -v
cargo run --locked -p convex --example swap_comparison
```

Build a current native candidate and compare it against the independently
captured reference without installing QuantLib or using the network:

```sh
cargo build --locked -p convex --example swap_comparison
python3 tests/scripts/swap_quantlib_compare.py offline \
  --candidate-binary target/debug/examples/swap_comparison \
  --report /private/tmp/convex-swap-comparison-report.json
```

`offline` checks every captured artifact against its unchanged numerical-policy
pin, validates the original successful execution, then runs the supplied native
binary against the fixed QuantLib outputs. It records that binary's hash,
current source hashes, actual command, clocks and raw outputs. Build the binary
first: this mode does not attest that an arbitrary supplied executable was built
from the current checkout. CI runs the build and comparison in sequence and
uploads the report and raw candidate outputs. It uses no comparator runtime.

The native exporter also runs with `--no-default-features` and
`--features schemars`. There is no new production dependency, server route or
adapter requirement. `replay` checks a captured run against its exact original
calculation/protocol sources; it refuses source changes. Future development
should use `offline` to compare a new native candidate with the unchanged
independent reference.

## Obtain the comparator and capture

This initial runtime pin targets the current macOS ARM64 development host. Use
Python 3.11+ in an isolated environment. Download this exact publisher file
from the [official QuantLib 1.40 release](https://pypi.org/project/QuantLib/1.40/#files):

```text
quantlib-1.40-cp38-abi3-macosx_11_0_arm64.whl
SHA256 fefb04e34a4f6a529e8d3a0506de6cd443d52ba85fbc13b4c30b7865ce5fa4ce
```

Its [publisher build identity](https://github.com/lballabio/quantlib-wheels/tree/fa275d5ea06e13d0dd2f475a3ceb857b8f388443)
is pinned in the input fixture. For another OS/architecture, add its verified
publisher artifact as a separately reviewed protocol change; a version string
or arbitrary package is insufficient. This test runtime pin does not limit
Convex's production deployment platforms.

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
runner performs the hash check again and verifies installed publisher payloads,
including the Python wrapper, native extension, metadata and relocated publisher
license under the supplied virtual environment, **before
importing QuantLib**. It checks the actual native engine version. It never
downloads or installs packages. A missing/wrong runtime returns a failure.

Use a fresh output directory for every attempt. Existing evidence directories
are never overwritten. A failed engine run retains its command, timestamps,
exit status and raw stdout/stderr; it cannot replay successfully. QuantLib runs
in the supplied separate Python process, so its evaluation-date state does not
enter the Convex library or server. The runner restores settings between cases.

## Matched input scope

The [full supplied-input fixture](../tests/fixtures/comparisons/swap-quantlib-inputs.json)
contains complete terms and no expected outputs. Its SHA256 is
`ba61375e251884208f70585cb0aeac034324016cc91623ec7abd0e0020b2211a`.

| Cases | Economics exercised |
|---|---|
| IRS seasoned, receiver and forward | Three-month indexed coupons, independent coupon/payment calendars, actual eligible resets, separate discount/projection curves and trade direction. |
| IRS amortizing/stepped | Coupon-boundary notional, fixed-rate, spread and gearing changes with published and projected observations. |
| IRS negative | Actual negative history, projection and discount rates; negative coupons remain allowed. |
| OIS seasoned and amortizing/stepped | Actual holiday/day weights, current coupon accrued, future compounding and coupon-boundary economic changes. |
| OIS lookback, observation shift and lockout | Explicit two-business-day lookback, shifted weights and three-business-day reuse, on the pinned observation calendar. |

All ten use ACT/360 coupons, unrounded cash, quarterly aligned dates and
two-business-day payment delay. The calendars are independently supplied
synthetic resources at version 1, with Saturday/Sunday weekends and explicit
holiday dates. Final, varying fixing observations include publication/known-at
timestamps and revision 1. Publication is declared at 08:00 UTC; valuations use
an explicit end-of-day information cutoff. Future records are not loaded as
known fixings into QuantLib.

Separate discount and index curves use their original exact versions, actual
dates, ACT/365F linear continuous zeros and disabled extrapolation. Their
reference date precedes valuation, so payment discount ratios must normalize
at valuation. There are no principal exchanges. Rates are annualized decimals;
cash/PV/accrued are absolute USD units at initial notional 1,000,000.

The IRS quotation interval is explicitly `IndexTenor` with three months. This
does not reinterpret the earlier monthly `ResetInterval` fixture as a standard
three-month index coupon. Stubs, independent multiple resets within a coupon,
intracoupon profile changes, rounding, contextual day counts, calendars outside
this supplied suite and other host/model routes are outside this comparator
scope. The broader native/manual tests remain relevant but do not supply their
missing external comparisons.

The QuantLib path builds its schedules from original terms and its bespoke
calendars, then constructs real `FixedRateCoupon`, indexed `IborCoupon` and
`OvernightIndexedCoupon` objects. It uses QuantLib index forecasts, historical
fixings, coupon accrued and `DiscountingSwapEngine`. It reads no Convex or
manual expected output to calculate its reference. The original full cash and
par results are compared only after both engine runs succeed.

API mapping is based on the pinned primary
[coupon bindings](https://github.com/lballabio/QuantLib-SWIG/blob/v1.40/SWIG/cashflows.i),
[swap engine bindings](https://github.com/lballabio/QuantLib-SWIG/blob/v1.40/SWIG/swap.i)
and [zero-curve bindings](https://github.com/lballabio/QuantLib-SWIG/blob/v1.40/SWIG/zerocurve.i).
The actual pinned runtime executed these bindings successfully. Recaptures must
resolve API or economic differences without changing the declared bounds.

## Evidence and closure

Before any comparison run, this suite declares absolute currency tolerance
`1e-7`, annualized rate tolerance `1e-12`, discount-ratio and year-fraction
tolerance `1e-13`, zero relative tolerance and exact date/identity matches.
There is no percentage or scale-based relaxation. The saved report includes
every mismatch and maximum error by unit. Existing numerical artifacts,
tolerances and solver defaults remain unchanged.

A run preserves the exact inputs, QuantLib/native raw stdout/stderr, actual
commands and clocks, Rust/Cargo version outputs, Cargo build record, executable
SHA256, build features/profile/flags, reviewed calculation-source hashes,
installed QuantLib payload hashes and artifact manifest. Replay checks those
bytes and bindings against the unchanged source/protocol, rejects incomplete
execution, and recomputes the comparisons offline. SHA256 bindings detect
changes; they are not independent human authentication of execution records.

The comparison tool always states `accepted_task: false` and `certified: false`;
it does not award acceptance. The reviewed task outcome combines the successful
external capture/offline replay with the preserved native AC1/AC3 and required
checks before adding CVX-019 to the harness engineering completion set. Existing
2,154 mandatory library records remain Planned. Do not replace mismatching
captures, relabel manual oracles, remove cases or widen bounds. Existing pinned
quantitative changes require independent review under the
[assurance policy](ASSURANCE.md). Library, market/model, host, operational and
latency release gates remain separate work.
