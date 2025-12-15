//! Central registry for market conventions.
//!
//! This module provides a unified registry for looking up bond conventions
//! and yield calculation rules based on market and instrument type.
//!
//! # Example
//!
//! ```rust
//! use convex_bonds::conventions::{ConventionRegistry, ConventionKey, Market, InstrumentType};
//!
//! let registry = ConventionRegistry::global();
//! let key = ConventionKey::new(Market::US, InstrumentType::GovernmentBond);
//!
//! if let Some(conventions) = registry.get(&key) {
//!     println!("Settlement days: {}", conventions.settlement_days());
//! }
//!
//! if let Some(rules) = registry.rules(&key) {
//!     println!("Yield convention: {}", rules.convention);
//! }
//! ```

use std::collections::HashMap;
use std::sync::OnceLock;

use super::market::{InstrumentType, Market};
use super::BondConventions;
use crate::types::YieldCalculationRules;

/// Key for convention lookup combining market and instrument type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConventionKey {
    /// The bond market
    pub market: Market,
    /// The instrument type within that market
    pub instrument_type: InstrumentType,
    /// Optional variant for sub-categories (e.g., "TIPS", "FRN")
    pub variant: Option<String>,
}

impl ConventionKey {
    /// Creates a new convention key.
    #[must_use]
    pub fn new(market: Market, instrument_type: InstrumentType) -> Self {
        Self {
            market,
            instrument_type,
            variant: None,
        }
    }

    /// Creates a new convention key with a variant.
    #[must_use]
    pub fn with_variant(market: Market, instrument_type: InstrumentType, variant: &str) -> Self {
        Self {
            market,
            instrument_type,
            variant: Some(variant.to_string()),
        }
    }

    // =========================================================================
    // Convenience constructors for common key types
    // =========================================================================

    /// US Treasury notes/bonds.
    #[must_use]
    pub fn us_treasury() -> Self {
        Self::new(Market::US, InstrumentType::GovernmentBond)
    }

    /// US Treasury bills.
    #[must_use]
    pub fn us_treasury_bill() -> Self {
        Self::new(Market::US, InstrumentType::TreasuryBill)
    }

    /// US TIPS.
    #[must_use]
    pub fn us_tips() -> Self {
        Self::new(Market::US, InstrumentType::InflationLinked)
    }

    /// US corporate bonds (investment grade).
    #[must_use]
    pub fn us_corporate_ig() -> Self {
        Self::new(Market::US, InstrumentType::CorporateIG)
    }

    /// US corporate bonds (high yield).
    #[must_use]
    pub fn us_corporate_hy() -> Self {
        Self::new(Market::US, InstrumentType::CorporateHY)
    }

    /// US municipal bonds.
    #[must_use]
    pub fn us_municipal() -> Self {
        Self::new(Market::US, InstrumentType::Municipal)
    }

    /// US agency bonds.
    #[must_use]
    pub fn us_agency() -> Self {
        Self::new(Market::US, InstrumentType::Agency)
    }

    /// UK gilts.
    #[must_use]
    pub fn uk_gilt() -> Self {
        Self::new(Market::UK, InstrumentType::GovernmentBond)
    }

    /// UK index-linked gilts.
    #[must_use]
    pub fn uk_gilt_linker() -> Self {
        Self::new(Market::UK, InstrumentType::InflationLinked)
    }

    /// German Bunds.
    #[must_use]
    pub fn german_bund() -> Self {
        Self::new(Market::Germany, InstrumentType::GovernmentBond)
    }

    /// German Bubills.
    #[must_use]
    pub fn german_bubill() -> Self {
        Self::new(Market::Germany, InstrumentType::TreasuryBill)
    }

    /// French OATs.
    #[must_use]
    pub fn french_oat() -> Self {
        Self::new(Market::France, InstrumentType::GovernmentBond)
    }

    /// Italian BTPs.
    #[must_use]
    pub fn italian_btp() -> Self {
        Self::new(Market::Italy, InstrumentType::GovernmentBond)
    }

