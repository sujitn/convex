//! Provider-neutral reference definitions and deterministic history selection.
//! This module owns no clock, runtime, database or pricing implementation.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use convex_core::calendars::BusinessDayConvention;
use convex_core::contracts::{
    BusinessDate, EntityId, ExactDecimal, IndexId, InstrumentId, ModelId, ReferenceDataId,
    SchemaVersion, SourceId, UtcTimestamp, Version,
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
    /// Exercise mechanics of the bond's `call_schedule`; a callable is valued only with them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_terms: Option<CallTerms>,
    /// Coupon mechanics of a floating rate note; an FRN is valued only with them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floating_mechanics: Option<FloatingMechanics>,
}

/// How a scheduled call may be exercised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallStyle {
    /// Any day from each call date until the next one, or maturity.
    American,
    /// Only on each listed call date.
    European,
    /// Only on the listed call dates, as one discrete exercise set.
    Bermudan,
}

/// Contractual call mechanics that the call schedule entries do not carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallTerms {
    /// Exercise style of every scheduled call.
    pub style: CallStyle,
    /// Calendar days of notice before each exercise, at most 366.
    pub notice_days: u32,
}

/// How a rate index publishes its fixings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateIndexKind {
    /// One fixing per index business day, compounded over each coupon period.
    Overnight,
    /// One fixing per reset period, for the index's own tenor.
    Term,
}

/// Floating coupon conventions. Every field is optional: a bond's own value overrides its
/// index's, and a value that neither declares is refused when the bond is valued.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloatingConventions {
    /// Overnight: index business days the observation period lags the accrual period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lookback_business_days: Option<u32>,
    /// Overnight: weights follow the observation period rather than the accrual period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_shift: Option<bool>,
    /// Overnight: final index business days that repeat the last fixing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lockout_business_days: Option<u32>,
    /// Term: signed index business days from each reset start to its fixing date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixing_offset_business_days: Option<i32>,
    /// Term: day count of the index's forward quotation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection_day_count: Option<DayCountConvention>,
}

impl FloatingConventions {
    /// Bounds, and no field of the other index kind.
    pub fn validate(&self, kind: Option<RateIndexKind>) -> Result<(), ReferenceError> {
        if self.lookback_business_days.is_some_and(|days| days > 30)
            || self.lockout_business_days.is_some_and(|days| days > 30)
            || self
                .fixing_offset_business_days
                .is_some_and(|days| !(-30..=30).contains(&days))
        {
            return Err(ReferenceError::InvalidInput(
                "floating conventions allow at most 30 business days",
            ));
        }
        let overnight = self.lookback_business_days.is_some()
            || self.observation_shift.is_some()
            || self.lockout_business_days.is_some();
        let term =
            self.fixing_offset_business_days.is_some() || self.projection_day_count.is_some();
        match kind {
            Some(RateIndexKind::Overnight) if term => Err(ReferenceError::InvalidInput(
                "an overnight index has no fixing offset or projection day count",
            )),
            Some(RateIndexKind::Term) if overnight => Err(ReferenceError::InvalidInput(
                "a term index has no lookback, observation shift or lockout",
            )),
            None if overnight && term => Err(ReferenceError::InvalidInput(
                "floating conventions mix overnight and term fields",
            )),
            _ => Ok(()),
        }
    }
}

/// A floating rate note's coupon mechanics: its exact rate-index resource and any
/// contractual values that override the index's declared conventions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloatingMechanics {
    /// Exact rate-index resource version.
    pub index: ReferencePin,
    /// This bond's own conventions, each overriding the index's.
    #[serde(default)]
    pub overrides: FloatingConventions,
}

