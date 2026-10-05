//! Provider-neutral reference definitions and deterministic history selection.
//! This module owns no clock, runtime, database or pricing implementation.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use convex_core::calendars::BusinessDayConvention;
use convex_core::contracts::{
    BusinessDate, EntityId, ExactDecimal, InstrumentId, ModelId, ReferenceDataId, SchemaVersion,
    SourceId, UtcTimestamp, Version,
};
use convex_core::daycounts::DayCountConvention;
use serde::{Deserialize, Serialize};

use crate::reference_data::{BondReferenceData, BondType};
use crate::reference_repository::{ReferenceError, ReferenceRecord, MAX_REFERENCE_VERSIONS};

/// Currently dispatched compiled calendar rule family. Bump when its semantics change.
pub const SUPPORTED_BOND_CALENDAR_RULES: &str = "convex-calendar-v1";

/// Economic time and information cutoff; both are supplied, never inferred from now().
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceLookup {
    /// Economic instant being reconstructed.
    pub as_of: UtcTimestamp,
    /// Latest information permitted in the reconstruction.
    pub known_at: UtcTimestamp,
}

/// Half-open economic validity interval [from, until).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveInterval {
    /// Inclusive economic start.
    pub from: UtcTimestamp,
    /// Exclusive economic end, or unbounded.
    pub until: Option<UtcTimestamp>,
}

impl EffectiveInterval {
    /// Reject reversed or empty validity intervals.
    pub fn validate(self) -> Result<(), ReferenceError> {
        if self.until.is_some_and(|end| end <= self.from) {
            return Err(ReferenceError::InvalidInput("invalid effective interval"));
        }
        Ok(())
    }
    /// Test exact boundary inclusion without changing precision.
    pub fn contains(self, time: UtcTimestamp) -> bool {
        time >= self.from && self.until.is_none_or(|end| time < end)
    }
}

/// Permitted evidence reference; no vendor SDK or raw licensed payload is required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceProvenance {
    /// Provider-neutral producing source.
    pub source: SourceId,
    /// Bounded retained evidence location or permitted hash.
    pub evidence_reference: String,
}

/// The closed set of calendars the current bond builder actually dispatches.
/// Additional/custom/joint calendars require their own implementation and fixtures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BondCalendar {
    /// Saturday/Sunday weekends and no holidays, explicitly chosen.
    WeekendsOnly,
    /// Existing SIFMA holiday implementation.
    Sifma,
    /// Existing TARGET2 holiday implementation.
    Target2,
}

/// Explicit irregular-period policy. Current service admission supports none and short first stubs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BondStub {
    /// No irregular period.
    None,
    /// Explicit short first period.
    ShortFirst,
    /// Explicit short last period.
    ShortLast,
    /// Long first period; currently refused rather than shortened silently.
    LongFirst,
    /// Long last period; currently refused rather than shortened silently.
    LongLast,
}

/// Exact resource pin; zero never means current.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferencePin {
    /// Internal resource identity.
    pub id: ReferenceDataId,
    /// Exact immutable version.
    pub version: Version,
}

/// Exact entity pin for issuer/parent relationships.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityPin {
    /// Internal entity identity.
    pub entity_id: EntityId,
    /// Exact immutable attributes version.
    pub version: Version,
}

/// Explicit supported scheduling/settlement settings; none are inferred from currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BondConvention {
    /// Unambiguous day-count variant, checked against the v1 label.
    pub day_count: DayCountConvention,
    /// Payment date adjustment in the current shared schedule implementation.
    pub business_day_convention: BusinessDayConvention,
    /// End-of-month schedule policy.
    pub end_of_month: bool,
    /// Settlement lag, bounded at ten business days.
    pub settlement_days: u32,
    /// Calendar-day ex-coupon period, zero explicitly means no exclusion.
    pub ex_coupon_days: u32,
}

impl BondConvention {
    /// Validate supported resource bounds; convention certification is separate.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        if self.settlement_days > 10 || self.ex_coupon_days > 30 {
            return Err(ReferenceError::InvalidInput(
                "invalid settlement/ex-coupon lag",
            ));
        }
        Ok(())
    }
}

