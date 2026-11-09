# Convex Excel Add-In

Mark-driven fixed income analytics for Microsoft Excel, using the Rust library through `convex-ffi`. The add-in lives in `excel/` (C# project `excel/Convex.Excel`) and ships as a zip on the GitHub releases page; this page also covers building it from source.

The add-in is small: a compact C FFI surface, a typed JSON
wire format, about 30 cell functions (UDFs), and a ribbon. Adding a new bond shape, spread
family, or pricing convention is a serde enum variant on the Rust side plus
one dispatch arm — no new C symbol, no new P/Invoke, no new UDF.

Behaviors worth knowing: reference bonds by CUSIP/ISIN/name instead of
handles; `CX.YAS` spills a full yield-and-spread analysis from one call;
identical requests are served from a generation-gated cache so multi-field
blocks and repeated recalcs cost one engine call; errors are standard Excel
errors with per-cell detail via `CX.LASTERROR` and a ribbon Error Log;
IntelliSense tooltips document every argument in the grid; the pricing and spread forms are
modeless so Excel stays interactive. On add-in load a full rebuild
re-registers cell-built objects, so reopened workbooks heal automatically
(Settings → "Rebuild handles on open"; manual: ribbon **Rebuild**).
Connected to a Convex server, `CX.LIVE` cells follow the server's live
results and update when the server publishes (see *Live server cells*).

## Installation

### Prerequisites
- Microsoft 365 Apps for Windows, 64-bit (Current or Monthly Enterprise channel).
  The grid functions need dynamic arrays.
- .NET Framework 4.7.2 (Windows 10 +)
- Visual Studio Build Tools or .NET SDK (only needed to build from source)

### From a release

1. Download `convex-excel-addin-v<version>.zip` from the GitHub release and
   extract it to one folder. Keep `convex_ffi.dll` beside `Convex.Excel64.xll`.
2. Excel refuses an add-in downloaded from the internet, so unblock the files
   first: `Get-ChildItem <folder> | Unblock-File`.
3. Load `Convex.Excel64.xll` as in *Loading the add-in*.

### Build from source

```powershell
# Build the Rust FFI (the `excel` profile builds with panic=unwind so a
# Rust panic becomes an in-cell error instead of killing Excel)
cargo build --profile excel -p convex-ffi

# Build the Excel add-in
cd excel/Convex.Excel
dotnet build --configuration Release
```

The Rust `convex_ffi.dll` is copied next to the `.xll` automatically by
the build.

### Loading the add-in

1. Open Excel → File → Options → Add-ins.
2. At the bottom, "Excel Add-ins" → Go.
3. Browse to `Convex.Excel64.xll` from the release, or, from a source build, to
   `excel/Convex.Excel/bin/Release/net472/publish/Convex.Excel64-packed.xll`.
4. OK.

From the repository root, `excel/Package-AddIn.ps1 -OutDir <folder>` stages a
source build the way a release ships it.

## Trader marks

Every pricing/risk/spread call accepts a single textual mark. The grammar:

| Form | Example | Meaning |
|---|---|---|
| Decimal price | `99.5`, `99.5C` | Clean price |
| Decimal price (dirty) | `99.5D`, `99.5 dirty` | Dirty price |
| 32nds | `99-16`, `99-16+` | Treasury 32nds (`+` = ½) |
| Yield | `4.65%`, `4.65%@SA` | Yield with optional frequency suffix |
| Spread | `+125bps@USD.SOFR` | Z-spread over benchmark (default) |
| Spread w/ type | `125 OAS@USD.TSY`, `-50 G@USD.TSY.10Y` | Explicit family + benchmark |

Frequency tokens: `A` (Annual), `SA` (SemiAnnual, default), `Q`, `M`, `Z`.
Spread types: `Z`, `G`, `I`, `OAS`, `DM`, `ASW`, `ASW_PROC`, `CREDIT`.

## Cell UDFs

Every UDF lives behind the `CX.` prefix and routes through the JSON FFI.
The full surface:

### Construction (returns a `#CX#…` handle)

| UDF | Description |
|---|---|
| `CX.BOND(id, couponDecimal, maturity, issue, [freq], [dayCount], [currency], [face])` | Fixed-rate bond. |
| `CX.BOND.CALLABLE(id, couponDecimal, maturity, issue, callDates, callPrices, [freq], [style], [dayCount])` | Callable bond. American/European/Bermudan. |
| `CX.BOND.FRN(id, spreadBps, maturity, issue, [index], [freq], [dayCount], [cap], [floor])` | Floating-rate note. |
| `CX.BOND.ZERO(id, maturity, issue, [compounding], [dayCount])` | Zero-coupon bond. |
| `CX.CURVE(name, refDate, tenors, values, [valueKind], [interp], [dayCount], [compounding])` | Discrete curve from `(tenor, value)` pairs. |
| `CX.CURVE.BOOTSTRAP(name, refDate, kinds, tenors, rates, [method], [interp], [dayCount])` | Bootstrap from market instruments. |

### Ticker-style referencing

Every analytics UDF accepts the bond/curve argument as **either** a `#CX#N`
handle **or** the identifier it was built with — CUSIP, ISIN, or free name
for bonds; the curve's own name for curves:

```excel
=CX.PRICE("912828YK0", DATE(2025,4,15), "99.5C")
=CX.SPREAD("912828YK0", "USD.SOFR", DATE(2025,4,15), "99.5C", "Z")
```

When the same identifier is built in two cells, the name resolves to the
most recent build (each cell's own handle is unaffected). Unknown names
return `#REF!`.

### Stateless analytics

| UDF | Description |
|---|---|
| `CX.PRICE(bond, settle, mark, [curve], [quoteFreq], [field])` | Returns clean / dirty / accrued / YTM. `field`: `clean` (default), `dirty`, `accrued`, `ytm`, `grid`. For a Z-spread use `CX.SPREAD`. |
| `CX.YAS(bond, settle, mark, curve, [quoteFreq], [swapCurve])` | Convenience yields, Z/ASW spreads, durations, convexity, DV01 and settlement invoice. |
| `CX.RISK(bond, settle, mark, [curve], [metric], [quoteFreq], [krdTenors])` | `metric`: `grid` (default), `mod_dur`, `mac_dur`, `convexity`, `dv01`, `spread_dur`, `krd`. |
| `CX.SPREAD(bond, curve, settle, mark, [type], [vol], [field])` | Z / G / I / ASW / ASW_PROC / OAS / DM / Credit. `field`: `bps` (default), `grid`. |
| `CX.SCENARIO(bond, curve, settle, mark, shifts, [kind], [pivot], [quoteFreq])` | Scenario ladder in one call — reprices under each bps shift holding the mark-implied Z fixed. `kind`: `parallel` (default), `steepener`, `flattener`, `key_rate`, `credit`. |
| `CX.CASHFLOWS(bond, settle)` | Cashflow grid: `Date, Amount, Kind`. |
| `CX.CURVE.QUERY(curve, tenor, [query], [tenorEnd])` | `query`: `zero` (default), `df`, `forward`. |

### Live cells (RTD)

A live cell recalculates by itself when a bond or curve it reads is rebuilt
or released (from a cell, a ribbon form or `CX.RELEASE`), and only then. No
F9 is needed under Automatic calculation. Each live cell returns one number.

| UDF | Field (default first) |
|---|---|
| `CX.PRICE.LIVE(bond, settle, mark, [curve], [quoteFreq], [field])` | `clean`, `dirty`, `accrued`, `ytm` |
| `CX.RISK.LIVE(bond, settle, mark, [curve], [metric], [quoteFreq])` | `mod_dur`, `mac_dur`, `convexity`, `dv01`, `spread_dur` |
| `CX.SPREAD.LIVE(bond, curve, settle, mark, [type], [vol], [field])` | `bps` |
| `CX.YAS.LIVE(bond, settle, mark, curve, [quoteFreq], [swapCurve], [field])` | `ytm`, `clean`, `dirty`, `accrued`, `accrued_days`, `current_yield`, `simple_yield`, `z_spread`, `mod_dur`, `mac_dur`, `convexity`, `dv01`, `principal`, `accrued_amount`, `settlement_amount` |

The arguments match the static functions, with two exceptions:
`CX.RISK.LIVE` has no key-rate tenors argument, and `CX.YAS.LIVE` adds
`field` because `CX.YAS` returns a grid. A grid field (`grid`, `krd`)
returns `#VALUE!`, and `=CX.LASTERROR(cell)` names the static function to
use instead.

How a change reaches the sheet:

1. The Rust registry records which names (CUSIP, ISIN, curve name, owning
   cell) and handles each rebuild or release changed.
2. The add-in's one RTD server (`Convex.Rtd`) recomputes only the live cells
   whose request names one of them. It sends a value to Excel only when the
   number changed.
3. Excel recalculates every formula that reads a changed live cell, as for
   any other cell.

A cell that references a handle cell (`=CX.PRICE.LIVE(A1, …)` with `#CX#101`
in `A1`) recalculates through Excel's own dependency graph. The registry's
change log matters for cells that name a bond or curve, such as `"USD.SOFR"`
or a CUSIP, which Excel cannot see. Identical live cells share one topic,
and live and static cells share one response cache, so they cost one engine
call per change.

Limits and settings:

- **Update rate.** Excel delivers changed live cells once per RTD throttle
  interval: 2,000 ms by default. Settings → "Excel RTD throttle" can set
  250 ms. That is Excel's own setting: it applies to every add-in and
  persists after Excel restarts. The add-in never changes it by itself.
- **Calculation mode.** Under Manual calculation, live cells update on F9.
- **Size.** At most 20,000 distinct live cells per Excel process; beyond
  that a cell returns `#VALUE!`.
- **Diagnostics.** `=CX.DIAG()` shows the live topic count, the recomputes
  and pushes so far, and the time the last change took.

This replaces the earlier live layer, which recomputed every live cell on
any registry change and had a "LIVE poll (ms)" setting. There is no
compatibility layer: `CX.PRICE` no longer has a `z_spread` field (use
`CX.SPREAD`), and the setting is gone.

### Live server cells

`CX.LIVE` shows a result that a Convex server publishes, and updates by
itself when the server publishes a new one. Set up the connection first (see
*Connecting to a Convex server*).

| UDF | Returns |
|---|---|
| `CX.LIVE(identifier, measure, [field])` | One field of the server's current result for an instrument (RTD). |
| `CX.LIVE.GRID(identifier, [measures], [trigger])` | Rows of measure, value and quality for an instrument that a `CX.LIVE` cell already follows, read from the add-in's cache. It is not live by itself: pass a `CX.LIVE` cell as `trigger` and the grid recalculates with that cell. |
| `CX.SNAP(identifier, measure)` | The server's current value, read once. It does not update, and shows `#N/A` until the server answers. |
| `CX.STATUS([field])` | The connection: `connection` (default), `user`, `server`, `runtime`, `last_message` (UTC), `instruments`, `books` (followed by `CX.LIMIT` cells), `topics`, `session`, `problem`, or `calculation` (Excel's calculation mode). |

- **identifier**: a CUSIP or ISIN with a valid check digit is looked up in the
  server's reference master (`POST /api/v1/reference/resolve`), which needs
  reference read on every instrument. Anything else is used as the server's
  instrument id, exactly as typed. Bond names are not accepted. A failed
  look-up is tried again after 5 minutes, and at once after a sign-in or a
  change of server.
- **measure**: a measure id the server's runtime publishes, such as
  `clean_price_mid`, `dirty_price_mid`, `ytm_mid`, `modified_duration_mid`,
  `dv01_mid` or `z_spread_mid`. The runtime's configuration chooses which;
  for a missing one, `=CX.LASTERROR(cell)` lists those the result has.
- **field**: `value` (default, a number), `quality` (`Valid`, `Indicative`,
  `Delayed`, `Stale`, `Invalid` or `Demo`), `status` (`Complete`, `Partial`
  or `Failed`), `revision`, `calculated_at` (an Excel date-time in UTC),
  `snapshot_id` or `configuration_version`. The last four record exactly what
  a number was computed from.

Live values are for display only. They are not the server's official results.

How a server result reaches the sheet:

1. The server works out which instruments a quote or curve change reaches,
   and publishes a new result for those only.
2. The add-in keeps the newest result of each instrument, by runtime and
   revision, and sends a cell's value to Excel only when it changed.
3. Excel recalculates every formula that reads a changed cell.

When there is nothing to show, the cell is `#N/A`, and `=CX.LASTERROR(cell)`
says why:

- no server is set, you are signed out, or the add-in is connecting or
  reconnecting (with the reason);
- the first result has not arrived yet, or the server's runtime has no
  result for the instrument;
- the identifier is unknown, or you are not entitled to the instrument
  (`FORBIDDEN`). A refused instrument is not requested again until you
  reconnect (click the connection button on the ribbon);
- the result is larger than one 64 KiB message (`RESULT_TOO_LARGE`), so it
  cannot be streamed. It is not requested again; a later result that fits
  replaces it;
- more than 256 instruments: one session follows at most 256, which is all
  one server runtime values. The rest wait for a free slot.

When the server's latest result failed but kept a last good result, `value`
shows the last good value and `quality` the quality the server gives it.

When the connection drops:

- every value stays, and `quality` becomes `Stale` until the stream resumes.
  A cell that never had a value shows `#N/A`;
- the add-in reconnects by itself, after 0.5 s doubling to 30 s, and
  receives every current result again. The server sends a heartbeat every
  30 s, so 45 s of silence counts as a lost connection.

Limits:

- An instrument stays followed for 30 s after its last cell goes, so a
  recalculation does not unsubscribe and resubscribe it. Subscription
  changes go out in batches every 100 ms.
- Server cells count towards the 20,000 live cells of an Excel process.
- Under Manual calculation, server cells update on F9. A new server cell
  shows `#N/A` until its result arrives, and under Manual calculation until
  the F9 after that.
- Excel's RTD throttle applies, as for local live cells.

### Limits

`CX.LIMIT` shows a limit of a book on the server, as the server's limit
monitor evaluates it. Ribbon **Limit Check** checks proposed trades against a
book's limits before trading. Both need a server with book limits (see the
server's [book limits](../crates/convex-server/README.md#book-limits)) and
the connection set up as for server cells.

| UDF | Returns |
|---|---|
| `CX.LIMIT(book, limit, [field])` | One field of a limit in the book's current limit set (RTD). |

- **book**: the server's book id. **limit**: the limit's id in the book's set.
- **field**:
  - `status` (default): `within`, `warning`, `unknown` or `breach`;
  - `value`, `lower`, `upper`, `warning_fraction`: numbers in the measure's
    unit (book currency, per bp, years, face or a fraction);
  - `utilisation`: the value over the bound that faces away from zero (the
    larger of two), so 1 is at the bound;
  - `member`: for a limit on each member, the one that set the status;
  - `measure`: `dv01`, `market_value`, `face BOND-1`, `key_rate_dv01 5Y`,
    `group_share issuer each` and so on;
  - `reason`: why the limit is `unknown`, such as
    `missing_result: BOND-1, BOND-2 (3 in all)`;
  - `previous`: the status before the last recorded change;
  - `record_id`: the server's audit record of the last change;
  - `evaluated_at`: when the value shown was evaluated (an Excel date-time in
    UTC);
  - `quality`: `Live`, `Demo` (a demonstration server) or `Stale`;
  - `book_version`, `limit_set_version`.

  Text fields that do not apply are empty; a number that does not apply is
  `#N/A`.

What updates, and when:

- The server records a limit, and streams it, only when its **status**
  changes. So `status` follows the server as it happens, but `value` and
  `utilisation` are those of the evaluation shown: they hold still while the
  status does. `evaluated_at` says how old they are. For current values,
  click **Check** in Limit Check with no trades.
- When a cell first follows a book, and again after a reconnect, the add-in
  reads the book's current status (`GET /api/v1/books/{book}/limits/status`),
  so every limit starts current.
- A change under a newer limit set reads the book again, so a limit the new
  set dropped goes to `#N/A`.

When there is nothing to show, the cell is `#N/A`, and `=CX.LASTERROR(cell)`
says why:

- no server, signed out, connecting or reconnecting, or the book's status is
  still being read;
- the book has no limit set (the cell starts showing once one is committed),
  or the set has no such limit;
- you are not entitled to the book's limits (`FORBIDDEN`): they need read
  access to the book, each instrument it holds and each one a limit names. A
  refused book is not requested again until you reconnect;
- the server does not monitor limits;
- more than 64 books: one session follows at most 64, every book the server's
  monitor watches. The rest wait for a free slot.

When the connection drops, limits stay with `quality` `Stale`, and each book
is read again once the add-in reconnects. A book stays followed for 30 s
after its last cell goes. At most 4 status reads run at once; a failed read
is tried again after 1 s doubling to 30 s, and a 429 waits for its
`Retry-After`.

**Limit Check** (ribbon Server group) is a pre-trade ticket:

1. Enter the book and the proposed trades: an instrument id, CUSIP or ISIN
   and a signed quantity in face, which is added to the held position (a
   trade, not a new holding). Rating and sector apply only to an instrument
   the book does not hold. **From selection** reads two to four columns
   (identifier, quantity, rating, sector) from the sheet; a header row is
   skipped.
2. **Check** reads the book's status now and sends the trades to
   `POST /api/v1/books/{book}/limits/check`, which stores nothing. Each limit
   shows its status, value and utilisation now and after the trades:
   coloured by status, bold where the trades make it worse, with the reason
   of an `unknown` one as a tooltip. A trade row the server refuses is marked
   with its reason. With no trades, Check shows the current status.
3. **Stamp CX.LIMIT** writes live `CX.LIMIT` cells at the selection for each
   limit the check shows. It needs a check of the book named in the Book box.

The two reads are separate requests, so a quote can arrive between them.

### Hedge advisor

| UDF | Description |
|---|---|
| `CX.RISKPROFILE(bond, curve, settle, mark, notionalFace, [vol])` | Position risk profile (DV01, KRD buckets, market value) as JSON — feed to `CX.HEDGE`. |
| `CX.HEDGE(strategy, profileJson, curve, settle)` | Hedge proposal grid: trades, residual DV01, cost. `strategy`: `duration_futures`, `barbell_futures`, `cash_bond_pair`, `interest_rate_swap`, `key_rate_futures`. |

### Diagnostics / utilities

| UDF | Description |
|---|---|
| `CX.SCHEMA(typeName)` | JSON schema for any DTO (`Mark`, `BondSpec`, `YasRequest`, …). |
| `CX.MARK(text)` | Parse a textual mark and return its canonical JSON form. |
| `CX.DESCRIBE(handle)` | Inspect a registered object's key fields. |
| `CX.LASTERROR(cellRef)` | Full `[code] message` detail behind an error value in a cell. |
| `CX.DIAG()` | A two-column grid: Rust-library load status, engine and add-in versions, object count, live topics and recomputes, then the server link (connection, session, instruments, books, topics, frames received, frame-to-topic latency, age of the last message) and the log file. |
| `CX.VERSION()` | Library version. |
| `CX.OBJECTS()` | Number of registered objects. |
| `CX.RELEASE(handle)` | Release one handle. |
| `CX.CLEAR()` | Release every handle (and the error log). |
| `CX.LIBRARY(requestJson)` | Run a complete `LibraryRequest` JSON; returns the result envelope as text. |
| `CX.LIBRARY.RANGE(requestCells)` | Same, with the request split across text cells; returns the result in one-column chunks. |
| `CX.MW(...)` | Make-whole call price for a callable bond with a make-whole spread. |

**Frame-to-topic latency** in `CX.DIAG()` is the p50 and p99 of the last 1,024
frames that changed a cell, from the frame's receipt to the last cell it
changed. It stops before Excel's RTD throttle and recalculation, so a cell can
show the value up to `RTD.ThrottleInterval` later.

**Error Log** on the ribbon has two tabs. **Cells** lists each cell's latest
error. **Connection** lists the last 500 connection events, newest first.

**The log file** is `%LOCALAPPDATA%\Convex\Excel\logs\convex-excel.log`. It
records the add-in's start (versions, paths, native load), Excel's exit, failed
identifier lookups and the connection events: link states and their reasons,
sign-in and token renewal, stream errors, refusals and background faults. It
rolls at 10 MB and keeps five files. Cell errors are not written to it, because one
recalculation can raise thousands. Anything that looks like a token is removed
before a line is kept. A second Excel process writes
`convex-excel-<pid>.log`; those files go after 14 days.

## Examples

### Build a fixed-rate bond and price it three ways

```excel
A1: =CX.BOND("US037833100", 0.05, DATE(2035,1,15), DATE(2025,1,15))     ' returns #CX#100
A2: =CX.PRICE(A1, DATE(2025,4,15), "99.5C")                              ' clean price as input
A3: =CX.PRICE(A1, DATE(2025,4,15), "4.65%@SA")                           ' yield mark
A4: =CX.PRICE(A1, DATE(2025,4,15), "99-16+")                             ' 32nds
A5: =CX.PRICE(A1, DATE(2025,4,15), "99.5C", , , "grid")                  ' full grid: clean/dirty/accrued/ytm/z
```

### Build a curve and compute spreads

```excel
B1: =CX.CURVE("USD.SOFR", TODAY(),
              {0.5, 1, 2, 5, 10, 30},
              {0.045, 0.046, 0.047, 0.048, 0.049, 0.050},
              "zero_rate", "linear")
B2: =CX.SPREAD(A1, B1, DATE(2025,4,15), "99.5C", "Z")                    ' Z-spread bps
B3: =CX.SPREAD(A1, B1, DATE(2025,4,15), "99.5C", "Z", , "grid")          ' grid: bps + spread DV01
```

### Government spreads use benchmark securities

Use `CX.LIBRARY` / `CX.LIBRARY.RANGE` with a full `government_spread`
request that carries the benchmark bond/bill quotes and calendars. `CX.SPREAD` has no argument
for benchmark securities and cannot compute a G-spread. Its error names
`params.government`; a zero-rate curve is not accepted as a government benchmark.
See [removed routes and Excel argument migration](../docs/internal/ROUTE-MIGRATION.md).

### Bootstrap a curve from market instruments

```excel
B7: =CX.CURVE.BOOTSTRAP("USD.SOFR.LIVE", TODAY(),
        {"deposit","deposit","swap","swap","swap","swap"},
        {0.25, 0.5, 2, 5, 10, 30},
        {0.0525, 0.0530, 0.0475, 0.0425, 0.0410, 0.0425},
        "global_fit", "linear", "Act360")
```

### Risk grid + KRD

```excel
C1: =CX.RISK(A1, DATE(2025,4,15), "99.5C")                               ' default grid
C2: =CX.RISK(A1, DATE(2025,4,15), "99.5C", , "dv01")                     ' scalar
C3: =CX.RISK(A1, DATE(2025,4,15), "99.5C", B1, "krd", , "2,5,10")        ' KRD grid
```

### FRN price by discount margin

```excel
D1: =CX.BOND.FRN("US-FRN-001", 75, DATE(2030,1,15), DATE(2025,1,15), "sofr", "Quarterly", "Act360")
D2: =CX.PRICE(D1, DATE(2025,4,15), "100D", B1)                           ' price mark
D3: =CX.SPREAD(D1, B1, DATE(2025,4,15), "100D", "DM")                    ' DM in bps
```

## Ribbon

The Convex tab has six groups:

- **Bonds**: New Bond (tabbed: fixed-rate, callable, FRN, zero) · Pricing
  Ticket · Spread Ticket.
- **Curves**: New Curve (tabbed: discrete, bootstrap) · Curve Viewer
  (chart of zero + 1Y forward).
- **Scenarios**: Run Scenario (parallel shift bumps over a CSV bps list).
- **Server**: the connection button (Connected, Connecting, Reconnecting,
  Signed out or No server; click it for details and to reconnect) · Sign in
  · Sign out · Server (see *Connecting to a Convex server*) · Limit Check
  (see *Limits*).
- **Tools**: Objects (browse / inspect / paste handle / release) · Schemas
  (JSON schema viewer for any DTO) · Settings (default frequency, day count,
  spread family, currency persisted to `%APPDATA%\Convex\settings.json`) ·
  Clear All.
- **Help**: About.

Every form routes through the same JSON RPC the cell UDFs use, so the
ribbon and the worksheet can never disagree.

## Connecting to a Convex server

Ribbon **Server** sets the connection. It is saved in
`%APPDATA%\Convex\server.json`; no token is ever written there.

- **Server URL**: `https://host[:port]` of convex-server. Plain `http://` is
  accepted only for this machine, so a sign-in token never crosses a network
  unencrypted. The add-in connects to `/api/v1/ws` on that host.
- **Sign in with**:
  - *Single sign-on (OIDC)*: authorization code with PKCE in your browser,
    returning to a loopback address on this machine (RFC 8252). Set the
    issuer, the client id of a public client, the scope (default `openid
    offline_access`), a `resource` if your identity provider needs one, and
    the redirect host (`127.0.0.1` unless you choose `localhost`, for which
    the add-in listens on both `127.0.0.1` and `::1`; no other host is
    accepted). Register
    `http://127.0.0.1` with any port as a redirect URI. Ribbon **Sign in**
    opens the browser.
    Tokens stay in memory, so after Excel restarts you sign in again. The
    add-in renews the token at 80% of its lifetime. The server reads a
    WebSocket's token only when the connection opens, so a renewal opens a
    second connection, subscribes it, and closes the first once the second
    is current; cells do not go `Stale`.
  - *Pasted token*: for development. The token is held in memory and never
    renewed. Saving with the Token box empty keeps the token you pasted;
    ribbon **Sign out** clears it.
  - *None*: a server in local development mode (`access.mode =
    "local_development"`, loopback only), such as the showcase.
- The server must accept the token (its `[access.introspection]` settings)
  and grant you the `reader` role with result read on the instruments; see
  the server's [ACCESS.md](../crates/convex-server/ACCESS.md). `CX.LIMIT`
  and Limit Check also need book read on the book, each instrument it holds
  and each one a limit names.

One Excel process holds one connection. The server counts HTTP requests and
WebSocket messages against one admission window per user (600 a minute by
default), so the add-in sends no WebSocket keep-alive frames (the server's
heartbeat keeps the connection busy), batches subscription changes, caches
identifier look-ups (up to 10,000) with at most 8 in flight, and waits for
the time a 429 response's `Retry-After` gives. Books are read when they are
first followed and after a reconnect, not polled.

To try it against the showcase: `docker build -t convex-showcase . && docker
run -p 8080:8080 convex-showcase` from the repository root, then Server URL
`http://localhost:8080`, Sign in with *None*, and
`=CX.LIVE("INSTRUMENT-001", "clean_price_mid")`. Its feed moves the price
once a second, and every value's quality is `Demo`.

## DTOs and the wire format

The complete request/response shapes are exposed via `=CX.SCHEMA(...)`:

```excel
=CX.SCHEMA("Mark")                ' textual + tagged JSON forms
=CX.SCHEMA("BondSpec")            ' fixed_rate / callable / floating_rate / zero_coupon / sinking_fund
=CX.SCHEMA("CurveSpec")           ' discrete | bootstrap
=CX.SCHEMA("PricingRequest")
=CX.SCHEMA("PricingResponse")
=CX.SCHEMA("RiskRequest")
=CX.SCHEMA("RiskResponse")
=CX.SCHEMA("SpreadRequest")
=CX.SCHEMA("SpreadResponse")
=CX.SCHEMA("CashflowRequest")
=CX.SCHEMA("CashflowResponse")
=CX.SCHEMA("CurveQueryRequest")
=CX.SCHEMA("CurveQueryResponse")
```

## Conventions

| Field | Convention |
|---|---|
| Coupon rates | Decimal (0.05 = 5%). |
| Spread bps | Basis points (75 = 75 bp). |
| OAS volatility | Decimal (0.01 = 1%). |
| Prices | Per 100 face. |
| Dates | ISO-8601 strings on the wire; Excel `DateTime` cells in UDFs. |
| Frequency | `Annual`, `SemiAnnual`, `Quarterly`, `Monthly`, `Zero` (or `A`/`SA`/`Q`/`M`). |
| Day count | `Act360`, `Act365Fixed`, `ActActIsda`, `ActActIcma`, `Thirty360US`, `Thirty360E`. |
| Compounding | `Annual`, `SemiAnnual`, `Quarterly`, `Monthly`, `Continuous`, `Simple`. |

## Architecture

```
   Excel cell  ─►  CX.* UDF  ─┐
                              ├─►  Cx (P/Invoke)  ─►  convex_ffi.dll  (Rust)
   Excel ribbon ─► form ──────┘                              │
                                                             ▼
                                              registry → typed bonds/curves
                                                             │
                                                             ▼
                                          convex-analytics → convex-bonds /
                                                              convex-curves /
                                                              convex-core
```

The C surface is small. Stateful calls build, describe, release, list, count
and clear bonds and curves. The stateless analytics calls are `convex_price`,
`convex_risk`, `convex_spread`, `convex_cashflows` and `convex_curve_query`,
plus `convex_library` for complete library requests. Utilities are
`convex_schema`, `convex_mark_parse`, `convex_string_free`, `convex_last_error`
and `convex_version`. See the [convex-ffi README](../crates/convex-ffi/README.md)
for the full list.

Bonds and curves live in a thread-safe Rust registry behind opaque
`u64` handles displayed in cells as `#CX#100`, `#CX#101`, ….
`convex_generation` counts registry changes, and `convex_changes_since`
returns the names and handles that changed after a given count, from a log
of the last 4,096 changes. The live layer (`Live/LiveHub.cs`) reads that log
to recompute only the live cells a change reaches.

Server cells go through the same hub. `server/ServerSource.cs` holds the one
WebSocket session with convex-server: it batches subscriptions, keeps each
instrument's newest result, reconnects, and switches to a new session when
the token is renewed. `server/ServerCells.cs` turns those results into the
hub's server topics. Books follow on the same session
(`server/ServerSource.Limits.cs`): each is read from the status route once
subscribed, then changes as the server records status changes.

## Errors

Stateless RPCs return a JSON envelope:

```json
{ "ok": "true",  "result": { ... } }
{ "ok": "false", "error": { "code": "invalid_input", "message": "...", "field": "mark" } }
```

Codes:

- `invalid_input` — bad JSON shape, unparseable mark, missing required field.
- `invalid_handle` — handle not in the registry, or wrong kind for the call.
- `analytics` — solver did not converge, settle ≥ maturity, or other domain error.
- `panic` — an internal engine error was caught at the FFI boundary
  (converted to an error envelope instead of crashing Excel).

The cell UDFs surface these as **standard Excel errors** so `IFERROR` /
`ISERROR` and error propagation work as in any built-in function:

| Excel error | Meaning |
|---|---|
| `#VALUE!` | malformed input — bad mark, unparseable cell, misaligned parallel ranges — or a caught engine `panic` |
| `#NAME?`  | unknown keyword — frequency, day count, spread type, field/metric name |
| `#REF!`   | handle (or ticker) not registered, or wrong object kind |
| `#NUM!`   | solver non-convergence or domain error |
| `#N/A`    | Rust library not loaded — run `=CX.DIAG()`; or a server cell with nothing to show yet (see *Live server cells*) |

The full message behind any error is one call away:
`=CX.LASTERROR(B2)` returns `[code] message` for the failing cell, and
the ribbon **Diagnostics** button reports whether the Rust library loaded.

## Demo workbook

`excel/ConvexDemo.xlsx` ships a Bonds / Curves / Spreads / Scenarios /
Schemas tour. The workbook is a derived artifact — regenerate it from
the canonical script:

```powershell
pip install openpyxl
python excel/build_demo.py
```

Open it with the add-in loaded; every formula should evaluate without
`#NAME?`. Any Excel error in a CX cell carries structured detail
(see *Errors*) — `=CX.LASTERROR(cell)` tells you which argument was
rejected.

## Smoke tests

```powershell
cargo test --locked -p convex-ffi --test smoke -- --test-threads=1
```

End-to-end tests cover: every spread family, FRN/zero/sinking-fund
pricing and risk, KRD, cashflow tag stability, schema introspection, mark
parsing, and the error envelope.

For a manual cell-side checklist see `excel/SMOKE_TEST.md`.

## Library API

See the [library API](../docs/LIBRARY-API.md) for request shapes and limits.
`CX.LIBRARY` returns the result envelope unchanged, with decimal strings and
error diagnostics intact. Coverage of the required operations and host
certification are still open.
