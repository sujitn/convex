# Offline quantitative comparison

CVX-040 adds comparison infrastructure over the existing pure library. It
preserves the existing `reconcile.py`, `ql_bench.py`, bond book, QuantLib
reference JSON, captured CSVs and reconciliation workflow. Their original
thresholds are unchanged. No comparator is a production pricing dependency.

The retained pack runs 70 day-count cases and 42 fixed-rate bonds: 392
measures in total. The current replay passes 287 and fails 105, across 21
30/360 bonds. Those differences remain enforced failures. They require a
bond discount-time/convention study and repair in the owning construction,
analytics and bond certification tasks. Matching retained values is not an
independent model approval.

## Run locally

Python 3.11+ needs no third-party package. Use the repository's pinned Rust
toolchain. Run from the repository root:

```sh
python3 reconciliation/comparison.py check
python3 -m unittest discover -s reconciliation -p 'test_comparison.py'
cargo build --locked -p convex-analytics --example offline_compare
python3 reconciliation/comparison.py run \
  --candidate-binary target/debug/examples/offline_compare \
  --report /tmp/convex-comparison.json
```

If `CARGO_TARGET_DIR` is set, use that directory for the executable path.
Building requires Cargo dependencies, which can be preloaded and built with
`--offline`. Fixture replay needs no market download, vendor session,
QuantLib installation, server, database or MQ. The candidate executable is
a trusted, reviewed local test tool; the runner is not an executable sandbox.

Exit 0 means every requested measure matched; exit 1 means a comparison
failed; exit 2 means invalid provenance/input/output or a process failure.
The full retained replay currently exits **1**. CI uploads the report on
failure and does not ignore this exit. The existing live QuantLib rebuild
workflow remains separately enabled.

The report includes expected/actual values, signed deltas, units, applied
absolute-plus-relative bounds, classifications, context hashes, all source
artifact pins, comparator capture status, candidate binary/output hashes,
source commit, dirty paths and source-file fingerprints. It also writes a
normalized `.candidate.json` beside the report, for this replay:

```sh
python3 reconciliation/comparison.py run \
  --candidate-results /tmp/convex-comparison.candidate.json \
  --report /tmp/convex-comparison-replay.json
```

A source-tree hash is not a binary attestation. Retain the actual build
receipt to bind the executable to its sources. Invalid new output replaces
an older pass report with a failure report when the output location is writable;
a failed report write also rejects the run and prints its error. Pricing uses only the supplied
context; report creation time never enters a pricing calculation.

## Source and qualification records

[The catalog](../tests/fixtures/comparisons/catalog.json) pins every admitted
pack, pending template and [version/license research record](comparator-records.json).

| Reference | Pin/selection | Current qualification |
|---|---|---|
| QuantLib | 1.40; release commit `06edddbdb85f006c43a449b5fc846a00c67477f3`; existing generator and output hashes | Retained outputs; original wheel/compiler/runtime/source capture is incomplete |
| ORE | 1.8.17.0; commit `3d75a69087911a7e2cf1e6882da7bc27135c0762` | Release identity selected; no source archive, submodule/binary capture or execution |
| ISDA | Model 1.8.3; USD RFR grid trade date 2022-06-21 with NYM calendar update | Version/grid selected; no model archive, numerical grid capture or execution |
| CME | SOFR educational numerical extract inspected 2026-10-04 | Document only; no native rate-futures product comparison |

