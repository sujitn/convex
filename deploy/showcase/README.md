# Fly.io showcase container

The root `Dockerfile` builds one image (app `convex-pricing-api`) that runs, under `tini`:

- `convex-server` on `127.0.0.1:8081` (config: `server.toml`, profile `demo`,
  access `local_development`, loopback only, no authentication).
- Caddy on public `:8080` (`Caddyfile`), reverse-proxying to the server.

`entrypoint.sh` deletes any old redb file, imports `seed.json` with
`convex-core-inputs`, starts the server, then execs Caddy. If the server dies,
`/health` through Caddy returns 502 and Fly's health check restarts the machine.

## Public policy (Caddy is the only write guard)

Allowed, everything else gets `403 {"error":"read_only_showcase",...}`:

- `GET`, `HEAD`, `OPTIONS` on any path (including the `/ws` upgrade)
- `POST` on the stateless calculators: `/api/v1/quote`, `/api/v1/batch/price`,
  `/api/v1/analytics` (exact), `/api/v1/runtime/valuation`,
  `/api/v1/reference/resolve`, `/api/v1/curves`, `/api/v1/curves/bootstrap`,
  `/api/v1/etf/*`, `/api/v1/portfolio/*`, `/api/v1/benchmark/*`,
  `/api/v1/liquidity/*`, `/api/v1/credit/*`

`POST /api/v1/curves` is allowed because the curve builder is in-memory and the
container reseeds on every boot.

## Storage and seed

Storage (`/app/data/convex.redb`) is ephemeral and reseeded on every boot.
`seed.json` holds one fictional bond (with its issuer, calendar, convention and
identifier mappings, from `crates/convex-ext-file/tests/fixtures/instrument-master.json`)
and six synthetic `Demo` curve recipes (from `tools/core_snapshot_workflow.py`).
Regenerate with `python deploy/showcase/make_seed.py`.

## CORS

Exact-origin match against `https://convex-demo.pages.dev` only. Cloudflare Pages
preview URLs (`https://<hash>.convex-demo.pages.dev`) will not pass; add them to
`cors_origins` in `server.toml` if needed.

## Run locally

    docker build -t convex-showcase . && docker run -p 8080:8080 convex-showcase

## Not yet available

The live-quote demo feed arrives with phase P2 (HTTP quote ingress); until then
the showcase serves seeded reference data and stateless calculators only.
