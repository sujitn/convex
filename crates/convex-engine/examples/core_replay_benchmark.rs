//! Sequential measurement over existing native owners and durable core ports.
//! Run via tools/core_replay_benchmark.py; no financial shortcuts or runtime telemetry.
use convex::library::{decode_request, execute, execute_json, LibraryErrorCode};
use convex_core::contracts::*;
use convex_engine::{
    core_workflow::CoreWorkflow,
    ports::{
        core_workflow::*,
        fixing::FixingRepository,
        reference_master::*,
        reference_repository::*,
        runtime::RuntimeStore,
        snapshot::{content_digest, SnapshotRepository},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap, error::Error, hint::black_box, path::Path, sync::Arc, time::Instant,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn failure(message: impl Into<String>) -> Box<dyn Error> {
    std::io::Error::other(message.into()).into()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema_version: u32,
    samples: usize,
    warmup: usize,
    curves: Vec<convex_engine::ports::snapshot::SnapshotCurveRecipe>,
    fixings: Vec<convex_engine::ports::fixing::PublishedFixing>,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    trades: usize,
    receipts_per_bond: usize,
    lifecycle_events: usize,
    classification: String,
    definition: CoreDefinition,
    next_definition: CoreDefinition,
    compact_definition: CoreDefinition,
    plan: CoreSnapshotPlan,
    requests: BTreeMap<String, Value>,
    replays: Vec<Value>,
}

fn sample<T: Serialize>(
    samples: usize,
    warmup: usize,
    mut call: impl FnMut() -> Result<T>,
) -> Result<(Value, T)> {
    let mut times = Vec::new();
    let mut digest = None;
    let mut last = None;
    let mut bytes = 0;
    for index in 0..samples + warmup {
        let start = Instant::now();
        let value = black_box(call()?);
        let elapsed = start.elapsed().as_nanos() as u64;
        // Check determinism outside the measured interval. Never accept failed native work as timing.
        let raw = serde_json::to_vec(&value)?;
        bytes = raw.len();
        let hash = content_digest(b"convex.measurement.output.v1\0", &raw);
        if digest.as_ref().is_some_and(|old| old != &hash) {
            return Err(failure("unstable measurement output"));
        }
        digest = Some(hash);
        if index >= warmup {
            times.push(elapsed);
        }
        last = Some(value);
    }
    let mut sorted = times.clone();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    Ok((
        json!({"samples_ns":times,"median_ns":median,"min_ns":sorted[0],
        "max_ns":sorted[sorted.len()-1],"output_check_digest":digest,"serialized_value_bytes":bytes}),
        last.ok_or_else(|| failure("no samples"))?,
    ))
}

fn native(request: &Value, samples: usize, warmup: usize) -> Result<Value> {
    let wire = request.to_string();
    let decoded = match decode_request(&wire) {
        Ok(value) => value,
        Err(error) if error.code == LibraryErrorCode::ResourceLimit => {
            return Ok(json!({"status":"capacity_rejected","phase":"native_decode","error":error}))
        }
        Err(error) => {
            return Err(failure(format!(
                "invalid benchmark native request: {error:?}"
            )))
        }
    };
    let (decode, _) = sample(samples, warmup, || {
        decode_request(&wire)
            .map(|_| ())
            .map_err(|e| failure(format!("{e:?}")))
    })?;
    let (typed, typed_value) = sample(samples, warmup, || {
        execute(&decoded).map_err(|e| failure(format!("native execution: {e:?}")))
    })?;
    let (json, output) = sample(samples, warmup, || {
        let output = execute_json(&wire);
        // A failed result is rejected below, outside its timed interval.
        Ok(output)
    })?;
    let envelope: Value = serde_json::from_str(&output)?;
    if envelope["ok"] != "true" || envelope["result"] != typed_value {
        return Err(failure("native wire/typed parity failed"));
    }
    Ok(
        json!({"status":"measured","input_bytes":wire.len(),"output_bytes":output.len(),
        "decode":decode,"typed_execute":typed,"wire_execute":json}),
    )
}

fn definition(snapshot: &CoreSnapshot) -> Result<&CoreDefinition> {
    match &snapshot.content.definition.definition {
        MasterDefinition::Calculation(v) => Ok(v),
        _ => Err(failure("calculation required")),
    }
}
fn sizes(snapshot: &CoreSnapshot) -> Result<Value> {
    Ok(json!({"snapshot_bytes":serde_json::to_vec(snapshot)?.len(),
        "definition_bytes":serde_json::to_vec(definition(snapshot)?)?.len(),
        "reference_inputs_bytes":serde_json::to_vec(&definition(snapshot)?.reference_inputs)?.len(),
        "calculation_requests_bytes":serde_json::to_vec(&definition(snapshot)?.calculations)?.len(),
        "curve_bytes":serde_json::to_vec(&snapshot.content.curves)?.len(),
        "fixing_bytes":serde_json::to_vec(&snapshot.content.fixings)?.len(),
        "checkpoint_bytes":serde_json::to_vec(&snapshot.content.checkpoint)?.len()}))
}
fn commit(
    rt: &tokio::runtime::Runtime,
    core: &CoreWorkflow,
    value: CoreDefinition,
    version: u64,
    at: UtcTimestamp,
) -> Result<()> {
    rt.block_on(core.references.commit(
        &ReferenceCommand {
            idempotency_key: RequestId::new(format!("measurement.{version}"))?,
            changes: vec![],
            master_changes: vec![MasterChange {
                expected_version: if version == 1 {
                    None
                } else {
                    Some(Version::new(version - 1)?)
                },
                definition: MasterDefinition::Calculation(Box::new(value)),
            }],
        },
        at,
    ))?;
    Ok(())
}
fn output(
    rt: &tokio::runtime::Runtime,
    core: &CoreWorkflow,
    snapshot: &CoreSnapshot,
    name: &str,
) -> Result<CoreResult> {
    let receipt = rt.block_on(core.run(snapshot.id.clone(), name.into(), false))?;
    let result = rt.block_on(core.result(&receipt.result_id))?;
    let value: Value = serde_json::from_str(&result.output)?;
    if value["ok"] != "true" {
        return Err(failure(format!(
            "managed benchmark {name} failed: {}",
            value["error"]
        )));
    }
    Ok(result)
}

fn measure_case(input: &Input, case: &Case, build: &ModelId) -> Result<Value> {
    if case.replays.is_empty()
        || !case.requests.contains_key("pnl")
        || case.requests.len() != case.definition.calculations.len()
    {
        return Err(failure(
            "complete benchmark replay/request coverage required",
        ));
    }
    let mut report = json!({"id":case.id,"trades":case.trades,"receipts_per_bond":case.receipts_per_bond,
        "classification":case.classification,"lifecycle_events":case.lifecycle_events,
        "definition_bytes":serde_json::to_vec(&case.definition)?.len()});
    // Isolate replay using calendars/publication rules, without market calibration.
    report["bond_lifecycle"] = native(&case.replays[0], input.samples, input.warmup)?;
    let mut minimal = case.replays[0].clone();
    minimal["inputs"]["publications"] = json!([]);
    report["bond_lifecycle_without_unused_publications"] =
        native(&minimal, input.samples, input.warmup)?;
    if report["bond_lifecycle"]["status"] == "measured"
        && report["bond_lifecycle_without_unused_publications"]["status"] == "measured"
    {
        if execute_json(&minimal.to_string()) != execute_json(&case.replays[0].to_string()) {
            return Err(failure("publication-free fixed bond replay changed output"));
        }
        report["bond_resource_comparison"] = json!("financial bytes equal");
    } else {
        report["bond_resource_comparison"] =
            json!("unavailable: original or diagnostic request exceeds ingress capacity");
    }
    match case.definition.validate() {
        Ok(()) => (),
        Err(error)
            if matches!(
                error,
                ReferenceError::CapacityExceeded
                    | ReferenceError::InvalidInput("expected 1..32 core calculations")
            ) =>
        {
            report["status"] = json!("capacity_rejected");
            report["phase"] = json!("core_definition");
            report["error"] = json!(format!("{error:?}"));
            return Ok(report);
        }
        Err(error) => return Err(failure(format!("invalid workload definition: {error:?}"))),
    }
    for (name, request) in &case.requests {
        if let Err(error) = decode_request(&request.to_string()) {
            if error.code != LibraryErrorCode::ResourceLimit {
                return Err(failure(format!("invalid workload {name}: {error:?}")));
            }
            report["status"] = json!("capacity_rejected");
            report["phase"] = json!("native_decode");
            report["calculation"] = json!(name);
            report["error"] = serde_json::to_value(error)?;
            return Ok(report);
        }
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let folder = tempfile::tempdir()?;
    let db = folder.path().join("state.redb");
    let snap = folder.path().join("snapshots");
    let (storage, results, fixings) = convex_ext_redb::create_redb_storage_with_core(&db)?;
    for row in &input.curves {
        rt.block_on(storage.curves.save_recipe(row))?;
    }
    rt.block_on(fixings.publish(&input.fixings))?;
    let snapshots = Arc::new(convex_ext_file::FileSnapshotRepository::new(&snap)?);
    let raw_results = results.clone();
    let core = CoreWorkflow::new(
        storage.bonds.clone(),
        storage.curves.clone(),
        fixings,
        snapshots.clone(),
        results,
        build.clone(),
    );
    drop(storage);
    commit(&rt, &core, case.definition.clone(), 1, case.plan.known_at)?;
    let start = Instant::now();
    let snapshot = rt.block_on(core.assemble(case.plan.clone()))?;
    report["assemble_first_ns"] = json!(start.elapsed().as_nanos() as u64);
    report["sizes"] = sizes(&snapshot)?;
    let reference = definition(&snapshot)?.reference_inputs.clone();
    let (reference_decode, _) = sample(input.samples, input.warmup, || {
        serde_json::from_value::<convex::library::LibraryInputs>(reference.clone())
            .map_err(Into::into)
    })?;
    report["reference_decode"] = reference_decode;
    let (verify, _) = sample(input.samples, input.warmup, || {
        snapshot.verify().map_err(Into::into)
    })?;
    report["snapshot_verify_memory"] = verify;
    let (adapter_read, _) = sample(input.samples, input.warmup, || {
        rt.block_on(snapshots.get_core(&snapshot.id))
            .map_err(Into::into)
    })?;
    report["snapshot_adapter_read_validate"] = adapter_read;
    let (assembly, _) = sample(input.samples, input.warmup, || {
        rt.block_on(core.assemble(case.plan.clone()))
            .map_err(Into::into)
    })?;
    report["assemble_existing"] = assembly;
    let (read, _) = sample(input.samples, input.warmup, || {
        rt.block_on(core.snapshot(&snapshot.id)).map_err(Into::into)
    })?;
    report["snapshot_read_validate"] = read;
    let (pnl, _) = sample(input.samples, input.warmup, || {
        rt.block_on(core.run(snapshot.id.clone(), "pnl".into(), true))
            .map_err(Into::into)
    })?;
    report["pnl_recompute_ack"] = pnl;
    let original = output(&rt, &core, &snapshot, "pnl")?;
    report["pnl_output_bytes"] = json!(original.output.len());
    report["pnl_output_digest"] = json!(original.output_digest);
    report["snapshot_id"] = json!(snapshot.id);
    let (cache, _) = sample(input.samples, input.warmup, || {
        rt.block_on(core.run(snapshot.id.clone(), "pnl".into(), false))
            .map_err(Into::into)
    })?;
    report["pnl_cached_ack"] = cache;
    let (verify, _) = sample(input.samples, input.warmup, || {
        original.verify().map_err(Into::into)
    })?;
    report["result_verify_memory"] = verify;
    let (adapter_read, _) = sample(input.samples, input.warmup, || {
        rt.block_on(raw_results.get_core_result(&original.id))
            .map_err(Into::into)
    })?;
    report["result_adapter_read_validate"] = adapter_read;
    let names: Vec<_> = case
        .definition
        .calculations
        .keys()
        .filter(|n| n.as_str() != "pnl")
        .cloned()
        .collect();
    let (risks, receipts) = sample(input.samples, input.warmup, || {
        names
            .iter()
            .map(|n| {
                rt.block_on(core.run(snapshot.id.clone(), n.clone(), true))
                    .map_err(Into::into)
            })
            .collect::<Result<Vec<_>>>()
    })?;
    report["risk_book_recompute_ack"] = risks;
    let mut native_risks = Vec::new();
    let mut result_bytes = original.output.len();
    for (name, receipt) in names.iter().zip(&receipts) {
        let result = output(&rt, &core, &snapshot, name)?;
        result_bytes += result.output.len();
        if report.get("risk_output_digests").is_none() {
            report["risk_output_digests"] = json!({});
        }
        report["risk_output_digests"][name] = json!(result.output_digest);
        let mut request = case.requests[name].clone();
        request["operation"]["input"]["snapshot_id"] = json!(snapshot.id);
        let expected: Value = serde_json::from_str(&execute_json(&request.to_string()))?;
        native_risks.push(request.to_string());
        if expected["ok"] != "true" || expected != serde_json::from_str::<Value>(&result.output)? {
            return Err(failure("managed risk native parity"));
        }
        if receipt.result_id != result.id {
            return Err(failure("risk result identity"));
        }
    }
    let (native_risk_book, _) = sample(input.samples, input.warmup, || {
        Ok(native_risks
            .iter()
            .map(|r| execute_json(r))
            .collect::<Vec<_>>())
    })?;
    report["native_risk_book_wire"] = native_risk_book;
    report["all_output_bytes"] = json!(result_bytes);
    let (get_result, _) = sample(input.samples, input.warmup, || {
        rt.block_on(core.result(&original.id)).map_err(Into::into)
    })?;
    report["pnl_result_read_validate"] = get_result;
    report["native_pnl"] = native(&case.requests["pnl"], input.samples, input.warmup)?;
    let direct: Value = serde_json::from_str(&execute_json(&case.requests["pnl"].to_string()))?;
    if direct != serde_json::from_str::<Value>(&original.output)? {
        return Err(failure("managed P&L native parity"));
    }
    let before = std::fs::metadata(&db)?.len();
    let mut plan = case.plan.clone();
    let mut pair_outputs = Vec::new();
    for (index, template) in [&case.next_definition, &case.compact_definition]
        .iter()
        .enumerate()
    {
        let version = index as u64 + 2;
        let mut next = (*template).clone();
        next.reference_inputs["pnl_checkpoint"] =
            json!({"snapshot_id":snapshot.id,"calculation":"pnl","build_id":build});
        commit(&rt, &core, next, version, plan.known_at)?;
        plan.definition.version = Version::new(version)?;
        let next_snapshot = rt.block_on(core.assemble(plan.clone()))?;
        let start = Instant::now();
        let result = output(&rt, &core, &next_snapshot, "pnl")?;
        let key = if index == 0 {
            "next_full"
        } else {
            "next_compact"
        };
        report[key] = json!({"sizes":sizes(&next_snapshot)?,"first_pnl_run_and_read_ns":start.elapsed().as_nanos() as u64});
        let (timing, _) = sample(input.samples, input.warmup, || {
            rt.block_on(core.run(next_snapshot.id.clone(), "pnl".into(), true))
                .map_err(Into::into)
        })?;
        report[key]["pnl_recompute_ack"] = timing;
        pair_outputs.push(result.output);
    }
    if pair_outputs[0] != pair_outputs[1] {
        return Err(failure("full/compact checkpoint financial bytes differ"));
    }
    report["full_compact_financial_bytes_equal"] = json!(true);
    report["redb_allocated_bytes"] =
        json!({"before_checkpoints":before,"after_checkpoints":std::fs::metadata(&db)?.len()});
    drop(core);
    drop(raw_results);
    let start = Instant::now();
    let (storage, results, fixings) = convex_ext_redb::create_redb_storage_with_core(&db)?;
    let recovered = CoreWorkflow::new(
        storage.bonds,
        storage.curves,
        fixings,
        Arc::new(convex_ext_file::FileSnapshotRepository::new(&snap)?),
        results,
        build.clone(),
    );
    report["store_reopen_ns"] = json!(start.elapsed().as_nanos() as u64);
    let repeated = output(&rt, &recovered, &snapshot, "pnl")?;
    if repeated != original {
        return Err(failure("reopen changed result"));
    }
    let receipt = rt.block_on(recovered.run(snapshot.id, "pnl".into(), true))?;
    if rt.block_on(recovered.result(&receipt.result_id))? != original {
        return Err(failure("reopen recompute changed result"));
    }
    report["reopen_cache_recompute_equal"] = json!(true);
    report["status"] = json!("measured");
    Ok(report)
}

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| failure("workload path required"))?;
    if std::fs::metadata(&path)?.len() > 64 * 1024 * 1024 {
        return Err(failure("workload exceeds64MiB"));
    }
    let input: Input = serde_json::from_slice(&std::fs::read(Path::new(&path))?)?;
    if input.schema_version != 1
        || !(3..=21).contains(&input.samples)
        || input.warmup > 3
        || input.cases.len() > 20
    {
        return Err(failure("measurement bounds"));
    }
    let build = ModelId::new(format!(
        "measurement.{}",
        content_digest(
            b"convex.measurement.build.v1\0",
            &std::fs::read(std::env::current_exe()?)?
        )
    ))?;
    let mut reports = Vec::new();
    for case in &input.cases {
        eprintln!("Measuring {}", case.id);
        reports.push(measure_case(&input, case, &build)?);
    }
    println!(
        "{}",
        json!({"schema_version":1,"status":"measured","samples":input.samples,"warmup":input.warmup,
        "build_id":build,"cases":reports,"execution_authority":"none","timing_unit":"nanoseconds",
        "scope":"sequential native owners and local redb/file ports; warm OS cache; no HTTP/network/colocation SLA"})
    );
    Ok(())
}
