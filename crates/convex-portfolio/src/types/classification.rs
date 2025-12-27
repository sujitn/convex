//! Flexible classification system for holdings.
//!
//! This module provides a dual-layer classification system:
//! - **Composite enums**: Normalized values for analytics (from `convex-bonds`)
//! - **Provider maps**: Preserve source data from any provider (Bloomberg, GICS, etc.)
//!
//! This design allows simple users to work with basic enums while licensed users
//! can preserve and query their full hierarchical taxonomies.
//!
//! ## Core Types (from convex-bonds)
//!
//! - [`CreditRating`]: Agency-agnostic credit rating (AAA to D)
//! - [`RatingBucket`]: Grouped rating categories
//! - [`Sector`]: Issuer sector classification
//! - [`Seniority`]: Capital structure position
//!
//! ## Provider Info Types (defined here)
//!
//! - [`SectorInfo`]: Sector with provider hierarchies
//! - [`RatingInfo`]: Rating with multi-agency data
//! - [`SeniorityInfo`]: Seniority with CoCo/AT1 details
//! - [`Classification`]: Unified container for all metadata

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Re-export core classification types from convex-bonds for backward compatibility
pub use convex_bonds::types::{CreditRating, RatingBucket, Sector, Seniority};

// =============================================================================
// SECTOR INFO (Provider Map Layer)
// =============================================================================

/// Source sector classifications from any provider.
///
/// Allows preserving full hierarchical classifications from BICS, GICS, ICB,
/// or internal taxonomies while providing a normalized composite for analytics.
///
/// # Examples
///
/// ```
/// use convex_portfolio::types::{Sector, SectorInfo};
///
/// let info = SectorInfo::new()
///     .with_classification("BICS", &["Financials", "Banking", "Commercial"])
///     .with_composite(Sector::Financial);
///
/// assert_eq!(info.composite, Some(Sector::Financial));
/// assert_eq!(info.level("BICS", 0), Some("Financials"));
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SectorInfo {
    /// Normalized sector for analytics.
    pub composite: Option<Sector>,

    /// Hierarchical classification by provider.
    ///
    /// Key: Provider name ("BICS", "GICS", "ICB", "Internal", etc.)
    /// Value: Levels from most general to most specific
    ///
    /// Example for BICS:
    /// ```text
    /// "BICS" -> ["Financials", "Banking", "Commercial Banking", "Regional Banks - US"]
    /// ```
    pub by_provider: HashMap<String, Vec<String>>,
}

impl SectorInfo {
    /// Creates a new empty sector info.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates sector info with just a composite sector.
    #[must_use]
    pub fn from_composite(sector: Sector) -> Self {
        Self {
            composite: Some(sector),
            by_provider: HashMap::new(),
        }
    }

    /// Adds a classification from a provider.
    #[must_use]
    pub fn with_classification(mut self, provider: &str, levels: &[&str]) -> Self {
        self.by_provider.insert(
            provider.to_string(),
            levels.iter().map(|s| (*s).to_string()).collect(),
        );
        self
    }

    /// Sets the composite sector.
    #[must_use]
    pub fn with_composite(mut self, sector: Sector) -> Self {
        self.composite = Some(sector);
        self
    }

    /// Gets a specific level from a provider (0-indexed).
    #[must_use]
    pub fn level(&self, provider: &str, depth: usize) -> Option<&str> {
        self.by_provider
            .get(provider)
            .and_then(|levels| levels.get(depth))
            .map(|s| s.as_str())
    }

    /// Gets all levels from a provider.
    #[must_use]
    pub fn levels(&self, provider: &str) -> Option<&[String]> {
        self.by_provider.get(provider).map(|v| v.as_slice())
    }

    /// Returns true if any provider data is available.
    #[must_use]
    pub fn has_provider_data(&self) -> bool {
        !self.by_provider.is_empty()
    }
}

// =============================================================================
// RATING INFO (Provider Map Layer)
// =============================================================================

/// Source ratings from any provider (flexible - no hardcoded agency list).
///
/// Stores original ratings from any agency while providing a normalized
/// composite for analytics.
///
/// # Examples
///
/// ```
/// use convex_portfolio::types::{CreditRating, RatingInfo};
///
/// let info = RatingInfo::new()
///     .with_rating("SP", "BBB+")
///     .with_rating("Moodys", "Baa1")
///     .with_composite(CreditRating::BBBPlus);
///
/// assert_eq!(info.rating("SP"), Some("BBB+"));
/// assert!(info.is_investment_grade().unwrap());
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RatingInfo {
    /// Normalized rating for analytics (user provides or derives).
    pub composite: Option<CreditRating>,

    /// Original ratings by provider (any agency).
    ///
    /// Key: Provider name ("SP", "Moodys", "Fitch", "DBRS", "JCR", "Kroll", etc.)
    /// Value: Original rating string as provided
    pub by_provider: HashMap<String, String>,
}