    /// Spanish Bonos.
    #[must_use]
    pub fn spanish_bono() -> Self {
        Self::new(Market::Spain, InstrumentType::GovernmentBond)
    }

    /// Japanese JGBs.
    #[must_use]
    pub fn japanese_jgb() -> Self {
        Self::new(Market::Japan, InstrumentType::GovernmentBond)
    }

    /// Swiss Confederation bonds.
    #[must_use]
    pub fn swiss() -> Self {
        Self::new(Market::Switzerland, InstrumentType::GovernmentBond)
    }

    /// Australian government bonds.
    #[must_use]
    pub fn australian() -> Self {
        Self::new(Market::Australia, InstrumentType::GovernmentBond)
    }

    /// Canadian government bonds.
    #[must_use]
    pub fn canadian() -> Self {
        Self::new(Market::Canada, InstrumentType::GovernmentBond)
    }

    /// Eurobonds (supranational).
    #[must_use]
    pub fn eurobond() -> Self {
        Self::new(Market::Eurozone, InstrumentType::Supranational)
    }
}

impl std::fmt::Display for ConventionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.variant {
            Some(v) => write!(f, "{}:{} ({})", self.market, self.instrument_type, v),
            None => write!(f, "{}:{}", self.market, self.instrument_type),
        }
    }
}

/// Central registry for market conventions.
///
/// The registry provides fast O(1) lookup for bond conventions and yield
/// calculation rules based on market and instrument type.
///
/// # Thread Safety
///
/// The registry is thread-safe and can be shared across threads.
///
/// # Performance
///
/// Convention lookup is designed to be < 10ns using `HashMap` with pre-hashed keys.
pub struct ConventionRegistry {
    /// Bond conventions indexed by key
    conventions: HashMap<ConventionKey, BondConventions>,
    /// Yield calculation rules indexed by key
    rules: HashMap<ConventionKey, YieldCalculationRules>,
}

/// Global singleton registry.
static GLOBAL_REGISTRY: OnceLock<ConventionRegistry> = OnceLock::new();

