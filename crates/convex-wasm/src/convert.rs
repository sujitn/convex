//! String/numeric parsing and formatting helpers shared across the WASM surface.

use std::str::FromStr;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use wasm_bindgen::prelude::*;

use convex_bonds::conventions::{InstrumentType, Market};
use convex_bonds::types::{CompoundingMethod, YieldConvention};
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    pub(crate) fn log(s: &str);
}

pub(crate) fn parse_date(s: &str) -> Result<Date, String> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return Err(format!("Invalid date format: {}. Expected YYYY-MM-DD", s));
    }

    let year: i32 = parts[0]
        .parse()
        .map_err(|_| format!("Invalid year: {}", parts[0]))?;
    let month: u32 = parts[1]
        .parse()
        .map_err(|_| format!("Invalid month: {}", parts[1]))?;
    let day: u32 = parts[2]
        .parse()
        .map_err(|_| format!("Invalid day: {}", parts[2]))?;

    Date::from_ymd(year, month, day).map_err(|e| format!("Invalid date {}: {:?}", s, e))
}

pub(crate) fn date_to_naive(date: Date) -> chrono::NaiveDate {
    date.into()
}

pub(crate) fn parse_day_count(s: &str) -> DayCountConvention {
    // Delegate to the canonical parser; fall back to the 30/360 US that this
    // endpoint defaulted to before, to preserve the "unknown input doesn't
    // blow up the demo" guarantee for browser callers.
    DayCountConvention::from_str(s).unwrap_or(DayCountConvention::Thirty360US)
}

pub(crate) fn parse_frequency(f: u32) -> Frequency {
    match f {
        1 => Frequency::Annual,
        2 => Frequency::SemiAnnual,
        4 => Frequency::Quarterly,
        12 => Frequency::Monthly,
        _ => Frequency::SemiAnnual,
    }
}

pub(crate) fn parse_currency(s: &str) -> Currency {
    match s.to_uppercase().as_str() {
        "USD" => Currency::USD,
        "EUR" => Currency::EUR,
        "GBP" => Currency::GBP,
        "JPY" => Currency::JPY,
        "CHF" => Currency::CHF,
        "AUD" => Currency::AUD,
        "CAD" => Currency::CAD,
        "NZD" => Currency::NZD,
        _ => Currency::USD,
    }
}

pub(crate) fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

/// Convert an f64 to Decimal, rounding to 10 d.p. so values like
/// 0.05000000000000001 don't leak through.
pub(crate) fn f64_to_decimal(f: f64) -> Decimal {
    Decimal::from_f64_retain(f)
        .map(|d| d.round_dp(10))
        .unwrap_or(Decimal::ZERO)
}

pub(crate) fn parse_market(s: &str) -> Market {
    match s.to_uppercase().as_str() {
        "US" | "USA" | "UNITED STATES" => Market::US,
        "UK" | "GB" | "UNITED KINGDOM" => Market::UK,
        "GERMANY" | "DE" | "GER" => Market::Germany,
        "FRANCE" | "FR" | "FRA" => Market::France,
        "ITALY" | "IT" | "ITA" => Market::Italy,
        "SPAIN" | "ES" | "ESP" => Market::Spain,
        "JAPAN" | "JP" | "JPN" => Market::Japan,
        "SWITZERLAND" | "CH" | "CHE" => Market::Switzerland,
        "AUSTRALIA" | "AU" | "AUS" => Market::Australia,
        "CANADA" | "CA" | "CAN" => Market::Canada,
        "NETHERLANDS" | "NL" | "NLD" => Market::Netherlands,
        "BELGIUM" | "BE" | "BEL" => Market::Belgium,
        "AUSTRIA" | "AT" | "AUT" => Market::Austria,
        "PORTUGAL" | "PT" | "PRT" => Market::Portugal,
        "IRELAND" | "IE" | "IRL" => Market::Ireland,
        "SWEDEN" | "SE" | "SWE" => Market::Sweden,
        "NORWAY" | "NO" | "NOR" => Market::Norway,
        "DENMARK" | "DK" | "DNK" => Market::Denmark,
        "FINLAND" | "FI" | "FIN" => Market::Finland,
        "EUROZONE" | "EUR" | "EU" => Market::Eurozone,
        _ => Market::US,
    }
}