impl RatingInfo {
    /// Creates a new empty rating info.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates rating info with just a composite rating.
    #[must_use]
    pub fn from_composite(rating: CreditRating) -> Self {
        Self {
            composite: Some(rating),
            by_provider: HashMap::new(),
        }
    }

    /// Adds a rating from a provider.
    #[must_use]
    pub fn with_rating(mut self, provider: &str, rating: &str) -> Self {
        self.by_provider
            .insert(provider.to_string(), rating.to_string());
        self
    }

    /// Sets the composite rating.
    #[must_use]
    pub fn with_composite(mut self, rating: CreditRating) -> Self {
        self.composite = Some(rating);
        self
    }

    /// Gets the rating from a specific provider.
    #[must_use]
    pub fn rating(&self, provider: &str) -> Option<&str> {
        self.by_provider.get(provider).map(|s| s.as_str())
    }

    /// Returns true if any provider data is available.
    #[must_use]
    pub fn has_provider_data(&self) -> bool {
        !self.by_provider.is_empty()
    }

    /// Returns the rating bucket based on composite.
    #[must_use]
    pub fn bucket(&self) -> Option<RatingBucket> {
        self.composite.map(|r| r.bucket())
    }

    /// Returns true if composite is investment grade.
    #[must_use]
    pub fn is_investment_grade(&self) -> Option<bool> {
        self.composite.map(|r| r.is_investment_grade())
    }
}

// =============================================================================
// SENIORITY INFO (Provider Map Layer)
// =============================================================================

/// Detailed seniority with capital structure info.
///
/// Handles complex instruments like AT1 CoCos, bail-in bonds, and
/// structural subordination (HoldCo vs OpCo).
///
/// # Examples
///
/// ```
/// use convex_portfolio::types::{Seniority, SeniorityInfo};
///
/// let info = SeniorityInfo::new()
///     .with_composite(Seniority::Hybrid)
///     .with_capital_tier("AT1")
///     .with_coco_trigger(0.05125, "MechanicalWritedown");
///
/// assert!(info.is_coco());
/// assert!(info.is_bailin_eligible());
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SeniorityInfo {
    /// Normalized seniority for analytics.
    pub composite: Option<Seniority>,

    /// Regulatory capital tier: "CET1", "AT1", "Tier2", "MREL", etc.
    pub capital_tier: Option<String>,

    /// Specific instrument type: "CoCo", "Preferred", "Legacy T1", etc.
    pub instrument_type: Option<String>,

    /// For CoCos/AT1: trigger level (e.g., 0.05125 for 5.125% CET1).
    pub trigger_level: Option<f64>,

    /// Trigger type: "MechanicalWritedown", "PON", "Conversion", etc.
    pub trigger_type: Option<String>,

    /// Bail-in rank (lower = first loss).
    pub bailin_rank: Option<u8>,

    /// Structural position: "HoldCo", "OpCo".
    pub structural_position: Option<String>,

    /// Provider-specific codes.
    pub by_provider: HashMap<String, String>,
}

impl SeniorityInfo {
    /// Creates a new empty seniority info.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates seniority info with just a composite seniority.
    #[must_use]
    pub fn from_composite(seniority: Seniority) -> Self {
        Self {
            composite: Some(seniority),
            ..Self::default()
        }
    }

    /// Sets the composite seniority.
    #[must_use]
    pub fn with_composite(mut self, seniority: Seniority) -> Self {
        self.composite = Some(seniority);
        self
    }

    /// Sets the regulatory capital tier.
    #[must_use]
    pub fn with_capital_tier(mut self, tier: &str) -> Self {
        self.capital_tier = Some(tier.to_string());
        self
    }

    /// Sets the instrument type.
    #[must_use]
    pub fn with_instrument_type(mut self, instrument_type: &str) -> Self {
        self.instrument_type = Some(instrument_type.to_string());
        self
    }

    /// Sets the CoCo trigger information.
    #[must_use]
    pub fn with_coco_trigger(mut self, level: f64, trigger_type: &str) -> Self {
        self.trigger_level = Some(level);
        self.trigger_type = Some(trigger_type.to_string());
        self
    }

    /// Sets the structural position.
    #[must_use]
    pub fn with_structural_position(mut self, position: &str) -> Self {
        self.structural_position = Some(position.to_string());
        self
    }

