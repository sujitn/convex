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

fn compact(book: &mut Book) {
    let data = &book.value["calculations"]["pnl"]["request"]["operation"]["input"];
    let mut rows = serde_json::Map::new();
    for at in data["before"]["holdings"].as_array().unwrap() {
        let life = &data["holdings"][at.as_u64().unwrap() as usize]["lifecycle"];
        if !life["events"].as_array().unwrap().is_empty() {
            rows.insert(
                life["trade_id"].as_str().unwrap().into(),
                life["events"].clone(),
            );
        }
    }
    for holding in book.value["calculations"]["pnl"]["request"]["operation"]["input"]["holdings"]
        .as_array_mut()
        .unwrap()
    {
        let life = &mut holding["lifecycle"];
        if let Some(prefix) = rows.get(life["trade_id"].as_str().unwrap()) {
            let count = prefix.as_array().unwrap().len();
            assert_eq!(
                &life["events"].as_array().unwrap()[..count],
                prefix.as_array().unwrap()
            );
            life["events"] = json!({"prefix":life["trade_id"],"tail":life["events"].as_array().unwrap()[count..]});
        }
    }
    book.value["reference_inputs"]["lifecycle_history"] = Value::Object(rows);
}
async fn recover(snapshot: &CoreSnapshot, path: &Path) -> CoreWorkflow {
    let (empty, results, fixings) =
        convex_ext_redb::create_redb_storage_with_core(path.join("empty.redb")).unwrap();
    let snapshots =
        Arc::new(convex_ext_file::FileSnapshotRepository::new(path.join("snapshots")).unwrap());
    snapshots.publish_core(snapshot).await.unwrap();
    CoreWorkflow::new(
        empty.bonds,
        empty.curves,
        fixings,
        snapshots,
        results,
        ModelId::new("checkpoint.test.build").unwrap(),
    )
}
#[tokio::test]
async fn shared_history_matches_full_replay_and_recovers_with_only_latest_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = seed(dir.path()).await;
    let (first, _, _) = priced(&book).await;
    rebase(&mut book, &first, 8);
    // Repeated point bindings may use separate opaque holding pool entries.
    // Sharing is justified by that actual duplication, not by a unique prefix.
    let data = &mut book.value["calculations"]["pnl"]["request"]["operation"]["input"];
    let duplicate = data["holdings"][0].clone();
    let index = data["holdings"].as_array().unwrap().len();
    data["holdings"].as_array_mut().unwrap().push(duplicate);
    data["steps"][7]["after"]["holdings"][0] = json!(index);
    commit(&book, 2).await;
    let (_, full, _) = priced(&book).await;
    let full_size = book.value.to_string().len();
    compact(&mut book);
    let packed = book.value.to_string().len();
    book.plan.definition.version = Version::new(3).unwrap();
    commit(&book, 3).await;
    let (snapshot, result, value) = priced(&book).await;
    assert_eq!(result.output, full.output);
    assert!(
        packed < full_size,
        "prefix sharing must reduce actual input bytes"
    );
    let mark = &value["result"]["data"]["report"]["trades"][0]["marks"][0];
    assert!(mark["claims"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["outstanding"]["value"] != "0"));
    assert!(mark["state"]["event_ids"].as_array().unwrap().len() > 1);
    let blank = tempfile::tempdir().unwrap();
    let recovered = recover(&snapshot, blank.path()).await;
    let receipt = recovered
        .run(snapshot.id.clone(), "pnl".into(), true)
        .await
        .unwrap();
    assert_eq!(
        recovered.result(&receipt.result_id).await.unwrap().output,
        full.output
    );
}
#[tokio::test]
async fn changed_original_terms_or_opening_prefix_cannot_be_captured() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = seed(dir.path()).await;
    let (first, _, _) = priced(&book).await;
    rebase(&mut book, &first, 8);
    compact(&mut book);
    let saved = book.value.clone();
    let id = book.value["reference_inputs"]["lifecycle_history"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    book.value["reference_inputs"]["lifecycle_history"][&id][0]["event"]["known_at"] =
        json!("2026-10-08T11:00:00Z");
    commit(&book, 2).await;
    assert!(book.core.assemble(book.plan.clone()).await.is_err());
    book.value = saved.clone();
    let life = &mut book.value["reference_inputs"]["lifecycle_history"][&id];
    life[0]["event"]["event_id"] = json!("changed.historical.identity");
    book.plan.definition.version = Version::new(3).unwrap();
    commit(&book, 3).await;
    assert!(book.core.assemble(book.plan.clone()).await.is_err());
    book.value["calculations"]["pnl"]["request"]["operation"]["input"]["holdings"][0]
        ["lifecycle"]["instrument"]["bond"]["terms"]["currency"] = json!("EUR");
    // Restore the actual archive so this case isolates changed original terms.
    book.value["reference_inputs"]["lifecycle_history"] =
        saved["reference_inputs"]["lifecycle_history"].clone();
    book.plan.definition.version = Version::new(4).unwrap();
    commit(&book, 4).await;
    assert!(book.core.assemble(book.plan.clone()).await.is_err());
}
#[tokio::test]
async fn missing_nested_overlong_and_unbound_archives_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = seed(dir.path()).await;
    let (first, _, _) = priced(&book).await;
    rebase(&mut book, &first, 8);
    compact(&mut book);
    commit(&book, 2).await;
    let (good, _, _) = priced(&book).await;
    for case in 0..8 {
        let mut content = good.content.clone();
        let MasterDefinition::Calculation(ref mut value) = content.definition.definition else {
            unreachable!()
        };
        let rows = value.reference_inputs["lifecycle_history"]
            .as_object_mut()
            .unwrap();
        let identity = rows.keys().next().unwrap().clone();
        match case {
            0 => {
                rows.remove(&identity);
            }
            1 => {
                *rows.get_mut(&identity).unwrap() = json!({"prefix":identity,"tail":[]});
            }
            2 => {
                let first = rows[&identity][0].clone();
                *rows.get_mut(&identity).unwrap() = json!(vec![first; 4097]);
            }
            3 => {
                content.checkpoint = None;
            }
            4 => {
                let first = rows[&identity][0].clone();
                *rows.get_mut(&identity).unwrap() = json!(vec![first; 3000]);
            }
            5 => {
                value.calculations.get_mut("pnl").unwrap().request["operation"]["input"]
                    ["holdings"][0] = json!(false);
            }
            6 => {
                value.calculations.get_mut("pnl").unwrap().request["operation"]["input"] = json!(1);
            }
            _ => {
                value.calculations.get_mut("pnl").unwrap().request["operation"]["kind"] =
                    json!("lifecycle_hedge_risk");
            }
        }
        let tampered = CoreSnapshot::new(content).unwrap();
        let blank = tempfile::tempdir().unwrap();
        let recovered = recover(&tampered, blank.path()).await;
        let error = recovered
            .run(tampered.id, "pnl".into(), true)
            .await
            .unwrap_err();
        if case == 4 {
            assert!(error.to_string().contains("32768"), "{error}");
        }
    }
}
