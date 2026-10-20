//! Offline full-product analytic-model capture and separate-process replay input.
#[path = "../../convex-ports/tests/support/configuration_fixtures.rs"]
mod configuration;
#[path = "../../../tests/fixtures/library/product_ufr.rs"]
mod fixture;
#[path = "../tests/support/calibration_replay.rs"]
mod support;
use convex::library::{decode_calibration_action, decode_calibration_inputs};
use convex_engine::{calibration_replay::*, curve_replay::replay_snapshot_curves};
use convex_ext_file::FileSnapshotRepository;
use convex_ports::snapshot::{MarketSnapshot, SnapshotRepository};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [directory, id] = args.as_slice() else {
        return Err("usage: product_ufr_snapshot SNAPSHOT_DIRECTORY FICTIONAL_CASE_ID".into());
    };
    let case = fixture::cases()
        .into_iter()
        .find(|case| case["id"] == *id)
        .ok_or("unknown supplied fictional case")?;
    let time = support::timestamp("2026-10-07T12:00:00Z");
    let record = capture_calibration(support::unsealed(case["inputs"].clone(), time), time, time)?;
    let snapshot =
        MarketSnapshot::new(support::content(configuration::snapshot::content(), record))?;
    let repository = FileSnapshotRepository::new(directory)?;
    repository.publish(&snapshot).await?;
    drop(repository);
    let reopened = FileSnapshotRepository::new(directory)?
        .get(snapshot.id())
        .await?
        .ok_or("snapshot absent")?;
    let native = replay_snapshot_curves(&reopened)?;
    let mut action = if *id == "flat_forward.rate.positive" {
        fixture::risk_action(&case)
    } else {
        json!({"kind":"build"})
    };
    if action["kind"] == "product_risk" {
        action["snapshot_id"] = json!(snapshot.id());
    }
    let inputs = decode_calibration_inputs(&case["inputs"])?;
    let operation = decode_calibration_action(&inputs, &action.to_string())?;
    let result = replay_calibration_operation(&reopened, &operation)?;
    println!(
        "{}",
        json!({"case_id":id,"snapshot_id":snapshot.id(),"schema_version":3,
        "native_curves":native.curves.len(),"action":action,"result":result,"certified":false})
    );
    Ok(())
}
