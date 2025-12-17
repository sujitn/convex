//! Sovereign issuers for government bond curves.
//!
//! This module defines sovereign issuers (UST, UK Gilts, German Bunds, etc.)
//! with their associated conventions.

use convex_core::types::Currency;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Supranational bond issuers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SupranationalIssuer {
    /// European Investment Bank
    EIB,
    /// European Bank for Reconstruction and Development
    EBRD,
    /// World Bank (IBRD)
    WorldBank,
    /// Asian Development Bank
    ADB,
    /// Inter-American Development Bank
    IADB,
    /// Kreditanstalt für Wiederaufbau (German development bank)
    KfW,
    /// European Stability Mechanism
    ESM,
    /// European Union bonds
    EU,
    /// African Development Bank
    AfDB,
    /// Nordic Investment Bank
    NIB,
}

impl SupranationalIssuer {
    /// Returns the primary currency for this issuer.
    #[must_use]
    pub fn primary_currency(&self) -> Currency {
        match self {
            Self::EIB | Self::EBRD | Self::ESM | Self::EU | Self::KfW => Currency::EUR,
            Self::WorldBank | Self::ADB | Self::IADB | Self::AfDB | Self::NIB => Currency::USD,
        }
    }

    /// Returns the issuer's full name.
    #[must_use]
    pub fn full_name(&self) -> &'static str {
        match self {
            Self::EIB => "European Investment Bank",
            Self::EBRD => "European Bank for Reconstruction and Development",
            Self::WorldBank => "World Bank (IBRD)",
            Self::ADB => "Asian Development Bank",
            Self::IADB => "Inter-American Development Bank",
            Self::KfW => "Kreditanstalt für Wiederaufbau",
            Self::ESM => "European Stability Mechanism",
            Self::EU => "European Union",
            Self::AfDB => "African Development Bank",
            Self::NIB => "Nordic Investment Bank",
        }
    }
}

impl fmt::Display for SupranationalIssuer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::EIB => "EIB",
            Self::EBRD => "EBRD",
            Self::WorldBank => "World Bank",
            Self::ADB => "ADB",
            Self::IADB => "IADB",
            Self::KfW => "KfW",
            Self::ESM => "ESM",
            Self::EU => "EU",
            Self::AfDB => "AfDB",
            Self::NIB => "NIB",
        };
        write!(f, "{name}")
    }
}

/// Sovereign/Government bond issuers.
///
/// Represents the issuer of government bonds for use in government curve
/// construction and G-spread calculations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Sovereign {
    // Americas
    /// US Treasury
    #[default]
    UST,
    /// Government of Canada
    Canada,
    /// Brazilian Tesouro Nacional
    Brazil,
    /// Mexican Bonos M
    Mexico,
    /// Argentine Republic
    Argentina,
    /// Republic of Chile
    Chile,
    /// Republic of Colombia
    Colombia,
    /// Republic of Peru
    Peru,

    // Europe - Eurozone
    /// German Bunds, Bobls, Schatz
    Germany,
    /// French OATs
    France,
    /// Italian BTPs
    Italy,
    /// Spanish Bonos and Obligaciones
    Spain,
    /// Dutch DSLs
    Netherlands,
    /// Belgian OLOs
    Belgium,
    /// Austrian RAGBs
    Austria,
    /// Finnish Government Bonds
    Finland,
    /// Irish Government Bonds
    Ireland,
    /// Portuguese Obrigações do Tesouro
    Portugal,
    /// Greek Government Bonds
    Greece,

    // Europe - Non-Eurozone
    /// UK Gilts
    UK,
    /// Swiss Confederation Bonds
    Switzerland,
    /// Swedish Government Bonds (SGBs)
    Sweden,
    /// Norwegian Government Bonds
    Norway,
    /// Danish Government Bonds
    Denmark,
    /// Polish Treasury Bonds
    Poland,
    /// Czech Republic Government Bonds
    CzechRepublic,
    /// Hungarian Government Bonds
    Hungary,
    /// Romanian Government Bonds
    Romania,

    // Asia-Pacific
    /// Japanese Government Bonds (JGBs)
    Japan,
    /// Chinese Government Bonds (CGBs)
    China,
    /// Australian Commonwealth Government Bonds (ACGBs)
    Australia,
    /// South Korean Treasury Bonds (KTBs)
    SouthKorea,
    /// Singapore Government Securities (SGS)
    Singapore,
    /// Hong Kong SAR Government Bonds
    HongKong,
    /// Indian Government Securities (G-Secs)
    India,
    /// Indonesian Government Bonds
    Indonesia,
    /// Malaysian Government Securities
    Malaysia,
    /// Thai Government Bonds
    Thailand,
    /// Philippine Government Bonds
    Philippines,
    /// New Zealand Government Bonds
    NewZealand,

    // Middle East & Africa
    /// South African Government Bonds
    SouthAfrica,
    /// Israeli Government Bonds (Shahar/Gilon)
    Israel,
    /// Turkish Government Bonds
    Turkey,
    /// Saudi Arabian Government Bonds
    SaudiArabia,
    /// UAE Government Bonds
    UAE,
    /// Qatari Government Bonds
    Qatar,

    /// Supranational issuer
    Supranational(SupranationalIssuer),
}

