# Convex desk

The trader's screens for a running `convex-server`: the pricing sheet and the bond
calculator. It is a separate build from the operator's `admin/` app, and only the
server publishes it, from its `desk_ui_dir` setting, at `/desk`.

| Screen | Reads | Writes |
|---|---|---|
| Pricing sheet | `/api/v1/reference/masters/pricing_rule`, `/api/v1/bonds`, `/api/v1/market/levels`, the `/api/v1/ws` stream | `POST /api/v1/market/levels` |
| Calculator | none | `POST /api/v1/bonds/:instrument_id/calculate` (read-only) |

## The pricing sheet

- **Rows.** One a bond with a pricing rule, in instrument id order: its description,
  CUSIP, coupon, maturity and method (`Price`, `Yield`, `Sprd 10Y`, `Z`, `DM`, `OAS`),
  then its level and its Live clean price, yield, Z-spread, G-spread and DV01.
- **Numbers.** Right-aligned in tabular figures. Prices per 100 at three decimals, and
  US Treasury notes and bonds in 32nds (`99-16+` is 99 16.5/32, `99-162` is 99 16 2/8/32). Yields in
  percent, spreads and margins in basis points. A level is sent back exactly as
  written, never as a float.
- **Live.** The sheet follows every row on the stream. Subscribing sends each bond's
  current result first; after a reconnection or a `RESYNC_REQUIRED` it subscribes
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
    field reads; Escape cancels.

  Each mark is posted at once, and the bond's next result prices it. While one is in
  flight, a held key's further nudges wait and only the latest level follows, since a
  level is absolute. A refused mark shows its code and reason, and the level reverts.
  The Mark column is its age. The sheet reads every mark again each 5 seconds, so
  another page's marks appear. A server restart holds no level, as the Live runtime
  holds none, so levels are marked again after one.

## The calculator

One level of a stored bond, by any method (price, yield, benchmark spread to a tenor,
Z-spread, discount margin, OAS), at a settlement date, returns the bond's other
measures, the benchmark a spread was quoted over, and each workout with the worst. The
level is sent as typed, in the method's unit.

## Access

The page calls only its own origin. In required access mode, paste a bearer token into
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
