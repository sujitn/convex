//! String/numeric parsing and formatting helpers shared across the WASM surface.

use std::str::FromStr;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use wasm_bindgen::prelude::*;

use convex_bonds::conventions::{InstrumentType, Market};
use convex_bonds::types::{CompoundingMethod, YieldConvention};
use convex_core::contracts::ExactDecimal;
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

    let date = Date::from_ymd(year, month, day).map_err(|e| format!("Invalid date {s}: {e:?}"))?;
    if date.to_string() != s {
        return Err("Date must use exact YYYY-MM-DD format".into());
    }
    Ok(date)
}

pub(crate) fn date_to_naive(date: Date) -> chrono::NaiveDate {
    date.into()
}

pub(crate) fn parse_day_count(s: &str) -> Result<DayCountConvention, String> {
    DayCountConvention::from_str(s).map_err(|_| format!("Unknown day count: {s}"))
}

pub(crate) fn parse_frequency(f: u32) -> Result<Frequency, String> {
    match f {
        1 => Ok(Frequency::Annual),
        2 => Ok(Frequency::SemiAnnual),
        4 => Ok(Frequency::Quarterly),
        12 => Ok(Frequency::Monthly),
        _ => Err(format!("Unsupported fixed coupon frequency: {f}")),
    }
}

pub(crate) fn parse_currency(s: &str) -> Result<Currency, String> {
    match s.to_uppercase().as_str() {
        "USD" => Ok(Currency::USD),
        "EUR" => Ok(Currency::EUR),
        "GBP" => Ok(Currency::GBP),
        "JPY" => Ok(Currency::JPY),
        "CHF" => Ok(Currency::CHF),
        "AUD" => Ok(Currency::AUD),
        "CAD" => Ok(Currency::CAD),
        "NZD" => Ok(Currency::NZD),
        _ => Err(format!("Unsupported currency: {s}")),
    }
}

pub(crate) fn decimal_to_f64(d: Decimal) -> Result<f64, String> {
    d.to_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| "Decimal cannot enter the finite numeric model".into())
}

/// Preserve the finite JSNumber's shortest decimal representation without rounding.
/// Exact string economics use the complete supplied-term library instead.
pub(crate) fn f64_to_decimal(f: f64) -> Result<Decimal, String> {
    if !f.is_finite() {
        return Err("Finite numeric input required".into());
    }
    ExactDecimal::parse(&f.to_string())
        .map(|n| n.decimal())
        .map_err(|e| e.to_string())
}

/// Shift the exact input decimal coefficient by two places, rejecting scale loss.
pub(crate) fn percentage_to_decimal(f: f64) -> Result<Decimal, String> {
    let value = f64_to_decimal(f)?;
    let scale = value
        .scale()
        .checked_add(2)
        .filter(|n| *n <= 28)
        .ok_or("Percentage exceeds exact decimal scale")?;
    Decimal::try_from_i128_with_scale(value.mantissa(), scale).map_err(|e| e.to_string())
}

pub(crate) fn finite(value: f64, name: &str) -> Result<f64, String> {
    if !value.is_finite() {
        return Err(format!("Nonfinite {name}"));
    }
    Ok(value)
}

pub(crate) fn positive(value: f64, name: &str) -> Result<f64, String> {
    finite(value, name)?;
    if value <= 0.0 {
        return Err(format!("Positive {name} required"));
    }
    Ok(value)
}

pub(crate) fn parse_market(s: &str) -> Result<Market, String> {
    Ok(match s.to_uppercase().as_str() {
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
        _ => return Err(format!("Unknown market: {s}")),
    })
}

pub(crate) fn parse_instrument_type(s: &str) -> Result<InstrumentType, String> {
    Ok(
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
            _ => return Err(format!("Unknown instrument_type: {s}")),
        },
    )
}

pub(crate) fn parse_yield_convention(s: &str) -> Result<YieldConvention, String> {
    Ok(
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
            _ => return Err(format!("Unknown yield_convention: {s}")),
        },
    )
}

pub(crate) fn parse_compounding(s: &str) -> Result<CompoundingMethod, String> {
    Ok(
        match s.to_uppercase().replace(['_', '-', ' '], "").as_str() {
            "SEMIANNUAL" | "SEMI" | "2" => CompoundingMethod::Periodic { frequency: 2 },
            "ANNUAL" | "1" => CompoundingMethod::Periodic { frequency: 1 },
            "QUARTERLY" | "4" => CompoundingMethod::Periodic { frequency: 4 },
            "MONTHLY" | "12" => CompoundingMethod::Periodic { frequency: 12 },
            "CONTINUOUS" | "CONT" => CompoundingMethod::Continuous,
            "SIMPLE" | "NONE" => CompoundingMethod::Simple,
            "DISCOUNT" => CompoundingMethod::Discount,
            _ => return Err(format!("Unknown compounding: {s}")),
        },
    )
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
            parse_day_count("30/360").unwrap(),
            DayCountConvention::Thirty360US
        ));
        assert!(matches!(
            parse_day_count("30/360 US").unwrap(),
            DayCountConvention::Thirty360US
        ));
        // EU 30E/360
        assert!(matches!(
            parse_day_count("30E/360").unwrap(),
            DayCountConvention::Thirty360E
        ));
        assert!(matches!(
            parse_day_count("30/360 EU").unwrap(),
            DayCountConvention::Thirty360E
        ));
        assert!(matches!(
            parse_day_count("30/360E").unwrap(),
            DayCountConvention::Thirty360E
        ));
        // Other conventions
        assert!(matches!(
            parse_day_count("ACT/365").unwrap(),
            DayCountConvention::Act365Fixed
        ));
        // Bare "ACT/ACT" maps to the ISDA interpretation (the canonical default).
        // Callers that want ICMA must spell it out as "ACT/ACT ICMA".
        assert!(matches!(
            parse_day_count("act/act").unwrap(),
            DayCountConvention::ActActIsda
        ));
    }

    #[test]
    fn test_parse_frequency() {
        assert!(matches!(parse_frequency(1).unwrap(), Frequency::Annual));
        assert!(matches!(parse_frequency(2).unwrap(), Frequency::SemiAnnual));
        assert!(matches!(parse_frequency(4).unwrap(), Frequency::Quarterly));
    }
}