/// Complete additional R0 terms omitted by the earlier reference contract.
/// First coupon/frequency/face/currency remain explicit on BondReferenceData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BondContractTerms {
    /// Economic validity of this terms version.
    pub effective: EffectiveInterval,
    /// Contractual accrual start, independent of issue date.
    pub dated_date: BusinessDate,
    /// Optional back-stub boundary.
    pub penultimate_coupon_date: Option<BusinessDate>,
    /// Explicit supported stub policy, checked against supplied boundary dates.
    pub stub: BondStub,
    /// Payment delay in business days; zero is the currently supported path.
    pub payment_lag: u32,
    /// Contractual final redemption amount in face units.
    pub redemption_value: ExactDecimal,
    /// Exact issuer attributes version, matching issuer_id.
    pub issuer: EntityPin,
    /// Exact calendar resource version.
    pub calendar: ReferencePin,
    /// Explicit calendar dispatch, verified against the pinned resource before service valuation.
    pub calendar_code: BondCalendar,
    /// Exact convention resource version.
    pub convention: ReferencePin,
    /// Explicit contractual settings, verified against the pinned resource.
    pub settings: BondConvention,
    /// Terms evidence, separate from recording time.
    pub provenance: ReferenceProvenance,
}

/// Versioned legal/issuer/reference entity attributes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityDefinition {
    /// Internal entity identity.
    pub entity_id: EntityId,
    /// Full supplied legal name.
    pub legal_name: String,
    /// Optional exact parent attributes pin.
    pub parent: Option<EntityPin>,
    /// Explicit jurisdiction/country label, not a currency fallback.
    pub country: String,
    /// Economic validity.
    pub effective: EffectiveInterval,
    /// Source evidence.
    pub provenance: ReferenceProvenance,
}

/// Versioned calendar dispatch and applicable date coverage.
/// Rule version is retained in replay provenance; build compatibility is a release concern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarDefinition {
    /// Internal calendar identity.
    pub calendar_id: ReferenceDataId,
    /// Actual supported pure-library implementation.
    pub code: BondCalendar,
    /// Explicit implementation/rule version.
    pub rule_version: ModelId,
    /// First supported date, inclusive.
    pub coverage_from: BusinessDate,
    /// Last supported date, inclusive.
    pub coverage_until: BusinessDate,
    /// Economic validity.
    pub effective: EffectiveInterval,
    /// Source evidence.
    pub provenance: ReferenceProvenance,
}

/// Versioned, explicit convention pack. It supplies no silent defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConventionDefinition {
    /// Internal convention identity.
    pub convention_id: ReferenceDataId,
    /// Complete supported settings.
    pub settings: BondConvention,
    /// Economic validity.
    pub effective: EffectiveInterval,
    /// Source evidence.
    pub provenance: ReferenceProvenance,
}

/// External identity namespace. Provider identifiers retain an explicit namespace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExternalIdentifier {
    /// ISIN, treated as bounded opaque input; format/check digits are not certified here.
    Isin {
        /// Supplied identifier.
        value: String,
    },
    /// CUSIP.
    Cusip {
        /// Supplied identifier.
        value: String,
    },
    /// LEI for entities.
    Lei {
        /// Supplied identifier.
        value: String,
    },
    /// Provider-specific identifier such as a RIC, FIGI or reference batch key.
    Provider {
        /// Explicit provider/namespace identity.
        namespace: SourceId,
        /// Supplied identifier, without case folding.
        value: String,
    },
}

impl ExternalIdentifier {
    /// Check size and content without guessing case normalization or check digits.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        let value = match self {
            Self::Isin { value }
            | Self::Cusip { value }
            | Self::Lei { value }
            | Self::Provider { value, .. } => value,
        };
        bounded_text(value, 128)
    }
}

/// Stable internal target, independent of provider symbol changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReferenceTarget {
    /// Instrument identity; valuation selects an effective/known version separately.
    Instrument {
        /// Internal instrument.
        instrument_id: InstrumentId,
    },
    /// Entity identity.
    Entity {
        /// Internal entity.
        entity_id: EntityId,
    },
}

/// One mapping assertion interval. A remap closes this interval and adds another mapping ID.
/// Later versions replace the entire assertion at known_at, including its interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentifierMapping {
    /// Internal assertion identity, retained through corrections/withdrawal.
    pub mapping_id: ReferenceDataId,
    /// Explicit namespace and symbol.
    pub identifier: ExternalIdentifier,
    /// Internal target; None withdraws a bad assertion while preserving history.
    pub target: Option<ReferenceTarget>,
    /// Validity of this assertion, independent of recording time.
    pub effective: EffectiveInterval,
    /// Source evidence.
    pub provenance: ReferenceProvenance,
}

