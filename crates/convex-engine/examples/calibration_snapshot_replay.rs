//! Actual offline capture/reopen/native quote-risk consumer over original inputs.
#[path = "../../convex-ports/tests/support/configuration_fixtures.rs"]
mod configuration_fixture;
#[path = "../tests/support/calibration_replay.rs"]
mod support;
use convex::library::{decode_calibration_action, decode_calibration_inputs};
use convex_engine::{calibration_replay::*, curve_replay::replay_snapshot_curves};
use convex_ext_file::FileSnapshotRepository;
use convex_ports::snapshot::{MarketSnapshot, SnapshotRepository};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--emit-schema"] {
        println!(
            "{}",
            convex_ports::configuration_schema::configuration_schema()
        );
        return Ok(());
    }
    if args == ["--emit-configuration"] {
        let mut document = configuration_fixture::document(1);
        document.assembly.curves.clear();
        document.assembly.calibration = Some(support::runtime_capture());
        document.validate()?;
        println!("{}", serde_json::to_string(&document)?);
        return Ok(());
    }
    let [mode, directory] = args.as_slice() else {
        return Err("usage: calibration_snapshot_replay --emit-schema | --emit-configuration | demo SNAPSHOT_DIRECTORY".into());
    };
    if mode != "demo" {
        return Err("expected demo".into());
    }
    let case = support::native::original_cases()
        .into_iter()
        .find(|c| c["id"] == "calibration.quote_risk")
        .ok_or("original risk fixture absent")?;
    let inputs = support::native::original_inputs(&case["request"]);
    let parsed = decode_calibration_inputs(&inputs)?;
    let as_of = parsed
        .calibration
        .as_ref()
        .and_then(|c| c.rate_context.as_ref())
        .ok_or("original context absent")?
        .as_of;
    let cutoff = parsed
        .fixings
        .iter()
        .fold(as_of, |time, f| time.max(f.known_at).max(f.published_at));
    let mut draft = support::unsealed(inputs, as_of);
    draft.recorded_at = cutoff;
    let record = capture_calibration(draft, as_of, cutoff)?;
    let snapshot = MarketSnapshot::new(support::content(
        configuration_fixture::snapshot::content(),
        record,
    ))?;
    let repository = FileSnapshotRepository::new(directory)?;
    repository.publish(&snapshot).await?;
    drop(repository);
    let reopened = FileSnapshotRepository::new(directory)?
        .get(snapshot.id())
        .await?
        .ok_or("snapshot absent")?;
    let native = replay_snapshot_curves(&reopened)?;
    let mut action = case["request"]["operation"]["action"].clone();
    action["snapshot_id"] = serde_json::json!(snapshot.id());
    let risk = decode_calibration_action(&parsed, &action.to_string())?;
    let result = replay_calibration_operation(&reopened, &risk)?;
    println!(
        "{}",
        serde_json::to_string(
            &serde_json::json!({"snapshot_id":snapshot.id(),"schema_version":3,"native_curves":native.curves.len(),"risk_action":action,"risk":result,"certified":false})
        )?
    );
    Ok(())
}
