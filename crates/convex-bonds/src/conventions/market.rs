//! Market and instrument type definitions.
//!
//! This module defines enumerations for bond markets and instrument types,
//! enabling programmatic convention lookup.

use serde::{Deserialize, Serialize};

/// Bond market classification.
///
/// Markets are organized by geographic region and regulatory environment.
/// Each market has specific conventions for day counts, settlement, and
/// yield calculations.
///
/// # Example
///
/// ```rust
/// use convex_bonds::conventions::Market;
///
/// let market = Market::US;
/// assert_eq!(market.region(), "North America");
/// assert_eq!(market.currency_code(), "USD");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Market {
    // =========================================================================
    // Tier 1: Full Coverage (US, EUR, GBP)
    // =========================================================================
    /// United States bond market
    US,
    /// United Kingdom bond market
    UK,
    /// Germany (Eurozone)
    Germany,
    /// France (Eurozone)
    France,
    /// Italy (Eurozone)
    Italy,
    /// Spain (Eurozone)
    Spain,
    /// Netherlands (Eurozone)
    Netherlands,
    /// Belgium (Eurozone)
    Belgium,
    /// Austria (Eurozone)
    Austria,
    /// Portugal (Eurozone)
    Portugal,
    /// Ireland (Eurozone)
    Ireland,
    /// Finland (Eurozone)
    Finland,
    /// Generic Eurozone (supranational, EIB, etc.)
    Eurozone,

    // =========================================================================
    // Tier 2: Standard Coverage
    // =========================================================================
    /// Japan
    Japan,
    /// Switzerland
    Switzerland,
    /// Australia
    Australia,
    /// Canada
    Canada,
    /// Sweden
    Sweden,
    /// Norway
    Norway,
    /// Denmark
    Denmark,
    /// New Zealand
    NewZealand,

    // =========================================================================
    // Tier 3: Basic Coverage (Emerging Markets)
    // =========================================================================
    /// Brazil
    Brazil,
    /// Mexico
    Mexico,
    /// South Korea
    SouthKorea,
    /// China (onshore CNY)
    China,
    /// India
    India,
    /// South Africa
    SouthAfrica,
    /// Poland
    Poland,
    /// Czech Republic
    CzechRepublic,
    /// Hungary
    Hungary,
    /// Turkey
    Turkey,
    /// Russia
    Russia,
    /// Singapore
    Singapore,
    /// Hong Kong
    HongKong,
    /// Taiwan
    Taiwan,
    /// Thailand
    Thailand,
    /// Malaysia
    Malaysia,
    /// Indonesia
    Indonesia,
    /// Philippines
    Philippines,

    /// Other/Custom market
    Other(u16),
}

