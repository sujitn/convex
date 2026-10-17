# Core position and reference updates

Run from the repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/core_book_workflow.py
```

This builds the existing server/importer, starts an owned local feed and the
existing analytics worker, checks complete book revisions, then stops its
processes. `--binary-dir /absolute/path/to/debug` reuses builds. The synthetic
book contains a bond, CDS, IRS, OIS and two rate futures with five hedge factors.

The example consumes an actual supplied fill and its pending-position change,
then a declared future holiday correction in a new calendar revision. It checks
native pricing/risk, the same closing positions in the constrained hedge proposal,
complete causal P&L reconciliation, rejected inconsistent releases, stream gaps,
rollback rejection, an interrupted committed result, and worker/server restart.
Saved result bytes, caches and saved-input recomputation must remain identical.

Its financial inputs are explicitly historical replay. The existing offline
importer stores their original reference transactions at their captured cutoffs;
the live HTTP reference writer is separately exercised with actual UTC commit
times, a lost acknowledgement, identical retry and a rejected CAS conflict.
That later reference revision stays inactive until explicitly released. No live
provider, model, execution or latency qualification is inferred.

## Publish a complete book revision

Core positions live in the complete native P&L definition stored through the
existing `ReferenceRepository` port. Include all quantities, pending quantities,
legal identities, fills, terms, lifecycle/cash facts and reference pins needed by
the supplied causal history. The same definition revision holds the named native
risk requests and shared reference inputs. A producer resolves its provider data
into these existing native contracts; the worker performs no vendor mapping.

1. Commit the complete replacement `CoreDefinition` through
   `POST /api/v1/reference/transactions`, using the existing `ReferenceCommand`,
   expected version and matching `Idempotency-Key` header. Related master changes
   can share that atomic reference transaction. A conflict commits nothing.
2. Confirm the durable receipt. After an uncertain outcome, read
   `GET /api/v1/reference/commits/<key>` or retry the identical command. Preserve
   the returned version and original commit timestamp.
3. Publish one complete normalized `book` record on the same ordered core input
   stream, naming that commit and exact definition revision. Complete stream
   images include the current book release alongside the subscribed market data.

The existing portfolio CRUD fixture interface is separate from this durable core
book. To update this mixed-instrument workflow, publish the complete core definition
and release above. No new server route, position database or event ledger is added.

## Subscription and record

Opt in by adding one book identity to the existing subscription:

```json
{"schema_version":1,"curves":[{"curve_id":"USD.OIS","source":"rates-feed"}],"fixings":[],"book":"desk.usd"}
```

The existing [market stream contract](MARKET-DATA-STREAM.md) still owns ordering,
source subscriptions, gaps, reconnects and full-image recovery. A `book` record
uses the same normalized envelope:

```json
{"schema_version":1,"kind":"book","payload":{"id":"desk.usd","version":2,"reference_commit":"desk.usd.change.2","workflow":{},"position_step":2}}
```

This illustrates the envelope: `workflow` must contain a complete valid
[analytics workflow configuration](STREAMING-ANALYTICS.md#workflow-configuration),
including its exact `plan.definition` pin and explicit native hedge policy.
`releases()` in the runnable example constructs complete working records.
The workflow ID and reference-definition ID remain those of the boot configuration.
Each source book version is a positive unsigned64 value; it is distinct from
the server's reference version and from the market stream sequence.

Each image contains exactly one release for the subscribed book. An update can
contain a book alone, market inputs alone, or both. Books are bounded to the
existing2MiB/65,536-node/depth64 transport limits. There is no position delta
language or financial script. The producer's latest full image must retain the
last complete release, including after reconnect.

The worker verifies the stored reference receipt and exact calculation record
before activation. It reads the immutable named revision even if another, newer
reference revision exists. Updating a reference record alone cannot silently
alter an already selected calculation's inputs.

## One consistent closing state

`position_step` selects the final `after` point in the supplied native P&L path
(index0–15). The complete P&L window must end there. Every configured holding must
appear exactly once, and its current risk request must match that point's full
instrument/reference identity, product, reference quantity and curve-set binding.
The reporting currency and legal entity must agree with the explicit hedge policy.

The worker copies current and pending quantities, position manifest, settlement
and explicit reporting FX pins from that same point into the hedge template.
It then attaches the newly calculated native risk vectors from the same saved
snapshot. Candidate eligibility, costs, limits, lots, factors and numerical
settings remain explicit policy inputs. Missing FX, incomplete coverage or a
mixed position/product/reference revision blocks completion.

All prices, factor risk, proposal constraints, lifecycle cash and causal P&L are
still calculated by the native library. Positions require their actual complete
fill/history facts; a snapshot difference is not a manufactured fill. Reference
corrections and contractual amendments retain their declared native semantics
and history. The worker neither invents events nor infers attribution by
subtracting unrelated reports. Earlier saved reports remain available by ID.

## Recovery and operation

Run the same `tools/streaming_analytics.py` command with the book-enabled
subscription, its boot workflow file and one local `--state` path. Adding book
mode changes the boot configuration identity: retain the prior receipt and use
an explicitly initialized state path with the intended starting version floors.
Market-only configurations and their existing schema1 receipts remain supported.

The same atomic local receipt uses schema2 for an admitted book, retaining its
full release/configuration, source-version floors and last complete bundle.
It also retains the highest observed book revision and its content hash, including
rejected releases, so restart cannot bypass a pending correction by reverting to
an older book. A corrected payload needs a new source book version.
It never restores readiness. After restart, the worker requires a complete image,
rechecks the reference receipt and book bindings, and resumes partial work through
existing durable snapshots/results. Its boot file need not be rewritten after
each accepted revision. Market version floors survive a book change as well.

A skipped stream frame cannot activate its release. Lower book/reference versions
cannot replace the admitted state; conflicting content at the same book version
stops the worker. An inconsistent pending release remains blocked on subsequent
market updates until a valid later release arrives. Source/receipt freshness,
native cutoff/entitlement/history checks, result expiry and build matching from
the existing worker still apply. The source must commit reference changes before
publishing the release; an uncommitted receipt remains unavailable.

Ready events include the source book revision/hash, reference commit/definition pin
and closing position manifest alongside native snapshot/result receipts. Consumers
must continue enforcing `valid_until` and output-stream liveness. Proposals have
no execution authority. One serial Unix worker owns its local receipt; distributed
ownership and measured product-specific throughput/latency remain further work.
