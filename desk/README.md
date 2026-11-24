# Convex desk

The trader's screens for a running `convex-server`: the pricing sheet, the RFQ blotter and
the bond calculator. It is a separate build from the operator's `admin/` app, and only the
server publishes it, from its `desk_ui_dir` setting, at `/desk`.

| Screen | Reads | Writes |
|---|---|---|
| Pricing sheet | `/api/v1/reference/masters/pricing_rule`, `/api/v1/bonds`, `/api/v1/market/levels`, the `/api/v1/ws` stream | `POST /api/v1/market/levels` |
| RFQ blotter | `/api/v1/books`, `/api/v1/bonds/:instrument_id`, `/api/v1/rfqs/:rfq_key`, the `/api/v1/ws` stream | `POST /api/v1/rfqs/:rfq_key/quote`, `/pass` |
| Calculator | none | `POST /api/v1/bonds/:instrument_id/calculate` (read-only) |

## The pricing sheet

- **Rows.** One a bond with a pricing rule, in instrument id order: its description,
  CUSIP, coupon, maturity and method (`Price`, `Yield`, `Sprd 10Y`, `Z`, `DM`, `OAS`),
  then its level and its Live clean price, yield, Z-spread, G-spread and DV01.
- **Numbers.** Right-aligned in tabular figures. Prices per 100 at three decimals, and
  US Treasury notes and bonds in 32nds (`99-16+` is 99 16.5/32, `99-162` is 99 16 2/8/32). Yields in
  percent, spreads and margins in basis points. A level is sent back exactly as
  written, never as a float.
- **Live.** The sheet follows every row on the stream once its rows are built.
  Subscribing sends each bond's current result first; after a reconnection or a `RESYNC_REQUIRED` it subscribes
  again, and the newer result of each bond is kept by runtime and revision. A clean
  price that moved flashes green up or red down.
- **State per row** is the server's: `valid`, `partial`, `failed` (hover for the
  error), `stale` by the result's quality, `expired` once past the `live_valid_until`
  the server stamped on it, and `awaiting` before the first result.
- **Desk levels.** When the server names a `desk_level_source`, the bonds whose levels
  the plan reads from it are the desk's (their level is blue):
  - ↑ and ↓ select a row;
  - `+` and `−` (or Shift with ↑ and ↓) move the level one basis point, of spread or
    of yield, and Alt makes it a quarter;
  - Enter types a level: a decimal yield (`0.0525`) or basis points, as the rule's
    field reads; Escape cancels. Either hands the keys back to the sheet.

  Each mark is posted at once, and the bond's next result prices it. While one is in
  flight, a held key's further nudges wait and only the latest level follows, since a
  level is absolute. A refused mark shows the server's code and reason for its level,
  and the bond reverts to the last level the server took. The Mark column is its age.
  The sheet reads every mark again each 5 seconds, so another page's marks appear; a
  read issued before this page's own mark was taken leaves that bond as it is. A server
  restart holds no level, as the Live runtime holds none, so levels are marked again
  after one.

## The RFQ blotter

- **Books.** Every book that is not retired, the first 64 by id, when the identity may list
  books (that needs every book). Otherwise, or to choose, type the books: they are kept in
  the URL (`/desk/rfqs?books=BOOK-IG,BOOK-HY`), and a session follows at most 64.
- **Rows.** Each open request on the books, the soonest deadline first, then the latest
  100 final ones, dimmed. A row shows the time left on the server's clock, the client and
  its tier, the client's side and size (`250K`, `5MM`), the bond, the book, the side of the
  desk's quote it trades on, the auto level (basis points of spread, or yield in percent)
  and price, the check (how many of the book's limits it puts in each status, the worst
  first, as if done), the state and the price quoted.
- **The price.** The auto price rounded to the tick in the desk's favour, so never tighter
  than priced: a bid down, an ask up, to a thousandth, and a Treasury's to a 256th, shown
  in 32nds. What the row shows is the price Send quotes. A bond without a two-way width is
  priced at the mid (`mid`), and one that could not be priced says `unpriced` (hover for
  why); the trader types either's price.
- **Keys.** ↑ and ↓ select a request, and the selection stays on it as rows arrive and
  end:
  - `S` sends the price shown. On a mid or unpriced request it opens the price instead.
  - `A` types a price, from the one shown: ↑ and ↓ move it a tick, Enter sends it as
    written, Escape cancels.
  - `P` passes.
  - Enter opens the request's inputs, Escape closes them.

  Only a priced request before its deadline is answered, once: the keys do nothing while
  an answer is in flight, and a refusal shows the server's code and reason.
- **Inputs.** The drawer reads the request's entries again each time it moves: the request
  and the bond it resolved to; the configuration and the tier's factor; the Live quote,
  the rule, its benchmark set and the two-way the price came from; each limit as checked;
  the quote; who sent each event and when.
- **Live.** The stream sends each open request, then every transition. After a
  reconnection or a `RESYNC_REQUIRED` the page subscribes again, and reads over HTTP each
  request it held open that the new subscription does not name, so one that ended
  meanwhile shows how; one that cannot be read then is read by a subscription 5 seconds
  later. A request the identity may not read is counted as withheld.

## The calculator

One level of a stored bond, by any method (price, yield, benchmark spread to a tenor,
Z-spread, discount margin, OAS), at a settlement date, returns the bond's other
measures, the benchmark a spread was quoted over, and each workout with the worst. The
level is sent as typed, in the method's unit.

## Access

The page calls only its own origin, at most 4 calls at once (the server refuses a subject
past its concurrent requests, 8 by default); the rest wait their turn. The blotter's
identity needs `trader` (answering) and
`reader` on the books, every instrument they hold or their limits name, and the bonds
quoted. In required access mode, paste a bearer token into
the header bar; it stays in page memory, never in storage, the URL or a cookie. The
stream receives it in the `convex.bearer.<base64url token>` subprotocol beside
`convex.v1`, since a browser cannot send `Authorization` on a WebSocket.

## Build and serve

The app is one package of the repository's npm workspace, beside `admin/` and
`api-client/`. From the repository root:

```sh
npm ci
npm run lint -w desk && npm test -w desk && npm run build -w desk   # writes desk/dist
```

Then configure the server and open `http://<host>:<port>/desk/`:

```toml
desk_ui_dir = "<repo>/desk/dist"         # or CONVEX_DESK_UI_DIR
desk_level_source = "desk"               # the plan's levels source; or CONVEX_DESK_LEVEL_SOURCE

[access]
cors_origins = ["http://127.0.0.1:8080"] # the origin the page is opened from
```

## Develop

`npm run dev -w desk` serves on port 3200 and proxies `/api`, the stream included, to
`CONVEX_API_URL`, which defaults to `http://127.0.0.1:8080`. List
`http://localhost:3200` in the server's `cors_origins` while developing.