impl Market {
    /// Returns the ISO 3166-1 alpha-2 country code.
    #[must_use]
    pub const fn country_code(&self) -> &'static str {
        match self {
            Self::US => "US",
            Self::UK => "GB",
            Self::Germany => "DE",
            Self::France => "FR",
            Self::Italy => "IT",
            Self::Spain => "ES",
            Self::Netherlands => "NL",
            Self::Belgium => "BE",
            Self::Austria => "AT",
            Self::Portugal => "PT",
            Self::Ireland => "IE",
            Self::Finland => "FI",
            Self::Eurozone => "EU",
            Self::Japan => "JP",
            Self::Switzerland => "CH",
            Self::Australia => "AU",
            Self::Canada => "CA",
            Self::Sweden => "SE",
            Self::Norway => "NO",
            Self::Denmark => "DK",
            Self::NewZealand => "NZ",
            Self::Brazil => "BR",
            Self::Mexico => "MX",
            Self::SouthKorea => "KR",
            Self::China => "CN",
            Self::India => "IN",
            Self::SouthAfrica => "ZA",
            Self::Poland => "PL",
            Self::CzechRepublic => "CZ",
            Self::Hungary => "HU",
            Self::Turkey => "TR",
            Self::Russia => "RU",
            Self::Singapore => "SG",
            Self::HongKong => "HK",
            Self::Taiwan => "TW",
            Self::Thailand => "TH",
            Self::Malaysia => "MY",
            Self::Indonesia => "ID",
            Self::Philippines => "PH",
            Self::Other(_) => "XX",
        }
    }

    /// Returns the primary currency code for this market.
    #[must_use]
    pub const fn currency_code(&self) -> &'static str {
        match self {
            Self::US => "USD",
            Self::UK => "GBP",
            Self::Germany
            | Self::France
            | Self::Italy
            | Self::Spain
            | Self::Netherlands
            | Self::Belgium
            | Self::Austria
            | Self::Portugal
            | Self::Ireland
            | Self::Finland
            | Self::Eurozone => "EUR",
            Self::Japan => "JPY",
            Self::Switzerland => "CHF",
            Self::Australia => "AUD",
            Self::Canada => "CAD",
            Self::Sweden => "SEK",
            Self::Norway => "NOK",
            Self::Denmark => "DKK",
            Self::NewZealand => "NZD",
            Self::Brazil => "BRL",
            Self::Mexico => "MXN",
            Self::SouthKorea => "KRW",
            Self::China => "CNY",
            Self::India => "INR",
            Self::SouthAfrica => "ZAR",
            Self::Poland => "PLN",
            Self::CzechRepublic => "CZK",
            Self::Hungary => "HUF",
            Self::Turkey => "TRY",
            Self::Russia => "RUB",
            Self::Singapore => "SGD",
            Self::HongKong => "HKD",
            Self::Taiwan => "TWD",
            Self::Thailand => "THB",
            Self::Malaysia => "MYR",
            Self::Indonesia => "IDR",
            Self::Philippines => "PHP",
            Self::Other(_) => "XXX",
        }
    }

    /// Returns the region name for this market.
    #[must_use]
    pub const fn region(&self) -> &'static str {
        match self {
            Self::US | Self::Canada | Self::Mexico => "North America",
            Self::UK
            | Self::Germany
            | Self::France
            | Self::Italy
            | Self::Spain
            | Self::Netherlands
            | Self::Belgium
            | Self::Austria
            | Self::Portugal
            | Self::Ireland
            | Self::Finland
            | Self::Eurozone
            | Self::Switzerland
            | Self::Sweden
            | Self::Norway
            | Self::Denmark
            | Self::Poland
            | Self::CzechRepublic
            | Self::Hungary
            | Self::Russia
            | Self::Turkey => "Europe",
            Self::Japan
            | Self::China
            | Self::SouthKorea
            | Self::Taiwan
            | Self::HongKong
            | Self::Singapore
            | Self::India
            | Self::Thailand
            | Self::Malaysia
            | Self::Indonesia
            | Self::Philippines => "Asia Pacific",
            Self::Australia | Self::NewZealand => "Oceania",
            Self::Brazil | Self::SouthAfrica => "Emerging Markets",
            Self::Other(_) => "Other",
        }
    }

    /// Returns whether this is a Eurozone market.
    #[must_use]
    pub const fn is_eurozone(&self) -> bool {
        matches!(
            self,
            Self::Germany
                | Self::France
                | Self::Italy
                | Self::Spain
                | Self::Netherlands
                | Self::Belgium
                | Self::Austria
                | Self::Portugal
                | Self::Ireland
                | Self::Finland
                | Self::Eurozone
        )
    }

    /// Returns the coverage tier for this market.
    #[must_use]
    pub const fn tier(&self) -> MarketTier {
        match self {
            Self::US | Self::UK | Self::Germany | Self::France | Self::Italy | Self::Spain => {
                MarketTier::Tier1
            }
            Self::Netherlands
            | Self::Belgium
            | Self::Austria
            | Self::Portugal
            | Self::Ireland
            | Self::Finland
            | Self::Eurozone
            | Self::Japan
            | Self::Switzerland
            | Self::Australia
            | Self::Canada
            | Self::Sweden
            | Self::Norway
            | Self::Denmark
            | Self::NewZealand => MarketTier::Tier2,
            _ => MarketTier::Tier3,
        }
    }

    /// Creates a market from a country code.
    #[must_use]
    pub fn from_country_code(code: &str) -> Option<Self> {
        match code.to_uppercase().as_str() {
            "US" => Some(Self::US),
            "GB" | "UK" => Some(Self::UK),
            "DE" => Some(Self::Germany),
            "FR" => Some(Self::France),
            "IT" => Some(Self::Italy),
            "ES" => Some(Self::Spain),
            "NL" => Some(Self::Netherlands),
            "BE" => Some(Self::Belgium),
            "AT" => Some(Self::Austria),
            "PT" => Some(Self::Portugal),
            "IE" => Some(Self::Ireland),
            "FI" => Some(Self::Finland),
            "EU" => Some(Self::Eurozone),
            "JP" => Some(Self::Japan),
            "CH" => Some(Self::Switzerland),
            "AU" => Some(Self::Australia),
            "CA" => Some(Self::Canada),
            "SE" => Some(Self::Sweden),
            "NO" => Some(Self::Norway),
            "DK" => Some(Self::Denmark),
            "NZ" => Some(Self::NewZealand),
            "BR" => Some(Self::Brazil),
            "MX" => Some(Self::Mexico),
            "KR" => Some(Self::SouthKorea),
            "CN" => Some(Self::China),
            "IN" => Some(Self::India),
            "ZA" => Some(Self::SouthAfrica),
            "PL" => Some(Self::Poland),
            "CZ" => Some(Self::CzechRepublic),
            "HU" => Some(Self::Hungary),
            "TR" => Some(Self::Turkey),
            "RU" => Some(Self::Russia),
            "SG" => Some(Self::Singapore),
            "HK" => Some(Self::HongKong),
            "TW" => Some(Self::Taiwan),
            "TH" => Some(Self::Thailand),
            "MY" => Some(Self::Malaysia),
            "ID" => Some(Self::Indonesia),
            "PH" => Some(Self::Philippines),
            _ => None,
        }
    }
}