/// Versioned rate index: provider-neutral identity, the fixing source to read, the
/// administrator's publication rule and the conventions its bonds usually follow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateIndexDefinition {
    /// Internal resource identity.
    pub rate_index_id: ReferenceDataId,
    /// Index identity of its fixings and of projection curve roles.
    pub index_id: IndexId,
    /// Currency of the index.
    pub currency: convex_core::Currency,
    /// Overnight or term fixings.
    pub kind: RateIndexKind,
    /// Fixing source whose published revisions are read.
    pub fixing_source: SourceId,
    /// Exact calendar resource of the index's business days.
    pub calendar: ReferencePin,
    /// Explicit calendar dispatch, verified against the pinned resource.
    pub calendar_code: BondCalendar,
    /// Index business days from an observation date to its publication, at most 10.
    pub publication_lag_business_days: u32,
    /// UTC time of day of each publication, in seconds after midnight.
    pub publication_utc_seconds: u32,
    /// Conventions this index's bonds follow unless their own terms say otherwise.
    #[serde(default)]
    pub conventions: FloatingConventions,
    /// Economic validity.
    pub effective: EffectiveInterval,
    /// Source evidence.
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
    /// Listed future identity, with exact master revisions selected independently.
    Futures {
        /// Internal exchange instrument identity.
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

/// Internal book identity; the same token grammar as every other reference identity.
pub type BookId = ReferenceDataId;

/// Maximum positions in one book snapshot.
///
/// A committed book is serialized four times in one redb transaction (current,
/// history, retained command and retained receipt), against the 8 MiB atomic
/// transaction budget. One row is at most about 0.9 KiB (instrument 128 B, source
/// identifier ~300 B, quantity 59 B, two 64 B classifications, framing), so 1,000
/// rows are at most ~0.9 MiB per copy and ~3.6 MiB per transaction. The bound also
/// stays below the 1,024 distinct-identity ceiling the HTTP access layer applies when
/// it scopes a request or response to instruments, so entitlement checks never
/// have to be skipped for a large book.
pub const MAX_BOOK_POSITIONS: usize = 1_000;

/// How a position quantity is denominated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantityUnit {
    /// Face (notional) amount in the instrument currency.
    Face,
    /// Number of units/securities.
    Units,
}

/// One position. The instrument is always a resolved internal identity; an external
/// identifier it was resolved from is retained only as evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookPosition {
    /// Resolved internal instrument.
    pub instrument_id: InstrumentId,
    /// Signed quantity; negative is short.
    pub quantity: ExactDecimal,
    /// Denomination of the quantity.
    pub unit: QuantityUnit,
    /// Optional exact instrument version the owning system priced against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_version: Option<Version>,
    /// External identifier the owning system supplied, kept for audit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_identifier: Option<ExternalIdentifier>,
    /// Supplied sector classification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    /// Supplied credit rating label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<String>,
}

/// Versioned position snapshot owned by one external system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookDefinition {
    /// Internal book identity.
    pub book_id: BookId,
    /// System of record that supplies the snapshots.
    pub owning_system: SourceId,
    /// Optional display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional reporting currency code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Optional description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Validity of this snapshot; a bounded end retires the book.
    pub effective: EffectiveInterval,
    /// Complete positions, unique by instrument.
    pub positions: Vec<BookPosition>,
    /// Source evidence.
    pub provenance: ReferenceProvenance,
}

impl BookDefinition {
    /// Validate bounds and uniqueness independently of backend state.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        if self.positions.len() > MAX_BOOK_POSITIONS {
            return Err(ReferenceError::CapacityExceeded);
        }
        for text in [&self.name, &self.description].into_iter().flatten() {
            bounded_text(text, 1024)?;
        }
        if let Some(code) = &self.currency {
            if convex_core::Currency::from_code(code).is_none() {
                return Err(ReferenceError::InvalidInput("unknown book currency"));
            }
        }
        let mut seen = BTreeSet::new();
        for position in &self.positions {
            if !seen.insert(position.instrument_id.as_str()) {
                return Err(ReferenceError::InvalidInput("duplicate instrument in book"));
            }
            position.validate()?;
        }
        Ok(())
    }
}

