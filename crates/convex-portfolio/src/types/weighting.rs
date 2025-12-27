//! Portfolio weighting methods.

use serde::{Deserialize, Serialize};

/// Portfolio weighting method for aggregations.
///
/// Determines how individual holding metrics are weighted when calculating
/// portfolio-level averages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
pub enum WeightingMethod {
    /// Weight by market value (most common for risk metrics)
    #[default]
    MarketValue,

    /// Weight by par/face value
    ParValue,

    /// Equal weight across all holdings
    EqualWeight,
}

impl WeightingMethod {
    /// Returns a human-readable name for the weighting method.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::MarketValue => "Market Value",
            Self::ParValue => "Par Value",
            Self::EqualWeight => "Equal Weight",
        }
    }

    /// Returns a short code for the weighting method.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::MarketValue => "MV",
            Self::ParValue => "PAR",
            Self::EqualWeight => "EQ",
        }
    }
}

impl std::fmt::Display for WeightingMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        assert_eq!(WeightingMethod::default(), WeightingMethod::MarketValue);
    }

    #[test]
    fn test_name_and_code() {
        assert_eq!(WeightingMethod::MarketValue.name(), "Market Value");
        assert_eq!(WeightingMethod::MarketValue.code(), "MV");
        assert_eq!(WeightingMethod::ParValue.name(), "Par Value");
        assert_eq!(WeightingMethod::ParValue.code(), "PAR");
        assert_eq!(WeightingMethod::EqualWeight.name(), "Equal Weight");
        assert_eq!(WeightingMethod::EqualWeight.code(), "EQ");
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", WeightingMethod::MarketValue), "Market Value");
    }

    #[test]
    fn test_serde() {
        let method = WeightingMethod::ParValue;
        let json = serde_json::to_string(&method).unwrap();
        let parsed: WeightingMethod = serde_json::from_str(&json).unwrap();
        assert_eq!(method, parsed);
    }
}
