#!/bin/sh
# The live showcase on demonstration data. Both commands print nothing on success.
#
#   live.sh activate DATABASE BIN_DIR SHOWCASE_DIR WORK_DIR INSTRUMENT
#     Offline, before the server opens DATABASE: record the showcase book and fresh
#     Demo curves at boot, then compile, validate, stage and activate SHOWCASE-USD
#     under the demo profile's capabilities (`convex-config --demo`).
#   live.sh feed URL INSTRUMENT
#     Post a synthetic indicative mid for INSTRUMENT once a second, for ever.
set -eu

# The current UTC instant at millisecond precision.
stamp() { date -u +%Y-%m-%dT%H:%M:%S.%3NZ; }

# One canonical showcase-feed mid image: INSTRUMENT SESSION SEQUENCE MILLIS MID.
image() {
	printf '{"schema_version":1,"key":{"instrument_id":"%s","source":"showcase-feed"},"session":"%s","sequence":%s,"source_time_ms":%s,"receive_time_ms":%s,"quality":"indicative","entitled":true,"entitlement_tags":[],"evidence_reference":null,"event":{"kind":"image","fields":[{"field":"mid_price","update":{"action":"set","value":{"value":"%s","unit":"price_per100"}}}]}}' \
		"$1" "$2" "$3" "$4" "$4" "$5"
}

# Run convex-config; print its report only when it refuses.
config() {
	report=$("$bin/convex-config" --demo "$db" "$@") || {
		printf '%s\n' "$report" >&2
		return 1
	}
}

activate() {
	db=$1 bin=$2 showcase=$3 work=$4 instrument=$5
	mkdir -p "$work"
	sed "s/@NOW@/$(stamp)/g" "$showcase/boot.template.json" >"$work/boot.json"
	"$bin/convex-core-inputs" "$db" "$work/boot.json" >/dev/null

	# The seed calendar is weekends-only: settle today, or on Monday at a weekend.
	case $(date -u +%u) in
	6) ahead=2 ;;
	7) ahead=1 ;;
	*) ahead=0 ;;
	esac
	cutoff=$(stamp)
	cat >"$work/selection.json" <<EOF
{
  "schema_version": 1,
  "pricing_config": "SHOWCASE-USD",
  "books": ["SHOWCASE-BOOK"],
  "source": "showcase-feed",
  "fields": ["mid_price"],
  "quote_max_age_ms": 600000,
  "max_source_skew_ms": 600000,
  "as_of": "$cutoff",
  "known_at": "$cutoff",
  "mode": "live",
  "settlement_date": "$(date -u -d "+$ahead day" +%F)",
  "measures": ["clean_price_mid", "dirty_price_mid", "accrued_interest", "ytm_mid",
    "modified_duration_mid", "convexity_mid", "dv01_mid", "z_spread_mid"],
  "numerical_settings": {"policy_id": "convex-bond-router-v1", "bump_bps": 1.0},
  "max_concurrent_calcs": 4
}
EOF
	config compile "$work/selection.json"
	printf '%s\n' "$report" >"$work/document.json"

	# The mid market-quote model prices the clean mid at the quoted mid: an exact fixture.
	event=$(image "$instrument" showcase-fixture 1 "$(date +%s%3N)" 99.5)
	printf '%s\n' "$event" >"$work/fixture-events.jsonl"
	at=$(stamp)
	cat >"$work/fixtures.json" <<EOF
[{"id":"showcase-mid","as_of":"$at","known_at":"$at","events":[$event],
  "expected":[{"instrument_id":"$instrument","measure":"clean_price_mid","value":99.5,"tolerance":0.0}]}]
EOF
	cat >"$work/staged.json" <<EOF
{"document":$(cat "$work/document.json"),"idempotency_key":"showcase-stage","actor":"showcase-boot",
 "reason":"compiled from SHOWCASE-BOOK at boot","staged_at":"$(stamp)"}
EOF
	config stage "$work/staged.json" "$work/fixtures.json" "$work/fixture-events.jsonl"
	cat >"$work/activation.json" <<EOF
{"id":"SHOWCASE-USD","configuration_version":1,"expected_revision":0,"idempotency_key":"showcase-activate",
 "actor":"showcase-boot","reason":"start the live showcase","activated_at":"$(stamp)",
 "mode":"restart_required","action":{"kind":"activate"}}
EOF
	config activate "$work/activation.json" "$work/fixtures.json" "$work/fixture-events.jsonl"
}

feed() {
	url=$1 instrument=$2
	session="showcase-$(date +%s)"
	sequence=0
	mid=99.5
	while :; do
		sequence=$((sequence + 1))
		# A bounded random walk of at most two cents a second.
		mid=$(awk -v m="$mid" -v s="$sequence" 'BEGIN {
			srand(s); m += (rand() - 0.5) * 0.04
			if (m < 98) m = 98; if (m > 101) m = 101
			printf "%.4f", m }')
		body="{\"events\":[$(image "$instrument" "$session" "$sequence" "$(date +%s%3N)" "$mid")]}"
		curl -fsS -o /dev/null -H 'content-type: application/json' --data-binary "$body" "$url" ||
			echo "showcase feed: quote post failed" >&2
		sleep 1
	done
}

command=$1
shift
case $command in
activate) activate "$@" ;;
feed) feed "$@" ;;
*)
	echo "usage: live.sh activate DATABASE BIN_DIR SHOWCASE_DIR WORK_DIR INSTRUMENT | feed URL INSTRUMENT" >&2
	exit 2
	;;
esac
