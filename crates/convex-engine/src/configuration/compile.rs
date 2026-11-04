//! Compile a configuration document from stored books, reference terms, one stored
//! pricing configuration and stored curve recipes, all read at one explicit cutoff.
//! Replay and Captured documents pin the recipes; Live documents name the curves only.
//! The result goes through the normal validate → stage → activate use cases.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};

use convex_core::contracts::*;
use convex_ports::{
    configuration::*,
    quote_event::{QuoteField, QuoteKey},
    reference_master::{BookId, MasterDefinition, MasterKey, ReferenceLookup},
    reference_repository::{ReferenceError, ReferenceRepository},
    snapshot::{QuoteRequirement, MAX_SNAPSHOT_INSTRUMENTS},
    storage::{ConfigStore, CurveStore},
};
use serde::{Deserialize, Serialize};

use super::{io, ConfigurationCapabilities, ConfigurationError};
use crate::curve_resolution::{resolve_curves, CurveResolutionError};

/// Books per compiled plan.
pub const MAX_PLAN_BOOKS: usize = 64;

/// What an operator chooses. Every other value is read from the repository at the
/// cutoff; nothing here has a default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanSelection {
    /// Contract version; only 1.
    pub schema_version: u32,
    /// Stored pricing configuration. Its latest version is the configuration identity.
    pub pricing_config: SourceId,
    /// Books whose live positions form the universe, 1 to 64.
    pub books: Vec<BookId>,
    /// Quote source for every instrument; it must be a composed provider.
    pub source: SourceId,
    /// Fields each quote image must carry.
    pub fields: Vec<QuoteField>,
    /// Quote freshness budget.
    pub quote_max_age_ms: i64,
    /// Largest allowed spread between quote source times.
    pub max_source_skew_ms: i64,
    /// Economic instant for books and reference terms.
    pub as_of: UtcTimestamp,
    /// Information cutoff for every stored read.
    pub known_at: UtcTimestamp,
    /// Runtime input mode.
    pub mode: RuntimeMode,
    /// Settlement date for every valuation.
    pub settlement_date: BusinessDate,
    /// Requested measures.
    pub measures: Vec<MeasureId>,
    /// Numerical policy and bump size.
    pub numerical_settings: NumericalSettings,
    /// Concurrent calculation limit.
    pub max_concurrent_calcs: usize,
}

