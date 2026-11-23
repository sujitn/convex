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

### Web apps

- **The desk app, `desk/`:** the pricing sheet (every bond with a pricing rule, repriced
  from the stream, Treasuries in 32nds, tick colours, each row's state as the server
  judged it, and keyboard nudges of a desk level) and the bond calculator. A bond the
  desk does not mark shows the level, or price, its Live result was priced from, a
  benchmark spread included.
- **One npm workspace at the repository root** over `api-client/`, `admin/` and
  `desk/`, with one lockfile. The admin app's API client, session and exact-number
  helpers moved into `api-client`, which both apps use. Build an app with
  `npm run build -w <app>` from the root. The CI job is now `Web Apps`.

### convex-core

- **A client tier's factor on a two-way quote's width.** `WidthFactor` is a unit-free
  factor from 0 to 10 (`MAX_WIDTH_FACTOR`), an exact decimal string on the wire.
  `QuoteWidth::scaled` multiplies a width by it exactly and keeps its unit: one keeps it,
  below one tightens it for a better client, and zero quotes the mid both ways. A scaled
  width past the unit's widest is refused (`WidthOutOfRange`), never capped, and one a
  decimal cannot hold is `Unrepresentable`; a factor out of range is `FactorOutOfRange`.
  The shared fixture gains seven cases and four refusals.

- **A two-way quote's width, in the quoting unit.** `WidthSchedule` holds
  widths by trade size in one `WidthUnit`: price points, 32nds or basis
  points. Each `SizeBucket` holds trades up to its face amount, inclusive, and
  only the last bucket may have no bound. `width_for` picks a trade's bucket,
  and a quote with no size takes the first.
  - The width is the full bid-to-ask distance, so each side lies half of it
    from the mid. `QuoteWidth::two_way` gives the bid and the ask as `Mark`s in
    the mid's own form. A price falls to its bid, and a yield or spread rises
    to its bid, since a higher yield or spread is a lower price.
  - Each side is exact. Nothing is rounded to a tick, and a side that a
    `Decimal` cannot hold exactly is refused rather than rounded.
  - Refused, with a typed `QuoteWidthError` whose `reason()` is a fixed
    sentence:
    - a unit that does not quote the level, such as basis points on a price
      (`WidthUnit::quotes` answers this from a `QuoteBasis` alone);
    - no buckets, more than 16, or bounds that are not positive and ascending;
    - a width above 20 points, 640 32nds or 1,000 bp;
    - a trade size that is not positive, or above the largest bound;
    - a bid price at or below zero.
- `Mark::quote_basis` (price, yield or spread), `Mark::level` (the quoted
  number in the mark's own unit) and `Mark::at_level` (the same mark at another
  level, keeping its price kind, yield frequency, or spread type and
  benchmark).
  - On the wire, a schedule is exact decimal strings with no unknown fields.
  - Desk fixtures in `tests/fixtures/quote-widths.json`: a Treasury in
    32nds, investment grade by benchmark spread and size, high yield in
    points, a yield in basis points, and each refusal.
- `cashflows::latest_trade_date` inverts `settlement_date`: the latest business
  day whose trade settles on a given date under the same lag, convention and
  calendar. It is `None` when nothing settles on that date, such as a weekend or
  a holiday. A business-day lag is inverted exactly, with no search past the
  calendar's coverage. A calendar-day lag tries the settlement and the closed days
  the convention moves onto it, however long the closure.

### convex-bonds

- `CalendarId::try_to_calendar` copies the memoised SIFMA, TARGET2, UK and Japan holiday
  tables instead of rebuilding them from 1970 to 2100 on every call, about 32 µs each.
  Building a stored bond's contract looks its calendar up several times. Valuing a
  three-call callable on its workouts falls from about 440 µs to 90 µs.

### convex-analytics

- **Breaking:** `hedge_cost_bps` is removed, with its re-exports from `risk`,
  `risk::hedging` and the `convex` facade; use `HeuristicCostFeed.cost_bps`.
- **Breaking:** the `cashflows::IrregularStubType` alias is removed; use
  `StubType`. The `cashflows::irregular` and `cashflows::schedule` modules, which
  only re-exported `convex_bonds` types, are removed too; the types stay at
  `cashflows::`.
- **Price to worst.** `workflow::quote_workout_price_to_worst` prices every
  eligible workout at one dated yield. It selects the lowest price among issuer
  calls and maturity. It enumerates the same paths, in the same order, with
  the same date budget as `quote_workout_worst`. Wherever that function solves
  every path, it inverts it: the yield to worst at the selected price is the
  given yield, on the same path. Holder puts are reported but never selected, and
  a supplied notice forces its path.

