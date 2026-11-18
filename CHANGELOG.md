# Changelog

Versions before 1.0 can break the API in any minor release. Each entry lists
the changes that matter when you upgrade.

## Unreleased

### Excel add-in

- **Live cells recompute only what a change reaches.** One RTD server
  (`Convex.Rtd`) now backs every live cell. When a bond or curve is rebuilt
  or released, only the live cells whose request names it recompute, and a
  cell updates only when its number changed. The old layer recomputed every
  live cell on any change.
- **Breaking, with no compatibility layer:**
  - `CX.PRICE.LIVE`, `CX.RISK.LIVE`, `CX.SPREAD.LIVE` and `CX.YAS.LIVE`
    return one number. `grid` and `krd` give `#VALUE!` and name the static
    function to use.
  - `CX.RISK.LIVE` drops the key-rate tenors argument and defaults to
    `mod_dur`. `CX.YAS.LIVE` adds a `field` argument (default `ytm`).
  - `CX.PRICE` no longer has a `z_spread` field (it returned `#N/A` for most
    marks); use `CX.SPREAD`. Its grid drops the Z row.
  - The "LIVE poll (ms)" setting is gone. Settings shows Excel's RTD
    throttle instead and can set 250 ms on request.
- Excel-DNA 1.9.0 (was 1.7.0), still on .NET Framework 4.7.2, 64-bit only.
- `CX.DIAG()` reports live topics, recomputes, pushes and the last drain
  time.
- **Live cells from a Convex server.** `CX.LIVE(identifier, measure,
  [field])` follows the server's published result for an instrument over one
  WebSocket per Excel process, and updates when the server publishes.
  - Fields: `value`, `quality`, `status`, `revision`, `calculated_at`,
    `snapshot_id`, `configuration_version`.
  - On disconnect values stay with quality `Stale`, and the add-in
    reconnects and resubscribes by itself.
  - A CUSIP or ISIN is looked up through `POST /api/v1/reference/resolve`;
    any other identifier is the server's instrument id.
  - `#N/A` with a `CX.LASTERROR` reason when there is nothing to show,
    including `FORBIDDEN`, `RESULT_TOO_LARGE` and the 256-instrument session
    limit.
- `CX.LIVE.GRID` (cached measures of a followed instrument), `CX.SNAP` (one
  read) and `CX.STATUS` (connection, user, runtime, last message, counts,
  calculation mode).
