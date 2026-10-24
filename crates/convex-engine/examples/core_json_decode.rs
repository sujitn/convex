//! Compare complete UTF-8 JSON decoding paths on an actual canonical snapshot.
//! Usage: core_json_decode SNAPSHOT SAMPLES WARMUP. No runtime cache or telemetry.
use convex_engine::ports::{core_workflow::CoreSnapshot, snapshot::MAX_SNAPSHOT_BYTES};
use serde_json::{json, Value};
use std::{error::Error, hint::black_box, io::Read, path::Path, time::Instant};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn require(condition: bool, message: &str) -> Result<()> {
    if !condition {
        return Err(std::io::Error::other(message).into());
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    require(
        args.len() == 4,
        "usage: core_json_decode SNAPSHOT SAMPLES WARMUP",
    )?;
    let samples: usize = args[2].parse()?;
    let warmup: usize = args[3].parse()?;
    require(
        (3..=31).contains(&samples) && (1..=3).contains(&warmup),
        "sample bound",
    )?;
    let path = Path::new(&args[1]);
    let metadata = std::fs::symlink_metadata(path)?;
    require(
        metadata.is_file() && metadata.len() <= MAX_SNAPSHOT_BYTES as u64,
        "snapshot file bound",
    )?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_SNAPSHOT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    require(
        bytes.len() <= MAX_SNAPSHOT_BYTES,
        "snapshot read byte bound",
    )?;
    let original: CoreSnapshot = serde_json::from_slice(&bytes)?;
    original.verify()?;
    require(
        serde_json::to_vec(&original)? == bytes,
        "canonical captured snapshot required",
    )?;
    let mut slice = Vec::with_capacity(samples);
    let mut utf8_str = Vec::with_capacity(samples);
    for index in 0..samples + warmup {
        // Alternate order; both paths include all parsing and allocations. The
        // string path also includes whole-buffer UTF-8 validation on EVERY call.
        for string_first in [index % 2 == 0, index % 2 != 0] {
            let start = Instant::now();
            let decoded: CoreSnapshot = black_box(if string_first {
                serde_json::from_str(std::str::from_utf8(black_box(&bytes))?)?
            } else {
                serde_json::from_slice(black_box(&bytes))?
            });
            let elapsed = start.elapsed().as_nanos() as u64;
            // All comparisons, reserialization and destruction stay outside the
            // interval; a wrong value or byte sequence fails the whole profile.
            require(decoded == original, "decoded inputs changed")?;
            require(
                serde_json::to_vec(&decoded)? == bytes,
                "decoded bytes changed",
            )?;
            decoded.verify()?;
            drop(black_box(decoded));
            if index >= warmup {
                if string_first {
                    utf8_str.push(elapsed);
                } else {
                    slice.push(elapsed);
                }
            }
        }
    }
    fn stats(times: Vec<u64>) -> Value {
        let mut sorted = times.clone();
        sorted.sort_unstable();
        json!({"samples_ns":times,"median_ns":sorted[sorted.len()/2],
            "min_ns":sorted[0],"max_ns":sorted[sorted.len()-1]})
    }
    println!(
        "{}",
        json!({"status":"passed","snapshot_id":original.id,"snapshot_bytes":bytes.len(),
        "samples":samples,"warmup":warmup,"from_slice":stats(slice),"utf8_then_from_str":stats(utf8_str),
        "qualification":"Alternating warm local decode samples. UTF-8 validation is timed; checks and destruction are outside timing. No financial or production latency certification."})
    );
    Ok(())
}