The release and license identities were checked against primary sources:
[QuantLib release](https://github.com/lballabio/QuantLib/releases/tag/v1.40)
and [license](https://www.quantlib.org/license.shtml),
[ORE release](https://github.com/OpenSourceRisk/Engine/releases/tag/1.8.17.0)
and [license description](https://opensourcerisk.org/faqs/),
[ISDA version](https://www.cdsmodel.com/download.html),
[license](https://www.cdsmodel.com/cds-disclaimer.html) and
[dated RFR grids](https://www.cdsmodel.com/rfr-test-grids.html).
The small CME quote example comes from
[Understanding SOFR Futures](https://www.cmegroup.com/education/articles-and-reports/understanding-sofr-futures).
The local record is a limited factual extraction, not a hash of the complete
remote pages, licensed source code or a binary. The external engines are not
installed or downloaded by the runner. No model-license agreement is
accepted by these tools. Future comparator builds must capture upstream
license/attribution bytes together with the exact build artifacts.

ORE and QuantLib share ancestry. Use an analytic invariant or a second
independent source to investigate disagreements. A pin proves identity,
not mathematical truth. Every current report has `certified: false`;
the library capability and release gates own certification.

## Manifest contract

[The retained pack](../tests/fixtures/comparisons/quantlib-1.40.json) is the
working schema example. Each case specifies its identity and operation,
complete economic inputs, contractual conventions, valuation/settlement/
known-at dates, synthetic versus historical data, reporting currency,
reference versions, curve/quote/fixing/calendar inputs, rounding policy,
model name/version/settings and explicit approximations. Empty data lists
mean the deterministic operation uses no externally supplied data; the
bond case names the reference's embedded QuantLib calendar version, while
the report pins the candidate's embedded calendar implementation.

For historical inputs each observation records ID, version, effective time,
publication/available time and value. An available time after `known_at`,
an effective time after valuation, a cutoff after valuation or a timezone
without UTC offset rejects the case. Retrospective reference generation
time is not historical market publication time. The retained terms/yields
are synthetic experiments, not historical market observations.

Each metric declares:

| Field | Meaning |
|---|---|
| `unit` | Exact native unit; no implicit percent/bp, cash/per-100 or currency conversion |
| `reference` | Pinned source artifact and exact JSON pointer to the retained expected value |
| `absolute_tolerance`, `relative_tolerance` | Nonnegative finite bounds in this measure's unit |
| `justification` | Why this threshold fits the product/reference/precision |
| `bump` | Explicit factor, size, sign, unit and rebuild rule when a bumped measure is admitted; null for the current analytic measures |

The comparison is `abs(actual - expected) <= absolute + relative * abs(expected)`.
Integer day counts compare exactly; fractions retain `1e-10`; bond prices/
accrued retain `1e-4`, durations `1e-3`, convexity `1e-2` from their original
fixture. These are **not** the tighter reconciliation-book thresholds and
do not certify the R0 target tolerances. Forty-two original BPV values have
no declared BPV threshold/bump policy and remain explicitly uncompared;
no DV01 tolerance or sign conversion is substituted. The other 43 retained
cases remain enumerated as unexecuted suites for owning product/capability
tasks. They cannot silently disappear from coverage.

Unknown fields/units, duplicate JSON/CSV keys or rows, NaN/infinity, missing
artifacts, changed hashes, escaping paths, incomplete versions/settings,
mixed contexts, duplicate/unexpected output cases and missing metric rows
reject the comparison. Missing cases, unsupported operations and explicit
calculation errors fail coverage. An error cannot contain plausible zero
or last-iterate metrics.

The pure adapter exposes `day_count` and `fixed_bond` only, without a new
pricing implementation. Other operations return Unsupported. It rejects
external curve/quote/fixing/calendar payloads and approximations for these
specific paths instead of pretending to consume them. Product tasks extend
the adapter to pass their complete inputs to the corresponding public APIs.

## Mismatch evidence

Matching declared conventions with a numerical miss yields
`implementation_error`, conservatively requiring investigation against
the retained reference. This label alone does not prove that an unqualified
comparator is correct. Different observed conventions without evidence
yield `unexplained_convention_mismatch`.

An exception must identify exact case/field/reference/candidate values,
owner, reason, tracking task, review date, expiry within 90 days, and a
pinned analytic invariant or second independent source with an explanation.
Only exact matching evidence yields `convention_mismatch`; stale or expired
records reject the pack. **Every mismatch still fails**, including an
evidenced one. An exception is a diagnosis, never a whitelist or pass waiver.
Declared owners and evidence are reviewable records, not authenticated
human approval. The retained pack currently contains no exceptions.

## Add a product fixture

Use the [ORE](comparison_templates/ore.json),
[ISDA](comparison_templates/isda.json) or
[exchange](comparison_templates/exchange.json) admission template. These
templates are pending requirements, contain no invented expected values,
and cannot run as fixtures. IRS/OIS tasks CVX-018/019, CDS tasks CVX-020/021
and futures task CVX-022 supply their cases; library coverage/certification
tasks decide required completeness.

1. Capture the exact upstream source/archive and dependency/submodule versions,
   license/attribution, compiled binary hash, compiler/platform, build/run
   arguments, numerical settings and permitted reproducibility environment.
   Store source, binary and executed receipt as distinct artifacts with
   `comparator_source`, `comparator_binary` and `comparator_execution` roles.
2. Preserve original economic/market/calendar/fixing inputs and unmodified
   comparator outputs. Supply available/effective times and an explicit
   historical cutoff. Export workbooks/grids into reviewed numerical data
   without replacing their originals.
3. Normalize explicit columns, units, signs and currencies; put any conversion
   mathematics in the owning product adapter and validate it independently.
4. Build a ready pack, add its hash to the catalog and add all new quantitative
   artifacts to the assurance pins. A `captured` pack requires a comparator
   with the complete runtime and license records; metadata alone cannot
   promote a retained/not-executed reference.
5. Implement standalone candidate operations and negative cases. Compare,
   investigate discrepancies and retain all actual outcomes. Admission and
   matching do not grant an LB0/LB1/R0/R1 certificate.

## External output integration

The normalizer reads captured files only. For ORE it accepts the NPV report
key `#TradeId` and explicitly mapped measure columns; monetary measures
require exact `NPVCurrency`. For ISDA it accepts a **product-exported** CSV
with `case_id`, selected value columns and `currency` when supplied. It does
not assume that all official ISDA zip/workbook formats share a schema.

An ORE mapping file, for example, contains:

```json
{"npv":{"column":"NPV","unit":"currency","currency":"USD"}}
```

```sh
python3 reconciliation/comparison.py normalize --format ore-npv \
  --raw /path/to/captured/npv.csv --mapping /path/to/reviewed/mapping.json \
  --output /tmp/ore-normalized.json
```

The output records the raw hash, ignored columns and `normalization_only`.
It is not a candidate run or a qualified reference. A product adapter joins
these values to captured context/conventions and the original source
artifacts. A candidate protocol has `schema_version: 1`, a producer name/
version/approximations, and cases containing `id`, `context_hash`, `status`,
observed `conventions`, `metrics` (`value` plus `unit`) and `error`. Preserve
signed PVs/upfront values; never invent a missing row or currency.

## Review and CI

Run both structural/tolerance conservation and assurance impact checks:

```sh
python3 reconciliation/comparison.py check --base <actual-pr-base>
python3 tools/assurance/check.py --base <actual-pr-base>
```

Normal checks verify the central pins as well as manifest artifact hashes.
The base guard rejects wider absolute **or** relative bounds, deleted packs,
cases or measures, changed units/operations and discarded retained coverage.
Fixture edits require a version increment. Pending product requirements
can be promoted only by admitting a captured pack with that requirement's
identity; removing a template cannot hide unimplemented products.

The assurance check additionally requires exact candidate impact records
and independent quantitative review for changing/refreshed existing pins,
even if the new hash and version match. Tightening a tolerance still needs
that review. No tool creates an approval or automatically refreshes pins.
Repository declarations cannot authenticate human reviewers or configure
GitHub branch protection. Administrators must require the checks and reviews
on protected branches.

The initial CVX-040 additions preserve all 47 existing quantitative pins;
their source-tree engineering review does not attest an external numerical
approval. Before a whole-extension PR, review against its actual base.
CI runs schema/pin checks, mutation tests, the pure adapter build and the
full offline replay. The 105 current misses and G04's synthetic spread
outputs continue to block production/library release readiness.

Bounds: 8 MiB JSON/CSV, 512 MiB per pinned source/binary artifact (streamed
hash), 16 comparator identities, 64 packs, 2,048 cases per pack, 64 measures
per case, 128 source artifacts per pack, 4,096 observations per data list,
and a 30-second candidate process timeout. The native paths limit dates to
years 1–9999 and 150 years of span. These are test-tool resource bounds,
not production latency/capacity targets. A trusted child may write to its
temporary output files until timeout; no OS disk/resource isolation is claimed.
