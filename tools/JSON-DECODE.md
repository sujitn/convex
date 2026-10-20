# Core snapshot JSON decoding

Run the existing release workflow and snapshot measurements using the commands in
[RELEASE-LATENCY.md](RELEASE-LATENCY.md). They use the same 36/60-trade books,
reference/curve/fixing ports, streaming worker, pricing, risk, hedge proposals and
P&L, including exact saved results and forced recomputation after restart.

To compare decoding paths directly on a retained canonical core snapshot:

```sh
export CARGO_INCREMENTAL=0 RUSTFLAGS=-Dwarnings RUSTDOCFLAGS=-Dwarnings
cargo build --locked --release -p convex-engine --example core_json_decode
target/release/examples/core_json_decode \
  /path/to/state.redb.core-snapshots/DIGEST.core.json 31 3
```

Use the release directory under `CARGO_TARGET_DIR` if set. Replace the example
path with an actual saved snapshot from the existing file adapter. The diagnostic
alternates slice and string decoding on the same bytes. Every string sample
includes whole-buffer UTF-8 validation. Exact typed values, canonical bytes and
the content digest must agree; comparisons and destruction follow each interval.

The file adapter validates UTF-8 once before calling `serde_json::from_str`.
It keeps the same locked parser and floating-point settings. Every read still
opens and bounds the real file, verifies the content digest and requested ID,
and compares canonical serialization with the exact stored bytes. Malformed
UTF-8/JSON, duplicate keys, altered number spellings, whitespace and corrupt
content still fail. Stored data and port contracts are unchanged.

The older snapshot diagnostic's `json_decode` stage deliberately retains
`from_slice` as an independent reference. Its `snapshot_port_read` stage measures
the current real adapter, including the selected decoder and all integrity
checks. Stage medians are not additive. Compare full workflow measurements as
well as this isolated decoder cost; warm local samples do not establish an SLA.