impl Sovereign {
    /// Returns the primary currency for this sovereign's domestic bonds.
    #[must_use]
    pub fn currency(&self) -> Currency {
        match self {
            Self::UST => Currency::USD,
            Self::Canada => Currency::CAD,
            Self::Brazil => Currency::BRL,
            Self::Mexico => Currency::MXN,
            Self::Argentina | Self::Chile | Self::Colombia | Self::Peru => Currency::USD,

            Self::Germany
            | Self::France
            | Self::Italy
            | Self::Spain
            | Self::Netherlands
            | Self::Belgium
            | Self::Austria
            | Self::Finland
            | Self::Ireland
            | Self::Portugal
            | Self::Greece => Currency::EUR,

            Self::UK => Currency::GBP,
            Self::Switzerland => Currency::CHF,
            Self::Sweden => Currency::SEK,
            Self::Norway => Currency::NOK,
            Self::Denmark => Currency::DKK,
            Self::Poland | Self::CzechRepublic | Self::Hungary | Self::Romania => Currency::EUR,

            Self::Japan => Currency::JPY,
            Self::China => Currency::CNY,
            Self::Australia => Currency::AUD,
            Self::Singapore => Currency::SGD,
            Self::HongKong => Currency::HKD,
            Self::India => Currency::INR,
            Self::NewZealand => Currency::NZD,
            Self::SouthKorea
            | Self::Indonesia
            | Self::Malaysia
            | Self::Thailand
            | Self::Philippines => Currency::USD,

            Self::SouthAfrica => Currency::ZAR,
            Self::Israel | Self::Turkey | Self::SaudiArabia | Self::UAE | Self::Qatar => {
                Currency::USD
            }

            Self::Supranational(s) => s.primary_currency(),
        }
    }

    /// Returns the common name for this sovereign's bonds.
    #[must_use]
    pub fn bond_name(&self) -> &'static str {
        match self {
            Self::UST => "Treasury",
            Self::UK => "Gilt",
            Self::Germany => "Bund",
            Self::France => "OAT",
            Self::Italy => "BTP",
            Self::Spain => "Bono",
            Self::Japan => "JGB",
            Self::Australia => "ACGB",
            Self::Canada => "GoC",
            Self::Netherlands => "DSL",
            Self::Belgium => "OLO",
            Self::Austria => "RAGB",
            Self::Switzerland => "Eidgenosse",
            Self::Sweden => "SGB",
            Self::China => "CGB",
            Self::SouthKorea => "KTB",
            Self::Singapore => "SGS",
            Self::India => "G-Sec",
            Self::Brazil => "NTN",
            Self::Mexico => "Bono M",
            Self::NewZealand => "NZGB",
            Self::Supranational(s) => match s {
                SupranationalIssuer::EIB => "EIB",
                SupranationalIssuer::WorldBank => "IBRD",
                SupranationalIssuer::KfW => "KfW",
                _ => "Supranational",
            },
            _ => "Government Bond",
        }
    }

    /// Returns the Bloomberg ticker prefix for this sovereign.
    #[must_use]
    pub fn bloomberg_prefix(&self) -> &'static str {
        match self {
            Self::UST => "GT",
            Self::UK => "GUKG",
            Self::Germany => "GDBR",
            Self::France => "GFRN",
            Self::Italy => "GBTPGR",
            Self::Spain => "GSPG",
            Self::Japan => "GJGB",
            Self::Australia => "GACGB",
            Self::Canada => "GCAN",
            _ => "G",
        }
    }

    /// Returns the standard benchmark tenors for this sovereign.
    #[must_use]
    pub fn standard_tenors(&self) -> &'static [u32] {
        match self {
            Self::UST => &[24, 36, 60, 84, 120, 240, 360],
            Self::UK => &[24, 60, 120, 360, 600],
            Self::Germany => &[24, 60, 120, 360],
            Self::Japan => &[24, 60, 120, 240, 360, 480],
            _ => &[24, 60, 120, 360],
        }
    }

    /// Returns whether this sovereign typically issues in a different currency.
    #[must_use]
    pub fn has_foreign_currency_issuance(&self) -> bool {
        matches!(
            self,
            Self::Brazil
                | Self::Mexico
                | Self::Argentina
                | Self::Turkey
                | Self::Indonesia
                | Self::Philippines
                | Self::SouthAfrica
        )
    }

    /// Creates a US Treasury sovereign.
    #[must_use]
    pub fn us_treasury() -> Self {
        Self::UST
    }

    /// Creates a UK Gilt sovereign.
    #[must_use]
    pub fn uk_gilt() -> Self {
        Self::UK
    }

    /// Creates a German Bund sovereign.
    #[must_use]
    pub fn german_bund() -> Self {
        Self::Germany
    }

    /// Creates a French OAT sovereign.
    #[must_use]
    pub fn french_oat() -> Self {
        Self::France
    }

    /// Creates a Japanese JGB sovereign.
    #[must_use]
    pub fn japanese_jgb() -> Self {
        Self::Japan
    }
}

