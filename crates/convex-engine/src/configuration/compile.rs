//! Compile a configuration document from stored books, reference terms, one stored
//! pricing configuration and stored curve recipes, all read at one explicit cutoff.
//! Replay and Captured documents pin the recipes; Live documents name the curves only.
//! The result goes through the normal validate → stage → activate use cases.
//!
//! A book bond with a pricing rule is quoted by its rule's field, and each security of
//! the benchmark set its rule names joins the universe at the version the set pins,
//! quoted by price.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};

use convex_core::contracts::*;
use convex_ports::{
    configuration::*,
    level_pricing::{ResolvedBenchmarkSet, ResolvedPricingRule, MAX_SNAPSHOT_BENCHMARK_SETS},
    quote_event::{QuoteField, QuoteKey},
    reference_master::{BookId, MasterDefinition, MasterKey, ReferenceLookup},
    reference_repository::{ReferenceError, ReferenceRepository},
    snapshot::QuoteRequirement,
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
    /// Where the quotes of bonds quoted by a level come from; absent, from `source`, as
    /// old as `quote_max_age_ms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<LevelQuoteSource>,
}

/// The quote source of the bonds whose pricing rule quotes them by a level, such as a
/// desk's, and how old a level may be. Each such bond takes its one quote from this
/// source, with its rule's field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelQuoteSource {
    /// The levels' source; it must be a composed provider.
    pub source: SourceId,
    /// Quote freshness budget for a level, such as a trading day for a desk's.
    pub max_age_ms: i64,
}

/// One instrument of a compiled universe, and the quote field its rule quotes it by, when
/// that is a level.
struct UniverseEntry {
    pin: InstrumentPin,
    level_field: Option<QuoteField>,
}

/// The benchmark sets one compile has read, each read once.
#[derive(Default)]
struct BenchmarkSets(BTreeSet<ReferenceDataId>);

impl BenchmarkSets {
    /// The set `id` in force at `lookup`, the first time it is named; none after. A set
    /// not in force is refused: a bond quoted to it could not be priced.
    async fn first_read(
        &mut self,
        references: &dyn ReferenceRepository,
        id: &ReferenceDataId,
        lookup: ReferenceLookup,
    ) -> Result<Option<ResolvedBenchmarkSet>, ConfigurationError> {
        if self.0.contains(id) {
            return Ok(None);
        }
        if self.0.len() >= MAX_SNAPSHOT_BENCHMARK_SETS {
            return Err(ConfigurationError::CapacityExceeded);
        }
        let set = references
            .benchmark_set_for_valuation(id, lookup)
            .await
            .map_err(reference_error)?
            .ok_or(ConfigurationError::Invalid(
                "a pricing rule names a benchmark set not in force at the cutoff",
            ))?;
        self.0.insert(id.clone());
        Ok(Some(set))
    }
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
    if universe.len() > selection.mode.max_instruments() {
        return Err(ConfigurationError::CapacityExceeded);
    }

    let mut pins = BTreeMap::new();
    let mut sets = BenchmarkSets::default();
    let mut benchmarks = BTreeMap::new();
    for instrument_id in universe.into_values() {
        let key = instrument_id.clone().into();
        let record = references
            .get_at(&key, lookup)
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
        let rule = references
            .pricing_rule_for_valuation(&key, lookup)
            .await
            .map_err(reference_error)?;
        if let Some(id) = rule
            .as_ref()
            .and_then(|rule| rule.benchmark_set_id.as_ref())
        {
            if let Some(set) = sets.first_read(references, id, lookup).await? {
                for security in set.securities {
                    let version = security.version;
                    let held = benchmarks
                        .entry(security.instrument_id.as_str().to_owned())
                        .or_insert(security);
                    if held.version != version {
                        return Err(ConfigurationError::Invalid(
                            "two benchmark sets pin different versions of one security",
                        ));
                    }
                }
            }
        }
        pins.insert(
            instrument_id.as_str().to_owned(),
            UniverseEntry {
                pin: InstrumentPin {
                    instrument_id,
                    version,
                },
                level_field: level_field(rule.as_ref()),
            },
        );
    }
    // Each benchmark is valued in Live, quoted by price, so its quotes are in every
    // snapshot and its dependents read its price.
    if !benchmarks.is_empty() && !selection.fields.contains(&QuoteField::MidPrice) {
        return Err(ConfigurationError::Invalid(
            "benchmark securities are quoted by price: the selection's fields include mid_price",
        ));
    }
    for (id, security) in benchmarks {
        let is_level_quoted = match pins.get(&id) {
            Some(held) if held.pin.version != security.version => {
                return Err(ConfigurationError::Invalid(
                    "a benchmark set pins another version of a book instrument",
                ))
            }
            Some(held) => held.level_field.is_some(),
            None => {
                let key = security.instrument_id.clone().into();
                let bond = references
                    .get_version(&key, security.version)
                    .await
                    .map_err(reference_error)?
                    .filter(|record| record.recorded_at <= selection.known_at)
                    .and_then(|record| record.bond)
                    .ok_or(ConfigurationError::Invalid(
                        "a benchmark security is not stored at its pinned version by the cutoff",
                    ))?;
                if !pricing.applies_to.matches(&bond) {
                    return Err(ConfigurationError::Invalid(
                        "pricing configuration does not apply to every benchmark security",
                    ));
                }
                let rule = references
                    .pricing_rule_for_valuation(&key, lookup)
                    .await
                    .map_err(reference_error)?;
                let is_level_quoted = level_field(rule.as_ref()).is_some();
                if !is_level_quoted {
                    pins.insert(
                        id,
                        UniverseEntry {
                            pin: InstrumentPin {
                                instrument_id: security.instrument_id,
                                version: security.version,
                            },
                            level_field: None,
                        },
                    );
                }
                is_level_quoted
            }
        };
        if is_level_quoted {
            return Err(ConfigurationError::Invalid(
                "a benchmark security is quoted by price, not by a level",
            ));
        }
    }
    if pins.len() > selection.mode.max_instruments() {
        return Err(ConfigurationError::CapacityExceeded);
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
    let provider = |source: &SourceId| {
        capabilities
            .providers
            .iter()
            .find(|p| &p.source == source && p.capabilities.contains(&mode_capability))
            .cloned()
            .ok_or(ConfigurationError::Unsupported(
                "quote source is not a composed provider for the selected mode",
            ))
    };
    let mut providers = vec![provider(&selection.source)?];
    let levels = selection.levels.as_ref();
    if let Some(levels) = levels.filter(|levels| levels.source != selection.source) {
        providers.push(provider(&levels.source)?);
    }
    // A bond quoted by a level reads its rule's field from the levels' source.
    let quotes = pins
        .values()
        .map(|pin| {
            let level_source = levels.filter(|_| pin.level_field.is_some());
            QuoteRequirement {
                key: QuoteKey {
                    instrument_id: pin.pin.instrument_id.clone().into(),
                    source: level_source
                        .map_or(&selection.source, |levels| &levels.source)
                        .as_str()
                        .to_owned(),
                },
                fields: pin
                    .level_field
                    .map_or_else(|| selection.fields.clone(), |field| vec![field]),
                max_age_ms: level_source
                    .map_or(selection.quote_max_age_ms, |levels| levels.max_age_ms),
            }
        })
        .collect();
    let instruments = pins.into_values().map(|pin| pin.pin).collect();
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
        providers,
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

/// The quote field a rule quotes its bond by, when that is a level.
fn level_field(rule: Option<&ResolvedPricingRule>) -> Option<QuoteField> {
    rule.and_then(|rule| rule.method.level_field())
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
