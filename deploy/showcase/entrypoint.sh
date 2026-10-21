#!/bin/sh
# Showcase container entrypoint. Storage is ephemeral: reseed on every boot.
set -eu

rm -rf /app/data/convex.redb /app/data/convex.redb.*

# Fail fast if the seed does not import.
/app/convex-core-inputs /app/data/convex.redb /app/showcase/seed.json

/app/convex-server /app/showcase/server.toml &
server=$!
caddy run --config /app/showcase/Caddyfile --adapter caddyfile &
edge=$!

# On stop, tini -g has signalled the whole group; let both drain, then exit 0.
trap 'kill -TERM "$server" "$edge" 2>/dev/null || true; wait; exit 0' TERM INT

# A failed Fly health check only removes the machine from routing; it never
# restarts it. Exit non-zero when either process dies, so the restart policy does.
while kill -0 "$server" 2>/dev/null && kill -0 "$edge" 2>/dev/null; do
	sleep 1
done
kill -TERM "$server" "$edge" 2>/dev/null || true
wait
exit 1
