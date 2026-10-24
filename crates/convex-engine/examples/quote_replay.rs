//! Run an offline deterministic ingestion trace; no wall-clock-generated observations.
use convex_engine::market_ingestion::{IngestionPolicy, MarketDataAssembler};
use convex_ext_file::FileQuoteReplay;
use convex_ports::market_data::StreamError;
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: quote_replay FILE [MAX_KEYS] [STALE_AFTER_MS]")?;
    let policy = IngestionPolicy {
        max_keys: args
            .next()
            .map(|v| v.parse())
            .transpose()?
            .unwrap_or(10_000),
        stale_after_ms: args.next().map(|v| v.parse()).transpose()?.unwrap_or(5_000),
        ..IngestionPolicy::default()
    };
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let mut replay = FileQuoteReplay::open(path)?;
    let mut assembler = MarketDataAssembler::new(policy)?;
    let stdout = std::io::stdout();
    let mut output = std::io::BufWriter::new(stdout.lock());
    let mut replay_time_ms = 0;
    loop {
        match replay.read_event() {
            Ok(event) => {
                replay_time_ms = replay_time_ms.max(event.receive_time_ms);
                let result = assembler.ingest(event, replay_time_ms);
                serde_json::to_writer(&mut output, &result)?;
                output.write_all(b"\n")?;
            }
            Err(StreamError::Closed) => {
                assembler.close();
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }
    output.flush()?;
    Ok(())
}