impl std::fmt::Display for Market {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.country_code())
    }
}

impl Default for Market {
    fn default() -> Self {
        Self::US
    }
}

/// Market coverage tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarketTier {
    /// Full coverage (US, UK, major EUR)
    Tier1,
    /// Standard coverage (developed markets)
    Tier2,
    /// Basic coverage (emerging markets)
    Tier3,
}

/// Bond instrument type classification.
///
/// Instrument types determine the specific conventions within a market.
///
/// # Example
///
/// ```rust
/// use convex_bonds::conventions::InstrumentType;
///
/// let inst = InstrumentType::GovernmentBond;
/// assert!(inst.is_government());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum InstrumentType {
    // =========================================================================
    // Government Securities
    // =========================================================================
    /// Standard government coupon bond (Treasury, Gilt, Bund, etc.)
    #[default]
    GovernmentBond,
    /// Treasury/Government bill (discount instrument)
    TreasuryBill,
    /// Government floating rate note
    GovernmentFRN,
    /// Inflation-linked government bond (TIPS, Linkers)
    InflationLinked,
    /// Government STRIPS (separate trading of principal and interest)
    Strips,

    // =========================================================================
    // Corporate Securities
    // =========================================================================
    /// Investment grade corporate bond
    CorporateIG,
    /// High yield (below investment grade) corporate bond
    CorporateHY,
    /// Corporate floating rate note
    CorporateFRN,
    /// Convertible bond
    Convertible,
    /// Commercial paper
    CommercialPaper,

    // =========================================================================
    // Other Securities
    // =========================================================================
    /// US Municipal bond (tax-exempt)
    Municipal,
    /// US Agency bond (FNMA, FHLMC, etc.)
    Agency,
    /// Supranational bond (World Bank, EIB, etc.)
    Supranational,
    /// Covered bond (Pfandbrief, etc.)
    CoveredBond,
    /// Asset-backed security
    ABS,
    /// Mortgage-backed security
    MBS,
}