pub(crate) fn parse_instrument_type(s: &str) -> InstrumentType {
    match s.to_uppercase().replace(['_', '-', ' '], "").as_str() {
        "GOVERNMENTBOND" | "GOVT" | "SOVEREIGN" | "GOV" => InstrumentType::GovernmentBond,
        "TREASURYBILL" | "TBILL" | "BILL" => InstrumentType::TreasuryBill,
        "CORPORATEIG" | "IG" | "INVESTMENTGRADE" | "CORPORATE" => InstrumentType::CorporateIG,
        "CORPORATEHY" | "HY" | "HIGHYIELD" | "JUNK" => InstrumentType::CorporateHY,
        "MUNICIPAL" | "MUNI" => InstrumentType::Municipal,
        "AGENCY" | "GSE" => InstrumentType::Agency,
        "INFLATIONLINKED" | "TIPS" | "LINKER" | "ILB" => InstrumentType::InflationLinked,
        "GOVERNMENTFRN" | "GOVFRN" => InstrumentType::GovernmentFRN,
        "CORPORATEFRN" | "CORPFRN" | "FRN" | "FLOATER" => InstrumentType::CorporateFRN,
        "COVERED" | "COVEREDBOND" | "PFANDBRIEF" => InstrumentType::CoveredBond,
        "ABS" | "ASSETBACKED" => InstrumentType::ABS,
        "MBS" | "MORTGAGEBACKED" => InstrumentType::MBS,
        "SUPRANATIONAL" | "SUPRA" => InstrumentType::Supranational,
        "COMMERCIALPAPER" | "CP" => InstrumentType::CommercialPaper,
        "CONVERTIBLE" | "CONV" => InstrumentType::Convertible,
        "STRIPS" | "STRIP" => InstrumentType::Strips,
        _ => InstrumentType::GovernmentBond,
    }
}

pub(crate) fn parse_yield_convention(s: &str) -> YieldConvention {
    match s.to_uppercase().replace(['_', '-', ' '], "").as_str() {
        "STREET" | "STREETCONVENTION" | "US" => YieldConvention::StreetConvention,
        "TRUE" | "TRUEYIELD" => YieldConvention::TrueYield,
        "ISMA" | "ICMA" => YieldConvention::ISMA,
        "SIMPLE" | "SIMPLEYIELD" | "JAPANESE" | "JGB" => YieldConvention::SimpleYield,
        "DISCOUNT" | "DISCOUNTYIELD" => YieldConvention::DiscountYield,
        "BONDEQUIVALENT" | "BEY" => YieldConvention::BondEquivalentYield,
        "MUNICIPAL" | "MUNI" | "TAXEQUIVALENT" => YieldConvention::MunicipalYield,
        "MOOSMULLER" | "GERMAN" => YieldConvention::Moosmuller,
        "BRAESSFANGMEYER" | "BRAESS" => YieldConvention::BraessFangmeyer,
        "ANNUAL" => YieldConvention::Annual,
        "CONTINUOUS" | "CONT" => YieldConvention::Continuous,
        _ => YieldConvention::StreetConvention,
    }
}

pub(crate) fn parse_compounding(s: &str) -> CompoundingMethod {
    match s.to_uppercase().replace(['_', '-', ' '], "").as_str() {
        "SEMIANNUAL" | "SEMI" | "2" => CompoundingMethod::Periodic { frequency: 2 },
        "ANNUAL" | "1" => CompoundingMethod::Periodic { frequency: 1 },
        "QUARTERLY" | "4" => CompoundingMethod::Periodic { frequency: 4 },
        "MONTHLY" | "12" => CompoundingMethod::Periodic { frequency: 12 },
        "CONTINUOUS" | "CONT" => CompoundingMethod::Continuous,
        "SIMPLE" | "NONE" => CompoundingMethod::Simple,
        "DISCOUNT" => CompoundingMethod::Discount,
        _ => CompoundingMethod::Periodic { frequency: 2 },
    }
}

/// Parse a tenor string like "5Y", "10Y", "6M", "3M" to years.
pub(crate) fn parse_tenor_to_years(tenor: &str) -> f64 {
    let tenor = tenor.trim().to_uppercase();
    if tenor.ends_with('Y') {
        tenor[..tenor.len() - 1].parse::<f64>().unwrap_or(10.0)
    } else if tenor.ends_with('M') {
        tenor[..tenor.len() - 1]
            .parse::<f64>()
            .map(|m| m / 12.0)
            .unwrap_or(1.0)
    } else {
        tenor.parse::<f64>().unwrap_or(10.0)
    }
}

pub(crate) fn format_market_name(market: Market) -> String {
    format!("{:?}", market)
}

