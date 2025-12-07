//! Currency type with ISO 4217 codes.

use serde::{Deserialize, Serialize};
use std::fmt;

/// ISO 4217 currency codes.
///
/// Represents currencies commonly used in fixed income markets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub enum Currency {
    /// United States Dollar
    #[default]
    USD,
    /// Euro
    EUR,
    /// British Pound Sterling
    GBP,
    /// Japanese Yen
    JPY,
    /// Swiss Franc
    CHF,
    /// Canadian Dollar
    CAD,
    /// Australian Dollar
    AUD,
    /// New Zealand Dollar
    NZD,
    /// Swedish Krona
    SEK,
    /// Norwegian Krone
    NOK,
    /// Danish Krone
    DKK,
    /// Hong Kong Dollar
    HKD,
    /// Singapore Dollar
    SGD,
    /// Chinese Yuan Renminbi
    CNY,
    /// Indian Rupee
    INR,
    /// Brazilian Real
    BRL,
    /// Mexican Peso
    MXN,
    /// South African Rand
    ZAR,
}

impl Currency {
    /// Returns the ISO 4217 3-letter code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Currency::USD => "USD",
            Currency::EUR => "EUR",
            Currency::GBP => "GBP",
            Currency::JPY => "JPY",
            Currency::CHF => "CHF",
            Currency::CAD => "CAD",
            Currency::AUD => "AUD",
            Currency::NZD => "NZD",
            Currency::SEK => "SEK",
            Currency::NOK => "NOK",
            Currency::DKK => "DKK",
            Currency::HKD => "HKD",
            Currency::SGD => "SGD",
            Currency::CNY => "CNY",
            Currency::INR => "INR",
            Currency::BRL => "BRL",
            Currency::MXN => "MXN",
            Currency::ZAR => "ZAR",
        }
    }

    /// Returns the currency symbol.
    #[must_use]
    pub fn symbol(&self) -> &'static str {
        match self {
            Currency::USD => "$",
            Currency::EUR => "€",
            Currency::GBP => "£",
            Currency::JPY => "¥",
            Currency::CHF => "CHF",
            Currency::CAD => "C$",
            Currency::AUD => "A$",
            Currency::NZD => "NZ$",
            Currency::SEK => "kr",
            Currency::NOK => "kr",
            Currency::DKK => "kr",
            Currency::HKD => "HK$",
            Currency::SGD => "S$",
            Currency::CNY => "¥",
            Currency::INR => "₹",
            Currency::BRL => "R$",
            Currency::MXN => "MX$",
            Currency::ZAR => "R",
        }
    }

    /// Returns the full currency name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Currency::USD => "United States Dollar",
            Currency::EUR => "Euro",
            Currency::GBP => "British Pound Sterling",
            Currency::JPY => "Japanese Yen",
            Currency::CHF => "Swiss Franc",
            Currency::CAD => "Canadian Dollar",
            Currency::AUD => "Australian Dollar",
            Currency::NZD => "New Zealand Dollar",
            Currency::SEK => "Swedish Krona",
            Currency::NOK => "Norwegian Krone",
            Currency::DKK => "Danish Krone",
            Currency::HKD => "Hong Kong Dollar",
            Currency::SGD => "Singapore Dollar",
            Currency::CNY => "Chinese Yuan Renminbi",
            Currency::INR => "Indian Rupee",
            Currency::BRL => "Brazilian Real",
            Currency::MXN => "Mexican Peso",
            Currency::ZAR => "South African Rand",
        }
    }

    /// Returns the ISO 4217 numeric code.
    #[must_use]
    pub fn numeric_code(&self) -> u16 {
        match self {
            Currency::USD => 840,
            Currency::EUR => 978,
            Currency::GBP => 826,
            Currency::JPY => 392,
            Currency::CHF => 756,
            Currency::CAD => 124,
            Currency::AUD => 36,
            Currency::NZD => 554,
            Currency::SEK => 752,
            Currency::NOK => 578,
            Currency::DKK => 208,
            Currency::HKD => 344,
            Currency::SGD => 702,
            Currency::CNY => 156,
            Currency::INR => 356,
            Currency::BRL => 986,
            Currency::MXN => 484,
            Currency::ZAR => 710,
        }
    }

    /// Returns true if this is a major reserve currency (G10).
    #[must_use]
    pub fn is_g10(&self) -> bool {
        matches!(
            self,
            Currency::USD
                | Currency::EUR
                | Currency::GBP
                | Currency::JPY
                | Currency::CHF
                | Currency::CAD
                | Currency::AUD
                | Currency::NZD
                | Currency::SEK
                | Currency::NOK
        )
    }

    /// Returns true if this is an emerging market currency.
    #[must_use]
    pub fn is_emerging(&self) -> bool {
        matches!(
            self,
            Currency::CNY | Currency::INR | Currency::BRL | Currency::MXN | Currency::ZAR
        )
    }

    /// Returns the standard number of decimal places for the currency.
    #[must_use]
    pub fn decimal_places(&self) -> u32 {
        match self {
            Currency::JPY => 0, // Yen has no decimal places
            _ => 2,
        }
    }

    /// Returns the typical settlement cycle for government bonds.
    #[must_use]
    pub fn standard_settlement_days(&self) -> u32 {
        match self {
            Currency::USD => 1, // US Treasuries T+1
            Currency::GBP => 1, // UK Gilts T+1
            Currency::EUR | Currency::CHF | Currency::CAD | Currency::AUD => 2,
            _ => 2,
        }
    }

    /// Parses a currency from a string code.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        match code.to_uppercase().as_str() {
            "USD" => Some(Currency::USD),
            "EUR" => Some(Currency::EUR),
            "GBP" => Some(Currency::GBP),
            "JPY" => Some(Currency::JPY),
            "CHF" => Some(Currency::CHF),
            "CAD" => Some(Currency::CAD),
            "AUD" => Some(Currency::AUD),
            "NZD" => Some(Currency::NZD),
            "SEK" => Some(Currency::SEK),
            "NOK" => Some(Currency::NOK),
            "DKK" => Some(Currency::DKK),
            "HKD" => Some(Currency::HKD),
            "SGD" => Some(Currency::SGD),
            "CNY" => Some(Currency::CNY),
            "INR" => Some(Currency::INR),
            "BRL" => Some(Currency::BRL),
            "MXN" => Some(Currency::MXN),
            "ZAR" => Some(Currency::ZAR),
            _ => None,
        }
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_currency_code() {
        assert_eq!(Currency::USD.code(), "USD");
        assert_eq!(Currency::EUR.code(), "EUR");
        assert_eq!(Currency::GBP.code(), "GBP");
        assert_eq!(Currency::JPY.code(), "JPY");
    }

    #[test]
    fn test_currency_symbol() {
        assert_eq!(Currency::USD.symbol(), "$");
        assert_eq!(Currency::EUR.symbol(), "€");
        assert_eq!(Currency::GBP.symbol(), "£");
        assert_eq!(Currency::JPY.symbol(), "¥");
    }

    #[test]
    fn test_currency_name() {
        assert_eq!(Currency::USD.name(), "United States Dollar");
        assert_eq!(Currency::EUR.name(), "Euro");
        assert_eq!(Currency::GBP.name(), "British Pound Sterling");
    }

    #[test]
    fn test_numeric_code() {
        assert_eq!(Currency::USD.numeric_code(), 840);
        assert_eq!(Currency::EUR.numeric_code(), 978);
        assert_eq!(Currency::GBP.numeric_code(), 826);
        assert_eq!(Currency::JPY.numeric_code(), 392);
    }

    #[test]
    fn test_from_code() {
        assert_eq!(Currency::from_code("usd"), Some(Currency::USD));
        assert_eq!(Currency::from_code("EUR"), Some(Currency::EUR));
        assert_eq!(Currency::from_code("gbp"), Some(Currency::GBP));
        assert_eq!(Currency::from_code("XXX"), None);
        assert_eq!(Currency::from_code(""), None);
    }

    #[test]
    fn test_decimal_places() {
        assert_eq!(Currency::USD.decimal_places(), 2);
        assert_eq!(Currency::EUR.decimal_places(), 2);
        assert_eq!(Currency::GBP.decimal_places(), 2);
        assert_eq!(Currency::JPY.decimal_places(), 0);
    }

    #[test]
    fn test_g10_currencies() {
        assert!(Currency::USD.is_g10());
        assert!(Currency::EUR.is_g10());
        assert!(Currency::GBP.is_g10());
        assert!(Currency::JPY.is_g10());
        assert!(Currency::CHF.is_g10());
        assert!(!Currency::CNY.is_g10());
        assert!(!Currency::BRL.is_g10());
    }

    #[test]
    fn test_emerging_currencies() {
        assert!(Currency::CNY.is_emerging());
        assert!(Currency::INR.is_emerging());
        assert!(Currency::BRL.is_emerging());
        assert!(Currency::MXN.is_emerging());
        assert!(Currency::ZAR.is_emerging());
        assert!(!Currency::USD.is_emerging());
        assert!(!Currency::EUR.is_emerging());
    }

    #[test]
    fn test_settlement_days() {
        assert_eq!(Currency::USD.standard_settlement_days(), 1); // T+1
        assert_eq!(Currency::GBP.standard_settlement_days(), 1);
        assert_eq!(Currency::EUR.standard_settlement_days(), 2);
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Currency::USD), "USD");
        assert_eq!(format!("{}", Currency::EUR), "EUR");
    }

    #[test]
    fn test_default() {
        assert_eq!(Currency::default(), Currency::USD);
    }

    #[test]
    fn test_serde() {
        let currency = Currency::EUR;
        let json = serde_json::to_string(&currency).unwrap();
        let parsed: Currency = serde_json::from_str(&json).unwrap();
        assert_eq!(currency, parsed);
    }

    #[test]
    fn test_clone_and_copy() {
        let c1 = Currency::USD;
        let c2 = c1; // Copy
        let c3 = c1; // Copy (Clone not needed for Copy types)
        assert_eq!(c1, c2);
        assert_eq!(c1, c3);
    }

    #[test]
    fn test_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Currency::USD);
        set.insert(Currency::EUR);
        set.insert(Currency::USD); // Duplicate

        assert_eq!(set.len(), 2);
        assert!(set.contains(&Currency::USD));
        assert!(set.contains(&Currency::EUR));
    }
}
