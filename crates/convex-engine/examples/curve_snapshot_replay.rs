//! Offline exact recipe replay consumer; no mutable market inputs.
#[path = "../../convex-ports/tests/support/configuration_fixtures.rs"]
mod configuration_fixture;
use configuration_fixture::snapshot as fixture;
#[path = "../tests/support/curve_replay.rs"]
mod support;
use convex_curves::contracts::{CurveKey, CurvePoint, CurveQuery};
use convex_engine::curve_replay::replay_snapshot_curves;
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
        document.assembly.curve_recipes = vec![support::record(support::recipe())];
        document.validate()?;
        convex_engine::curve_replay::rebuild_curve_recipes(&document.assembly.curve_recipes)?;
        println!("{}", serde_json::to_string(&document)?);
        return Ok(());
    }
    if args == ["--schema-one"] {
        let snapshot = MarketSnapshot::new(fixture::content())?;
        println!("{}", String::from_utf8(snapshot.canonical_bytes()?)?);
        return Ok(());
    }
    let [mode, directory] = args.as_slice() else {
        return Err("usage: curve_snapshot_replay --schema-one | --emit-schema | --emit-configuration | demo SNAPSHOT_DIRECTORY".into());
    };
    if mode != "demo" {
        return Err("expected demo".into());
    }
    let snapshot = MarketSnapshot::new(support::content(vec![support::record(support::recipe())]))?;
    let repository = FileSnapshotRepository::new(directory)?;
    repository.publish(&snapshot).await?;
    drop(repository);
    let reopened = FileSnapshotRepository::new(directory)?
        .get(snapshot.id())
        .await?
        .ok_or("snapshot absent")?;
    let built = replay_snapshot_curves(&reopened)?;
    let key: CurveKey = serde_json::from_value(support::recipe()["key"].clone())?;
    let native = built.curves.get(&key)?;
    let query = native.query(&CurveQuery::DiscountFactor {
        point: CurvePoint::Date(convex_core::Date::parse("2028-10-02")?),
    })?;
    println!(
        "{}",
        serde_json::to_string(
            &serde_json::json!({"snapshot_id":reopened.id(),"schema_version":reopened.content().schema_version,"recipe":native.recipe(),"diagnostics":native.diagnostics(),"query":query,"certified":false})
        )?
    );
    Ok(())
}