pub(crate) fn format_instrument_type(inst: InstrumentType) -> String {
    match inst {
        InstrumentType::GovernmentBond => "Government Bond".to_string(),
        InstrumentType::TreasuryBill => "Treasury Bill".to_string(),
        InstrumentType::GovernmentFRN => "Government FRN".to_string(),
        InstrumentType::InflationLinked => "Inflation Linked".to_string(),
        InstrumentType::Strips => "STRIPS".to_string(),
        InstrumentType::CorporateIG => "Corporate IG".to_string(),
        InstrumentType::CorporateHY => "Corporate HY".to_string(),
        InstrumentType::CorporateFRN => "Corporate FRN".to_string(),
        InstrumentType::Convertible => "Convertible".to_string(),
        InstrumentType::CommercialPaper => "Commercial Paper".to_string(),
        InstrumentType::Municipal => "Municipal".to_string(),
        InstrumentType::Agency => "Agency".to_string(),
        InstrumentType::Supranational => "Supranational".to_string(),
        InstrumentType::CoveredBond => "Covered Bond".to_string(),
        InstrumentType::ABS => "ABS".to_string(),
        InstrumentType::MBS => "MBS".to_string(),
    }
}

pub(crate) fn format_yield_convention(conv: YieldConvention) -> String {
    match conv {
        YieldConvention::StreetConvention => "Street Convention".to_string(),
        YieldConvention::TrueYield => "True Yield".to_string(),
        YieldConvention::ISMA => "ISMA/ICMA".to_string(),
        YieldConvention::SimpleYield => "Simple Yield".to_string(),
        YieldConvention::DiscountYield => "Discount Yield".to_string(),
        YieldConvention::BondEquivalentYield => "Bond Equivalent".to_string(),
        YieldConvention::MunicipalYield => "Municipal (Tax-Equiv)".to_string(),
        YieldConvention::Moosmuller => "Moosmuller".to_string(),
        YieldConvention::BraessFangmeyer => "Braess-Fangmeyer".to_string(),
        YieldConvention::Annual => "Annual".to_string(),
        YieldConvention::Continuous => "Continuous".to_string(),
    }
}

pub(crate) fn format_compounding(method: CompoundingMethod) -> String {
    match method {
        CompoundingMethod::Periodic { frequency: 1 } => "Annual".to_string(),
        CompoundingMethod::Periodic { frequency: 2 } => "Semi-Annual".to_string(),
        CompoundingMethod::Periodic { frequency: 4 } => "Quarterly".to_string(),
        CompoundingMethod::Periodic { frequency: 12 } => "Monthly".to_string(),
        CompoundingMethod::Periodic { frequency: n } => format!("{n}x/year"),
        CompoundingMethod::Continuous => "Continuous".to_string(),
        CompoundingMethod::Simple => "Simple".to_string(),
        CompoundingMethod::Discount => "Discount".to_string(),
        CompoundingMethod::ActualPeriod { .. } => "Actual Period".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_date() {
        let date = parse_date("2024-06-15").unwrap();
        assert_eq!(date, Date::from_ymd(2024, 6, 15).unwrap());
    }

    #[test]
    fn test_parse_date_invalid() {
        assert!(parse_date("invalid").is_err());
        assert!(parse_date("2024/06/15").is_err());
    }

    #[test]
    fn test_parse_day_count() {
        // US 30/360
        assert!(matches!(
            parse_day_count("30/360"),
            DayCountConvention::Thirty360US
        ));
        assert!(matches!(
            parse_day_count("30/360 US"),
            DayCountConvention::Thirty360US
        ));
        // EU 30E/360
        assert!(matches!(
            parse_day_count("30E/360"),
            DayCountConvention::Thirty360E
        ));
        assert!(matches!(
            parse_day_count("30/360 EU"),
            DayCountConvention::Thirty360E
        ));
        assert!(matches!(
            parse_day_count("30/360E"),
            DayCountConvention::Thirty360E
        ));
        // Other conventions
        assert!(matches!(
            parse_day_count("ACT/365"),
            DayCountConvention::Act365Fixed
        ));
        // Bare "ACT/ACT" maps to the ISDA interpretation (the canonical default).
        // Callers that want ICMA must spell it out as "ACT/ACT ICMA".
        assert!(matches!(
            parse_day_count("act/act"),
            DayCountConvention::ActActIsda
        ));
    }

    #[test]
    fn test_parse_frequency() {
        assert!(matches!(parse_frequency(1), Frequency::Annual));
        assert!(matches!(parse_frequency(2), Frequency::SemiAnnual));
        assert!(matches!(parse_frequency(4), Frequency::Quarterly));
    }
}