### convex-portfolio

- **Breaking:** `KeyRateProfile::to_key_rate_durations` is removed.

### convex-ffi

- New C export `convex_changes_since(cursor)`: the names and handles whose
  resolution changed after a registry generation, from a 4,096-entry log.

### convex-server

- **Requests for quote.** `POST /api/v1/rfqs` takes a source's batch of up to 64 requests
  (128 KiB) and answers each: accepted and priced, a retry of one received before, or
  refused with a code. Each is priced at receipt from the bond's Live level through its
  rule's stage, at the rule's width for its size scaled by its client tier's factor in the
  active configuration, on the desk side the client trades on; a bond without a width is
  priced at the mid, flagged, and one that cannot be priced is recorded with the reason for
  the trader to price. It is checked against its book's limits as if done.
  `POST /api/v1/rfqs/:rfq_key/quote` and `/pass` are the trader's;
  `POST /api/v1/rfqs/:rfq_key/outcome` the source's. `GET /api/v1/rfqs` lists the open
  requests and `GET /api/v1/rfqs/:rfq_key` one request's entries. At most 256 are open; a
  deadline expires one once, and a restart expires every one left open. `done` changes no
  book. Every transition is an `rfq` audit entry written before it applies, carrying what
  it was made from, and retention prunes them with limit records.
- **Breaking:** two roles, `trader` (`LevelWrite`, `RfqRespond`) and `rfq_source`
  (`RfqIngest`). Marking desk levels needs `trader`; `reference_writer` no longer marks
  them. A grant may hold each of the eight roles once.
- **One audit writer.** The limit monitor and the RFQ desk take entry ids and append under
  one lock, so ids commit in order. `Server::with_limit_monitor` is now
  `Server::with_audit_records`, which composes both.

- **The desk app and the desk's levels.**
  - `desk_ui_dir` serves the built desk app (`desk/`: the pricing sheet and the bond
    calculator) at `/desk`, as `admin_ui_dir` serves `/admin`, under the same policy and
    startup checks.
  - `desk_level_source` names the Live plan's level source as the desk's.
    `POST /api/v1/market/levels` marks a level-quoted bond's level there as a `manual`
    quote, checked against its rule's field and its method's range. The quote ingress
    owns that source: it numbers and stamps each mark under the lock that orders every
    quote, and `POST /api/v1/market/quotes` now refuses a feed's event on it
    (`DESK_LEVEL_SOURCE`).
    `GET /api/v1/market/levels` reads back each bond's last mark. Startup refuses the
    setting without a Live runtime or when the plan reads no level, or anything else,
    from the source.
- **A browser opens the stream with its token.** A page offers `convex.v1`, which the
  server selects, and `convex.bearer.<token>`, the bearer token base64url-encoded, as
  WebSocket subprotocols, since a browser cannot send `Authorization` on a WebSocket.
  Only the stream reads it, it is removed before the handler and never echoed, and a request
  sending a token both ways is refused (400). Clients sending `Authorization` are
  unchanged.

- **A ninth callable on HW1F trees no longer stops the runtime.** A runtime plan still
  values at most 8 callables on HW1F trees, and a configuration holding more is still
  refused at validation (`TOO_MANY_CALLABLES`). But a rule, its end, or a bond revision
  committed after the configuration was checked can put more there, and the runtime
  picks that up at its next activation. Before, that stopped the whole runtime
  (`Failed`), and a restart from the checkpoint the activation saved failed the same
  way. Now the first 8 by instrument id are valued and each callable beyond them fails
  every measure with `SOLVER_LIMIT`, saying why, whatever its quote. A callable whose
  rule no longer applies is not counted, since it fails before a tree runs. In Live a
  callable that crosses the bound because another callable's rule changed is valued
  again at that tick.
  `POST /api/v1/runtime/valuation` refuses the same callables in a supplied snapshot, so
  it reproduces what the runtime published.
- **A reply on HW1F trees names its model.** A request-time reply for a callable on
  HW1F trees (the quote routes, the batch and the calculator) carries `option_model`: the
  configuration's `mean_reversion`, `volatility` and `reason`, and its `discount_curve`,
  the model an OAS is taken on.