impl fmt::Display for Sovereign {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::UST => "US Treasury",
            Self::Canada => "Canada",
            Self::Brazil => "Brazil",
            Self::Mexico => "Mexico",
            Self::Argentina => "Argentina",
            Self::Chile => "Chile",
            Self::Colombia => "Colombia",
            Self::Peru => "Peru",
            Self::Germany => "Germany",
            Self::France => "France",
            Self::Italy => "Italy",
            Self::Spain => "Spain",
            Self::Netherlands => "Netherlands",
            Self::Belgium => "Belgium",
            Self::Austria => "Austria",
            Self::Finland => "Finland",
            Self::Ireland => "Ireland",
            Self::Portugal => "Portugal",
            Self::Greece => "Greece",
            Self::UK => "United Kingdom",
            Self::Switzerland => "Switzerland",
            Self::Sweden => "Sweden",
            Self::Norway => "Norway",
            Self::Denmark => "Denmark",
            Self::Poland => "Poland",
            Self::CzechRepublic => "Czech Republic",
            Self::Hungary => "Hungary",
            Self::Romania => "Romania",
            Self::Japan => "Japan",
            Self::China => "China",
            Self::Australia => "Australia",
            Self::SouthKorea => "South Korea",
            Self::Singapore => "Singapore",
            Self::HongKong => "Hong Kong",
            Self::India => "India",
            Self::Indonesia => "Indonesia",
            Self::Malaysia => "Malaysia",
            Self::Thailand => "Thailand",
            Self::Philippines => "Philippines",
            Self::NewZealand => "New Zealand",
            Self::SouthAfrica => "South Africa",
            Self::Israel => "Israel",
            Self::Turkey => "Turkey",
            Self::SaudiArabia => "Saudi Arabia",
            Self::UAE => "UAE",
            Self::Qatar => "Qatar",
            Self::Supranational(s) => return write!(f, "{s}"),
        };
        write!(f, "{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sovereign_currency() {
        assert_eq!(Sovereign::UST.currency(), Currency::USD);
        assert_eq!(Sovereign::UK.currency(), Currency::GBP);
        assert_eq!(Sovereign::Germany.currency(), Currency::EUR);
        assert_eq!(Sovereign::Japan.currency(), Currency::JPY);
        assert_eq!(Sovereign::France.currency(), Currency::EUR);
    }

    #[test]
    fn test_sovereign_bond_name() {
        assert_eq!(Sovereign::UST.bond_name(), "Treasury");
        assert_eq!(Sovereign::UK.bond_name(), "Gilt");
        assert_eq!(Sovereign::Germany.bond_name(), "Bund");
        assert_eq!(Sovereign::France.bond_name(), "OAT");
        assert_eq!(Sovereign::Japan.bond_name(), "JGB");
    }

    #[test]
    fn test_sovereign_display() {
        assert_eq!(format!("{}", Sovereign::UST), "US Treasury");
        assert_eq!(format!("{}", Sovereign::UK), "United Kingdom");
        assert_eq!(format!("{}", Sovereign::Germany), "Germany");
    }

    #[test]
    fn test_supranational() {
        let eib = Sovereign::Supranational(SupranationalIssuer::EIB);
        assert_eq!(eib.currency(), Currency::EUR);
        assert_eq!(eib.bond_name(), "EIB");
    }

    #[test]
    fn test_standard_tenors() {
        let ust_tenors = Sovereign::UST.standard_tenors();
        assert!(ust_tenors.contains(&24));
        assert!(ust_tenors.contains(&120));
        assert!(ust_tenors.contains(&360));
    }

    #[test]
    fn test_bloomberg_prefix() {
        assert_eq!(Sovereign::UST.bloomberg_prefix(), "GT");
        assert_eq!(Sovereign::UK.bloomberg_prefix(), "GUKG");
        assert_eq!(Sovereign::Germany.bloomberg_prefix(), "GDBR");
    }
}
