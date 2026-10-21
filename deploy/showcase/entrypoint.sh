#!/bin/sh
# Showcase container entrypoint. Storage is ephemeral: reseed on every boot.
set -eu

rm -rf /app/data/convex.redb /app/data/convex.redb.*

# Fail fast if the seed does not import.
/app/convex-core-inputs /app/data/convex.redb /app/showcase/seed.json

/app/convex-server /app/showcase/server.toml &

# Caddy is PID-replacing. If convex-server dies, /health through Caddy returns
# 502, so Fly's HTTP health check fails and the machine is restarted.
exec caddy run --config /app/showcase/Caddyfile --adapter caddyfile
