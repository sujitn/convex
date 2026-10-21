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
  `/api/v1/reference/resolve`, `/api/v1/etf/*`, `/api/v1/portfolio/*`,
  `/api/v1/benchmark/*`, `/api/v1/liquidity/*`, `/api/v1/credit/*`

`POST /api/v1/curves` and `/api/v1/curves/bootstrap` are not public. Both
write to the unbounded in-memory `CurveBuilder`, so the frontend's curve
calibration page fails on the showcase until P1 makes bootstrap stateless.

## Storage and seed

Storage (`/app/data/convex.redb`) is ephemeral and reseeded on every boot.
`seed.json` holds one fictional bond (with its issuer, calendar, convention and
identifier mappings, from `crates/convex-ext-file/tests/fixtures/instrument-master.json`)
and six synthetic `Demo` curve recipes (from `tools/core_snapshot_workflow.py`).
Regenerate it with `python3 deploy/showcase/make_seed.py`; the script first
materializes the packed test fixtures it reads.

The curve recipes are a dated capture: `source_time` is 2026-10-07 and
`max_age_ms` is one day. A core snapshot assembled at the current time rejects
them as stale. Nothing public consumes them in this phase. P1, which prices
by curve, needs fresh recipes or an explicit historical cutoff.

## CORS

Exact-origin match against `https://convex-demo.pages.dev` only. Cloudflare Pages
preview URLs (`https://<hash>.convex-demo.pages.dev`) will not pass; add them to
`cors_origins` in `server.toml` if needed.

## Run locally

    docker build -t convex-showcase . && docker run -p 8080:8080 convex-showcase

## Not yet available

The live-quote demo feed arrives with phase P2 (HTTP quote ingress); until then
the showcase serves seeded reference data and stateless calculators only.