/// Read the selection's inputs at its cutoff and return the document. The runtime
/// identity is `<pricing config>-v<version>`, so each compiled version is its own
/// runtime and keeps its own checkpoint.
pub async fn assemble_from_repository(
    selection: &PlanSelection,
    references: &dyn ReferenceRepository,
    configs: &dyn ConfigStore,
    curves: &dyn CurveStore,
    capabilities: &ConfigurationCapabilities,
) -> Result<ConfigurationDocument, ConfigurationError> {
    if selection.schema_version != 1
        || selection.books.is_empty()
        || selection.books.len() > MAX_PLAN_BOOKS
        || selection.known_at < selection.as_of
    {
        return Err(ConfigurationError::Invalid(
            "selection needs schema 1, 1 to 64 books and known_at at or after as_of",
        ));
    }
    let lookup = ReferenceLookup {
        as_of: selection.as_of,
        known_at: selection.known_at,
    };
    let pricing = io(configs.get(selection.pricing_config.as_str()))
        .await?
        .ok_or(ConfigurationError::Invalid(
            "pricing configuration not found",
        ))?;
    if !pricing.active || pricing.updated_at > selection.known_at.datetime().timestamp_millis() {
        return Err(ConfigurationError::Invalid(
            "pricing configuration is inactive or later than the cutoff",
        ));
    }
    let version = Version::new(pricing.version)
        .map_err(|_| ConfigurationError::Invalid("invalid pricing configuration version"))?;

    let mut universe = BTreeMap::new();
    for book_id in selection.books.iter().collect::<BTreeSet<_>>() {
        let record = references
            .get_master_at(&MasterKey::Book(book_id.clone()), lookup)
            .await
            .map_err(reference_error)?;
        // The lookup returns only a version recorded by known_at and effective at as_of.
        let Some(MasterDefinition::Book(book)) = record.map(|r| r.definition) else {
            return Err(ConfigurationError::Invalid(
                "book not found or not effective at the cutoff",
            ));
        };
        for position in &book.positions {
            universe.insert(
                position.instrument_id.as_str().to_owned(),
                position.instrument_id.clone(),
            );
        }
    }
    if universe.is_empty() {
        return Err(ConfigurationError::Invalid(
            "selected books hold no positions",
        ));
    }
    if universe.len() > MAX_SNAPSHOT_INSTRUMENTS {
        return Err(ConfigurationError::CapacityExceeded);
    }

    let mut instruments = Vec::with_capacity(universe.len());
    for instrument_id in universe.into_values() {
        let record = references
            .get_at(&instrument_id.clone().into(), lookup)
            .await
            .map_err(reference_error)?;
        let Some((version, bond)) = record.and_then(|r| r.bond.map(|b| (r.version, b))) else {
            return Err(ConfigurationError::Invalid(
                "book instrument has no reference terms at the cutoff",
            ));
        };
        if !pricing.applies_to.matches(&bond) {
            return Err(ConfigurationError::Invalid(
                "pricing configuration does not apply to every book instrument",
            ));
        }
        instruments.push(InstrumentPin {
            instrument_id,
            version,
        });
    }

    let roles = &pricing.analytics_curves;
    let mut roots = vec![roles.discount_curve.clone()];
    for id in [
        &roles.government_curve,
        &roles.swap_curve,
        &roles.frn_reference_curve,
    ]
    .into_iter()
    .flatten()
    {
        if !roots.contains(id) {
            roots.push(id.clone());
        }
    }
    let resolved = resolve_curves(curves, &roots, selection.known_at)
        .await
        .map_err(curve_error)?;

    let mode_capability = match selection.mode {
        RuntimeMode::Live => ProviderCapability::LiveImages,
        RuntimeMode::Replay => ProviderCapability::Replay,
        RuntimeMode::Captured => ProviderCapability::CanonicalQuotes,
    };
    let provider = capabilities
        .providers
        .iter()
        .find(|p| p.source == selection.source && p.capabilities.contains(&mode_capability))
        .cloned()
        .ok_or(ConfigurationError::Unsupported(
            "quote source is not a composed provider for the selected mode",
        ))?;
    let quotes = instruments
        .iter()
        .map(|pin| QuoteRequirement {
            key: QuoteKey {
                instrument_id: pin.instrument_id.clone().into(),
                source: selection.source.as_str().to_owned(),
            },
            fields: selection.fields.clone(),
            max_age_ms: selection.quote_max_age_ms,
        })
        .collect();
    let configuration = ConfigurationPin {
        id: selection.pricing_config.clone(),
        version,
    };
    let document = ConfigurationDocument {
        schema_version: 1,
        process: ProcessRequirements {
            storage: capabilities.storage.clone(),
            result_transport: capabilities.result_transport.clone(),
            max_concurrent_calcs: selection.max_concurrent_calcs,
            shared_writers: false,
        },
        providers: vec![provider],
        runtime: RuntimeSettings {
            runtime_id: SourceId::new(format!("{}-v{}", pricing.config_id, pricing.version))
                .map_err(|_| ConfigurationError::Invalid("invalid runtime identity"))?,
            mode: selection.mode,
            settlement_date: selection.settlement_date,
            build_id: capabilities.build_id.clone(),
            measures: selection.measures.clone(),
            numerical_settings: selection.numerical_settings.clone(),
        },
        assembly: SnapshotAssembly {
            as_of: selection.as_of,
            known_at: selection.known_at,
            generation: Version::new(1).map_err(|_| ConfigurationError::Storage)?,
            configuration,
            configuration_capture: None,
            instruments,
            quotes,
            fixings: vec![],
            required_curves: roots
                .iter()
                .map(|id| CurveId::new(id.as_str()))
                .collect::<Result<_, _>>()
                .map_err(|_| ConfigurationError::Invalid("invalid configured curve identity"))?,
            curves: vec![],
            // Live reads each configured curve's current revision at every snapshot (D50);
            // the resolution above still refuses a curve missing or stale at the cutoff.
            curve_recipes: match selection.mode {
                RuntimeMode::Live => vec![],
                RuntimeMode::Replay | RuntimeMode::Captured => resolved.records().to_vec(),
            },
            calibration: None,
            max_source_skew_ms: selection.max_source_skew_ms,
        },
        pricing,
        overrides: vec![],
    };
    document.validate()?;
    Ok(document)
}

fn reference_error(error: ReferenceError) -> ConfigurationError {
    match error {
        ReferenceError::CapacityExceeded => ConfigurationError::CapacityExceeded,
        ReferenceError::UnknownOutcome => ConfigurationError::UnknownOutcome,
        _ => ConfigurationError::Storage,
    }
}

fn curve_error(error: CurveResolutionError) -> ConfigurationError {
    match error {
        CurveResolutionError::Port(error) => error.into(),
        _ => ConfigurationError::Invalid(
            "a configured curve has no fresh recipe of its role at the cutoff",
        ),
    }
}