/// Typed master key. Same text in different resource kinds is a different key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MasterKey {
    /// Entity key.
    Entity(EntityId),
    /// Calendar key.
    Calendar(ReferenceDataId),
    /// Convention key.
    Convention(ReferenceDataId),
    /// Mapping assertion key.
    Identifier(ReferenceDataId),
}

impl MasterKey {
    /// Stable storage key; not a query language or vendor ID.
    pub fn storage_key(&self) -> String {
        match self {
            Self::Entity(id) => format!("entity/{}", id.as_str()),
            Self::Calendar(id) => format!("calendar/{}", id.as_str()),
            Self::Convention(id) => format!("convention/{}", id.as_str()),
            Self::Identifier(id) => format!("identifier/{}", id.as_str()),
        }
    }
}

/// Initial closed set of reference resources. Instrument terms stay in ReferenceRecord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "definition",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MasterDefinition {
    /// Entity attributes.
    Entity(Box<EntityDefinition>),
    /// Calendar rules dispatch.
    Calendar(Box<CalendarDefinition>),
    /// Product convention pack.
    Convention(Box<ConventionDefinition>),
    /// External mapping assertion.
    Identifier(Box<IdentifierMapping>),
}

impl MasterDefinition {
    /// Typed internal identity.
    pub fn key(&self) -> MasterKey {
        match self {
            Self::Entity(v) => MasterKey::Entity(v.entity_id.clone()),
            Self::Calendar(v) => MasterKey::Calendar(v.calendar_id.clone()),
            Self::Convention(v) => MasterKey::Convention(v.convention_id.clone()),
            Self::Identifier(v) => MasterKey::Identifier(v.mapping_id.clone()),
        }
    }
    /// Economic validity.
    pub fn effective(&self) -> EffectiveInterval {
        match self {
            Self::Entity(v) => v.effective,
            Self::Calendar(v) => v.effective,
            Self::Convention(v) => v.effective,
            Self::Identifier(v) => v.effective,
        }
    }
    /// Validate complete bounded fields, independently of backend state.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        self.effective().validate()?;
        let provenance = match self {
            Self::Entity(v) => {
                bounded_text(&v.legal_name, 1024)?;
                bounded_text(&v.country, 128)?;
                if v.parent
                    .as_ref()
                    .is_some_and(|p| p.entity_id == v.entity_id)
                {
                    return Err(ReferenceError::InvalidInput("entity cannot parent itself"));
                }
                &v.provenance
            }
            Self::Calendar(v) => {
                if v.coverage_from >= v.coverage_until {
                    return Err(ReferenceError::InvalidInput("invalid calendar coverage"));
                }
                &v.provenance
            }
            Self::Convention(v) => {
                v.settings.validate()?;
                &v.provenance
            }
            Self::Identifier(v) => {
                v.identifier.validate()?;
                &v.provenance
            }
        };
        bounded_text(&provenance.evidence_reference, 512)
    }
}

/// Create or replace a complete definition by compare-and-set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MasterChange {
    /// None requires a never-used key; Some requires that exact current version.
    pub expected_version: Option<Version>,
    /// Complete definition; omission never means merge with old attributes.
    pub definition: MasterDefinition,
}

/// Immutable resource record with economic and recorded time kept separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MasterRecord {
    /// Persisted contract version.
    pub schema_version: SchemaVersion,
    /// Positive monotonic version per typed key.
    pub version: Version,
    /// Application-supplied commit time.
    pub recorded_at: UtcTimestamp,
    /// Full immutable definition.
    pub definition: MasterDefinition,
}

/// Resolution identifies the assertion and target version, including entity mappings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentifierResolution {
    /// Exact historical mapping assertion consumed.
    pub mapping: MasterRecord,
    /// Stable internal target.
    pub target: ReferenceTarget,
    /// Effective/known-at target version.
    pub target_version: Version,
}

/// Fully resolved resources for a service valuation. No repository enters pure pricing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedBondReference {
    /// Pinned bond terms version.
    pub record: ReferenceRecord,
    /// Pinned issuer attributes.
    pub issuer: MasterRecord,
    /// Pinned calendar definition.
    pub calendar: MasterRecord,
    /// Pinned convention pack.
    pub convention: MasterRecord,
}