impl InstrumentType {
    /// Returns true if this is a government security.
    #[must_use]
    pub const fn is_government(&self) -> bool {
        matches!(
            self,
            Self::GovernmentBond
                | Self::TreasuryBill
                | Self::GovernmentFRN
                | Self::InflationLinked
                | Self::Strips
        )
    }

    /// Returns true if this is a corporate security.
    #[must_use]
    pub const fn is_corporate(&self) -> bool {
        matches!(
            self,
            Self::CorporateIG | Self::CorporateHY | Self::CorporateFRN | Self::Convertible
        )
    }

    /// Returns true if this is a floating rate instrument.
    #[must_use]
    pub const fn is_floating(&self) -> bool {
        matches!(self, Self::GovernmentFRN | Self::CorporateFRN)
    }

    /// Returns true if this is a discount instrument.
    #[must_use]
    pub const fn is_discount(&self) -> bool {
        matches!(
            self,
            Self::TreasuryBill | Self::CommercialPaper | Self::Strips
        )
    }

    /// Returns true if this is an inflation-linked instrument.
    #[must_use]
    pub const fn is_inflation_linked(&self) -> bool {
        matches!(self, Self::InflationLinked)
    }

    /// Returns the typical name for this instrument type in the given market.
    #[must_use]
    pub const fn market_name(&self, market: Market) -> &'static str {
        match (self, market) {
            (Self::GovernmentBond, Market::US) => "Treasury Note/Bond",
            (Self::GovernmentBond, Market::UK) => "Gilt",
            (Self::GovernmentBond, Market::Germany) => "Bundesanleihe",
            (Self::GovernmentBond, Market::France) => "OAT",
            (Self::GovernmentBond, Market::Italy) => "BTP",
            (Self::GovernmentBond, Market::Spain) => "Bono",
            (Self::GovernmentBond, Market::Japan) => "JGB",
            (Self::GovernmentBond, Market::Canada) => "Canada Bond",
            (Self::GovernmentBond, Market::Australia) => "ACGB",
            (Self::GovernmentBond, Market::Switzerland) => "Confederation Bond",
            (Self::GovernmentBond, _) => "Government Bond",

            (Self::TreasuryBill, Market::US) => "T-Bill",
            (Self::TreasuryBill, Market::UK) => "Treasury Bill",
            (Self::TreasuryBill, Market::Germany) => "Bubill",
            (Self::TreasuryBill, Market::France) => "BTF",
            (Self::TreasuryBill, Market::Italy) => "BOT",
            (Self::TreasuryBill, Market::Spain) => "Letra",
            (Self::TreasuryBill, Market::Japan) => "FB",
            (Self::TreasuryBill, _) => "Treasury Bill",

            (Self::InflationLinked, Market::US) => "TIPS",
            (Self::InflationLinked, Market::UK) => "Index-Linked Gilt",
            (Self::InflationLinked, Market::Germany) => "Bundei",
            (Self::InflationLinked, Market::France) => "OATi",
            (Self::InflationLinked, Market::Italy) => "BTPei",
            (Self::InflationLinked, _) => "Inflation-Linked Bond",

            (Self::CorporateIG, _) => "Investment Grade Corporate",
            (Self::CorporateHY, _) => "High Yield Corporate",
            (Self::Municipal, _) => "Municipal Bond",
            (Self::Agency, _) => "Agency Bond",
            (Self::Supranational, _) => "Supranational Bond",
            _ => "Bond",
        }
    }
}