impl ConventionRegistry {
    /// Returns the global convention registry.
    ///
    /// The registry is lazily initialized on first access and remains
    /// available for the lifetime of the program.
    ///
    /// # Example
    ///
    /// ```rust
    /// use convex_bonds::conventions::{ConventionRegistry, ConventionKey};
    ///
    /// let registry = ConventionRegistry::global();
    /// let rules = registry.rules(&ConventionKey::us_treasury());
    /// ```
    #[must_use]
    pub fn global() -> &'static Self {
        GLOBAL_REGISTRY.get_or_init(Self::new)
    }

    /// Creates a new registry with all known conventions.
    #[must_use]
    pub fn new() -> Self {
        let mut registry = Self {
            conventions: HashMap::new(),
            rules: HashMap::new(),
        };

        // Register all known conventions
        registry.register_us_conventions();
        registry.register_uk_conventions();
        registry.register_eurozone_conventions();
        registry.register_tier2_conventions();
        registry.register_emerging_market_conventions();

        registry
    }

    /// Looks up bond conventions for the given key.
    ///
    /// Returns `None` if no conventions are registered for the key.
    #[must_use]
    pub fn get(&self, key: &ConventionKey) -> Option<&BondConventions> {
        self.conventions.get(key).or_else(|| {
            // Fall back to key without variant
            if key.variant.is_some() {
                let base_key = ConventionKey::new(key.market, key.instrument_type);
                self.conventions.get(&base_key)
            } else {
                None
            }
        })
    }

    /// Looks up yield calculation rules for the given key.
    ///
    /// Returns `None` if no rules are registered for the key.
    #[must_use]
    pub fn rules(&self, key: &ConventionKey) -> Option<&YieldCalculationRules> {
        self.rules.get(key).or_else(|| {
            // Fall back to key without variant
            if key.variant.is_some() {
                let base_key = ConventionKey::new(key.market, key.instrument_type);
                self.rules.get(&base_key)
            } else {
                None
            }
        })
    }

    /// Looks up conventions using market and instrument type directly.
    #[must_use]
    pub fn get_by_market(
        &self,
        market: Market,
        instrument_type: InstrumentType,
    ) -> Option<&BondConventions> {
        self.get(&ConventionKey::new(market, instrument_type))
    }

    /// Looks up rules using market and instrument type directly.
    #[must_use]
    pub fn rules_by_market(
        &self,
        market: Market,
        instrument_type: InstrumentType,
    ) -> Option<&YieldCalculationRules> {
        self.rules(&ConventionKey::new(market, instrument_type))
    }

    /// Returns all registered convention keys.
    #[must_use]
    pub fn keys(&self) -> Vec<&ConventionKey> {
        self.conventions.keys().collect()
    }

    /// Returns the number of registered conventions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.conventions.len()
    }

    /// Returns true if no conventions are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.conventions.is_empty()
    }

    /// Returns default rules for a market (falls back to generic rules).
    #[must_use]
    pub fn default_rules_for_market(&self, market: Market) -> YieldCalculationRules {
        // Try government bond first
        if let Some(rules) = self.rules_by_market(market, InstrumentType::GovernmentBond) {
            return rules.clone();
        }

        // Fall back to corporate IG
        if let Some(rules) = self.rules_by_market(market, InstrumentType::CorporateIG) {
            return rules.clone();
        }

        // Fall back to regional defaults
        if market.is_eurozone() {
            return YieldCalculationRules::eurobond();
        }

        // Ultimate fallback
        YieldCalculationRules::default()
    }

    // =========================================================================
    // Registration methods
    // =========================================================================

    fn register(
        &mut self,
        key: ConventionKey,
        conventions: BondConventions,
        rules: YieldCalculationRules,
    ) {
        self.conventions.insert(key.clone(), conventions);
        self.rules.insert(key, rules);
    }

    fn register_us_conventions(&mut self) {
        use super::{us_corporate, us_treasury};

        // US Treasury Notes/Bonds
        self.register(
            ConventionKey::us_treasury(),
            us_treasury::note_bond(),
            YieldCalculationRules::us_treasury(),
        );

        // US Treasury Bills
        self.register(
            ConventionKey::us_treasury_bill(),
            us_treasury::bill(),
            YieldCalculationRules::us_treasury_bill(),
        );

        // US TIPS
        self.register(
            ConventionKey::us_tips(),
            us_treasury::tips(),
            YieldCalculationRules::us_treasury(), // Same as treasury for base yield
        );

        // US Corporate IG
        self.register(
            ConventionKey::us_corporate_ig(),
            us_corporate::investment_grade(),
            YieldCalculationRules::us_corporate(),
        );

        // US Corporate HY
        self.register(
            ConventionKey::us_corporate_hy(),
            us_corporate::high_yield(),
            YieldCalculationRules::us_corporate(),
        );

        // US Municipal
        self.register(
            ConventionKey::us_municipal(),
            us_corporate::municipal(),
            YieldCalculationRules::us_municipal(),
        );

        // US Agency
        self.register(
            ConventionKey::us_agency(),
            us_corporate::agency(),
            YieldCalculationRules::us_corporate(),
        );
    }

    fn register_uk_conventions(&mut self) {
        use super::uk_gilt;

        // UK Gilts
        self.register(
            ConventionKey::uk_gilt(),
            uk_gilt::conventional(),
            YieldCalculationRules::uk_gilt(),
        );

        // UK Index-Linked Gilts
        self.register(
            ConventionKey::uk_gilt_linker(),
            uk_gilt::index_linked(),
            YieldCalculationRules::uk_gilt(),
        );

        // UK Corporate (use eurobond-style)
        self.register(
            ConventionKey::new(Market::UK, InstrumentType::CorporateIG),
            BondConventions::default(),
            YieldCalculationRules::eurobond(),
        );
    }

    fn register_eurozone_conventions(&mut self) {
        use super::{eurobond, german_bund};

        // German Bunds
        self.register(
            ConventionKey::german_bund(),
            german_bund::bund(),
            YieldCalculationRules::german_bund(),
        );

        // German Bubills
        self.register(
            ConventionKey::german_bubill(),
            german_bund::bubill(),
            YieldCalculationRules::us_treasury_bill(), // Discount instruments
        );

        // French OATs
        self.register(
            ConventionKey::french_oat(),
            eurobond::french_oat(),
            YieldCalculationRules::french_oat(),
        );

        // Italian BTPs
        self.register(
            ConventionKey::italian_btp(),
            eurobond::italian_btp(),
            YieldCalculationRules::italian_btp(),
        );

        // Spanish Bonos
        self.register(
            ConventionKey::spanish_bono(),
            eurobond::spanish_bono(),
            YieldCalculationRules::spanish_bono(),
        );

        // Other Eurozone sovereigns (Netherlands, Belgium, Austria, etc.)
        let eurozone_markets = [
            Market::Netherlands,
            Market::Belgium,
            Market::Austria,
            Market::Portugal,
            Market::Ireland,
            Market::Finland,
        ];

        for market in eurozone_markets {
            self.register(
                ConventionKey::new(market, InstrumentType::GovernmentBond),
                eurobond::actual_actual(),
                YieldCalculationRules::eurobond(),
            );
        }

        // Eurobonds / Supranational
        self.register(
            ConventionKey::eurobond(),
            eurobond::supranational(),
            YieldCalculationRules::eurobond(),
        );

        // Eurozone Corporate
        self.register(
            ConventionKey::new(Market::Eurozone, InstrumentType::CorporateIG),
            eurobond::standard(),
            YieldCalculationRules::eurobond(),
        );
    }

    fn register_tier2_conventions(&mut self) {
        use super::{eurobond, japanese_jgb};

        // Japanese JGBs
        self.register(
            ConventionKey::japanese_jgb(),
            japanese_jgb::jgb(),
            YieldCalculationRules::japanese_jgb(),
        );

        // Swiss Confederation
        self.register(
            ConventionKey::swiss(),
            eurobond::actual_actual(), // Similar to EUR sovereigns
            YieldCalculationRules::swiss(),
        );

        // Australian Government Bonds
        self.register(
            ConventionKey::australian(),
            eurobond::actual_actual(),
            YieldCalculationRules::australian(),
        );

        // Canadian Government Bonds
        self.register(
            ConventionKey::canadian(),
            eurobond::actual_actual(),
            YieldCalculationRules::canadian(),
        );

        // Scandinavian countries (use Eurobond conventions)
        let scandinavian = [Market::Sweden, Market::Norway, Market::Denmark];
        for market in scandinavian {
            self.register(
                ConventionKey::new(market, InstrumentType::GovernmentBond),
                eurobond::actual_actual(),
                YieldCalculationRules::eurobond(),
            );
        }

        // New Zealand
        self.register(
            ConventionKey::new(Market::NewZealand, InstrumentType::GovernmentBond),
            eurobond::actual_actual(),
            YieldCalculationRules::australian(), // Similar to Australia
        );
    }

    fn register_emerging_market_conventions(&mut self) {
        use super::eurobond;

        // EM markets typically follow either US or EUR conventions
        // with some local variations

        let em_markets_usd_style = [Market::Mexico, Market::Brazil];

        for market in em_markets_usd_style {
            self.register(
                ConventionKey::new(market, InstrumentType::GovernmentBond),
                eurobond::actual_actual(),
                YieldCalculationRules::us_corporate(), // USD-style
            );
        }

        let em_markets_eur_style = [
            Market::Poland,
            Market::CzechRepublic,
            Market::Hungary,
            Market::Turkey,
            Market::Russia,
            Market::SouthAfrica,
        ];

        for market in em_markets_eur_style {
            self.register(
                ConventionKey::new(market, InstrumentType::GovernmentBond),
                eurobond::actual_actual(),
                YieldCalculationRules::eurobond(),
            );
        }

        let asian_markets = [
            Market::China,
            Market::SouthKorea,
            Market::Taiwan,
            Market::Singapore,
            Market::HongKong,
            Market::Thailand,
            Market::Malaysia,
            Market::Indonesia,
            Market::Philippines,
            Market::India,
        ];

        for market in asian_markets {
            self.register(
                ConventionKey::new(market, InstrumentType::GovernmentBond),
                eurobond::actual_actual(),
                YieldCalculationRules::eurobond(), // Default to ISMA
            );
        }
    }
}