    /// Sets the bail-in rank.
    #[must_use]
    pub fn with_bailin_rank(mut self, rank: u8) -> Self {
        self.bailin_rank = Some(rank);
        self
    }

    /// Adds a provider-specific code.
    #[must_use]
    pub fn with_provider(mut self, provider: &str, code: &str) -> Self {
        self.by_provider
            .insert(provider.to_string(), code.to_string());
        self
    }

    /// Returns true if this is bail-inable.
    #[must_use]
    pub fn is_bailin_eligible(&self) -> bool {
        self.composite
            .map(|s| s.is_bailin_eligible())
            .unwrap_or(false)
    }

    /// Returns true if this is a CoCo (has trigger information).
    #[must_use]
    pub fn is_coco(&self) -> bool {
        self.trigger_level.is_some()
    }

    /// Returns the typical recovery rate.
    #[must_use]
    pub fn typical_recovery(&self) -> Option<f64> {
        self.composite.map(|s| s.typical_recovery())
    }
}

// =============================================================================
// UNIFIED CLASSIFICATION
// =============================================================================

/// Complete classification for a holding.
///
/// Combines sector, rating, and seniority with additional metadata.
///
/// # Examples
///
/// ```
/// use convex_portfolio::types::{
///     Classification, CreditRating, RatingInfo, Sector, SectorInfo,
///     Seniority, SeniorityInfo,
/// };
///
/// let classification = Classification::new()
///     .with_sector(SectorInfo::from_composite(Sector::Financial))
///     .with_rating(RatingInfo::from_composite(CreditRating::A))
///     .with_seniority(SeniorityInfo::from_composite(Seniority::Subordinated))
///     .with_issuer("Example Bank")
///     .with_country("US");
///
/// assert_eq!(classification.sector.composite, Some(Sector::Financial));
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Classification {
    /// Sector classification.
    pub sector: SectorInfo,

    /// Credit rating.
    pub rating: RatingInfo,

    /// Seniority / capital structure.
    pub seniority: SeniorityInfo,

    /// Issuer name.
    pub issuer: Option<String>,

    /// Issuer identifier (LEI, Bloomberg ID, etc.).
    pub issuer_id: Option<String>,

    /// Country (ISO 3166-1 alpha-2).
    pub country: Option<String>,

    /// Region (Americas, EMEA, APAC, etc.).
    pub region: Option<String>,

    /// Fully custom fields (user-defined).
    pub custom: HashMap<String, String>,
}

impl Classification {
    /// Creates a new empty classification.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the sector.
    #[must_use]
    pub fn with_sector(mut self, sector: SectorInfo) -> Self {
        self.sector = sector;
        self
    }

    /// Sets the rating.
    #[must_use]
    pub fn with_rating(mut self, rating: RatingInfo) -> Self {
        self.rating = rating;
        self
    }

    /// Sets the seniority.
    #[must_use]
    pub fn with_seniority(mut self, seniority: SeniorityInfo) -> Self {
        self.seniority = seniority;
        self
    }

    /// Sets the issuer.
    #[must_use]
    pub fn with_issuer(mut self, issuer: &str) -> Self {
        self.issuer = Some(issuer.to_string());
        self
    }

    /// Sets the issuer ID.
    #[must_use]
    pub fn with_issuer_id(mut self, issuer_id: &str) -> Self {
        self.issuer_id = Some(issuer_id.to_string());
        self
    }

    /// Sets the country.
    #[must_use]
    pub fn with_country(mut self, country: &str) -> Self {
        self.country = Some(country.to_string());
        self
    }

    /// Sets the region.
    #[must_use]
    pub fn with_region(mut self, region: &str) -> Self {
        self.region = Some(region.to_string());
        self
    }

    /// Sets a custom field.
    #[must_use]
    pub fn with_custom(mut self, key: &str, value: &str) -> Self {
        self.custom.insert(key.to_string(), value.to_string());
        self
    }

