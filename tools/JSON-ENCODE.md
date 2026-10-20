# Core snapshot JSON serialization

Build and run the existing 36/60-trade release update and snapshot measurements
using [RELEASE-LATENCY.md](RELEASE-LATENCY.md). They retain the reference, curve,
fixing, snapshot and result ports, streaming worker and full pricing, risk,
hedge-proposal and P&L comparisons, including restart and saved-input replay.

To compare verified serialization on an actual saved canonical snapshot:

```sh
export CARGO_INCREMENTAL=0 RUSTFLAGS=-Dwarnings RUSTDOCFLAGS=-Dwarnings
cargo build --locked --release -p convex-engine --example core_json_encode
target/release/examples/core_json_encode \
  /path/to/state.redb.core-snapshots/DIGEST.core.json 31 3
```

Use the release directory under `CARGO_TARGET_DIR` if set. The diagnostic
alternates the original `verify` plus generic serde serialization with
`CoreSnapshot::canonical_bytes` on the same immutable inputs. Both paths include
content guards and hashing inside timing. Every returned buffer must match the
original generic serde bytes, decoded value and identity; comparison and buffer
destruction follow each interval.

The file adapter uses the verified content serialization to assemble the existing
two-field `id`/`content` JSON envelope. Content is serialized once within each
call. The identity is still encoded with serde. Independent tests compare the
complete envelope with generic serde, including numeric precision, escaping,
optional issuer/checkpoint fields and size boundaries.

Every read still reopens and bounds the real file, checks the content digest and
requested identity, and compares the entire canonical envelope with the raw
bytes. Publication retains its whole-file bound, atomic no-clobber write and
durability acknowledgment. Content and whole-file size limits stay separate;
the canonical-byte helper retains the original content bound, and adapters
apply the whole-file bound. Stored data and port traits are unchanged.

The older snapshot diagnostic retains independent generic `content_json` and
`envelope_json` stages. Its `snapshot_port_read` stage measures the actual changed
adapter. Stage medians are not additive. Compare complete updates as well as
isolated verified serialization; warm local samples do not establish an SLA.
