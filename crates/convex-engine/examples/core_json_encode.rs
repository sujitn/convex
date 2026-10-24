//! Compare verified JSON encoding paths on an actual canonical snapshot.
//! Usage: core_json_encode SNAPSHOT SAMPLES WARMUP. No runtime cache or telemetry.
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
        "usage: core_json_encode SNAPSHOT SAMPLES WARMUP",
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
    let mut original_path = Vec::with_capacity(samples);
    let mut combined_path = Vec::with_capacity(samples);
    for index in 0..samples + warmup {
        // Alternate order on the same immutable value. Each path validates
        // the original plan/schema/size and content identity inside the interval.
        for combined in [index % 2 == 0, index % 2 != 0] {
            let start = Instant::now();
            let encoded = black_box(if combined {
                black_box(&original).canonical_bytes()?
            } else {
                black_box(&original).verify()?;
                serde_json::to_vec(black_box(&original))?
            });
            let elapsed = start.elapsed().as_nanos() as u64;
            // Exact bytes, typed values and identity are checked outside the
            // interval; returned buffers are also destroyed outside timing.
            require(encoded == bytes, "encoded bytes changed")?;
            let decoded: CoreSnapshot = serde_json::from_slice(&encoded)?;
            require(decoded == original, "encoded inputs changed")?;
            decoded.verify()?;
            drop(black_box(encoded));
            if index >= warmup {
                if combined {
                    combined_path.push(elapsed);
                } else {
                    original_path.push(elapsed);
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
        "samples":samples,"warmup":warmup,"verify_then_encode":stats(original_path),"verified_canonical_bytes":stats(combined_path),
        "qualification":"Alternating warm local verified-encoding samples. All content guards/hash checks are timed; comparison and destruction are outside timing. No financial or production latency certification."})
    );
    Ok(())
}
