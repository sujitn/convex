//! Trader mark — the input to bond pricing.
//!
//! A `Mark` is one of three things the trader can quote: a price (per 100,
//! clean or dirty), a yield (decimal, with compounding), or a spread (bps,
//! over a named benchmark). Pricing functions accept a `Mark` and return
//! the other two.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use super::{Frequency, Spread, SpreadType};

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

/// Error parsing a mark from text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkParseError(pub String);

impl fmt::Display for MarkParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mark parse error: {}", self.0)
    }
}

impl std::error::Error for MarkParseError {}

impl FromStr for Mark {
    type Err = MarkParseError;

    /// Parse a trader-style mark string.
    ///
    /// Grammar (case-insensitive, whitespace-tolerant):
    ///
    /// ```text
    ///   PRICE  := <number>[ ('C'|'CLEAN'|'D'|'DIRTY') ]      e.g. "99.5", "99.5C", "99.5D"
    ///          | <int>'-'<int>['+']                          32nds form: "99-16", "99-16+"
    ///   YIELD  := <number>'%'[ '@' <FREQ> ]                  e.g. "4.65%", "4.65%@SA"
    ///   SPREAD := ['+'|'-']<number>[ 'BPS' ][ ' ' <SPREADTYPE> ] '@' <BENCH>
    ///                                                        e.g. "+125bps@USD.SOFR",
    ///                                                        "125 OAS@USD.TSY"
    ///   FREQ        := A | SA | Q | M | CONT | ANNUAL | SEMI | SEMI_ANNUAL | QUARTERLY | MONTHLY
    ///   SPREADTYPE  := Z | G | I | OAS | DM | ASW | ASW_PROC | CREDIT  (default Z)
    ///   BENCH       := any non-whitespace identifier (e.g. USD.SOFR, USD.TSY.10Y)
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let raw = s.trim();
        if raw.is_empty() {
            return Err(MarkParseError("empty input".into()));
        }

        // Spread (must look at '@')
        if let Some(at_pos) = raw.rfind('@') {
            // Heuristic: if '@' appears AFTER a '%', it's a yield with frequency suffix.
            let percent_pos = raw.find('%');
            if percent_pos.map_or(true, |p| at_pos < p) {
                return parse_spread(raw, at_pos);
            }
        }

        // Yield: contains '%'
        if raw.contains('%') {
            return parse_yield(raw);
        }

        // 32nds: "99-16" or "99-16+"
        if let Some(dash) = raw.find('-') {
            // distinguish a leading negative sign from a 32nds dash
            if dash > 0 && raw.as_bytes()[dash - 1].is_ascii_digit() {
                return parse_32nds(raw, dash);
            }
        }

        // Decimal price (with optional clean/dirty suffix)
        parse_price(raw)
    }
}

fn parse_price(raw: &str) -> Result<Mark, MarkParseError> {
    let upper = raw.to_ascii_uppercase();
    let (num_part, kind) = if let Some(stripped) = upper.strip_suffix("CLEAN") {
        (stripped.trim(), PriceKind::Clean)
    } else if let Some(stripped) = upper.strip_suffix("DIRTY") {
        (stripped.trim(), PriceKind::Dirty)
    } else if let Some(stripped) = upper.strip_suffix('C') {
        (stripped.trim(), PriceKind::Clean)
    } else if let Some(stripped) = upper.strip_suffix('D') {
        (stripped.trim(), PriceKind::Dirty)
    } else {
        (upper.as_str(), PriceKind::Clean)
    };
    let value = Decimal::from_str(num_part.trim())
        .map_err(|_| MarkParseError(format!("invalid price number {raw:?}")))?;
    Ok(Mark::Price { value, kind })
}

