//! Explicit issuer reference selection and complete native closing-book binding.
use super::*;
use convex::library::{
    BookScenarioProductInput, HedgeOperation, LifecycleInput, LifecycleInstrumentInput,
    RiskProductInput,
};
use convex::lifecycle::LifecycleAction;
use convex_ports::issuer_reference::{issuer_scope_pin, IssuerScopeDefinition};

pub(super) fn selection(record: &MasterRecord) -> Result<Option<ReferencePin>> {
    let MasterDefinition::Calculation(value) = &record.definition else {
        return Err(invalid("core calculation reference required"));
    };
    issuer_scope_pin(&value.reference_inputs).map_err(|_| invalid("invalid issuer scope pin"))
}

pub(super) fn native_inputs(value: &serde_json::Value) -> Result<LibraryInputs> {
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("issuer_scope");
        object.remove("pnl_checkpoint");
        object.remove("lifecycle_history");
    }
    serde_json::from_value(value).map_err(|_| invalid("invalid core reference inputs"))
}

pub(super) fn captured(snapshot: &CoreSnapshot) -> Result<Option<&IssuerScopeDefinition>> {
    match (
        selection(&snapshot.content.definition)?,
        &snapshot.content.issuer_scope,
    ) {
        (None, None) => Ok(None),
        (Some(pin), Some(record)) => {
            let MasterDefinition::IssuerScope(value) = &record.definition else {
                return Err(invalid("captured issuer resource kind mismatch"));
            };
            if value.id != pin.id
                || record.version != pin.version
                || record.recorded_at > snapshot.content.plan.known_at
                || !value.effective.contains(snapshot.content.plan.as_of)
                || value
                    .directory
                    .issuers
                    .iter()
                    .filter_map(|s| s.default.as_ref())
                    .any(|n| {
                        n.effective_date.date().as_naive_date()
                            > snapshot.content.plan.as_of.datetime().date_naive()
                    })
            {
                return Err(invalid("issuer resource identity/version/cutoff mismatch"));
            }
            value
                .validate(Some(snapshot.content.plan.known_at))
                .map_err(|_| invalid("invalid captured issuer directory"))?;
            Ok(Some(value))
        }
        _ => Err(invalid("complete captured issuer scope required")),
    }
}

fn product(
    scope: &IssuerScopeDefinition,
    product: &RiskProductInput,
    instrument: &convex::credit::ReferencePin,
    life: Option<&LifecycleInput>,
) -> Result<()> {
    if !matches!(
        product,
        RiskProductInput::Bond { .. } | RiskProductInput::Cds { .. }
    ) {
        return Ok(());
    }
    let mapping = scope
        .directory
        .instruments
        .iter()
        .find(|m| m.id == instrument.id && m.version == instrument.version)
        .ok_or_else(|| invalid("complete exact issuer security mapping required"))?;
    let issuer = scope
        .directory
        .issuers
        .iter()
        .find(|i| i.entity_id == mapping.entity_id)
        .ok_or_else(|| invalid("mapped issuer absent"))?;
    if let RiskProductInput::Cds { terms, .. } = product {
        if terms.reference.entity_id != mapping.entity_id {
            return Err(invalid("CDS reference entity differs from issuer mapping"));
        }
    }
    if let Some(life) = life {
        if let LifecycleInstrumentInput::Bond { credit, .. } = &life.instrument {
            if credit
                .as_ref()
                .is_some_and(|c| c.entity != mapping.entity_id)
            {
                return Err(invalid("bond legal issuer differs from directory"));
            }
        }
        if let Some(notice) = &issuer.default {
            let defaults: Vec<_> = life
                .events
                .iter()
                .filter(|s| matches!(s.event.action, LifecycleAction::CreditDefault(_)))
                .collect();
            if defaults.len() != 1
                || defaults[0].event.event_id.as_str() != notice.id.as_str()
                || defaults[0].event.trade_id != life.trade_id
                || defaults[0].event.effective_date != notice.effective_date.date()
                || defaults[0].event.known_at != notice.known_at
            {
                return Err(invalid(
                    "every issuer-affected holding needs its actual scoped notice/default event",
                ));
            }
        } else if life
            .events
            .iter()
            .any(|s| matches!(s.event.action, LifecycleAction::CreditDefault(_)))
        {
            return Err(invalid("actual default is absent from issuer directory"));
        }
    } else if issuer.default.is_some() {
        return Err(invalid("issuer default requires native lifecycle risk"));
    }
    Ok(())
}

pub(super) fn validate(snapshot: &CoreSnapshot) -> Result<()> {
    let Some(scope) = captured(snapshot)? else {
        return Ok(());
    };
    let value = definition(snapshot)?;
    for calculation in value.calculations.values() {
        let request: LibraryRequest = serde_json::from_value(calculation.request.clone())
            .map_err(|_| invalid("invalid native issuer book request"))?;
        match request.operation {
            LibraryOperation::GraphHedgeRisk {
                product: p, input, ..
            } => product(scope, &p, &input.identity.instrument, None)?,
            LibraryOperation::LifecycleHedgeRisk {
                product: p,
                input,
                lifecycle,
                ..
            } => product(scope, &p, &input.identity.instrument, Some(&lifecycle))?,
            LibraryOperation::BookPnl { input } => {
                let close = &input
                    .steps
                    .last()
                    .ok_or_else(|| invalid("issuer book closing point missing"))?
                    .after;
                if close.point.positions.len() != close.holdings.len() {
                    return Err(invalid("complete issuer book coverage required"));
                }
                for (position, index) in close.point.positions.iter().zip(&close.holdings) {
                    let holding = input
                        .holdings
                        .get(*index)
                        .ok_or_else(|| invalid("issuer holding missing"))?;
                    if holding.id != position.identity.id {
                        return Err(invalid("issuer holding identity mismatch"));
                    }
                    if let BookScenarioProductInput::Product { product: p } = &holding.product {
                        product(
                            scope,
                            p,
                            &position.identity.instrument,
                            Some(&holding.lifecycle),
                        )?;
                    }
                }
            }
            LibraryOperation::Hedge {
                action:
                    HedgeOperation::SolveIssuerLifecycle {
                        scope: admitted, ..
                    },
                ..
            } => {
                let pin = selection(&snapshot.content.definition)?
                    .ok_or_else(|| invalid("issuer pin missing"))?;
                if admitted.reference.id != pin.id
                    || admitted.reference.version != pin.version
                    || admitted.directory != scope.directory
                {
                    return Err(invalid("hedge scope differs from captured issuer source"));
                }
            }
            LibraryOperation::Hedge { .. } => {
                return Err(invalid(
                    "captured issuer scope requires issuer-aware native hedge admission",
                ))
            }
            _ => return Err(invalid("unsupported managed issuer operation")),
        }
    }
    Ok(())
}
