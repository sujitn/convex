#!/bin/sh
# Showcase container entrypoint. Storage is ephemeral: reseed on every boot.
set -eu

rm -rf /app/data/convex.redb /app/data/convex.redb.* /app/data/boot

# Fail fast if the seed does not import or the live configuration does not activate.
/app/convex-core-inputs /app/data/convex.redb /app/showcase/seed.json >/dev/null
instrument=INSTRUMENT-001 # the bond seed.json defines
/app/showcase/live.sh activate /app/data/convex.redb /app /app/showcase /app/data/boot "$instrument"

/app/convex-server /app/showcase/server.toml &
server=$!
caddy run --config /app/showcase/Caddyfile --adapter caddyfile &
edge=$!
/app/showcase/live.sh feed http://127.0.0.1:8081/api/v1/market/quotes "$instrument" &
feed=$!

# On stop, tini -g has signalled the whole group; let all three drain, then exit 0.
trap 'kill -TERM "$server" "$edge" "$feed" 2>/dev/null || true; wait; exit 0' TERM INT

# Boot-time curves are fresh for one day. After 23 hours stop cleanly: the next
# request starts the machine again (auto_start_machines), which reseeds and restamps.
deadline=$(($(date +%s) + 23 * 3600))

# A failed Fly health check only removes the machine from routing; it never
# restarts it. Exit non-zero when any process dies, so the restart policy does.
while kill -0 "$server" 2>/dev/null && kill -0 "$edge" 2>/dev/null && kill -0 "$feed" 2>/dev/null; do
	if [ "$(date +%s)" -ge "$deadline" ]; then
		echo "showcase: daily refresh of the boot-time curves; stopping" >&2
		kill -TERM "$server" "$edge" "$feed" 2>/dev/null || true
		wait
		exit 0
	fi
	sleep 1
done
kill -TERM "$server" "$edge" "$feed" 2>/dev/null || true
wait
exit 1