fn parse_32nds(raw: &str, dash: usize) -> Result<Mark, MarkParseError> {
    let (head, rest) = raw.split_at(dash);
    let rest = &rest[1..]; // skip '-'
    let whole: i64 = head
        .trim()
        .parse()
        .map_err(|_| MarkParseError(format!("invalid 32nds whole part in {raw:?}")))?;
    let (frac_str, plus) = match rest.strip_suffix('+') {
        Some(s) => (s, true),
        None => (rest, false),
    };
    let frac: i64 = frac_str
        .trim()
        .parse()
        .map_err(|_| MarkParseError(format!("invalid 32nds fraction in {raw:?}")))?;
    if !(0..32).contains(&frac) {
        return Err(MarkParseError(format!(
            "32nds fraction out of range in {raw:?}"
        )));
    }
    let mut value = Decimal::from(whole) + Decimal::from(frac) / Decimal::from(32);
    if plus {
        value += Decimal::ONE / Decimal::from(64);
    }
    Ok(Mark::Price {
        value,
        kind: PriceKind::Clean,
    })
}

fn parse_yield(raw: &str) -> Result<Mark, MarkParseError> {
    let percent = raw
        .find('%')
        .ok_or_else(|| MarkParseError("yield missing %".into()))?;
    let num_part = raw[..percent].trim();
    let after = raw[percent + 1..].trim();

    let value_pct = Decimal::from_str(num_part)
        .map_err(|_| MarkParseError(format!("invalid yield number {raw:?}")))?;
    let value = value_pct / Decimal::ONE_HUNDRED;

    let frequency = if after.is_empty() {
        Frequency::SemiAnnual
    } else {
        let token = after.trim_start_matches('@').trim();
        parse_frequency(token)?
    };

    Ok(Mark::Yield { value, frequency })
}

fn parse_frequency(token: &str) -> Result<Frequency, MarkParseError> {
    match token.to_ascii_uppercase().as_str() {
        "A" | "ANN" | "ANNUAL" => Ok(Frequency::Annual),
        "SA" | "SEMI" | "SEMIANNUAL" | "SEMI_ANNUAL" | "SEMI-ANNUAL" => Ok(Frequency::SemiAnnual),
        "Q" | "QTR" | "QUARTERLY" => Ok(Frequency::Quarterly),
        "M" | "MO" | "MONTHLY" => Ok(Frequency::Monthly),
        "Z" | "ZERO" => Ok(Frequency::Zero),
        other => Err(MarkParseError(format!("unknown frequency {other:?}"))),
    }
}

fn parse_spread_type(token: &str) -> Result<SpreadType, MarkParseError> {
    match token.to_ascii_uppercase().as_str() {
        "Z" | "ZSPREAD" | "Z-SPREAD" => Ok(SpreadType::ZSpread),
        "G" | "GSPREAD" | "G-SPREAD" => Ok(SpreadType::GSpread),
        "I" | "ISPREAD" | "I-SPREAD" => Ok(SpreadType::ISpread),
        "OAS" => Ok(SpreadType::OAS),
        "DM" | "DISCOUNT_MARGIN" | "DISCOUNTMARGIN" => Ok(SpreadType::DiscountMargin),
        "ASW" | "ASW_PAR" | "ASWPAR" => Ok(SpreadType::AssetSwapPar),
        "ASW_PROC" | "ASW_PROCEEDS" | "ASWPROCEEDS" => Ok(SpreadType::AssetSwapProceeds),
        "CREDIT" => Ok(SpreadType::Credit),
        other => Err(MarkParseError(format!("unknown spread type {other:?}"))),
    }
}