impl std::fmt::Display for InstrumentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::GovernmentBond => "Government Bond",
            Self::TreasuryBill => "Treasury Bill",
            Self::GovernmentFRN => "Government FRN",
            Self::InflationLinked => "Inflation-Linked",
            Self::Strips => "STRIPS",
            Self::CorporateIG => "Corporate IG",
            Self::CorporateHY => "Corporate HY",
            Self::CorporateFRN => "Corporate FRN",
            Self::Convertible => "Convertible",
            Self::CommercialPaper => "Commercial Paper",
            Self::Municipal => "Municipal",
            Self::Agency => "Agency",
            Self::Supranational => "Supranational",
            Self::CoveredBond => "Covered Bond",
            Self::ABS => "ABS",
            Self::MBS => "MBS",
        };
        write!(f, "{}", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_country_code() {
        assert_eq!(Market::US.country_code(), "US");
        assert_eq!(Market::UK.country_code(), "GB");
        assert_eq!(Market::Germany.country_code(), "DE");
        assert_eq!(Market::Japan.country_code(), "JP");
    }

    #[test]
    fn test_market_currency() {
        assert_eq!(Market::US.currency_code(), "USD");
        assert_eq!(Market::UK.currency_code(), "GBP");
        assert_eq!(Market::Germany.currency_code(), "EUR");
        assert_eq!(Market::France.currency_code(), "EUR");
        assert_eq!(Market::Japan.currency_code(), "JPY");
    }

    #[test]
    fn test_market_region() {
        assert_eq!(Market::US.region(), "North America");
        assert_eq!(Market::UK.region(), "Europe");
        assert_eq!(Market::Japan.region(), "Asia Pacific");
    }

    #[test]
    fn test_market_is_eurozone() {
        assert!(Market::Germany.is_eurozone());
        assert!(Market::France.is_eurozone());
        assert!(Market::Italy.is_eurozone());
        assert!(!Market::US.is_eurozone());
        assert!(!Market::UK.is_eurozone());
    }

    #[test]
    fn test_market_tier() {
        assert_eq!(Market::US.tier(), MarketTier::Tier1);
        assert_eq!(Market::UK.tier(), MarketTier::Tier1);
        assert_eq!(Market::Germany.tier(), MarketTier::Tier1);
        assert_eq!(Market::Japan.tier(), MarketTier::Tier2);
        assert_eq!(Market::Brazil.tier(), MarketTier::Tier3);
    }

    #[test]
    fn test_market_from_country_code() {
        assert_eq!(Market::from_country_code("US"), Some(Market::US));
        assert_eq!(Market::from_country_code("GB"), Some(Market::UK));
        assert_eq!(Market::from_country_code("UK"), Some(Market::UK));
        assert_eq!(Market::from_country_code("DE"), Some(Market::Germany));
        assert_eq!(Market::from_country_code("XX"), None);
    }

    #[test]
    fn test_instrument_type_predicates() {
        assert!(InstrumentType::GovernmentBond.is_government());
        assert!(InstrumentType::TreasuryBill.is_government());
        assert!(!InstrumentType::CorporateIG.is_government());

        assert!(InstrumentType::CorporateIG.is_corporate());
        assert!(InstrumentType::CorporateHY.is_corporate());
        assert!(!InstrumentType::GovernmentBond.is_corporate());

        assert!(InstrumentType::TreasuryBill.is_discount());
        assert!(InstrumentType::Strips.is_discount());
        assert!(!InstrumentType::GovernmentBond.is_discount());

        assert!(InstrumentType::GovernmentFRN.is_floating());
        assert!(InstrumentType::CorporateFRN.is_floating());
        assert!(!InstrumentType::CorporateIG.is_floating());
    }

    #[test]
    fn test_instrument_type_market_name() {
        assert_eq!(
            InstrumentType::GovernmentBond.market_name(Market::US),
            "Treasury Note/Bond"
        );
        assert_eq!(
            InstrumentType::GovernmentBond.market_name(Market::UK),
            "Gilt"
        );
        assert_eq!(
            InstrumentType::GovernmentBond.market_name(Market::Germany),
            "Bundesanleihe"
        );
        assert_eq!(
            InstrumentType::TreasuryBill.market_name(Market::US),
            "T-Bill"
        );
        assert_eq!(
            InstrumentType::InflationLinked.market_name(Market::US),
            "TIPS"
        );
    }

    #[test]
    fn test_instrument_type_display() {
        assert_eq!(
            format!("{}", InstrumentType::GovernmentBond),
            "Government Bond"
        );
        assert_eq!(format!("{}", InstrumentType::CorporateIG), "Corporate IG");
    }
}
