//! Snapshot read diagnostics over captured inputs and the server's redb snapshot port.
//! Use tools/core_snapshot_io.py with the server stopped; no financial calculation or
//! runtime telemetry.
use convex_core::contracts::SnapshotId;
use convex_engine::ports::{
    core_workflow::CoreSnapshot,
    snapshot::{content_digest, SnapshotRepository, MAX_SNAPSHOT_BYTES},
};
use serde_json::{json, Value};
use std::{error::Error, hint::black_box, time::Instant};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn require(condition: bool, message: &str) -> Result<()> {
    if !condition {
        return Err(std::io::Error::other(message).into());
    }
    Ok(())
}
fn measure<T>(
    samples: usize,
    warmup: usize,
    mut call: impl FnMut() -> Result<T>,
    mut check: impl FnMut(&T) -> Result<()>,
) -> Result<Value> {
    let mut times = Vec::with_capacity(samples);
    for index in 0..samples + warmup {
        let start = Instant::now();
        let value = black_box(call()?);
        let elapsed = start.elapsed().as_nanos() as u64;
        // Compare and destroy returned values outside the interval. Failed work
        // never produces an accepted profile. Real read totals are also timed.
        check(&value)?;
        drop(black_box(value));
        if index >= warmup {
            times.push(elapsed);
        }
    }
    let mut sorted = times.clone();
    sorted.sort_unstable();
    Ok(
        json!({"samples_ns": times, "median_ns": sorted[sorted.len()/2],
        "min_ns":sorted[0], "max_ns":sorted[sorted.len()-1]}),
    )
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    require(
        args.len() == 5,
        "usage: core_snapshot_io DATABASE SNAPSHOT_ID SAMPLES WARMUP",
    )?;
    let id = SnapshotId::new(args[2].as_str())?;
    let samples: usize = args[3].parse()?;
    let warmup: usize = args[4].parse()?;
    require(
        (3..=31).contains(&samples) && (1..=3).contains(&warmup),
        "sample bound",
    )?;
    let repository = convex_ext_redb::open_redb_storage(&args[1])?.snapshots;
    let runtime = tokio::runtime::Runtime::new()?;
    let snapshot = runtime
        .block_on(repository.get_core(&id))?
        .ok_or("snapshot absent from the database")?;
    let bytes = snapshot.canonical_bytes()?;
    require(bytes.len() <= MAX_SNAPSHOT_BYTES, "snapshot byte bound")?;
    snapshot.verify()?;
    let content = serde_json::to_vec(&snapshot.content)?;
    let envelope = serde_json::to_vec(&snapshot)?;
    require(envelope == bytes, "canonical captured snapshot required")?;
    let expected = content_digest(b"convex.core-snapshot.v1\0", &content);
    require(
        snapshot.id.as_str() == format!("sha256:{expected}"),
        "snapshot identity",
    )?;
    let stages = json!({
        "json_decode": measure(samples,warmup, || Ok(serde_json::from_slice::<CoreSnapshot>(&bytes)?),
            |v| require(*v==snapshot,"decoded snapshot changed"))?,
        "content_clone": measure(samples,warmup, || Ok(snapshot.content.clone()),
            |v| require(*v==snapshot.content,"cloned content changed"))?,
        "content_json": measure(samples,warmup, || Ok(serde_json::to_vec(&snapshot.content)?),
            |v| require(*v==content,"canonical content changed"))?,
        "content_sha256": measure(samples,warmup, || Ok(content_digest(b"convex.core-snapshot.v1\0",&content)),
            |v| require(*v==expected,"content hash changed"))?,
        "envelope_json": measure(samples,warmup, || Ok(serde_json::to_vec(&snapshot)?),
            |v| require(*v==envelope,"canonical envelope changed"))?,
        "verify": measure(samples,warmup, || Ok(snapshot.verify()?), |_| Ok(()))?,
        "snapshot_port_read": measure(samples,warmup,
            || Ok(runtime.block_on(repository.get_core(&snapshot.id))?),
            |v| require(v.as_ref()==Some(&snapshot),"snapshot port changed inputs"))?
    });
    println!(
        "{}",
        json!({"status":"passed","snapshot_id":snapshot.id,
        "snapshot_bytes":bytes.len(),"content_bytes":content.len(),"samples":samples,"warmup":warmup,
        "input_digest":content_digest(b"convex.snapshot-io.input.v1\0",&bytes),"stages":stages,
        "qualification":"Warm local read diagnostics. Stage medians are not additive; returned-value checking/destruction is outside each stage. No financial, cold-disk or production latency certificate."})
    );
    Ok(())
}