impl BookPosition {
    /// Validate bounded evidence and classification text.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        if let Some(identifier) = &self.source_identifier {
            identifier.validate()?;
        }
        for text in [&self.sector, &self.rating].into_iter().flatten() {
            bounded_text(text, 64)?;
        }
        Ok(())
    }
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
    /// Complete issuer/security/default scope.
    IssuerScope(ReferenceDataId),
    /// Full versioned native core calculation definitions.
    Calculation(ReferenceDataId),
    /// Entity key.
    Entity(EntityId),
    /// Calendar key.
    Calendar(ReferenceDataId),
    /// Convention key.
    Convention(ReferenceDataId),
    /// Mapping assertion key.
    Identifier(ReferenceDataId),
    /// Dated exchange futures contract.
    Futures(InstrumentId),
    /// Eligible physical-delivery securities/CF revision.
    Basket(ReferenceDataId),
    /// Actual cash hedge candidates and their market/eligibility inputs.
    CashUniverse(ReferenceDataId),
    /// Position snapshot owned by an external book management system.
    Book(BookId),
    /// Rate index publication rule and declared conventions.
    RateIndex(ReferenceDataId),
}

impl MasterKey {
    /// Stable storage key; not a query language or vendor ID.
    pub fn storage_key(&self) -> String {
        match self {
            Self::IssuerScope(id) => format!("issuer_scope/{}", id.as_str()),
            Self::Calculation(id) => format!("calculation/{}", id.as_str()),
            Self::Entity(id) => format!("entity/{}", id.as_str()),
            Self::Calendar(id) => format!("calendar/{}", id.as_str()),
            Self::Convention(id) => format!("convention/{}", id.as_str()),
            Self::Identifier(id) => format!("identifier/{}", id.as_str()),
            Self::Futures(id) => format!("futures/{}", id.as_str()),
            Self::Basket(id) => format!("basket/{}", id.as_str()),
            Self::CashUniverse(id) => format!("cash_universe/{}", id.as_str()),
            Self::Book(id) => format!("book/{}", id.as_str()),
            Self::RateIndex(id) => format!("rate_index/{}", id.as_str()),
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
    /// Explicit issuer mappings and actual default notices.
    IssuerScope(Box<crate::issuer_reference::IssuerScopeDefinition>),
    /// Full native core contracts/settings, with market values resolved separately.
    Calculation(Box<crate::core_workflow::CoreDefinition>),
    /// Entity attributes.
    Entity(Box<EntityDefinition>),
    /// Calendar rules dispatch.
    Calendar(Box<CalendarDefinition>),
    /// Product convention pack.
    Convention(Box<ConventionDefinition>),
    /// External mapping assertion.
    Identifier(Box<IdentifierMapping>),
    /// Full dated exchange contract; no instrument-family inference.
    Futures(Box<crate::futures_reference::FuturesDefinition>),
    /// Eligible securities and published conversion factors.
    Basket(Box<crate::futures_reference::BasketDefinition>),
    /// Exact cash hedge input universe.
    CashUniverse(Box<crate::futures_reference::CashUniverseDefinition>),
    /// Complete position snapshot; omission never means merge with an older snapshot.
    Book(Box<BookDefinition>),
    /// Rate index publication rule and declared conventions.
    RateIndex(Box<RateIndexDefinition>),
}

impl MasterDefinition {
    /// Typed internal identity.
    pub fn key(&self) -> MasterKey {
        match self {
            Self::IssuerScope(v) => MasterKey::IssuerScope(v.id.clone()),
            Self::Calculation(v) => MasterKey::Calculation(v.id.clone()),
            Self::Entity(v) => MasterKey::Entity(v.entity_id.clone()),
            Self::Calendar(v) => MasterKey::Calendar(v.calendar_id.clone()),
            Self::Convention(v) => MasterKey::Convention(v.convention_id.clone()),
            Self::Identifier(v) => MasterKey::Identifier(v.mapping_id.clone()),
            Self::Futures(v) => MasterKey::Futures(v.contract.instrument_id.clone()),
            Self::Basket(v) => MasterKey::Basket(v.basket.stamp.id.clone()),
            Self::CashUniverse(v) => MasterKey::CashUniverse(v.universe.stamp.id.clone()),
            Self::Book(v) => MasterKey::Book(v.book_id.clone()),
            Self::RateIndex(v) => MasterKey::RateIndex(v.rate_index_id.clone()),
        }
    }
    /// Economic validity.
    pub fn effective(&self) -> EffectiveInterval {
        match self {
            Self::IssuerScope(v) => v.effective,
            Self::Calculation(v) => v.effective,
            Self::Entity(v) => v.effective,
            Self::Calendar(v) => v.effective,
            Self::Convention(v) => v.effective,
            Self::Identifier(v) => v.effective,
            Self::Futures(v) => v.effective,
            Self::Basket(v) => v.effective,
            Self::CashUniverse(v) => v.effective,
            Self::Book(v) => v.effective,
            Self::RateIndex(v) => v.effective,
        }
    }
    /// Validate complete bounded fields, independently of backend state.
    pub fn validate(&self) -> Result<(), ReferenceError> {
        self.effective().validate()?;
        let provenance = match self {
            Self::IssuerScope(v) => {
                v.validate(None)?;
                &v.provenance
            }
            Self::Calculation(v) => {
                v.validate()?;
                &v.provenance
            }
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
            Self::Futures(v) => {
                v.contract
                    .validate()
                    .map_err(|e| ReferenceError::InvalidInput(e.reason))?;
                &v.provenance
            }
            Self::Basket(v) => {
                v.basket
                    .validate()
                    .map_err(|e| ReferenceError::InvalidInput(e.reason))?;
                &v.provenance
            }
            Self::CashUniverse(v) => {
                v.universe
                    .validate()
                    .map_err(|e| ReferenceError::InvalidInput(e.reason))?;
                &v.provenance
            }
            Self::Book(v) => {
                v.validate()?;
                &v.provenance
            }
            Self::RateIndex(v) => {
                if v.publication_lag_business_days > 10 || v.publication_utc_seconds >= 86_400 {
                    return Err(ReferenceError::InvalidInput(
                        "publication lag is at most 10 business days, at a time of day",
                    ));
                }
                v.conventions.validate(Some(v.kind))?;
                &v.provenance
            }
        };
        bounded_text(&provenance.evidence_reference, 512)
    }

    /// Supplied source stamp for the closed futures/cash resource set.
    pub fn futures_stamp(&self) -> Option<&convex_core::futures::ReferenceStamp> {
        match self {
            Self::Futures(v) => Some(&v.contract.stamp),
            Self::Basket(v) => Some(&v.basket.stamp),
            Self::CashUniverse(v) => Some(&v.universe.stamp),
            _ => None,
        }
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
    /// A floating rate note's pinned rate index and index calendar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floating: Option<ResolvedFloating>,
}

/// The resources a floating rate note's coupons depend on, beyond its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedFloating {
    /// Pinned rate index.
    pub index: MasterRecord,
    /// The index's pinned calendar.
    pub calendar: MasterRecord,
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
    if bond.bond_type == BondType::FloatingRate {
        if terms.payment_lag > 30 {
            return Err(ReferenceError::InvalidInput(
                "payment lag is at most 30 business days",
            ));
        }
    } else if terms.payment_lag != 0 {
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
    if let Some(call) = &terms.call_terms {
        validate_call_terms(bond, terms, call)?;
    }
    if let Some(mechanics) = &terms.floating_mechanics {
        if bond.bond_type != BondType::FloatingRate || bond.floating_terms.is_none() {
            return Err(ReferenceError::InvalidInput(
                "floating mechanics require a floating rate note with floating terms",
            ));
        }
        mechanics.overrides.validate(None)?;
    }
    Ok(())
}

/// Explicit call mechanics belong only to a callable with a dated, priced schedule.
fn validate_call_terms(
    bond: &BondReferenceData,
    terms: &BondContractTerms,
    call: &CallTerms,
) -> Result<(), ReferenceError> {
    if bond.bond_type != BondType::FixedCallable || !bond.is_callable {
        return Err(ReferenceError::InvalidInput(
            "call terms require a fixed callable bond",
        ));
    }
    if bond.call_schedule.iter().any(|entry| entry.is_make_whole) {
        return Err(ReferenceError::Unsupported(
            "make-whole calls need their own terms and model",
        ));
    }
    if call.notice_days > 366
        || bond.call_schedule.is_empty()
        || bond
            .call_schedule
            .windows(2)
            .any(|pair| pair[0].call_date >= pair[1].call_date)
        || bond.call_schedule.iter().any(|entry| {
            entry.call_date <= terms.dated_date.date()
                || entry.call_date > bond.maturity_date
                || entry.call_price <= rust_decimal::Decimal::ZERO
        })
    {
        return Err(ReferenceError::InvalidInput(
            "call schedule needs sorted unique dates inside the bond's life, positive prices and at most 366 notice days",
        ));
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
    )?;
    match (is_valued_floating(bond), &reference.floating) {
        (true, Some(floating)) => validate_floating_resources(bond, floating, lookup),
        (false, None) => Ok(()),
        _ => Err(ReferenceError::InvalidInput(
            "floating resources belong exactly to a floating rate note",
        )),
    }
}

/// A floating rate note the service values: floating terms without a cap or floor, and
/// explicit mechanics pinning its rate index.
pub fn is_valued_floating(bond: &BondReferenceData) -> bool {
    bond.bond_type == BondType::FloatingRate
        && bond
            .floating_terms
            .as_ref()
            .is_some_and(|f| f.cap.is_none() && f.floor.is_none())
        && bond
            .contractual_terms
            .as_ref()
            .is_some_and(|t| t.floating_mechanics.is_some())
}

/// Check a floating rate note's pinned rate index and index calendar against its terms.
pub fn validate_floating_resources(
    bond: &BondReferenceData,
    floating: &ResolvedFloating,
    lookup: ReferenceLookup,
) -> Result<(), ReferenceError> {
    let mechanics = bond
        .contractual_terms
        .as_ref()
        .and_then(|t| t.floating_mechanics.as_ref())
        .ok_or(ReferenceError::InvalidInput("missing floating mechanics"))?;
    let terms = bond
        .floating_terms
        .as_ref()
        .ok_or(ReferenceError::InvalidInput("missing floating terms"))?;
    for record in [&floating.index, &floating.calendar] {
        record.definition.validate()?;
        if record.recorded_at > lookup.known_at
            || !record.definition.effective().contains(lookup.as_of)
        {
            return Err(ReferenceError::InvalidInput(
                "reference resource outside effective/known-at scope",
            ));
        }
    }
    let (MasterDefinition::RateIndex(index), MasterDefinition::Calendar(calendar)) =
        (&floating.index.definition, &floating.calendar.definition)
    else {
        return Err(ReferenceError::InvalidInput(
            "pinned floating resources conflict with terms",
        ));
    };
    if index.rate_index_id != mechanics.index.id
        || floating.index.version != mechanics.index.version
        || calendar.calendar_id != index.calendar.id
        || floating.calendar.version != index.calendar.version
        || calendar.code != index.calendar_code
        || index.currency != bond.currency
    {
        return Err(ReferenceError::InvalidInput(
            "pinned floating resources conflict with terms",
        ));
    }
    if calendar.rule_version.as_str() != SUPPORTED_BOND_CALENDAR_RULES {
        return Err(ReferenceError::Unsupported(
            "calendar rule version is not implemented by this build",
        ));
    }
    use convex_core::ids::FloatingRateIndex as I;
    let kind = match terms.index {
        I::Sofr | I::Estr | I::Sonia => Some(RateIndexKind::Overnight),
        I::Euribor(_) | I::TermSofr(_) => Some(RateIndexKind::Term),
        I::Other(_) => None,
    };
    if kind.is_some_and(|kind| kind != index.kind) {
        return Err(ReferenceError::InvalidInput(
            "the bond's index and its rate-index resource differ in kind",
        ));
    }
    mechanics.overrides.validate(Some(index.kind))
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
    // A fixed callable is admitted only with its explicit call mechanics, and a floating
    // rate note only with its rate index and no cap or floor, which need an option model.
    let callable = bond.bond_type == BondType::FixedCallable && terms.call_terms.is_some();
    let floating = is_valued_floating(bond);
    if !(callable
        || floating
        || matches!(bond.bond_type, BondType::FixedBullet | BondType::ZeroCoupon))
        || (!callable && (bond.is_callable || !bond.call_schedule.is_empty()))
        || bond.is_putable
        || bond.is_sinkable
        || (!floating && bond.floating_terms.is_some())
        || bond.inflation_index.is_some()
        || bond.inflation_base_index.is_some()
        || bond.has_deflation_floor
    {
        return Err(ReferenceError::Unsupported(
            "service reference valuation supports nominal fixed bullets, zeros, callables with explicit call terms and uncapped floating rate notes with a rate index",
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
        if let MasterDefinition::IssuerScope(scope) = &change.definition {
            scope.validate(Some(recorded_at))?;
            if let Some(MasterRecord {
                definition: MasterDefinition::IssuerScope(old),
                ..
            }) = &previous
            {
                scope.directory.preserves(&old.directory).map_err(|_| {
                    ReferenceError::InvalidInput(
                        "issuer mappings/default facts cannot be removed or reassigned",
                    )
                })?;
            }
        }
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
        if let Some(stamp) = change.definition.futures_stamp() {
            if stamp.version.get() != number || stamp.available_at > recorded_at {
                return Err(ReferenceError::InvalidInput(
                    "supplied futures resource version/availability conflicts with committed revision",
                ));
            }
        }
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
    mut bond_version: impl FnMut(
        &convex_core::ids::InstrumentId,
        Version,
    ) -> Result<Option<ReferenceRecord>, ReferenceError>,
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
            MasterDefinition::IssuerScope(scope) => {
                for issuer in &scope.directory.issuers {
                    let value =
                        resource(&MasterKey::Entity(issuer.entity_id.clone()), issuer.version)?;
                    if !matches!(value.definition,MasterDefinition::Entity(ref e) if e.entity_id==issuer.entity_id
                    && e.effective.contains(scope.effective.from)
                    && match (scope.effective.until,e.effective.until) {
                        (_,None)=>true,
                        (Some(a),Some(b))=>a<=b,
                        (None,Some(_))=>false,
                    }) {
                        return Err(ReferenceError::InvalidInput(
                            "exact effective issuer entity attributes required",
                        ));
                    }
                }
            }
            MasterDefinition::Calculation(definition) => {
                if let Some(pin) =
                    crate::issuer_reference::issuer_scope_pin(&definition.reference_inputs)?
                {
                    let value = resource(&MasterKey::IssuerScope(pin.id), pin.version)?;
                    if !matches!(value.definition, MasterDefinition::IssuerScope(_)) {
                        return Err(ReferenceError::InvalidInput(
                            "exact issuer scope resource required",
                        ));
                    }
                }
            }
            MasterDefinition::Book(book) => {
                // Pinned rows must name an instrument version that already exists.
                for position in &book.positions {
                    let Some(version) = position.reference_version else {
                        continue;
                    };
                    let id: convex_core::ids::InstrumentId = position.instrument_id.clone().into();
                    let known = commit
                        .records
                        .iter()
                        .find(|r| r.instrument_id == id && r.version == version)
                        .cloned()
                        .map(|r| Ok(Some(r)))
                        .unwrap_or_else(|| bond_version(&id, version))?;
                    if known.is_none_or(|r| r.bond.is_none() || r.recorded_at > commit.recorded_at)
                    {
                        return Err(ReferenceError::InvalidInput(
                            "book position pins a missing instrument version",
                        ));
                    }
                }
            }
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
                    Some(ReferenceTarget::Futures { instrument_id }) => {
                        let key = MasterKey::Futures(instrument_id.clone());
                        if !commit
                            .master_records
                            .iter()
                            .any(|r| r.definition.key() == key)
                        {
                            resource(
                                &key,
                                Version::new(1)
                                    .map_err(|_| ReferenceError::InvalidInput("invalid version"))?,
                            )?;
                        }
                    }
                    None => {}
                }
            }
            _ => {}
        }
    }
    crate::futures_reference::validate_futures_links(commit, &mut resource, &mut bond_version)?;
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
