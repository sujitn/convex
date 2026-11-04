//! Explicit P&L window carry; native lifecycle state is always rebuilt and replayed.
use super::*;
use convex::library::{CausalPnlPointBinding, MixedBookPnlInput};
use convex::workflow::{
    accumulate_pnl_windows, continue_pnl_reporting, validate_pnl_window_totals, PnlWindowTotal,
};
use convex_ports::snapshot::content_digest;
use serde_json::{json, Value};

fn source(snapshot: &CoreSnapshot) -> Result<Option<CoreCheckpointSource>> {
    definition(snapshot)?
        .reference_inputs
        .get("pnl_checkpoint")
        .map(|v| {
            let source: CoreCheckpointSource = serde_json::from_value(v.clone())
                .map_err(|_| invalid("invalid P&L checkpoint selection"))?;
            if !valid_name(&source.calculation) {
                return Err(invalid("checkpoint calculation name"));
            }
            Ok(source)
        })
        .transpose()
}
fn book(snapshot: &CoreSnapshot, name: &str) -> Result<MixedBookPnlInput> {
    match materialize(snapshot, name)?.operation {
        LibraryOperation::BookPnl { input } => Ok(*input),
        _ => Err(invalid("checkpoint requires native book P&L")),
    }
}
fn expanded(
    data: &MixedBookPnlInput,
    binding: &CausalPnlPointBinding,
    reset: bool,
) -> Result<Value> {
    let mut point =
        serde_json::to_value(&binding.point).map_err(|_| invalid("checkpoint point"))?;
    if reset {
        point["fills"] = json!([]);
    }
    let holdings = binding
        .holdings
        .iter()
        .map(|i| {
            data.holdings
                .get(*i)
                .ok_or_else(|| invalid("checkpoint holding coverage"))
        })
        .collect::<Result<Vec<_>>>()?;
    let market = data
        .markets
        .get(binding.market)
        .ok_or_else(|| invalid("checkpoint market coverage"))?;
    let mut inputs = data.resources.clone();
    inputs.curves = market.curves.clone();
    inputs.calibration = market.calibration.clone();
    inputs.fixings.extend(binding.additional_fixings.clone());
    inputs
        .publications
        .extend(binding.additional_publications.clone());
    inputs.fixings.sort_by(|a, b| {
        (&a.index_id, a.observation_date, a.revision, &a.source_id).cmp(&(
            &b.index_id,
            b.observation_date,
            b.revision,
            &b.source_id,
        ))
    });
    let origins = if data.opening_quantities.is_empty() {
        data.before
            .point
            .positions
            .iter()
            .map(|p| p.current.clone())
            .collect()
    } else {
        data.opening_quantities.clone()
    };
    Ok(
        json!({"point":point,"holdings":holdings,"inputs":inputs,"opening_quantities":origins,
        "reporting_currency":data.reporting_currency}),
    )
}
fn fingerprint(value: &Value) -> String {
    content_digest(
        b"convex.core-pnl-opening.v1\0",
        value.to_string().as_bytes(),
    )
}
fn totals(report: &Value, scope: &str) -> Result<Vec<PnlWindowTotal>> {
    let rows = report
        .get(scope)
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("checkpoint totals missing"))?;
    rows.iter().map(|r| serde_json::from_value(json!({"basis":r["basis"],"currency":r["currency"],"total":r["attribution"]["total"]}))
        .map_err(|_| invalid("checkpoint native totals invalid"))).collect()
}
#[derive(Serialize)]
struct Continuity {
    completed_windows: u64,
    completed_causes: u64,
    local_totals: Vec<PnlWindowTotal>,
    reporting_totals: Vec<PnlWindowTotal>,
}
fn summary(snapshot: &CoreSnapshot, report: &Value) -> Result<Continuity> {
    let causes = report
        .pointer("/input/causes")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("checkpoint native causes missing"))?
        .len() as u64;
    if !(1..=16).contains(&causes) {
        return Err(invalid("checkpoint native window cause bound"));
    }
    let mut local = totals(report, "local_totals")?;
    let mut reporting = totals(report, "reporting_totals")?;
    let (windows, phases) = if let Some(prior) = &snapshot.content.checkpoint {
        let old_local: Vec<PnlWindowTotal> = serde_json::from_value(prior.local_totals.clone())
            .map_err(|_| invalid("checkpoint local carry"))?;
        let old_reporting: Vec<PnlWindowTotal> =
            serde_json::from_value(prior.reporting_totals.clone())
                .map_err(|_| invalid("checkpoint reporting carry"))?;
        local = accumulate_pnl_windows(&old_local, &local)
            .map_err(|_| invalid("checkpoint local totals mismatch/overflow"))?;
        let input: convex::workflow::CausalPnlInput =
            serde_json::from_value(report["input"].clone())
                .map_err(|_| invalid("checkpoint native path missing"))?;
        let before = input
            .points
            .first()
            .ok_or_else(|| invalid("checkpoint opening point missing"))?;
        let after = input
            .points
            .get(causes as usize)
            .ok_or_else(|| invalid("checkpoint closing point missing"))?;
        reporting = continue_pnl_reporting(
            &old_local,
            &old_reporting,
            &reporting,
            before,
            after,
            input.reporting_currency,
        )
        .map_err(|_| invalid("checkpoint reporting totals/FX mismatch/overflow"))?;
        (
            prior.completed_windows.checked_add(1),
            prior.completed_causes.checked_add(causes),
        )
    } else {
        (Some(1), Some(causes))
    };
    Ok(Continuity {
        completed_windows: windows.ok_or_else(|| invalid("checkpoint window count overflow"))?,
        completed_causes: phases.ok_or_else(|| invalid("checkpoint cause count overflow"))?,
        local_totals: local,
        reporting_totals: reporting,
    })
}
impl CoreWorkflow {
    pub(super) async fn capture_checkpoint(
        &self,
        snapshot: &CoreSnapshot,
    ) -> Result<Option<CoreCheckpoint>> {
        let Some(source) = source(snapshot)? else {
            return Ok(None);
        };
        if source.build_id != self.build {
            return Err(invalid("checkpoint source build differs from host"));
        }
        let previous = self.snapshot(&source.snapshot_id).await?;
        let old_definition = definition(&previous)?;
        let new_definition = definition(snapshot)?;
        if old_definition.id != new_definition.id
            || previous.content.plan.definition.version >= snapshot.content.plan.definition.version
            || previous.content.plan.as_of > snapshot.content.plan.as_of
            || previous.content.plan.known_at > snapshot.content.plan.known_at
            || previous.content.issuer_scope != snapshot.content.issuer_scope
        {
            return Err(invalid("checkpoint book/revision/cutoff/issuer changed"));
        }
        let old = book(&previous, &source.calculation)?;
        let new = book(snapshot, &source.calculation)?;
        let close = old
            .steps
            .last()
            .ok_or_else(|| invalid("checkpoint source closing point missing"))?;
        let closing = expanded(&old, &close.after, true)?;
        let opening = expanded(&new, &new.before, false)?;
        if opening != closing {
            return Err(invalid(
                "checkpoint opening differs from exact saved closing inputs",
            ));
        }
        let result_id = CoreResult::key(&source.snapshot_id, &source.calculation, &source.build_id);
        let result = self.result(&result_id).await?;
        if result.snapshot_id != source.snapshot_id
            || result.calculation != source.calculation
            || result.build_id != source.build_id
        {
            return Err(invalid("checkpoint result identity mismatch"));
        }
        let output: Value = serde_json::from_str(&result.output)
            .map_err(|_| invalid("checkpoint source envelope"))?;
        if output["ok"] != "true" {
            return Err(invalid("checkpoint requires successful priced P&L"));
        }
        let report = &output["result"]["data"]["report"];
        let summary = summary(&previous, report)?;
        let mut ids: BTreeSet<String> = previous
            .content
            .checkpoint
            .as_ref()
            .map(|c| c.retired_fill_ids.iter().cloned().collect())
            .unwrap_or_default();
        for fill in &close.after.point.fills {
            ids.insert(fill.id.as_str().to_owned());
        }
        if ids.len() > 4096 {
            return Err(TraitError::CapacityExceeded.into());
        }
        Ok(Some(CoreCheckpoint {
            source,
            result_id,
            output_digest: result.output_digest,
            opening_digest: fingerprint(&opening),
            completed_windows: summary.completed_windows,
            completed_causes: summary.completed_causes,
            retired_fill_ids: ids.into_iter().collect(),
            local_totals: serde_json::to_value(summary.local_totals)
                .map_err(|_| invalid("checkpoint local summary"))?,
            reporting_totals: serde_json::to_value(summary.reporting_totals)
                .map_err(|_| invalid("checkpoint reporting summary"))?,
        }))
    }
}
pub(super) fn check_build(snapshot: &CoreSnapshot, build: &ModelId) -> Result<()> {
    if snapshot
        .content
        .checkpoint
        .as_ref()
        .is_some_and(|c| &c.source.build_id != build)
    {
        return Err(invalid("checkpoint requires its qualified source build"));
    }
    Ok(())
}
pub(super) fn validate(snapshot: &CoreSnapshot) -> Result<()> {
    match (source(snapshot)?, &snapshot.content.checkpoint) {
        (None, None) => {
            for calculation in definition(snapshot)?.calculations.values() {
                if calculation
                    .request
                    .pointer("/operation/input/opening_quantities")
                    .and_then(Value::as_array)
                    .is_some_and(|v| !v.is_empty())
                    || calculation
                        .request
                        .pointer("/operation/opening_quantities")
                        .and_then(Value::as_array)
                        .is_some_and(|v| !v.is_empty())
                {
                    return Err(invalid(
                        "original opening quantities require a captured checkpoint",
                    ));
                }
            }
            Ok(())
        }
        (Some(source), Some(c)) => {
            let data = book(snapshot, &source.calculation)?;
            let ids: BTreeSet<_> = c.retired_fill_ids.iter().collect();
            if source != c.source
                || c.completed_windows == 0
                || c.completed_causes < c.completed_windows
                || c.completed_causes > c.completed_windows.saturating_mul(16)
                || c.retired_fill_ids.len() > 4096
                || ids.len() != c.retired_fill_ids.len()
                || c.result_id
                    != CoreResult::key(&source.snapshot_id, &source.calculation, &source.build_id)
                || c.output_digest.len() != 64
                || !c.output_digest.bytes().all(|b| b.is_ascii_hexdigit())
                || fingerprint(&expanded(&data, &data.before, false)?) != c.opening_digest
                || definition(snapshot)?
                    .calculations
                    .values()
                    .filter(|v| {
                        v.request.pointer("/operation/kind").and_then(Value::as_str)
                            == Some("book_pnl")
                    })
                    .count()
                    != 1
            {
                return Err(invalid("checkpoint capture/opening/coverage mismatch"));
            }
            for binding in std::iter::once(&data.before)
                .chain(data.steps.iter().map(|s| &s.after))
                .chain(data.standalone.iter().filter_map(|s| match s {
                    convex::library::CausalPnlCounterfactual::Available { information } => {
                        Some(information.as_ref())
                    }
                    _ => None,
                }))
            {
                if binding
                    .point
                    .fills
                    .iter()
                    .any(|f| ids.contains(&f.id.as_str().to_owned()))
                {
                    return Err(invalid(
                        "checkpoint fill already consumed in a prior window",
                    ));
                }
            }
            for v in [&c.local_totals, &c.reporting_totals] {
                let rows: Vec<PnlWindowTotal> = serde_json::from_value(v.clone())
                    .map_err(|_| invalid("checkpoint total schema"))?;
                // Validate units/keys without changing the captured financial digits.
                validate_pnl_window_totals(&rows)
                    .map_err(|_| invalid("checkpoint total units/coverage"))?;
            }
            Ok(())
        }
        _ => Err(invalid("complete captured checkpoint required")),
    }
}
pub(super) fn output(snapshot: &CoreSnapshot, name: &str, output: String) -> Result<String> {
    let Some(c) = &snapshot.content.checkpoint else {
        return Ok(output);
    };
    if name != c.source.calculation {
        return Ok(output);
    }
    let mut value: Value =
        serde_json::from_str(&output).map_err(|_| invalid("checkpoint native envelope"))?;
    if value["ok"] != "true" {
        return Ok(output);
    }
    let totals = summary(snapshot, &value["result"]["data"]["report"])?;
    value["result"]["data"]["continuity"] =
        serde_json::to_value(totals).map_err(|_| invalid("checkpoint output totals"))?;
    serde_json::to_string(&value).map_err(|_| invalid("checkpoint output serialization"))
}
