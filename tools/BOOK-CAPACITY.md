# Larger core books

Run the complete synthetic 36-trade book:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_capacity_workflow.py
```

It builds the existing server/importer and a small native comparison executable.
It uses temporary local redb/file stores and the existing ordered stream worker.
`--lots 10 --receipts 0` selects 60 distinct trades over the same six economic
templates. `--binary-dir PATH` reuses a build directory containing both server
binaries and `examples/book_capacity_native`. These are synthetic mechanics and
capacity checks, not a live book distribution or latency certificate.

The example covers bond, CDS, stepped IRS, seasoned OIS, overnight and term
futures. It checks native pricing/risk/P&L/hedge parity, a real curve revision,
an actual partial coupon receipt, an exact opening checkpoint with history
sharing, loss of a committed hedge acknowledgment, worker/server restart, old
result bytes, cached recovery and saved-input recomputation. Matured bonds keep
their actual outstanding claims and zero fresh instrument risk; the live IRS
must reprice after the discount update. All calculations in a ready release use
one captured input set. A failed calculation or stale source withholds readiness.

## Select the saved capacity profile

Add one explicit field to the existing versioned calculation definition:

```json
{
  "reference_inputs": {
    "core_capacity": "book_128"
  }
}
```

Retain all the normal reference fields alongside it. This is an operational
selection, captured in the same reference commit and immutable snapshot as the
full financial terms. It is stripped before native reference reconstruction.
Unknown values, null and arbitrary numeric budgets reject. Old definitions
without the field keep their original admission and native JSON limits.

| Bound | Original definition | `book_128` definition |
|---|---:|---:|
| Named calculations | 32 | 130 |
| Definition bytes | 1 MiB | 8 MiB |
| Definition JSON values | 65,536 | 524,288 |
| Materialized native request JSON values | 32,768 | 131,072 |
| Materialized native request bytes | 2 MiB | 2 MiB |
| Stored native result bytes | 4 MiB | 16 MiB |

130 names allow up to 128 risk bindings plus P&L and a named hedge calculation.
These are independent ceilings. A large or long-history combination can reach
the byte, node, work or result bound earlier; the profile does not promise that
every 128-trade book fits. Native P&L still has at most 128 positions, 16 causes
per window, 33 points and 4,224 product evaluations. Events remain bounded at
4,096 per lifecycle, JSON containers at 4,096 items, depth at 64 and text at
8,192 bytes. No solver tolerance, conditioning limit, calculation method or
financial default is relaxed. Full native lifecycle replay remains.

The shared native parser keeps duplicate-field, exact-decimal, lexical-number,
depth, text, container and byte guards. `execute_book_json` admits only book
P&L, graph/lifecycle risk and hedge operations with the larger node budget.
Ordinary `execute_json`, C/host entry points and unrelated HTTP routes retain
their existing transport/admission bounds.

Existing reference writes accept up to 8 MiB; captured reference/core reads have
a bounded 32 MiB transport inspection budget. Authentication, quotas, both
authorization checks and complete nested scope extraction remain. Book scans
bound total work and count distinct resource identities, so repeated source
pins cannot exhaust the identity allowance. They retain the 1,024 distinct
resource ceiling. No resource is skipped to admit a larger book.

The existing offline importer accepts a wrapper of at most 16 MiB only when
it contains an explicit `book_128` calculation definition. Ordinary imports
retain their 2 MiB limit. The definition and all source records still undergo
their own admission checks before the store is opened.

## Same-security legal lots

The worker uses the explicit native `solve_book` action for the larger profile.
Each legal lot retains its ID, audit pins, quantity, actual lifecycle risk and
owned cash claims. Different lots of one security must have the same security
revision, full fresh reference risk, economic/calculation revisions, methods,
factor bindings, reference units, currency, entity and FX. Per-lot audit record
IDs remain distinct and are retained in the emitted problem. Changed financial
bindings or duplicate lot identities reject.

Current/pending exposures are evaluated for each lot by the existing native
owner. Candidate holding limits use the checked Decimal sum of all current and
pending quantities in that security. Candidate identities remain unique, and a
held-security candidate must match an actual complete holding reference. Actual
default/maturity exclusions and complete issuer coverage still apply. A scoped
book supplies the captured issuer directory; an unscoped book supplies explicit
`scope: null`. Existing ordinary solve actions retain their alias rejection.
The proposal remains an instrument quantity; execution and allocation to lots
are separate responsibilities.

The example declares its own synthetic inventory capacities: 30 million face
for the bond, 10 million notional for IRS/CDS and 100 futures contracts. Trade,
liquidity, whole-contract, factor, cost and numerical limits retain the existing
paper policy. These are supplied test inputs, not changed server defaults or
production limits. Native infeasibility checks remain active, including when
separate lots together breach a candidate holding bound.

Larger results record their storage selection and must match the immutable
source definition on cached runs and reads. Old persisted rows omit the new
selection and retain their original byte representation and 4 MiB bound.
The redb adapter bounds the escaped storage wrapper separately; it does not
reserialize or round financial output. The 60-trade partial-cash case produced
4.52 MB of valid native P&L and motivated this explicit storage increase.

No portfolio partitioning, distributed coordinator, new storage interface,
database, message queue, provider integration or persistent cache is introduced.
One ordered publisher/worker and same-build checkpoint replay remain. Live
provider, multi-server, deployment latency and independent model/platform
qualification remain separate from these local checks.