impl Default for ConventionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_registry() {
        let registry = ConventionRegistry::global();
        assert!(!registry.is_empty());
    }

    #[test]
    fn test_us_treasury_lookup() {
        let registry = ConventionRegistry::global();
        let key = ConventionKey::us_treasury();

        let conventions = registry.get(&key).expect("US Treasury conventions");
        assert_eq!(conventions.settlement_days(), 1);

        let rules = registry.rules(&key).expect("US Treasury rules");
        assert_eq!(rules.frequency, convex_core::types::Frequency::SemiAnnual);
    }

    #[test]
    fn test_uk_gilt_lookup() {
        let registry = ConventionRegistry::global();
        let key = ConventionKey::uk_gilt();

        let rules = registry.rules(&key).expect("UK Gilt rules");
        assert!(rules.has_ex_dividend());
    }

    #[test]
    fn test_german_bund_lookup() {
        let registry = ConventionRegistry::global();
        let key = ConventionKey::german_bund();

        let rules = registry.rules(&key).expect("German Bund rules");
        assert_eq!(rules.frequency, convex_core::types::Frequency::Annual);
    }

    #[test]
    fn test_italian_btp_lookup() {
        let registry = ConventionRegistry::global();
        let key = ConventionKey::italian_btp();

        let rules = registry.rules(&key).expect("Italian BTP rules");
        assert!(rules.has_ex_dividend());
    }

    #[test]
    fn test_japanese_jgb_lookup() {
        let registry = ConventionRegistry::global();
        let key = ConventionKey::japanese_jgb();

        let rules = registry.rules(&key).expect("Japanese JGB rules");
        assert!(rules.compounding.is_simple());
    }

    #[test]
    fn test_convention_key_display() {
        let key = ConventionKey::us_treasury();
        let display = format!("{}", key);
        assert!(display.contains("US"));
        assert!(display.contains("Government Bond"));

        let key_with_variant =
            ConventionKey::with_variant(Market::US, InstrumentType::GovernmentBond, "10Y");
        let display = format!("{}", key_with_variant);
        assert!(display.contains("10Y"));
    }

    #[test]
    fn test_get_by_market() {
        let registry = ConventionRegistry::global();

        let conventions = registry
            .get_by_market(Market::US, InstrumentType::GovernmentBond)
            .expect("US Treasury by market");
        assert_eq!(conventions.settlement_days(), 1);
    }

    #[test]
    fn test_default_rules_for_market() {
        let registry = ConventionRegistry::global();

        let us_rules = registry.default_rules_for_market(Market::US);
        assert!(!us_rules.has_ex_dividend());

        let uk_rules = registry.default_rules_for_market(Market::UK);
        assert!(uk_rules.has_ex_dividend());

        let eur_rules = registry.default_rules_for_market(Market::Germany);
        assert_eq!(eur_rules.frequency, convex_core::types::Frequency::Annual);
    }

    #[test]
    fn test_registry_keys() {
        let registry = ConventionRegistry::global();
        let keys = registry.keys();

        // Should have US, UK, EUR, and more
        assert!(keys.len() > 20);
    }

    #[test]
    fn test_fallback_without_variant() {
        let registry = ConventionRegistry::global();

        // Key with variant that doesn't exist
        let key =
            ConventionKey::with_variant(Market::US, InstrumentType::GovernmentBond, "nonexistent");

        // Should fall back to base key
        let rules = registry.rules(&key);
        assert!(rules.is_some());
    }

    #[test]
    fn test_all_tier1_markets_registered() {
        let registry = ConventionRegistry::global();

        let tier1_keys = [
            ConventionKey::us_treasury(),
            ConventionKey::uk_gilt(),
            ConventionKey::german_bund(),
            ConventionKey::french_oat(),
            ConventionKey::italian_btp(),
            ConventionKey::spanish_bono(),
        ];

        for key in &tier1_keys {
            assert!(registry.rules(key).is_some(), "Missing rules for {:?}", key);
        }
    }
}
