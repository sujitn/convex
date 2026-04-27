//! Trader mark — the input to bond pricing.
//!
//! A `Mark` is one of three things the trader can quote: a price (per 100,
//! clean or dirty), a yield (decimal, with compounding), or a spread (bps,
//! over a named benchmark). Pricing functions accept a `Mark` and return
//! the other two.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

use super::{Frequency, Spread};

/// Whether a price excludes (Clean) or includes (Dirty) accrued interest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum PriceKind {
    /// Clean price (excludes accrued).
    #[default]
    Clean,
    /// Dirty price (includes accrued).
    Dirty,
}

impl fmt::Display for PriceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PriceKind::Clean => write!(f, "Clean"),
            PriceKind::Dirty => write!(f, "Dirty"),
        }
    }
}

/// Trader's mark on a bond. Tagged with `mark` for JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "mark", rename_all = "snake_case")]
pub enum Mark {
    /// Price per 100 face.
    Price {
        /// Price per 100 face. rust_decimal lacks a JsonSchema impl, so the schema
        /// is `f64`; serde-float renders it as a JSON number.
        #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
        value: Decimal,
        /// Clean or dirty.
        kind: PriceKind,
    },
    /// Yield as decimal (0.05 = 5%) at the given compounding frequency.
    Yield {
        /// Yield as decimal.
        #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
        value: Decimal,
        /// Compounding frequency the yield is quoted on.
        frequency: Frequency,
    },
    /// Spread in basis points over a named benchmark curve.
    Spread {
        /// Spread value (bps + type).
        value: Spread,
        /// Benchmark curve identifier (e.g. `"USD.SOFR"`).
        benchmark: String,
    },
}

impl fmt::Display for Mark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mark::Price { value, kind } => write!(f, "{value} ({kind})"),
            Mark::Yield { value, frequency } => {
                write!(f, "{}% YTM ({frequency})", value * Decimal::ONE_HUNDRED)
            }
            Mark::Spread { value, benchmark } => write!(f, "{value} over {benchmark}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SpreadType;
    use rust_decimal_macros::dec;

    #[test]
    fn serde_roundtrip_price() {
        let m = Mark::Price {
            value: dec!(99.5),
            kind: PriceKind::Clean,
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"mark\":\"price\""));
        assert_eq!(m, serde_json::from_str(&json).unwrap());
    }

    #[test]
    fn serde_roundtrip_yield() {
        let m = Mark::Yield {
            value: dec!(0.0438),
            frequency: Frequency::SemiAnnual,
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"mark\":\"yield\""));
        assert_eq!(m, serde_json::from_str(&json).unwrap());
    }

    #[test]
    fn serde_roundtrip_spread() {
        let m = Mark::Spread {
            value: Spread::new(dec!(125), SpreadType::ZSpread),
            benchmark: "USD.TSY".into(),
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"mark\":\"spread\""));
        assert!(json.contains("USD.TSY"));
        assert_eq!(m, serde_json::from_str(&json).unwrap());
    }
}
