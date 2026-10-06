# CVX-019 external swap comparison

CVX-019 AC2 is still open. The [comparison runner](../tests/scripts/swap_quantlib_compare.py)
and [native exporter](../crates/convex/examples/swap_comparison.rs) are prepared
for an actual QuantLib 1.40 run. The native exporter has executed; the QuantLib
coupon path has not executed on this machine because the pinned runtime download
fails. Python control tests and native outputs do not close this acceptance gate.
The earlier [twenty manual references](SWAP-VALUATION.md) remain separate evidence.

## Start locally

From the repository root on `codex/production-readiness`:

```sh
python3 tests/scripts/swap_quantlib_compare.py check
python3 -m unittest discover -s tests/scripts -p test_swap_quantlib_compare.py -v
cargo run --locked -p convex --example swap_comparison
```

These commands check the immutable input bytes and rejection controls, then
print native outputs for all ten contracts. The exporter also runs with
`--no-default-features` and `--features schemars`. They do not import QuantLib
or fabricate an external reference. There is no new production dependency,
server route or adapter requirement.

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
including the Python wrapper, native extension, metadata and license, **before
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
Source inspection alone is not executed evidence; API or economic differences
must be resolved on the actual runtime without changing the declared bounds.

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

The engineering report always states `accepted_task: false` and
`certified: false`. After a real successful run, review the coupon dates,
fixing/quotation lineage, all cash/PV/accrued/par differences and recorded
limitations, retain the capture as immutable task evidence, and run the
CVX-019 requirements/harness checks before accepting AC2. Do not silently
replace a mismatching capture, relabel a manual oracle, remove cases or widen
tolerances. Existing source/evidence pin changes must follow the repository's
change-review policy. Library, market/model, host, operational and latency
release gates remain separate work.