/// Validate complete contract fields before pricing, with no model substitutions.
pub fn validate_valuation_terms(bond: &BondReferenceData) -> Result<(), ReferenceError> {
    let terms = bond
        .contractual_terms
        .as_ref()
        .ok_or(ReferenceError::InvalidInput(
            "complete contractual terms required; migrate reference data",
        ))?;
    terms.effective.validate()?;
    terms.settings.validate()?;
    match terms.stub {
        BondStub::None
            if bond.first_coupon_date.is_none() && terms.penultimate_coupon_date.is_none() => {}
        BondStub::ShortFirst
            if bond.first_coupon_date.is_some() && terms.penultimate_coupon_date.is_none() => {}
        BondStub::ShortLast => {
            return Err(ReferenceError::Unsupported(
                "short last stubs require completed shared schedule support",
            ))
        }
        BondStub::LongFirst | BondStub::LongLast => {
            return Err(ReferenceError::Unsupported(
                "long stubs require completed shared schedule support",
            ))
        }
        _ => {
            return Err(ReferenceError::InvalidInput(
                "stub policy conflicts with coupon boundaries",
            ))
        }
    }
    if terms.payment_lag != 0 {
        return Err(ReferenceError::Unsupported(
            "reference pricing payment lag is not implemented",
        ));
    }
    bounded_text(&terms.provenance.evidence_reference, 512)?;
    if terms.dated_date.date() >= bond.maturity_date
        || terms.redemption_value.decimal() <= rust_decimal::Decimal::ZERO
        || terms.issuer.entity_id.as_str() != bond.issuer_id
        || terms
            .penultimate_coupon_date
            .is_some_and(|d| d.date() <= terms.dated_date.date() || d.date() >= bond.maturity_date)
        || bond
            .first_coupon_date
            .is_some_and(|d| d <= terms.dated_date.date() || d > bond.maturity_date)
        || (bond
            .first_coupon_date
            .zip(terms.penultimate_coupon_date)
            .is_some_and(|(a, b)| a >= b.date()))
    {
        return Err(ReferenceError::InvalidInput(
            "inconsistent contractual dates/redemption/issuer",
        ));
    }
    let day_count = DayCountConvention::from_str(&bond.day_count)
        .map_err(|_| ReferenceError::InvalidInput("unknown day-count convention"))?;
    if day_count != terms.settings.day_count {
        return Err(ReferenceError::InvalidInput(
            "day-count label conflicts with contractual convention",
        ));
    }
    if ![1, 2, 4, 12].contains(&bond.frequency) {
        return Err(ReferenceError::Unsupported(
            "unsupported reference pricing frequency",
        ));
    }
    if bond.bond_type == BondType::FixedBullet {
        validate_bullet_boundaries(bond, terms)?;
    }
    Ok(())
}

fn validate_bullet_boundaries(
    bond: &BondReferenceData,
    terms: &BondContractTerms,
) -> Result<(), ReferenceError> {
    let months = (12 / bond.frequency) as i32;
    let first = bond.first_coupon_date.unwrap_or(terms.dated_date.date());
    if bond.first_coupon_date.is_some() {
        let mut regular = terms
            .dated_date
            .date()
            .add_months(months)
            .map_err(|_| ReferenceError::InvalidInput("coupon date outside calendar range"))?;
        if terms.settings.end_of_month && terms.dated_date.date().is_end_of_month() {
            regular = regular.end_of_month();
        }
        if first > regular {
            return Err(ReferenceError::Unsupported(
                "long first coupon period is not supported",
            ));
        }
    }
    let mut date = first;
    for step in 0..=1200 {
        if date == bond.maturity_date {
            return Ok(());
        }
        if step == 1200 {
            return Err(ReferenceError::CapacityExceeded);
        }
        if date > bond.maturity_date {
            return Err(ReferenceError::InvalidInput(
                "coupon boundaries imply an undeclared stub",
            ));
        }
        date = date
            .add_months(months)
            .map_err(|_| ReferenceError::InvalidInput("coupon date outside calendar range"))?;
        if terms.settings.end_of_month && first.is_end_of_month() {
            date = date.end_of_month();
        }
    }
    Err(ReferenceError::CapacityExceeded)
}