    /// Gets a custom field.
    #[must_use]
    pub fn custom(&self, key: &str) -> Option<&str> {
        self.custom.get(key).map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sector tests (now testing convex-bonds types)
    #[test]
    fn test_sector_basics() {
        assert_eq!(Sector::Government.name(), "Government");
        assert!(Sector::Government.is_government_related());
        assert!(Sector::MortgageBacked.is_securitized());
        assert!(!Sector::Corporate.is_securitized());
    }

    #[test]
    fn test_sector_info() {
        let info = SectorInfo::new()
            .with_classification("BICS", &["Financials", "Banking", "Commercial"])
            .with_composite(Sector::Financial);

        assert_eq!(info.composite, Some(Sector::Financial));
        assert_eq!(info.level("BICS", 0), Some("Financials"));
        assert_eq!(info.level("BICS", 1), Some("Banking"));
        assert_eq!(info.level("BICS", 2), Some("Commercial"));
        assert_eq!(info.level("BICS", 3), None);
        assert_eq!(info.level("GICS", 0), None);
    }

    // Rating tests (now testing convex-bonds types)
    #[test]
    fn test_credit_rating_basics() {
        assert!(CreditRating::AAA.is_investment_grade());
        assert!(CreditRating::BBBMinus.is_investment_grade());
        assert!(!CreditRating::BBPlus.is_investment_grade());
        assert!(CreditRating::BBPlus.is_high_yield());
        assert!(!CreditRating::D.is_high_yield());
    }

    #[test]
    fn test_credit_rating_score() {
        assert_eq!(CreditRating::AAA.score(), 1);
        assert_eq!(CreditRating::D.score(), 22);
        assert_eq!(CreditRating::NotRated.score(), 99);
    }

    #[test]
    fn test_credit_rating_ordering() {
        assert!(CreditRating::AAA < CreditRating::AA);
        assert!(CreditRating::BBBMinus < CreditRating::BBPlus);
    }

    #[test]
    fn test_credit_rating_bucket() {
        assert_eq!(CreditRating::AAA.bucket(), RatingBucket::AAA);
        assert_eq!(CreditRating::AAPlus.bucket(), RatingBucket::AA);
        assert_eq!(CreditRating::AA.bucket(), RatingBucket::AA);
        assert_eq!(CreditRating::AAMinus.bucket(), RatingBucket::AA);
        assert_eq!(CreditRating::D.bucket(), RatingBucket::Default);
    }

    #[test]
    fn test_credit_rating_from_str() {
        assert_eq!(CreditRating::parse("AAA"), Some(CreditRating::AAA));
        assert_eq!(CreditRating::parse("Aa1"), Some(CreditRating::AAPlus));
        assert_eq!(CreditRating::parse("Baa2"), Some(CreditRating::BBB));
        assert_eq!(CreditRating::parse("XXX"), None);
    }

    #[test]
    fn test_rating_info() {
        let info = RatingInfo::new()
            .with_rating("SP", "BBB+")
            .with_rating("Moodys", "Baa1")
            .with_composite(CreditRating::BBBPlus);

        assert_eq!(info.composite, Some(CreditRating::BBBPlus));
        assert_eq!(info.rating("SP"), Some("BBB+"));
        assert_eq!(info.rating("Moodys"), Some("Baa1"));
        assert_eq!(info.rating("Fitch"), None);
        assert!(info.is_investment_grade().unwrap());
    }

    // Seniority tests (now testing convex-bonds types)
    #[test]
    fn test_seniority_basics() {
        assert!(Seniority::SeniorSecured < Seniority::SeniorUnsecured);
        assert!(Seniority::Subordinated < Seniority::Hybrid);
        assert!(!Seniority::SeniorSecured.is_bailin_eligible());
        assert!(Seniority::Subordinated.is_bailin_eligible());
    }

    #[test]
    fn test_seniority_recovery() {
        assert_eq!(Seniority::SeniorSecured.typical_recovery(), 0.60);
        assert_eq!(Seniority::Equity.typical_recovery(), 0.0);
    }

    #[test]
    fn test_seniority_info_coco() {
        let info = SeniorityInfo::new()
            .with_composite(Seniority::Hybrid)
            .with_capital_tier("AT1")
            .with_coco_trigger(0.05125, "MechanicalWritedown");

        assert!(info.is_coco());
        assert!(info.is_bailin_eligible());
        assert_eq!(info.trigger_level, Some(0.05125));
    }

    // Classification tests
    #[test]
    fn test_classification() {
        let classification = Classification::new()
            .with_sector(SectorInfo::from_composite(Sector::Financial))
            .with_rating(RatingInfo::from_composite(CreditRating::A))
            .with_seniority(SeniorityInfo::from_composite(Seniority::Subordinated))
            .with_issuer("Example Bank")
            .with_country("US")
            .with_custom("internal_id", "12345");

        assert_eq!(classification.sector.composite, Some(Sector::Financial));
        assert_eq!(classification.rating.composite, Some(CreditRating::A));
        assert_eq!(
            classification.seniority.composite,
            Some(Seniority::Subordinated)
        );
        assert_eq!(classification.issuer, Some("Example Bank".to_string()));
        assert_eq!(classification.custom("internal_id"), Some("12345"));
    }
}
