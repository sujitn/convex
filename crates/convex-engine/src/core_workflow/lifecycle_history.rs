//! Lossless shared event prefixes. Financial state still comes from native replay.
use super::*;
use convex::library::LifecycleStep;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prefix {
    prefix: String,
    tail: Vec<Value>,
}

fn archives(definition: &CoreDefinition) -> Result<BTreeMap<String, Vec<LifecycleStep>>> {
    let Some(value) = definition.reference_inputs.get("lifecycle_history") else {
        return Ok(BTreeMap::new());
    };
    let rows: BTreeMap<String, Vec<LifecycleStep>> = serde_json::from_value(value.clone())
        .map_err(|_| invalid("invalid lifecycle history archive"))?;
    if rows.is_empty()
        || rows.len() > 128
        || rows.iter().any(|(id, life)| {
            life.is_empty()
                || life.len() > 4096
                || life.iter().any(|s| s.event.trade_id.as_str() != id)
        })
    {
        return Err(invalid("lifecycle history identity/coverage/bounds"));
    }
    Ok(rows)
}

fn nodes(value: &Value) -> usize {
    1 + match value {
        Value::Array(rows) => rows.iter().map(nodes).sum(),
        Value::Object(rows) => rows.values().map(nodes).sum(),
        _ => 0,
    }
}

fn expand_life(
    life: &mut Value,
    rows: &BTreeMap<String, Vec<LifecycleStep>>,
    budget: &mut usize,
) -> Result<()> {
    let Some(events) = life.get("events") else {
        return Err(invalid("lifecycle history events missing"));
    };
    if events.is_array() {
        return Ok(());
    }
    let selection: Prefix = serde_json::from_value(events.clone())
        .map_err(|_| invalid("invalid lifecycle history prefix selection"))?;
    if life.get("trade_id").and_then(Value::as_str) != Some(selection.prefix.as_str()) {
        return Err(invalid("lifecycle history trade mismatch"));
    }
    let original = rows
        .get(&selection.prefix)
        .ok_or_else(|| invalid("missing lifecycle history prefix"))?;
    if original.len().saturating_add(selection.tail.len()) > 4096 {
        return Err(invalid("expanded lifecycle history exceeds4096events"));
    }
    let mut events =
        serde_json::to_value(original).map_err(|_| invalid("lifecycle history serialization"))?;
    events
        .as_array_mut()
        .ok_or_else(|| invalid("lifecycle history event array"))?
        .extend(selection.tail);
    let extra = nodes(&events).saturating_sub(nodes(&life["events"]));
    *budget = budget
        .checked_sub(extra)
        .ok_or_else(|| invalid("expanded lifecycle request exceeds32768nodes"))?;
    life["events"] = events;
    Ok(())
}

/// Expand only the two existing managed lifecycle request locations. This is not
/// a general JSON reference language, and archives cannot reference one another.
pub(super) fn expand(snapshot: &CoreSnapshot, request: &Value) -> Result<Value> {
    let rows = archives(definition(snapshot)?)?;
    if rows.is_empty() {
        return Ok(request.clone());
    }
    let mut request = request.clone();
    let mut budget = 32_768_usize
        .checked_sub(nodes(&request))
        .ok_or_else(|| invalid("native lifecycle request exceeds32768nodes"))?;
    match request.pointer("/operation/kind").and_then(Value::as_str) {
        Some("lifecycle_hedge_risk") => {
            let life = request
                .pointer_mut("/operation/lifecycle")
                .ok_or_else(|| invalid("missing lifecycle risk history"))?;
            expand_life(life, &rows, &mut budget)?;
        }
        Some("book_pnl") => {
            let holdings = request
                .pointer_mut("/operation/input/holdings")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| invalid("missing lifecycle P&L holdings"))?;
            for holding in holdings {
                let life = holding
                    .get_mut("lifecycle")
                    .ok_or_else(|| invalid("missing holding lifecycle"))?;
                expand_life(life, &rows, &mut budget)?;
            }
        }
        _ => (),
    }
    Ok(request)
}

/// Every archived prefix must be the exact opening of this captured P&L window.
/// Existing checkpoint validation binds that opening to the successful previous
/// close, including all source events and original claim ownership.
pub(super) fn validate(snapshot: &CoreSnapshot) -> Result<()> {
    let rows = archives(definition(snapshot)?)?;
    if rows.is_empty() {
        return Ok(());
    }
    let source = snapshot
        .content
        .checkpoint
        .as_ref()
        .ok_or_else(|| invalid("lifecycle compaction requires a captured checkpoint"))?;
    let LibraryOperation::BookPnl { input } =
        materialize(snapshot, &source.source.calculation)?.operation
    else {
        return Err(invalid("lifecycle compaction requires native P&L"));
    };
    let openings: Vec<_> = input
        .before
        .holdings
        .iter()
        .filter_map(|i| input.holdings.get(*i))
        .map(|h| {
            serde_json::to_value(&h.lifecycle.events)
                .map_err(|_| invalid("lifecycle opening serialization"))
        })
        .collect::<Result<Vec<_>>>()?;
    for life in rows.values() {
        let value = serde_json::to_value(life)
            .map_err(|_| invalid("lifecycle history opening serialization"))?;
        if !openings.contains(&value) {
            return Err(invalid("lifecycle archive differs from captured opening"));
        }
    }
    Ok(())
}