- **A callable priced beyond its OAS bracket keeps its price measures, and the bracket
  is wider.** The OAS solver brackets -500 to 5,000 bp (was 1,000 bp), the range an
  `oas` level takes. A price whose OAS is beyond it, such as a distressed callable's,
  publishes its clean and dirty price and accrued, and `oas_mid` and the effective risk
  fail with `SOLVER_LIMIT`, saying why; every measure failed before. A solve from a price
  takes about 5–11% longer.
- **Callables by OAS.** A `pricing_rule` may quote a callable by `oas` (basis points
  over the discount curve at which its HW1F trees price it). The callable is priced at
  its OAS under `convex-optional-hw1f-v1`, publishing the OAS as quoted, the price at it
  and the effective risk. In Live its plan reads `mid_spread` as a note's margin is
  read, even beside a price, and a curve revision values it again; a callable under the
  rule whose feed sends only a price fails each measure for its missing OAS. At request
  time `GET /api/v1/quotes/:id` takes it as `level`, and still takes `market_price`. An
  OAS takes the solver's bracket (below). The rule is refused at commit for any bond but a
  callable the HW1F model values: European or Bermudan fixed-price calls, none
  make-whole, and no other embedded terms.
- **A bond whose rule no longer applies is refused at request time.** A bond revised
  after its rule was committed, so that the rule no longer applies to its terms (a
  callable under an `oas` rule revised to American calls or to a bullet), is refused with
  400 `INVALID_INPUT` (`RULE_DOES_NOT_APPLY`) by a level or a price, on the quote routes,
  alone and in a batch, and in the calculator by every method, before anything is valued.
  A callable or note was a 200 whose every measure failed, and a bullet by price was
  priced as if it had no rule. In Live each measure still fails, saying why, whatever its
  quote.
- **The calculator takes a callable's OAS**, whatever its rule, for a callable the HW1F
  model values, valued by the runtime as `GET /api/v1/quotes/:id` values it: the HW1F
  measures, the unpublished `snapshot`, `input` and `pricing_rule` (with the model it was
  valued on), and no `workouts`. A callable under a `price` or `yield` rule is valued on
  its trees at the OAS, though Live values it on its workouts. Any other callable's OAS
  is 400, and a server without a runtime refuses it (422 `UNSUPPORTED_PRODUCT`). By
  `price` or `yield` a callable is still calculated on its workouts.
- **Floating rate notes by discount margin.** A `pricing_rule` may quote a floating rate
  note by `discount_margin` (basis points over the frozen projection of its index,
  discounted on the discount curve). Live and request-time quotes value it from its
  margin under `convex-floating-dm-v1`, publishing the margin as quoted and the price at
  it; `GET /api/v1/quotes/:id` takes it as `level`. A margin is from -2,000 to 5,000 bp.
  The rule is refused at commit for any other bond, and `z_spread` and
  `benchmark_spread` stay fixed-bullet only.
- **The calculator takes floating rate notes**, by `price`, or by `discount_margin` under
  a margin rule, valued by the runtime as `GET /api/v1/quotes/:id` values them. The reply
  has the note's measures, the unpublished `snapshot`, `input` and `pricing_rule`; a
  note has no `workouts`. A server without a runtime refuses notes (422
  `UNSUPPORTED_PRODUCT`), and a margin for a note quoted by price is 400.
- A request-time level for a callable on its workouts, or a note quoted by its margin,
  outside its method's range is 400 before it is valued; it was a 200 whose every
  measure failed.
- **The calculator takes callables.** `POST /api/v1/bonds/:instrument_id/calculate`
  values a callable with European or Bermudan calls, at most 128, on its workouts, from a
  `price` or a `yield` to worst, whatever its rule quotes. It needs no runtime and reads
  no curve.
  - The reply has the workout model's clean price, accrued, dirty price and `ytw`, as
    `GET /api/v1/quotes/:id` gives them.
  - `workouts` lists each call whose notice can still be given, with its `call_price`
    and yield at the clean price, then maturity. `worst` adds the `years` to it.
  - A spread method or a settlement on a closed day is 400. An American callable, an
    ACT/ACT ICMA one, a floating rate note or another bond is 422
    `UNSUPPORTED_PRODUCT`.
