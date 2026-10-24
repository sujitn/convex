//! Durable bounded-window carry over existing reference, snapshot and result ports.
#![cfg(unix)]
use convex_core::contracts::*;
use convex_engine::{
    core_workflow::CoreWorkflow,
    ports::{
        core_workflow::*, fixing::FixingRepository, reference_master::*, reference_repository::*,
        snapshot::*,
    },
};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};

struct Book {
    core: CoreWorkflow,
    plan: CoreSnapshotPlan,
    value: Value,
}
async fn seed(path: &Path) -> Book {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/library/date-rolls-core.json");
    let data: Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let (storage, results, fixings) =
        convex_ext_redb::create_redb_storage_with_core(path.join("state.redb")).unwrap();
    for row in data["curves"].as_array().unwrap() {
        storage
            .curves
            .save_recipe(&serde_json::from_value(row.clone()).unwrap())
            .await
            .unwrap();
    }
    fixings
        .publish(&serde_json::from_value::<Vec<_>>(data["fixings"].clone()).unwrap())
        .await
        .unwrap();
    let mut value = data["definition"].clone();
    value["calculations"]
        .as_object_mut()
        .unwrap()
        .retain(|name, _| name == "pnl");
    let mut plan: CoreSnapshotPlan = serde_json::from_value(data["plan"].clone()).unwrap();
    plan.definition.version = Version::new(1).unwrap();
    let core = CoreWorkflow::new(
        storage.bonds,
        storage.curves,
        fixings,
        Arc::new(convex_ext_file::FileSnapshotRepository::new(path.join("snapshots")).unwrap()),
        results,
        ModelId::new("checkpoint.test.build").unwrap(),
    );
    let mut book = Book { core, plan, value };
    commit(&book, 1).await;
    book.value["provenance"]["evidence_reference"] = json!("Synthetic checkpoint control");
    book
}
async fn commit(book: &Book, version: u64) {
    book.core
        .references
        .commit(
            &ReferenceCommand {
                idempotency_key: RequestId::new(format!("checkpoint.{version}")).unwrap(),
                changes: vec![],
                master_changes: vec![MasterChange {
                    expected_version: if version == 1 {
                        None
                    } else {
                        Some(Version::new(version - 1).unwrap())
                    },
                    definition: MasterDefinition::Calculation(Box::new(
                        serde_json::from_value(book.value.clone()).unwrap(),
                    )),
                }],
            },
            book.plan.known_at,
        )
        .await
        .unwrap();
}
async fn priced(book: &Book) -> (CoreSnapshot, CoreResult, Value) {
    let snapshot = book.core.assemble(book.plan.clone()).await.unwrap();
    let receipt = book
        .core
        .run(snapshot.id.clone(), "pnl".into(), false)
        .await
        .unwrap();
    let output = book.core.result(&receipt.result_id).await.unwrap();
    let value: Value = serde_json::from_str(&output.output).unwrap();
    assert_eq!(value["ok"], "true", "{value}");
    (snapshot, output, value)
}
fn rebase(book: &mut Book, prior: &CoreSnapshot, causes: usize) {
    let old = book.value["calculations"]["pnl"]["request"]["operation"]["input"].clone();
    let close = old["steps"].as_array().unwrap().last().unwrap()["after"].clone();
    let market = close["market"].as_u64().unwrap() as usize;
    let set = book.value["calculations"]["pnl"]["curve_sets"][market]
        .as_str()
        .unwrap()
        .to_owned();
    let mut before = close.clone();
    before["point"]["fills"] = json!([]);
    before["market"] = json!(0);
    let holdings: Vec<Value> = close["holdings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| old["holdings"][i.as_u64().unwrap() as usize].clone())
        .collect();
    before["holdings"] = json!((0..holdings.len()).collect::<Vec<_>>());
    let mut steps = vec![];
    let version = book.plan.definition.version.get() + 1;
    for n in 0..causes {
        let mut point = before.clone();
        point["market"] = json!(1);
        point["point"]["snapshot_id"] = json!(format!("checkpoint.{version}.{n}"));
        point["point"]["manifest"] = json!({"id":format!("checkpoint.{version}.{n}"),"version":1});
        let point_id = point["point"]["snapshot_id"].clone();
        for fx in point["point"]["fx"].as_array_mut().unwrap() {
            fx["snapshot_id"] = point_id.clone();
        }
        point["point"]["positions"][1]["pending_quantity"] =
            json!((-75000 + version as i64 * 8000 + n as i64 * 1000).to_string());
        steps.push(json!({"cause":"position","after":point}));
    }
    let mut new = old.clone();
    if old
        .get("opening_quantities")
        .and_then(Value::as_array)
        .is_none_or(|r| r.is_empty())
    {
        new["opening_quantities"] = json!(old["before"]["point"]["positions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["current"].clone())
            .collect::<Vec<_>>());
    }
    new["before"] = before;
    new["holdings"] = json!(holdings);
    new["markets"] = json!([old["markets"][market], old["markets"][market]]);
    new["steps"] = json!(steps);
    new["standalone"] = json!((0..causes)
        .map(|_| json!({"kind":"unavailable","reason":"Ordered supplied pending changes"}))
        .collect::<Vec<_>>());
    book.value["calculations"]["pnl"]["request"]["operation"]["input"] = new;
    book.value["calculations"]["pnl"]["curve_sets"] = json!(["opening", "current"]);
    let pins = prior.content.plan.curve_sets[&set].clone();
    book.plan.curve_sets = [
        ("opening".to_string(), pins.clone()),
        ("current".to_string(), pins),
    ]
    .into();
    book.plan.definition.version = Version::new(version).unwrap();
    book.value["reference_inputs"]["pnl_checkpoint"] =
        json!({"snapshot_id":prior.id,"calculation":"pnl","build_id":"checkpoint.test.build"});
}
#[tokio::test]
async fn twenty_four_causes_keep_positions_claims_and_recompute_without_prior_sources() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = seed(dir.path()).await;
    let (first, first_result, old) = priced(&book).await;
    rebase(&mut book, &first, 8);
    commit(&book, 2).await;
    let (second, _, middle) = priced(&book).await;
    assert_eq!(
        middle["result"]["data"]["continuity"]["completed_causes"],
        16
    );
    let old_mark = &old["result"]["data"]["report"]["trades"][0]["marks"][8];
    let start = &middle["result"]["data"]["report"]["trades"][0]["marks"][0];
    for field in ["state", "claims", "local", "recorded_cash"] {
        assert_eq!(old_mark[field], start[field], "{field}");
    }
    assert!(start["claims"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["outstanding"]["value"] != "0"));
    rebase(&mut book, &second, 8);
    commit(&book, 3).await;
    let (third, result, last) = priced(&book).await;
    assert_eq!(last["result"]["data"]["continuity"]["completed_causes"], 24);
    assert_eq!(last["result"]["data"]["continuity"]["completed_windows"], 3);
    assert_eq!(
        book.core.result(&first_result.id).await.unwrap(),
        first_result
    );
    drop(book);
    let blank = tempfile::tempdir().unwrap();
    let (empty, results, fixings) =
        convex_ext_redb::create_redb_storage_with_core(blank.path().join("empty.redb")).unwrap();
    let snapshots = Arc::new(
        convex_ext_file::FileSnapshotRepository::new(blank.path().join("only-latest")).unwrap(),
    );
    snapshots.publish_core(&third).await.unwrap();
    let recovered = CoreWorkflow::new(
        empty.bonds,
        empty.curves,
        fixings,
        snapshots,
        results,
        ModelId::new("checkpoint.test.build").unwrap(),
    );
    let receipt = recovered
        .run(third.id.clone(), "pnl".into(), true)
        .await
        .unwrap();
    assert_eq!(recovered.result(&receipt.result_id).await.unwrap(), result);
}
#[tokio::test]
async fn changed_opening_quantity_events_and_market_fail_before_publication() {
    for change in ["position", "event", "market", "source"] {
        let dir = tempfile::tempdir().unwrap();
        let mut book = seed(dir.path()).await;
        let (first, _, _) = priced(&book).await;
        rebase(&mut book, &first, 1);
        let data = &mut book.value["calculations"]["pnl"]["request"]["operation"]["input"];
        match change {
            "position" => data["before"]["point"]["positions"][0]["pending_quantity"] = json!("1"),
            "event" => {
                data["holdings"][0]["lifecycle"]["events"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }
            "market" => {
                book.plan.curve_sets.get_mut("opening").unwrap()[0].version =
                    Version::new(1).unwrap();
            }
            _ => {
                book.value["reference_inputs"]["pnl_checkpoint"]["snapshot_id"] =
                    json!("sha256:missing")
            }
        }
        commit(&book, 2).await;
        assert!(
            book.core.assemble(book.plan.clone()).await.is_err(),
            "{change}"
        );
        assert_eq!(
            std::fs::read_dir(dir.path().join("snapshots"))
                .unwrap()
                .count(),
            1
        );
    }
}
#[tokio::test]
async fn missing_result_wrong_build_and_duplicate_completed_fill_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = seed(dir.path()).await;
    let first = book.core.assemble(book.plan.clone()).await.unwrap();
    rebase(&mut book, &first, 1);
    commit(&book, 2).await;
    assert!(book.core.assemble(book.plan.clone()).await.is_err());
    book.core
        .run(first.id.clone(), "pnl".into(), false)
        .await
        .unwrap();
    let second = book.core.assemble(book.plan.clone()).await.unwrap();
    let mut content = second.content.clone();
    content.checkpoint.as_mut().unwrap().source.build_id = ModelId::new("different").unwrap();
    let wrong = CoreSnapshot::new(content).unwrap();
    let repo = convex_ext_file::FileSnapshotRepository::new(dir.path().join("snapshots")).unwrap();
    repo.publish_core(&wrong).await.unwrap();
    assert!(book.core.run(wrong.id, "pnl".into(), false).await.is_err());
    let mut content = second.content;
    content.checkpoint.as_mut().unwrap().retired_fill_ids =
        vec!["consumed.fill".into(), "consumed.fill".into()];
    let duplicate = CoreSnapshot::new(content).unwrap();
    repo.publish_core(&duplicate).await.unwrap();
    assert!(book.core.snapshot(&duplicate.id).await.is_err());
}

#[tokio::test]
async fn completed_fills_preserve_original_claim_owner_and_cannot_be_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = seed(dir.path()).await;
    let data = &mut book.value["calculations"]["pnl"]["request"]["operation"]["input"];
    let mut after = data["steps"].as_array().unwrap().last().unwrap()["after"].clone();
    after["point"]["snapshot_id"] = json!("actual.fill.point");
    after["point"]["manifest"] = json!({"id":"actual.fill.point","version":1});
    for fx in after["point"]["fx"].as_array_mut().unwrap() {
        fx["snapshot_id"] = json!("actual.fill.point");
    }
    let position = &mut after["point"]["positions"][1];
    position["current"]["quantity"] = json!("351000");
    let fill = json!({"id":"actual.fill","trade_id":position["identity"]["id"],"instrument":position["identity"]["instrument"],"quantity":"1000",
        "consideration":{"value":"-10","currency":"USD","unit":"currency"},"fees":{"value":"-1","currency":"USD","unit":"currency"},
        "date":after["point"]["valuation_date"],"known_at":after["point"]["known_at"],"source":"actual.recorder"});
    after["point"]["fills"] = json!([fill.clone()]);
    data["steps"]
        .as_array_mut()
        .unwrap()
        .push(json!({"cause":"position","after":after}));
    data["standalone"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"unavailable","reason":"Actual supplied fill"}));
    book.plan.definition.version = Version::new(2).unwrap();
    commit(&book, 2).await;
    let (source, _, _) = priced(&book).await;
    rebase(&mut book, &source, 8);
    commit(&book, 3).await;
    let (snapshot, _, output) = priced(&book).await;
    assert_eq!(
        snapshot
            .content
            .checkpoint
            .as_ref()
            .unwrap()
            .retired_fill_ids,
        vec!["actual.fill"]
    );
    let report = &output["result"]["data"]["report"];
    assert_eq!(
        output["result"]["data"]["continuity"]["completed_causes"],
        17
    );
    assert_eq!(
        report["input"]["points"][0]["positions"][1]["current"]["quantity"],
        "351000"
    );
    assert_eq!(report["opening_quantities"][1]["quantity"], "350000");
    assert_eq!(
        report["trades"][1]["marks"][0]["state"]["position"]["value"],
        "0.35"
    );
    let data = &mut book.value["calculations"]["pnl"]["request"]["operation"]["input"];
    data["steps"][0]["after"]["point"]["fills"] = json!([fill]);
    book.plan.definition.version = Version::new(4).unwrap();
    commit(&book, 4).await;
    let error = book
        .core
        .assemble(book.plan.clone())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("fill already consumed"), "{error}");
}