- Sign-in: single sign-on (OIDC authorization code with PKCE over a loopback
  redirect, renewed at 80% of the token's lifetime without a gap in values),
  a pasted token for development, or none for a local development server.
  Ribbon **Server** group: connection status, Sign in, Sign out and the
  server settings (`%APPDATA%\Convex\server.json`; tokens are never saved).
- **Book limits in cells.** `CX.LIMIT(book, limit, [field])` shows a limit
  of a book on the server: `status` by default (`within`, `warning`,
  `unknown` or `breach`), or `value`, `utilisation`, `lower`, `upper`,
  `warning_fraction`, `member`, `measure`, `reason`, `previous`,
  `evaluated_at`, `record_id`, `quality`, `book_version` or
  `limit_set_version`.
  - Books follow on the same connection. Each book is read once followed and
    after a reconnect, then changes as the server records a status change.
    The server records a limit only when its status changes, so `value` and
    `utilisation` are as of `evaluated_at`.
  - On disconnect limits stay with quality `Stale`. One connection follows
    at most 64 books. `#N/A` with a `CX.LASTERROR` reason when there is
    nothing to show (no limit set, no such limit, `FORBIDDEN`, no limit
    monitor).
  - `CX.STATUS("books")` counts the books followed.
- **Pre-trade limit check.** Ribbon **Limit Check** checks proposed trades
  (signed quantities by instrument id, CUSIP or ISIN, entered or read from
  the selection) against a book's limits through
  `POST /api/v1/books/{book_id}/limits/check`, and shows each limit now and
  after the trades. It stores nothing. **Stamp CX.LIMIT** writes live limit
  cells for the book.
- **The release package loads.** The zip now holds the packed
  `Convex.Excel64.xll` and `convex_ffi.dll`. It used to hold the unpacked
  loader without two DLLs that loader needs. Excel refuses a downloaded add-in,
  so unblock the files first (see the zip's `README.txt`).
- The add-in carries the workspace version: `CX.DIAG()` and **About** show it
  (`CX.DIAG()` showed 1.0.0.0). Microsoft 365 Apps, 64-bit, is the stated
  minimum, not Excel 2016.
- **A log file and connection history.** The add-in writes
  `%LOCALAPPDATA%\Convex\Excel\logs\convex-excel.log` (10 MB, five files) with
  its start, Excel's exit and every connection event, with tokens removed. The
  ribbon **Error Log** gains a **Connection** tab with the last 500 events.
- `CX.DIAG()` adds the server link: connection, session, instruments, books,
  topics, frames received, frame-to-topic latency (p50 and p99, before Excel's
  throttle), the age of the last message, and the log file. Existing rows keep
  their positions.
- **New Bond** and **New Curve** no longer hang or crash Excel. **Build** and
  **Build + paste handle** asked Excel for the calling cell, which is valid only
  during a cell calculation.
- Form labels no longer wrap and clip at higher display scales: each label
  column now fits its widest label.
- **An installer.** Each release adds a per-user MSI that needs no administrator
  rights, installs under `%LOCALAPPDATA%\Convex\Excel\<version>\` and registers
  the add-in with Excel. It upgrades in place, refuses a downgrade, and refuses
  to run while Excel is open. It is not code-signed yet. See
  [Deploying the Excel add-in](docs/EXCEL-DEPLOYMENT.md).
- **An update check.** Once a day the add-in reads where GitHub's
  `releases/latest` redirects to. When a newer version is out, **Update**
  appears in the ribbon's Help group and opens the release page. It never
  downloads anything. Settings, the MSI property `UPDATECHECK=0` or the policy
  value `HKCU\Software\Policies\Convex\Excel\UpdateCheck=0` turn it off, and
  the feed can point at an internal mirror. `CX.DIAG()` gains an "Update check"
  row at the end.

### convex-analytics

- **Breaking:** `hedge_cost_bps` is removed, with its re-exports from `risk`,
  `risk::hedging` and the `convex` facade; use `HeuristicCostFeed.cost_bps`.
- **Breaking:** the `cashflows::IrregularStubType` alias is removed; use
  `StubType`. The `cashflows::irregular` and `cashflows::schedule` modules, which
  only re-exported `convex_bonds` types, are removed too; the types stay at
  `cashflows::`.

### convex-portfolio

- **Breaking:** `KeyRateProfile::to_key_rate_durations` is removed.

### convex-ffi

- New C export `convex_changes_since(cursor)`: the names and handles whose
  resolution changed after a registry generation, from a 4,096-entry log.

### convex-server

- **A pricing rule or benchmark set with an end date is not in force after it.** A
  version with `effective.until`, committed through `POST /api/v1/reference/transactions`
  or the import, ends the rule or set at that time, which may be later than the commit.
  After it, a bond whose rule ended is quoted by price; a bond whose rule names an ended
  set is refused, naming the set, whether asked by level or by price; a compile refuses a
  rule naming it; and a Live runtime applies the end at its next activation.
  **Breaking:** an ended version used to fall back to the version before it, and a
  version recorded later for a past interval now ends every version after it, so a
  correction of history restates the current version after it.
- **`POST /api/v1/quote` and `/batch/price` price under the stored rule.** Each takes
  `level` in place of `market_price`. Inline terms equal to the bond's stored terms at
  `known_at` are priced as `GET /api/v1/quotes/:id` prices the stored bond, with the same
  `pricing_rule` report (and, on `POST`, the same headers). A batch reads each benchmark
  set once. **Breaking:**
  - a `market_price` request or item for a bond whose rule names a set now reads the
    set, and is refused, naming the tenor, when a benchmark has no usable Live price;
  - its report names the set's securities, so a caller not entitled to them is refused
    (403), and a batch holding such an item is refused whole;
  - a bond whose rule quotes it by a level no longer takes the configuration's
    percent-of-price bid/ask width, sent a level or a price;
  - a callable's or note's quote on these routes carries its rule's report.

  With supplied curves, or with no configuration and no runtime, no rule is read and a
  `level` is refused.
- **Breaking: the `/api/v1/portfolios*` alias over books is removed.** Write a
  book with `PUT /api/v1/books/:book_id`, and retire one with
  `POST /api/v1/books/:book_id/retire`. The alias's batch create has no
  replacement.
- **Retiring a book has effect.** `POST /api/v1/books/:book_id/retire`
  (`Idempotency-Key` and `If-Match` required) closes the current snapshot at
  the commit time. A retired book is refused by analytics by book, the limit
  status and pre-trade check, and limit proposals, approvals and
  acknowledgements (409 `BOOK_RETIRED`); a set that clears its limits is still
  taken. The limit monitor stops watching it, and a configuration compiled at
  a cutoff after the retirement (known by its `known_at`, effective at its
  `as_of`) does not find it. A snapshot written over the retired version
  revives it. A snapshot that has not started yet cannot be retired
  (409 `BOOK_NOT_STARTED`).
- **Breaking:** a reference transaction or import that writes a book is
  refused (403 `BOOKS_ROUTE_REQUIRED`); write books through `/api/v1/books`.
  One that also writes a limit gets `LIMITS_ROUTE_REQUIRED`, checked first.
- **Breaking:** the `websocket_port` setting and `CONVEX_WEBSOCKET_PORT` are
  removed. WebSockets share the HTTP listener, and a file or environment that
  still sets either fails at startup as unknown.
- **A Live runtime values up to 4,096 instruments, and one session follows them all.**
  A session's valuation cap is 4,096 (was 256). A
  `subscribe_valuations` or `unsubscribe_valuations` message lists at most 256
  instruments; more is refused with `at most 256 instruments a message`, so a session
  following 4,096 sends 16. The limit monitor keeps priced terms for twice the
  instruments a runtime values (was twice 256). Captured and Replay runtimes still value
  at most 256.
- **Breaking: `GET /api/v1/runtime/results` returns pages.** It returns
  `{results, next_after, more}`: at most `limit` results (256 unless asked, at most
  1,024) in instrument ID order, after the instrument `after`. Read the next page with
  `?after=<next_after>` while `more` is true. Pages are read at different times, so keep
  the newer record by `(runtime_id, revision)`. A bad `limit` or `after`, or another
  query parameter, is 400. The admin app and the demo frontend read every page.
- **The results route works outside local development at any width.** The access layer
  inspected its output and refused a response naming more than 1,024 resources, which a
  runtime of more than about 170 instruments passes. The route already requires wildcard
  grants on instruments, curves and sources, so its output is no longer inspected.
- **Readers no longer copy the runtime's results.** A valuation subscription, the limit
  monitor, limit status and the pre-trade check read only the instruments they need,
  and every WebSocket session shares each streamed record.
- **A valuation subscription sends current results first.** After
  `subscribe_valuations`, the session sends the current result of each listed
  instrument that has one, then the `subscribed` acknowledgement. A client no
  longer needs to read `GET /api/v1/runtime/results` to seed. Subscribing again
  to instruments already followed resends their results, which is how a client
  recovers after `RESYNC_REQUIRED`.
- **One session can follow every instrument and every limited book.** The
  per-session topic quota is now 4,160 by default and at most (was 256): 4,096
  instruments, as many as a Live runtime values, plus the 64 books the limit monitor
  watches. Before, a session following 256 instruments could follow no book.
- Clients should still keep the newer valuation by `(runtime_id, revision)`: a
  result sent at subscription can be newer than one already on its way.
- **A result too large for one 64 KiB message no longer closes the session.** It
  arrives as a `RESULT_TOO_LARGE` error naming the instrument, `runtime_id` and
  `revision`; read the result from `GET /api/v1/runtime/results`.
- The `subscribed` acknowledgement's `count` is the number of distinct instruments
  listed; a repeated id counts once.
- **Listing a book a session already follows no longer counts against its 64.**
  `subscribe_limits` used to refuse when the books followed plus the books listed
  passed 64, so listing a followed book again could be refused. It now counts
  distinct new books, as valuations do, and `subscribed.count` is the distinct
  books listed.
- **A book's recorded limit status changes can be read.**
  `GET /api/v1/books/{book_id}/limits/records?after=&limit=` returns the changes
  the limit monitor recorded for a book, oldest first, each as its
  `limit_status` message carried it, with a cursor for the next page. A reader
  following new records passes the last record id it has. It needs what status
  needs, and a record naming an instrument the caller cannot read is left out
  and counted. Without a limit monitor it answers 503.
- **A breached limit can be acknowledged.**
  `POST /api/v1/books/{book_id}/limits/acknowledgements` records that a risk
  manager has seen a limit in breach or unknown, until an expiry at most seven
  days ahead, with a note. It changes no bound. Status, the pre-trade check,
  records and `limit_status` messages carry the acknowledgement in force on each
  `breach` or `unknown` limit, as `acknowledgement`. It ends at its expiry, when
  withdrawn (`.../acknowledgements/withdraw`), or when a set version changes the
  limit. The current acknowledgements, each version and the history are read
  under `.../acknowledgements`. New codes: 409 `LIMIT_NOT_ACKNOWLEDGEABLE`,
  404 `LIMIT_NOT_FOUND` and `LIMIT_ACKNOWLEDGEMENT_NOT_FOUND`, 422
  `LIMIT_RECORD_MISMATCH`.
- **A clear hold on recorded limit status changes.** The limit monitor records a change
  to a more severe status at once, and one to a less severe status only once the limit has
  stayed less severe for `limits.clear_hold_seconds` (`CONVEX_LIMITS_CLEAR_HOLD_SECONDS`,
  default 5, 0 to 300; 0 records every change at once, as before). A value hovering at a
  bound records its breach once rather than at every crossing. Status and the pre-trade
  check are unchanged; `limit_status` messages, the records and the breach and unknown
  gauges show a breach until it has stayed clear for the hold. **Breaking:**
  `ServerConfig` gains `limits` (`LimitSettings`), so a struct literal of it needs the
  field or `..Default::default()`.
- **Limit records can be pruned after a retention period.**
  `limits.record_retention_days` (`CONVEX_LIMITS_RECORD_RETENTION_DAYS`, 8 to 3,650; unset
  by default, which keeps every record, as before). The limit monitor prunes
  `limit_status` records older than the period at start, before it records any book, and
  then hourly. It keeps every record an acknowledgement in force names, each watched
  limit's latest record since the start (hourly), and the store's last entry, so ids
  continue. A store at its 1,000,000-entry cap with a period set records again once some
  records are older than the period. `convex_audit_pruned_total` counts the removals.
  **Breaking:** `LimitSettings` gains `record_retention_days`, so a struct literal of it
  needs the field or `..Default::default()`.
- **One limit's records can be read.** `limit_id` on
  `GET /api/v1/books/{book_id}/limits/records` returns only that limit's records. A
  request reads at most 10,000 of the book's records, and `next_after` is the last one it
  examined, so a page can hold no records with `more` true; pass `next_after` on. Another
  limit's records are skipped and not counted in `withheld`.
- **A limit can carry a temporary excess.** `excess` on a limit in a set gives wider
  bounds until an expiry at most thirty days after the proposal, with a reason, through
  the set's two-subject proposal. Until it expires the limit is judged against the excess
  bounds, and status, the pre-trade check, records and `limit_status` messages show them
  as `lower` and `upper`, with `excess` naming the limit's own bounds and the expiry. Its
  approval and its expiry are recorded status changes, and the monitor wakes for an expiry
  with no tick or commit. New code: 409 `LIMIT_EXCESS_EXPIRED`.
- **Bonds can be quoted by a level.** A `pricing_rule` reference resource quotes a
  stored fixed bullet by `price`, `yield`, `z_spread` or `benchmark_spread` to one tenor
  of a `benchmark_set` of Treasuries. `GET /api/v1/quotes/{id}?level=` prices it from a
  level at request time. A Live runtime prices it from its own quote under the rule: the
  level is a `mid_yield` or `mid_spread` quote, a `manual` one is the desk's mark, and a
  tick on a benchmark reprices exactly the bonds quoted to it. `g_spread_mid` is
  published for a bullet whose rule names a set it matures within.

### convex-engine

- **One request-time stage for every route.** `PricingEngine::rule_in_force` reads the
  set a bond's rule pin names, once per request through `BenchmarkSetReads`, and
  `price_in_force` prices under it with the engine's shared benchmark yields. They
  replace `pricing_rule_in_force` and `benchmark_yield_cache`. `rule_for_inline_terms`
  gives the stored rule for inline terms equal to the stored ones.
  - **Breaking:**
    - `PricingRuleInForce` holds the rule's pin (`ResolvedPricingRule`) and an
      `Arc<BenchmarkSetQuotes>`;
    - `reference_at_for_pricing` and `stored_terms` return the `ResolvedBondReference`,
      with the rule pin, so the rule is read once with the reference;
    - `price_under_rule` no longer checks the rule's instrument, since the pin comes
      from the bond's own reference.
- **Breaking:** `BuiltCurve::to_points`, the older day/zero graph format, is removed,
  and with it `EngineError::CurveBuildError`, which only it returned.
- **Breaking: `MAX_RUNTIME_INSTRUMENTS` is 4,096 (was 256),** a Live plan's bound. A
  Captured or Replay plan is refused past 256 with its own reason, since each of its
  events stores every result in the checkpoint. The runtime's valuation cache is sized to its plan (it held 256, so the 257th
  valuation of one snapshot failed). A configuration's dry run sizes its shared cache
  to 4,096 and resolves a Live document within the Live bounds.
- **Breaking:** `SnapshotAssembler::resolve_with_configuration` takes the
  `SnapshotBounds` to resolve within; `resolve` and `assemble` keep the standard ones.
- Plan and checkpoint validation are linear in the instruments (both counted or searched
  inside the per-instrument loop), and so is a dry run's per-instrument work.
- **Breaking:** `PricingEngine::subscribe_results` streams shared records,
  `Arc<PublishedValuation>`, and holds two records per instrument the runtime can value
  (8,192, was 256).
- `PricingEngine::runtime_results_of` reads the current records of the listed
  instruments, and `runtime_results_page` a page in instrument ID order
  (`RuntimeResultsPage`). Both share the runtime's records; `runtime_results` still
  copies every one.
- **Breaking:** `limits::evaluate_limits` takes the time of the evaluation, which decides
  whether a temporary excess applies. It still reads no clock.
- **Breaking:** `PlanSelection` gains `levels` (`LevelQuoteSource`): where the quotes of
  bonds quoted by a level come from, and how old one may be. The compile quotes such a
  bond by its rule's field and adds each benchmark security its rule's set names, at the
  version the set pins.
- **Breaking:** `level_pricing::price_under_rule` takes a `BenchmarkYieldCache`, which
  solves each benchmark's yield once for each price; `price_in_snapshot` prices a
  snapshot's bond under the rule its reference pins, from the snapshot's benchmark quotes.
- `g_spread_mid` is a runtime measure (`snapshot::G_SPREAD_MEASURE`).

### convex-ports

- **A Live snapshot holds up to 4,096 instruments.** `MarketSnapshot::bounded(content,
  SnapshotBounds::Live)` makes one, identified within `MAX_LIVE_IDENTITY_BYTES`
  (128 MiB) rather than `MAX_SNAPSHOT_BYTES` (16 MiB); its `next` keeps its bounds.
  `MarketSnapshot::new`, deserialization and storage keep 256 instruments and 16 MiB.
  4,096 instruments with every text at its validator's bound write 125.2 MiB with a small
  frame.
- `RuntimeMode::max_instruments` (4,096 Live, 256 Captured or Replay) and
  `RuntimeMode::snapshot_bounds`. A `ConfigurationDocument` holds up to 4,096 instruments
  for a Live runtime; the configuration JSON schema's `maxItems` is 4,096.
- `MAX_CHECKPOINT_RESULTS` (256) names the results and outbox records a stored checkpoint
  holds; its validation finds each outbox record by subject.
- **Breaking:** `LimitDefinition` gains `excess` (`LimitExcess`) and `LimitEvaluation`
  gains `excess` (`ExcessApplied`), so a struct literal of either needs the new field.
  Both are optional on the wire, and stored sets read as before.
- `AuditStore::entity_page(entity_type, entity_id, after, limit)` returns up to
  1,000 of one entity's entries after a cursor, and whether there are more. It
  has a default that scans through `query`, so existing stores still compile.
- **Breaking:** `MasterKey`, `MasterKind` and `MasterDefinition` gain
  `LimitAcknowledgements`, a book's limit acknowledgements
  (`LimitAcknowledgementsDefinition`), so an exhaustive match on them needs a new
  arm.
- `AuditStore::prune(&AuditPrune)` removes up to 4,096 entries of one event type older
  than a timestamp, after a cursor, in one transaction, and returns `Pruned` (how many,
  the next cursor, whether more may remain). It passes over the IDs it is told to keep and
  never removes the log's last entry. The default refuses with `Unsupported`, so existing
  stores still compile and are never pruned. Bounds: `MAX_PRUNE_BATCH` (4,096 removals),
  `MAX_PRUNE_KEEP` (8,192 kept IDs), `MAX_PRUNE_READ` (16,384 entries read a call).
- **Breaking:** `ResolvedBondReference` gains `pricing_rule` (`ResolvedPricingRule`, a
  compact pin of the rule in force), and `SnapshotContent` gains `benchmark_sets`
  (`Arc<[ResolvedBenchmarkSet]>`): each set the references' rules name, once, at most 64
  (`MAX_SNAPSHOT_BENCHMARK_SETS`), shared by every tick of a Live activation.
  `InputSet::quotes` returns an iterator over a bond's own quotes and its benchmarks'.
- `ReferenceRepository::pricing_rule_for_valuation` and `benchmark_set_for_valuation`
  resolve a bond's rule and a set in force; `MarketSnapshot::pricing_rule`,
  `benchmark_set` and `benchmark_securities` read them back; `PricingMethod::quote_field`
  and `reads_discount_curve` give the quote field and curve a method reads.
- **A book, pricing rule or benchmark set version with a bounded end retires it in dated
  reads.** Each kind declares its `VersionSelection` (`MasterKind::version_selection`),
  and `select_master_at` applies it. These three take the newest version started by
  `as_of` (`NewestStarted`), so a dated read after the end finds none, where it used to
  fall back to the version before. Identifier mappings take the newest known assertion
  (`NewestKnown`); other kinds keep the newest version whose interval holds `as_of`
  (`NewestHolding`). `MasterDefinition::kind` gives a definition's kind. A book's interval cannot end
  after the commit that closes it, so `BookDefinition::is_retired` (the interval is
  closed) means it has ended.

### convex-ext-redb

- **Breaking:** `RedbAuditStore::new` returns `Result`. The audit store keeps
  an index by entity, written with each entry.
- **Breaking: old stores are refused, not adopted.**
  - An audit log whose entries do not each have their index key fails at open;
    entries are no longer indexed when the store opens.
  - A database holding configurations without the application schema marker
    fails; it is no longer imported into history.
  - A reference repository with only the three bond tables fails; the master
    tables are no longer added to it.
- The audit store implements `prune`: each entry goes with its index key, in one
  transaction. Redb reuses the freed pages but does not shrink the file; `convex-redb
  compact` does that offline.

## 0.14.0

This is the first crates.io release since 0.11.1. The v0.12.0 and v0.13.0 tags
exist on GitHub, but their crates were never published, so this entry covers
everything since 0.11.1.

### Crates

- **New on crates.io:** `convex-rates` (fixed/floating and overnight index
  swap contracts, their cash flows and multi-curve valuation) and
  `convex-credit` (single-name CDS valuation and running/upfront quote
  conversion, following the ISDA CDS Standard Model 1.8.3).
- **Dependency order changed.** `convex-core` now depends on `convex-math`
  (the reverse of before). The published order is math, core, curves, then
  rates, credit and bonds, then analytics.
- **Minimum Rust version is 1.95** (was 1.75).
- Every published crate now ships its LICENSE file. `convex-credit` also
  ships the ISDA CDS Standard Model Public License.
- Workspace-only crates: `convex-traits` is now `convex-ports`;
  `convex-ext-json` was removed. A new `convex` facade crate (not on
  crates.io) re-exports the library and adds the JSON `library()` API.

### convex-math

- New modules: `decimal` (`BoundedDecimal`, exact decimal sums with a size
  limit), `portable` (`exp`, `log`, `pow` from `libm`, so results match across
  platforms), `stats` and `bounds`.
- `libm` is pinned to 0.2.15 with soft floats, and `nalgebra` uses
  `libm-force`. See [docs/PORTABLE-NUMERICS.md](docs/PORTABLE-NUMERICS.md).

### convex-core

- New modules: `cashflows` (shared coupon schedules and amounts), `contracts`
  (request and result types with explicit units), `fixings` (index fixing
  history), `futures` (futures reference terms) and `ids`.
- Many constructors and calculations now return `Result` and reject invalid
  input instead of panicking or rounding it away.

### convex-curves

- New `contracts` module: describe a curve as data (`CurveRecipe`), build a
  set with `build_curve_set`, and read it with `CurveQuery`. Supports node
  curves, fitting to market quotes, Nelson-Siegel and Svensson, Monotone
  Convex, and an ultimate-forward-rate tail. The tail converges forwards to
  the UFR; it is not the regulatory Smith-Wilson method.

### convex-bonds

- New modules: `contracts` (`BondTerms`, a serializable bond description),
  `arrc` (compounded overnight coupons, SOFR style) and `fixings`.
- Removed `risk` (use `convex-analytics`), `indices` (fixing history moved to
  `convex_core::fixings`; overnight compounding to `arrc`) and
  `curve_instruments` (calibration instruments are in `convex-curves`).
- Yield and price functions return `Result`; for example
  `YieldSolver::dirty_price_from_yield` and `clean_price_from_yield`.

### convex-analytics

- Curve calibration: `rate_calibration` (deposits, FRAs, futures, swaps, OIS
  and tenor basis), `swap_calibration`, `cash_calibration` (government and
  repo curves) and `credit_calibration` (CDS curves).
- Callable and putable bonds: `optional_bond`, and Hull-White one-factor
  calibration to swaptions in `calibration`.
- Futures valuation and risk (`futures`, `futures_reference`), quote
  sensitivities (`quote_risk`), credit risk for bonds (`credit_risk`), floating
  coupon caps and floors (`floating_coupon`), trade lifecycle events
  (`lifecycle`) and multi-product workflows (`workflow`).
- Hedge advisor in `risk::hedging`: DV01-neutral proposals using bond futures,
  cash bonds or swaps, a comparison report and a plain-text summary. Costs come
  from a fixed heuristic table unless you supply a `CostFeed`.
- Accuracy fixes to I-spread, OAS, asset swap spreads and several day counts.

### Bindings and applications (not on crates.io)

- JSON `library()` API: one request format shared by Rust (`convex::library`),
  the C ABI (`convex_library`), Java, Excel (`CX.LIBRARY`), WebAssembly and
  MCP. See [docs/LIBRARY-API.md](docs/LIBRARY-API.md).
- Java binding in `bindings/java` (JDK 22+, Panama FFM). Build from source;
  it is not on Maven Central yet.
- Excel add-in reworked around `CX.*` functions, including live-updating
  `.LIVE` variants and `CX.YAS`.
- Convex Terminal, a browser app on the WebAssembly engine with public market
  data: <https://sujitn.github.io/convex/>.
- `convex-server` rebuilt around a single pricing runtime with live quotes,
  snapshots and replay, stored in one redb database.

### Release packaging

- The release workflow publishes crates in dependency order, requires a green
  CI run on the tagged commit, and builds every artifact before uploading
  anything.
- Each GitHub release now carries npm tarballs of the WebAssembly engine
  (`web` and `nodejs` targets) next to the Excel add-in zip. Each tarball
  includes the MIT license and the ISDA CDS Standard Model Public License.

## 0.11.1 and earlier

See the [GitHub releases](https://github.com/sujitn/convex/releases).
