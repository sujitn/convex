# Fly.io showcase container

The root `Dockerfile` builds one image (app `convex-pricing-api`) that runs, under `tini`:

- `convex-server` on `127.0.0.1:8081` (config: `server.toml`, profile `demo`,
  access `local_development`, loopback only, no authentication).
- Caddy on public `:8080` (`Caddyfile`), reverse-proxying to the server.

`entrypoint.sh` deletes any old redb state, imports `seed.json` with
`convex-core-inputs`, then starts the server and Caddy. If either process dies,
the entrypoint exits non-zero and Fly's restart policy replaces the container.
The `/health` check only takes a failing machine out of routing; it does not
restart it.

## Public policy (Caddy is the only write guard)

Caddy forwards only the requests listed here; every other request gets
`403 {"error":"read_only_showcase",...}`. The server still applies its own
rules to what is forwarded.

- `GET` and `HEAD` on any path, including the `/ws` upgrade, and CORS `OPTIONS`
  preflights. The server answers only valid preflights for existing routes.
- `POST` on the stateless calculators: `/api/v1/quote`, `/api/v1/batch/price`,
  `/api/v1/analytics` (exact), `/api/v1/runtime/valuation`,
  `/api/v1/reference/resolve`, `/api/v1/curves/bootstrap`, `/api/v1/etf/*`,
  `/api/v1/portfolio/*`, `/api/v1/benchmark/*`, `/api/v1/liquidity/*`,
  `/api/v1/credit/*`

`POST /api/v1/curves` (the durable recipe store) and `POST /api/v1/market/quotes`
(the live quote ingress) are not public.
`/api/v1/curves/bootstrap` calibrates and returns a curve without storing it.

## Storage and seed

Storage (`/app/data/convex.redb`) is ephemeral and reseeded on every boot.
`seed.json` holds one fictional bond (with its issuer, calendar, convention and
identifier mappings, from `crates/convex-ext-file/tests/fixtures/instrument-master.json`),
six synthetic `Demo` curve recipes (from `tools/core_snapshot_workflow.py`) and
one pricing configuration, `SHOWCASE-USD`, that gives USD bonds the `D.OIS`
discount and `P.SOFR` swap-benchmark curves. Regenerate it with
`python3 deploy/showcase/make_seed.py`; the script first materializes the packed
test fixtures it reads.

The seed is a dated capture: everything is recorded at 2026-10-07T12:00:00Z and
the curves' `max_age_ms` is one day. Curve reads and price-by-ID take an
explicit cutoff, so the showcase prices at the capture time, for example
`GET /api/v1/quotes/INSTRUMENT-001?market_price=99.5&known_at=2026-10-07T12:00:00Z`.
Without `known_at` the cutoff is the current time, and the request is refused
with `MISSING_INPUT` (stale curve) rather than priced without curves. The response headers report the
reference, configuration and curve versions consumed.

## CORS

Exact-origin match against `https://convex-demo.pages.dev` only. Cloudflare Pages
preview URLs (`https://<hash>.convex-demo.pages.dev`) will not pass; add them to
`cors_origins` in `server.toml` if needed.

## Run locally

    docker build -t convex-showcase . && docker run -p 8080:8080 convex-showcase

## Not yet available

The live-quote demo feed arrives with phase P2 (HTTP quote ingress); until then
the showcase serves seeded reference data and stateless calculators only.