/// Require matching immutable resources known by the cutoff, before production valuation.
pub fn validate_resolved_bond(
    reference: &ResolvedBondReference,
    lookup: ReferenceLookup,
) -> Result<(), ReferenceError> {
    let bond = reference
        .record
        .bond
        .as_ref()
        .ok_or(ReferenceError::NotFound)?;
    if reference.record.recorded_at > lookup.known_at {
        return Err(ReferenceError::NotFound);
    }
    validate_service_resources(
        bond,
        &reference.issuer,
        &reference.calendar,
        &reference.convention,
        lookup,
    )
}

/// Check caller-supplied terms against exact resolved service resources.
pub fn validate_service_resources(
    bond: &BondReferenceData,
    issuer: &MasterRecord,
    calendar: &MasterRecord,
    convention: &MasterRecord,
    lookup: ReferenceLookup,
) -> Result<(), ReferenceError> {
    crate::reference_repository::validate_bond_reference(bond)?;
    let terms = bond
        .contractual_terms
        .as_ref()
        .ok_or(ReferenceError::InvalidInput("missing terms"))?;
    // Expanded families require their complete contractual models in subsequent tasks.
    if !matches!(bond.bond_type, BondType::FixedBullet | BondType::ZeroCoupon)
        || bond.is_callable
        || !bond.call_schedule.is_empty()
        || bond.is_putable
        || bond.is_sinkable
        || bond.floating_terms.is_some()
        || bond.inflation_index.is_some()
        || bond.inflation_base_index.is_some()
        || bond.has_deflation_floor
    {
        return Err(ReferenceError::Unsupported(
            "service reference valuation currently supports nominal fixed bullets and zeros",
        ));
    }
    if !terms.effective.contains(lookup.as_of) {
        return Err(ReferenceError::NotFound);
    }
    for record in [issuer, calendar, convention] {
        record.definition.validate()?;
        if record.recorded_at > lookup.known_at
            || !record.definition.effective().contains(lookup.as_of)
        {
            return Err(ReferenceError::InvalidInput(
                "reference resource outside effective/known-at scope",
            ));
        }
    }
    if let MasterDefinition::Calendar(definition) = &calendar.definition {
        if definition.rule_version.as_str() != SUPPORTED_BOND_CALENDAR_RULES {
            return Err(ReferenceError::Unsupported(
                "calendar rule version is not implemented by this build",
            ));
        }
    }
    validate_resource_pins(bond, issuer, calendar, convention)
}

/// Plan bounded compare-and-set master changes; adapters commit the resulting receipt atomically.
pub fn plan_master_changes(
    changes: &[MasterChange],
    recorded_at: UtcTimestamp,
    mut current: impl FnMut(&MasterKey) -> Result<Option<MasterRecord>, ReferenceError>,
) -> Result<Vec<MasterRecord>, ReferenceError> {
    if changes.len() > crate::reference_repository::MAX_REFERENCE_CHANGES {
        return Err(ReferenceError::CapacityExceeded);
    }
    let mut keys = BTreeSet::new();
    let mut records = Vec::with_capacity(changes.len());
    for change in changes {
        change.definition.validate()?;
        let key = change.definition.key();
        if !keys.insert(key.clone()) {
            return Err(ReferenceError::InvalidInput(
                "duplicate master key in command",
            ));
        }
        let previous = current(&key)?;
        let number = match (change.expected_version, previous) {
            (None, None) => 1,
            (Some(expected), Some(previous)) if expected == previous.version => {
                if recorded_at < previous.recorded_at {
                    return Err(ReferenceError::InvalidInput(
                        "recording time cannot move backwards",
                    ));
                }
                previous
                    .version
                    .get()
                    .checked_add(1)
                    .ok_or(ReferenceError::CapacityExceeded)?
            }
            _ => {
                return Err(ReferenceError::Conflict(
                    "master expected version differs from current",
                ))
            }
        };
        records.push(MasterRecord {
            schema_version: SchemaVersion::V2,
            version: Version::new(number).map_err(|_| ReferenceError::CapacityExceeded)?,
            recorded_at,
            definition: change.definition.clone(),
        });
    }
    Ok(records)
}