fn parse_spread(raw: &str, at_pos: usize) -> Result<Mark, MarkParseError> {
    let (head, tail) = raw.split_at(at_pos);
    let benchmark = tail[1..].trim().to_string();
    if benchmark.is_empty() {
        return Err(MarkParseError("missing benchmark after '@'".into()));
    }

    // head = "+125bps" or "125 OAS" or "-50 G" etc.
    let head = head.trim();
    let upper = head.to_ascii_uppercase();

    // Split off optional spread-type token: last whitespace separates value from type.
    let (num_token, type_token) = if let Some(idx) = upper.rfind(char::is_whitespace) {
        let (a, b) = upper.split_at(idx);
        (a.trim().to_string(), Some(b.trim().to_string()))
    } else {
        (upper.clone(), None)
    };

    // Strip optional 'BPS' suffix from the number side.
    let num_clean = num_token
        .trim_end_matches("BPS")
        .trim_end_matches("BP")
        .trim();
    let bps = Decimal::from_str(num_clean)
        .map_err(|_| MarkParseError(format!("invalid spread number {raw:?}")))?;

    let spread_type = match type_token.as_deref() {
        None | Some("") => SpreadType::ZSpread,
        Some(t) => parse_spread_type(t)?,
    };

    Ok(Mark::Spread {
        value: Spread::new(bps, spread_type),
        benchmark,
    })
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

    fn parse(s: &str) -> Mark {
        s.parse().unwrap_or_else(|e| panic!("{s:?}: {e}"))
    }

    #[test]
    fn parse_decimal_price_default_clean() {
        assert_eq!(
            parse("99.5"),
            Mark::Price {
                value: dec!(99.5),
                kind: PriceKind::Clean
            }
        );
    }

    #[test]
    fn parse_clean_dirty_suffix() {
        assert_eq!(
            parse("99.5C"),
            Mark::Price {
                value: dec!(99.5),
                kind: PriceKind::Clean
            }
        );
        assert_eq!(
            parse("99.5 dirty"),
            Mark::Price {
                value: dec!(99.5),
                kind: PriceKind::Dirty
            }
        );
        assert_eq!(
            parse("101.25 D"),
            Mark::Price {
                value: dec!(101.25),
                kind: PriceKind::Dirty
            }
        );
    }

    #[test]
    fn parse_32nds() {
        // 99-16 = 99 + 16/32 = 99.5
        assert_eq!(
            parse("99-16"),
            Mark::Price {
                value: dec!(99.5),
                kind: PriceKind::Clean
            }
        );
        // 99-16+ = 99 + 16/32 + 1/64 = 99.515625
        assert_eq!(
            parse("99-16+"),
            Mark::Price {
                value: dec!(99.515625),
                kind: PriceKind::Clean
            }
        );
    }

    #[test]
    fn parse_yield_default_semi() {
        assert_eq!(
            parse("4.65%"),
            Mark::Yield {
                value: dec!(0.0465),
                frequency: Frequency::SemiAnnual
            }
        );
    }

    #[test]
    fn parse_yield_with_frequency() {
        assert_eq!(
            parse("4.65%@A"),
            Mark::Yield {
                value: dec!(0.0465),
                frequency: Frequency::Annual
            }
        );
        assert_eq!(
            parse("4.65% @ Q"),
            Mark::Yield {
                value: dec!(0.0465),
                frequency: Frequency::Quarterly
            }
        );
    }

    #[test]
    fn parse_spread_basic() {
        assert_eq!(
            parse("+125bps@USD.SOFR"),
            Mark::Spread {
                value: Spread::new(dec!(125), SpreadType::ZSpread),
                benchmark: "USD.SOFR".into()
            }
        );
    }

    #[test]
    fn parse_spread_with_type() {
        assert_eq!(
            parse("125 OAS@USD.TSY"),
            Mark::Spread {
                value: Spread::new(dec!(125), SpreadType::OAS),
                benchmark: "USD.TSY".into()
            }
        );
        assert_eq!(
            parse("-50 G@USD.TSY.10Y"),
            Mark::Spread {
                value: Spread::new(dec!(-50), SpreadType::GSpread),
                benchmark: "USD.TSY.10Y".into()
            }
        );
    }

    #[test]
    fn parse_invalid() {
        assert!("".parse::<Mark>().is_err());
        assert!("abc".parse::<Mark>().is_err());
        assert!("99.5X".parse::<Mark>().is_err());
        assert!("125bps@".parse::<Mark>().is_err());
    }
}