- **Callables on their workouts.** A callable whose pricing rule quotes it by `price` or
  `yield` (its yield to worst) is valued on each call date and maturity: model
  `convex-callable-workout-v1`, in Live and at request time, with no option model and
  no curve.
  - From a price, it gives the yield to worst. From a yield to worst, it gives the price
    to worst.
  - It publishes `clean_price_mid`, `dirty_price_mid`, `accrued_interest`, `ytw_mid` and
    `years_to_worst_mid`. A request-time quote carries `ytw`.
  - European or Bermudan calls only, at most 128. A `yield` rule is now accepted at
    commit for such a callable.
  - These callables do not count against the cap of 8 callables valued on HW1F trees.
  - **Behaviour change:** a callable that already has a `price` rule moves off HW1F.
    It no longer publishes `oas_mid`, effective duration or convexity, or `pv01_mid`.
- **The bond calculator: `POST /api/v1/bonds/:instrument_id/calculate`.** It takes one
  level of a stored bond by any method (`price`, `yield`, `z_spread` or
  `benchmark_spread`, whatever the bond's rule quotes) and a settlement date, prices it
  as `GET /api/v1/quotes/:id` does under the approved context at `known_at`, and returns
  the router's mid measures, the G-spread over the set the rule names, `input`,
  `pricing_rule`, each `workout` with its yield and the `worst`. A fixed bullet's or
  zero's one workout is maturity. It is read-only, mid only, and reports the versions
  it read in the same headers as a quote. Fixed bullets take every method and zeros a
  price; floating rate notes are refused (422 `UNSUPPORTED_PRODUCT`). A
  body is at most 64 KiB (413 `CAPACITY_EXCEEDED`). New histogram
  `convex_bond_calculator_seconds`.
- **A pricing rule's width gives the two-way in its quoting unit.** A rule's optional
  `width` holds widths by trade size in price points or 32nds for a `price` rule, or
  basis points for a yield or spread. At request time (`GET /api/v1/quotes/:id`,
  `POST /api/v1/quote` and each batch item) it widens what the rule quotes, at the
  first size bucket: each side's level is priced as the mid's is, and the router values
  the bid and ask prices. The `pricing_rule` report adds `two_way` (`unit`, `width`, and
  the `bid` and `ask` levels). A commit is refused for a width in a unit the method
  does not quote, an invalid schedule, or a callable or floating rate note, which are
  quoted mid only. Live runtimes publish mid measures only and do not apply it.
  **Breaking:** a bond quoted by price whose rule has a width no longer takes the
  configuration's percent-of-price width.
- **A pricing rule or benchmark set with an end date is not in force after it.** A
  version with `effective.until`, committed through `POST /api/v1/reference/transactions`
  or the import, ends the rule or set at that time, which may be later than the commit.
  After it, a bond whose rule ended is quoted by price; a bond whose rule names an ended
  set is refused, naming the set, whether asked by level or by price; a compile refuses a
  rule naming it; and a Live runtime applies the end at its next activation whose cutoff
  is after the end. A rule version that has ended by its commit is not checked against
  its bond's current terms, so a rule can be ended after its bond is deleted or changed.
  `EffectiveInterval::has_ended_by` names the test.
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

- **Pricing a trade's size.** `PricingEngine::price_for_trade` is the stage for a
  `TradeSize`: the rule's width for its face's bucket, scaled by its
  client tier's `WidthFactor`; a face past the schedule or a scaled width past its unit's
  widest is refused. `live_quote_input` reads the quote a bond's Live record was valued
  from, and `live_price` its clean price mid, under the conditions `live_clean_price_mid`
  reads a benchmark's. `RequestContext` carries the active configuration's version and
  its client tiers (`width_factor`).
- **Breaking:** `PlanSelection` requires `client_tiers`.
- **A Live result carries the quote it was valued from.** The snapshot valuation records
  the field of the bond's quote it read, the level under a rule that quotes the bond by
  one and otherwise its price, with its exact observation. It is carried through the
  valuation cache to `SnapshotValuationResult::quote_input` (a new argument of `new`) and
  `PublishedValuation::quote_input`. A refused bond reads none, and
  neither does a record whose delivery continuity was lost.
- **A request's OAS for any callable on HW1F trees.** `QuotedInput::Oas` is an OAS a
  request names whatever the bond's rule quotes. The request valuation values the callable
  on its trees and records the method on its quote (`QuoteRequirement::quoted_by`) when
  the rule quotes it otherwise; the snapshot valuation reads it ahead of the rule.
  `RuntimePlan::validate` refuses a plan carrying one.
- **The HW1F callable bound is per callable.** A runtime no longer fails with
  `RuntimeError::Configuration` when a snapshot holds more than `MAX_RUNTIME_CALLABLES`
  callables on trees. The snapshot valuation fails each measure of a callable after the
  first 8 by instrument id (`ErrorCode::SolverLimit`,
  `optional_valuation::BEYOND_CALLABLE_BOUND`) before a tree runs. The runtime reports
  it, as it does a rule that no longer applies, ahead of the inputs' quality. A Live tick
  under a new reference section values each callable that moved across the bound.
- **Callables by OAS.** The HW1F model reads a callable's rule's field when its quote
  declares it, as the workout and floating models do, and prices an OAS through the
  library's `OptionalBond` `Price`, then its risk at it; `values_own_level` counts it. Its
  input and output are `CallableQuote` (a clean price or an OAS) and
  `CallableValuation`, as the other models'. At an OAS, risk is run first and its base
  price read, so the price is run alone only when risk fails. Live reads a callable's
  OAS as it reads a note's margin: a Live compile reads its rule's field, and a quote
  declaring it is read by it, even beside a price (`PricingMethod::is_level_declared`).
- **Floating rate notes by discount margin.** The floating model reads a note's rule's
  field, as the workout model does, and prices a margin through the library's `Bond`
  `Price` with a discount spread; a margin out of range fails each measure.
  `optional_valuation::values_own_level` names the models that value their own level.
  New `RequestValuationError::Level`: a request's level out of its method's range, or
  for a bond its rule quotes by price, is refused before anything is valued (400 over
  HTTP).
- **`workout_valuation::calculate_on_workouts`**: the calculator's callables. From a
  level by `price` or `yield`, it gives the `WorkoutValuation` Live and request-time
  quotes give, and each `Workout` (maturity, or a call at its price) with its yield at
  the clean price, in date order, and the `worst`. `WorkoutCalculation::bond_quote`
  reads it as a `BondQuoteOutput`. New `LevelError::Unsupported`, for terms or
  conventions the model does not value (`UnsupportedConvention`; 422
  `UNSUPPORTED_PRODUCT` over HTTP).
- **`workout_valuation`**: model `convex-callable-workout-v1`, which values a callable
  quoted by price or yield to worst on its workouts (`WORKOUT_SNAPSHOT_MODEL`,
  `WORKOUT_MEASURES`).
  - **Breaking:** `optional_valuation::product_model` takes the bond's pricing method
    beside the bond.
  - **Breaking:** `PricingEngine::value_request` takes the `ResolvedBondReference` and a
    `QuotedInput` mark, a price or a callable's yield to worst.
  - `optional_valuation::is_runtime_product` says whether the runtime, not the router,
    values a bond at request time.
  - The HW1F cap of 8 counts only callables valued on HW1F trees.
- **`price_by_method`** (and `PricingEngine::price_by_method`): the calculator's stage.
  It prices a level by any method, against the set the bond's rule names, mid only.
  `PricingEngine::rule_in_force_mid_only` reads a rule's set from its pin without its
  width, and `QuotedInput::quoted_by` reads a value as a method quotes it. The stage's
  own result is mid only when no width applies; `price_under_rule` alone gives a bond
  quoted by price the configuration's width.
- **The level stage applies a rule's width.** `PricingRuleInForce` gains `width`.
  **Breaking:**
  - `rule_in_force` takes a `RequestPricingRule` (the pin and the width): from
    `rule_for_inline_terms`, which now returns one from its single rule read, or from
    the new `pinned_rule_for_request`, which reads a stored bond's pin at its version;
  - `RulePricing::price_width_applies` is replaced by `bid_ask` (`BidAsk`): the
    configuration's width, the rule's (`TwoWay`, each side's level and clean price), or
    mid only. `BidAsk::apply` sets the router's `PricingInput` from it.
- `PricingRouter::price_at` admits what `is_quoted_two_way` admits, with the same
  refusals as before.
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

- **Client tiers in the configuration.** `ConfigurationDocument::client_tiers` lists each
  `ClientTier`'s `WidthFactor`, at most 64, each tier once; a change is a hot activation.
  **Breaking:** the field is required, and a stored document without it no longer loads.
  Validation and activation read the active document of their configuration id, so a
  store whose active document predates the field is moved to a new id: save the pricing
  configuration under a new id, compile, stage and activate it, and set
  `configuration_id` to it. `RfqRequest::client_tier` is a `ClientTier`.
- **A prune names its event types.** `AuditPrune::event_types` replaces `event_type`: 1
  to 4 types removed in one walk.
- **`AuditStore::entries_before`** reads up to 1,000 entries below an ID, newest first, so
  a reader walks back from the log's end only as far as it needs; redb reads the range
  backwards.
- **`QuoteInput`**: the field of its quote a valuation read the bond's mark from, with its
  `FieldObservation`. `PublishedValuation::quote_input` carries it, and is omitted when the
  attempt read no quote.
- **`QuoteRequirement::quoted_by`**: the method a request quotes a quote by when its bond's
  rule quotes it otherwise. A snapshot takes it only as an OAS in the quote's one field,
  for the one callable it values, which the HW1F model must value (`NOT_AN_OAS_CALLABLE`).
  Omitted when absent, so a snapshot without it keeps its bytes and id; a configuration
  document refuses it. `QuoteRequirement::quoted_method` is the method a quote is read by.
- **The RFQ model** (`rfq`). `RfqRequest` is a client's request as its source sends it: the
  source and its RFQ id, the client and its tier, the bond by `instrument_id` or by an
  ISIN, CUSIP or provider identifier, the client's side, the face, the settlement date, the
  desk's book and a deadline. `validate` refuses an id or tier empty, too long, or holding
  surrounding spaces or a control character, a bond named twice, not at all or by an
  invalid identifier, a size not positive, and a deadline outside 1 to 300 seconds after
  receipt, each with an `RFQ_*` code. `transition` is the closed state table from
  `received` through `priced` and `quoted` to `done`, `traded_away`, `covered`, `passed`,
  `expired` or `cancelled`: a final request refuses every event (`RFQ_FINAL`), a source's
  outcome after an expiry included, and a move the table lacks is `RFQ_TRANSITION`. Each
  event has one actor (server, trader or source), and `SourceOutcome` names the only
  events a source sends. An expiry says whether its deadline passed or the server
  restarted, and a client buy lifts the desk's ask and shortens its position. `RfqBatch`
  is one ingest's requests; ingest will refuse more than `MAX_RFQ_BATCH` (64) or a body
  over `MAX_RFQ_BODY_BYTES` (128 KiB), which 64 of the largest requests fit.
- **`PricingMethod::Oas`** (`oas`): applies only to a callable the HW1F model values
  (`is_oas_callable`, which `is_workout_callable` now builds on), with no benchmark set, and
  reads `mid_spread`. `PricingMethod::is_level_declared` says whether a quote is read by
  the rule's level, given the fields it declares: whenever it declares it, even beside a
  price. The snapshot's valuation and its quality both use it.
  `RULE_DOES_NOT_APPLY` names it.
- **`PricingMethod::DiscountMargin`** (`discount_margin`): applies only to a floating rate
  note, with no benchmark set, and reads `mid_spread`. `RULE_DOES_NOT_APPLY` names it.
- `BondQuoteOutput::without_measures`: a fresh quote with every measure absent, for a
  caller to set the measures it has.
- `ResolvedBondReference::calendar_definition`: the pinned calendar's definition.
  `NOT_A_WORKOUT_CALLABLE` says why `is_workout_callable` refuses a bond.
- **Breaking:** `PricingMethod::requires_plain_bullet`,
  `PricingRuleDefinition::requires_plain_bullet` and
  `ResolvedPricingRule::requires_plain_bullet` are replaced by `applies_to(bond)`.
  - A fixed bullet takes every method. Any other bond takes `price` with no set.
  - A callable valued on its workouts (`is_workout_callable`: European or Bermudan
    fixed-price calls, at most `MAX_WORKOUT_CALLS` (128), nothing beyond its calls) also
    takes `yield`, its yield to worst.
  - A commit refuses any other rule with `RULE_DOES_NOT_APPLY`.
- `BondReferenceData::has_terms_beyond_calls` (a put, sinking fund, or floating or
  inflation terms), which `has_embedded_terms` now uses, and
  `ResolvedBondReference::pricing_method`.
- **Breaking:** `PricingRuleDefinition` gains `width` (`Option<WidthSchedule>`), so a
  struct literal needs the new field; stored rules read as before. `validate` checks the
  schedule and that its unit quotes the method (`PricingMethod::quote_basis`); more than
  16 buckets, like more than 32 securities in a benchmark set, is `InvalidInput` (400
  over HTTP). A commit refuses a width for a bond that is not quoted two-way
  (`is_quoted_two_way`, `WIDTH_NEEDS_TWO_WAY`). The pin a snapshot holds,
  `ResolvedPricingRule`, does not carry it; a request reads it with the pin as a
  `RequestPricingRule`, through `ReferenceRepository::pricing_rule_for_request` (in
  force at a lookup) or `pinned_pricing_rule_for_request` (at a pin's version), both
  provided methods.
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
