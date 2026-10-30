//! Complete supplied valuation contexts and replayable cache identities.
#![forbid(unsafe_code)]

use convex_core::contracts::{
    BusinessDate, MeasureId, ModelId, UtcTimestamp, ValuationSubject, Version,
};
use convex_core::Currency;
use convex_ports::{
    snapshot::{content_digest, MarketSnapshot},
    TraitError,
};
use serde::{Deserialize, Serialize};

pub use convex_ports::configuration::{ConfigurationPin, NumericalSettings};

/// Scenario identity; transformed inputs must be supplied rather than fetched implicitly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ValuationScenario {
    /// Untransformed snapshot.
    Base,
    /// Exact scenario revision (execution requires its owning implementation).
    Named {
        /// Internal scenario identity.
        id: ModelId,
        /// Immutable scenario revision.
        version: Version,
    },
}

/// Complete context specification; validation never fills omitted fields from current state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValuationContextSpec {
    /// Exact immutable information set.
    pub snapshot_id: convex_core::contracts::SnapshotId,
    /// Captured input generation, never a revision read after calculation.
    pub generation: Version,
    /// Exact economic instrument/trade revision.
    pub subject: ValuationSubject,
    /// Economic valuation instant.
    pub valuation_time: UtcTimestamp,
    /// Information cutoff; must match the selected snapshot.
    pub known_at: UtcTimestamp,
    /// Explicit settlement date. Other policies need distinct implementations.
    pub settlement_date: BusinessDate,
    /// Exact configuration.
    pub configuration: ConfigurationPin,
    /// Quantitative model identity.
    pub model_id: ModelId,
    /// Executable/library build identity supplied by the composition root.
    pub build_id: ModelId,
    /// Explicit reporting currency; this slice does not invent FX conversion.
    pub reporting_currency: Currency,
    /// Explicit fixing cutoff; must match the assembled fixing information set.
    pub fixing_known_at: UtcTimestamp,
    /// Requested scenario.
    pub scenario: ValuationScenario,
    /// Actual numerical settings.
    pub numerical_settings: NumericalSettings,
    /// Nonempty unique measure set, normalized by identity.
    pub measures: Vec<MeasureId>,
}

/// Structurally complete immutable context. Binding to resolved inputs is checked separately.
/// Its cache key is derived once, when it is made, and is never serialized (G32).
#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "ValuationContextSpec", into = "ValuationContextSpec")]
pub struct ValuationContext {
    spec: ValuationContextSpec,
    key: String,
}
// Equality and `Debug` read the spec alone; the key is derived from it.
impl PartialEq for ValuationContext {
    fn eq(&self, other: &Self) -> bool {
        self.spec == other.spec
    }
}
impl std::fmt::Debug for ValuationContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ValuationContext").field(&self.spec).finish()
    }
}

impl ValuationContext {
    /// Validate the complete specification and canonicalize only measure ordering.
    pub fn new(mut spec: ValuationContextSpec) -> Result<Self, TraitError> {
        if spec.measures.is_empty()
            || spec.measures.len() > 64
            || spec.numerical_settings.bump_bps.get() <= 0.0
            || spec.numerical_settings.bump_bps.get() > 10_000.0
        {
            return Err(TraitError::InvalidInput(
                "invalid measure set/numerical settings",
            ));
        }
        spec.measures.sort();
        if spec.measures.windows(2).any(|w| w[0] == w[1]) {
            return Err(TraitError::InvalidInput("duplicate requested measure"));
        }
        let bytes = serde_json::to_vec(&spec)
            .map_err(|_| TraitError::InvalidInput("context serialization failed"))?;
        let key = content_digest(b"convex.valuation-context.v1\0", &bytes);
        Ok(Self { spec, key })
    }
    /// All supplied fields; there is no mutable accessor.
    pub fn spec(&self) -> &ValuationContextSpec {
        &self.spec
    }
    /// Require exact snapshot/generation/configuration/reference/time binding before cache access.
    pub fn validate_snapshot(&self, snapshot: &MarketSnapshot) -> Result<(), TraitError> {
        let content = snapshot.content();
        let spec = &self.spec;
        if &spec.snapshot_id != snapshot.id()
            || spec.generation != content.generation
            || spec.valuation_time != content.as_of
            || spec.known_at != content.known_at
            || spec.fixing_known_at != content.known_at
            || spec.configuration.id.as_str() != content.configuration.config_id
            || spec.configuration.version.get() != content.configuration.version
        {
            return Err(TraitError::Conflict(
                "valuation context differs from immutable snapshot",
            ));
        }
        let ValuationSubject::InstrumentVersion {
            instrument_id,
            version,
        } = &spec.subject
        else {
            return Err(TraitError::Unsupported(
                "snapshot trade economics are not implemented",
            ));
        };
        if !snapshot
            .reference(instrument_id.as_str())
            .is_some_and(|reference| &reference.record.version == version)
        {
            return Err(TraitError::Conflict(
                "instrument revision is not pinned by snapshot",
            ));
        }
        Ok(())
    }
    /// SHA-256 of the full canonical context, including generation/build/settings.
    /// Retain this context alongside the key so the run can be replayed and inspected.
    pub fn cache_key(&self) -> &str {
        &self.key
    }
}
impl TryFrom<ValuationContextSpec> for ValuationContext {
    type Error = TraitError;
    fn try_from(value: ValuationContextSpec) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<ValuationContext> for ValuationContextSpec {
    fn from(value: ValuationContext) -> Self {
        value.spec
    }
}