/// Validate cross-resource integrity against planned versions and one backend transaction.
/// Referenced versions must already be known or be committed in this same command.
pub fn validate_reference_links(
    commit: &crate::reference_repository::ReferenceCommit,
    mut bond_current: impl FnMut(
        &convex_core::ids::InstrumentId,
    ) -> Result<Option<ReferenceRecord>, ReferenceError>,
    mut master_version: impl FnMut(&MasterKey, Version) -> Result<Option<MasterRecord>, ReferenceError>,
) -> Result<(), ReferenceError> {
    let mut resource =
        |key: &MasterKey, version: Version| -> Result<MasterRecord, ReferenceError> {
            let record = commit
                .master_records
                .iter()
                .find(|r| r.definition.key() == *key && r.version == version)
                .cloned()
                .map(|r| Ok(Some(r)))
                .unwrap_or_else(|| master_version(key, version))?
                .ok_or(ReferenceError::InvalidInput(
                    "missing pinned reference resource",
                ))?;
            if record.recorded_at > commit.recorded_at {
                return Err(ReferenceError::InvalidInput(
                    "reference resource is not yet known",
                ));
            }
            Ok(record)
        };
    for record in &commit.records {
        let Some(bond) = &record.bond else {
            continue;
        };
        let terms = bond
            .contractual_terms
            .as_ref()
            .ok_or(ReferenceError::InvalidInput("missing contractual terms"))?;
        let issuer = resource(
            &MasterKey::Entity(terms.issuer.entity_id.clone()),
            terms.issuer.version,
        )?;
        let calendar = resource(
            &MasterKey::Calendar(terms.calendar.id.clone()),
            terms.calendar.version,
        )?;
        let convention = resource(
            &MasterKey::Convention(terms.convention.id.clone()),
            terms.convention.version,
        )?;
        // Admission supports storage of expanded bond terms, while service valuation
        // has the stricter product/model gate in validate_resolved_bond.
        validate_resource_pins(bond, &issuer, &calendar, &convention)?;
    }
    for record in &commit.master_records {
        match &record.definition {
            MasterDefinition::Entity(entity) => {
                let mut parent = entity.parent.clone();
                let mut seen = BTreeSet::from([entity.entity_id.clone()]);
                while let Some(pin) = parent {
                    if seen.len() >= 128 || !seen.insert(pin.entity_id.clone()) {
                        return Err(ReferenceError::InvalidInput(
                            "cyclic or excessively deep entity hierarchy",
                        ));
                    }
                    let record = resource(&MasterKey::Entity(pin.entity_id), pin.version)?;
                    let MasterDefinition::Entity(definition) = record.definition else {
                        return Err(ReferenceError::InvalidInput("invalid parent resource kind"));
                    };
                    parent = definition.parent;
                }
            }
            MasterDefinition::Identifier(mapping) => {
                match &mapping.target {
                    Some(ReferenceTarget::Instrument { instrument_id }) => {
                        let id: convex_core::ids::InstrumentId = instrument_id.clone().into();
                        let target = commit
                            .records
                            .iter()
                            .find(|r| r.instrument_id == id)
                            .cloned()
                            .map(|r| Ok(Some(r)))
                            .unwrap_or_else(|| bond_current(&id))?;
                        if target
                            .is_none_or(|r| r.bond.is_none() || r.recorded_at > commit.recorded_at)
                        {
                            return Err(ReferenceError::InvalidInput(
                                "mapping target instrument is missing",
                            ));
                        }
                    }
                    Some(ReferenceTarget::Entity { entity_id }) => {
                        // Entity targets bind stable identity; attributes are resolved at query time.
                        let key = MasterKey::Entity(entity_id.clone());
                        if !commit
                            .master_records
                            .iter()
                            .any(|r| r.definition.key() == key)
                        {
                            // Version 1 establishes the never-reused identity.
                            let version = Version::new(1)
                                .map_err(|_| ReferenceError::InvalidInput("invalid version"))?;
                            resource(&key, version)?;
                        }
                    }
                    None => {}
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_resource_pins(
    bond: &BondReferenceData,
    issuer: &MasterRecord,
    calendar: &MasterRecord,
    convention: &MasterRecord,
) -> Result<(), ReferenceError> {
    let terms = bond
        .contractual_terms
        .as_ref()
        .ok_or(ReferenceError::InvalidInput("missing contractual terms"))?;
    match (
        &issuer.definition,
        &calendar.definition,
        &convention.definition,
    ) {
        (
            MasterDefinition::Entity(entity),
            MasterDefinition::Calendar(cal),
            MasterDefinition::Convention(conv),
        ) if entity.entity_id == terms.issuer.entity_id
            && issuer.version == terms.issuer.version
            && cal.calendar_id == terms.calendar.id
            && calendar.version == terms.calendar.version
            && cal.code == terms.calendar_code
            && cal.coverage_from <= terms.dated_date
            && cal.coverage_until.date() >= bond.maturity_date
            && conv.convention_id == terms.convention.id
            && convention.version == terms.convention.version
            && conv.settings == terms.settings =>
        {
            Ok(())
        }
        _ => Err(ReferenceError::InvalidInput(
            "pinned reference resources conflict with terms",
        )),
    }
}

/// Select a bond version known by cutoff and applicable at economic time.
/// Tombstones supersede all earlier versions from their recorded time onward.
pub fn select_bond_at(
    records: impl IntoIterator<Item = ReferenceRecord>,
    lookup: ReferenceLookup,
) -> Result<Option<ReferenceRecord>, ReferenceError> {
    let mut selected: Option<ReferenceRecord> = None;
    for (n, record) in records.into_iter().enumerate() {
        if n >= MAX_REFERENCE_VERSIONS {
            return Err(ReferenceError::CapacityExceeded);
        }
        if record.recorded_at > lookup.known_at {
            continue;
        }
        let applicable = record
            .bond
            .as_ref()
            .map(|b| {
                b.contractual_terms
                    .as_ref()
                    .map(|t| t.effective.contains(lookup.as_of))
                    .unwrap_or(lookup.as_of >= record.recorded_at)
            })
            .unwrap_or(lookup.as_of >= record.recorded_at);
        if applicable
            && selected
                .as_ref()
                .is_none_or(|old| record.version > old.version)
        {
            selected = Some(record);
        }
    }
    Ok(selected.filter(|r| r.bond.is_some()))
}

/// Select resource history. Mapping assertions replace the full interval at known_at.
pub fn select_master_at(
    records: impl IntoIterator<Item = MasterRecord>,
    lookup: ReferenceLookup,
) -> Result<Option<MasterRecord>, ReferenceError> {
    let mut selected: Option<MasterRecord> = None;
    for (n, record) in records.into_iter().enumerate() {
        if n >= MAX_REFERENCE_VERSIONS {
            return Err(ReferenceError::CapacityExceeded);
        }
        if record.recorded_at > lookup.known_at {
            continue;
        }
        let applicable = matches!(record.definition, MasterDefinition::Identifier(_))
            || record.definition.effective().contains(lookup.as_of);
        if applicable
            && selected
                .as_ref()
                .is_none_or(|old| record.version > old.version)
        {
            selected = Some(record);
        }
    }
    Ok(selected.filter(|r| r.definition.effective().contains(lookup.as_of)))
}

/// Resolve mapping history from one read snapshot, refusing every multiple assertion match.
pub fn select_mapping_at(
    records: impl IntoIterator<Item = MasterRecord>,
    identifier: &ExternalIdentifier,
    lookup: ReferenceLookup,
) -> Result<Option<MasterRecord>, ReferenceError> {
    identifier.validate()?;
    let mut latest: BTreeMap<MasterKey, MasterRecord> = BTreeMap::new();
    for (n, record) in records.into_iter().enumerate() {
        if n >= MAX_REFERENCE_VERSIONS {
            return Err(ReferenceError::CapacityExceeded);
        }
        if record.recorded_at > lookup.known_at
            || !matches!(record.definition, MasterDefinition::Identifier(_))
        {
            continue;
        }
        let key = record.definition.key();
        if latest.get(&key).is_none_or(|r| record.version > r.version) {
            latest.insert(key, record);
        }
        if latest.len() > crate::reference_repository::MAX_REFERENCE_RECORDS {
            return Err(ReferenceError::CapacityExceeded);
        }
    }
    let mut found = None;
    for record in latest.into_values() {
        if let MasterDefinition::Identifier(mapping) = &record.definition {
            if mapping.identifier == *identifier
                && mapping.effective.contains(lookup.as_of)
                && mapping.target.is_some()
            {
                if found.is_some() {
                    return Err(ReferenceError::AmbiguousIdentifier);
                }
                found = Some(record);
            }
        }
    }
    Ok(found)
}

fn bounded_text(value: &str, limit: usize) -> Result<(), ReferenceError> {
    if value.trim().is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(ReferenceError::InvalidInput(
            "invalid or oversized reference text",
        ));
    }
    Ok(())
}
