//! Common DTO types.

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::{ApiError, ApiResult};

/// Date input for API requests.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DateInput {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl DateInput {
    /// Convert to internal Date type.
    pub fn to_date(&self) -> ApiResult<Date> {
        Date::from_ymd(self.year, self.month, self.day)
            .map_err(|e| ApiError::Validation(format!("Invalid date: {}", e)))
    }
}

impl From<Date> for DateInput {
    fn from(date: Date) -> Self {
        Self {
            year: date.year(),
            month: date.month(),
            day: date.day(),
        }
    }
}

/// Currency codes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum CurrencyCode {
    #[default]
    Usd,
    Eur,
    Gbp,
    Jpy,
    Chf,
    Cad,
    Aud,
}

impl From<CurrencyCode> for Currency {
    fn from(code: CurrencyCode) -> Self {
        match code {
            CurrencyCode::Usd => Currency::USD,
            CurrencyCode::Eur => Currency::EUR,
            CurrencyCode::Gbp => Currency::GBP,
            CurrencyCode::Jpy => Currency::JPY,
            CurrencyCode::Chf => Currency::CHF,
            CurrencyCode::Cad => Currency::CAD,
            CurrencyCode::Aud => Currency::AUD,
        }
    }
}

impl From<Currency> for CurrencyCode {
    fn from(currency: Currency) -> Self {
        match currency {
            Currency::USD => CurrencyCode::Usd,
            Currency::EUR => CurrencyCode::Eur,
            Currency::GBP => CurrencyCode::Gbp,
            Currency::JPY => CurrencyCode::Jpy,
            Currency::CHF => CurrencyCode::Chf,
            Currency::CAD => CurrencyCode::Cad,
            Currency::AUD => CurrencyCode::Aud,
            _ => CurrencyCode::Usd,
        }
    }
}

/// Frequency codes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum FrequencyCode {
    Annual,
    #[default]
    SemiAnnual,
    Quarterly,
    Monthly,
}

impl From<FrequencyCode> for Frequency {
    fn from(code: FrequencyCode) -> Self {
        match code {
            FrequencyCode::Annual => Frequency::Annual,
            FrequencyCode::SemiAnnual => Frequency::SemiAnnual,
            FrequencyCode::Quarterly => Frequency::Quarterly,
            FrequencyCode::Monthly => Frequency::Monthly,
        }
    }
}

impl From<Frequency> for FrequencyCode {
    fn from(freq: Frequency) -> Self {
        match freq {
            Frequency::Annual => FrequencyCode::Annual,
            Frequency::SemiAnnual => FrequencyCode::SemiAnnual,
            Frequency::Quarterly => FrequencyCode::Quarterly,
            Frequency::Monthly => FrequencyCode::Monthly,
            _ => FrequencyCode::SemiAnnual,
        }
    }
}

/// Day count convention codes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum DayCountCode {
    Act360,
    Act365Fixed,
    ActActIsda,
    ActActIcma,
    #[default]
    Thirty360Us,
    Thirty360E,
}

impl From<DayCountCode> for DayCountConvention {
    fn from(code: DayCountCode) -> Self {
        match code {
            DayCountCode::Act360 => DayCountConvention::Act360,
            DayCountCode::Act365Fixed => DayCountConvention::Act365Fixed,
            DayCountCode::ActActIsda => DayCountConvention::ActActIsda,
            DayCountCode::ActActIcma => DayCountConvention::ActActIcma,
            DayCountCode::Thirty360Us => DayCountConvention::Thirty360US,
            DayCountCode::Thirty360E => DayCountConvention::Thirty360E,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_input_to_date_valid() {
        let input = DateInput {
            year: 2025,
            month: 12,
            day: 15,
        };
        let date = input.to_date().unwrap();
        assert_eq!(date.year(), 2025);
        assert_eq!(date.month(), 12);
        assert_eq!(date.day(), 15);
    }

    #[test]
    fn test_date_input_to_date_invalid() {
        let input = DateInput {
            year: 2025,
            month: 13,
            day: 1,
        };
        assert!(input.to_date().is_err());
    }

    #[test]
    fn test_date_input_from_date() {
        let date = Date::from_ymd(2025, 6, 20).unwrap();
        let input: DateInput = date.into();
        assert_eq!(input.year, 2025);
        assert_eq!(input.month, 6);
        assert_eq!(input.day, 20);
    }

    #[test]
    fn test_currency_code_conversions() {
        assert!(matches!(Currency::from(CurrencyCode::Usd), Currency::USD));
        assert!(matches!(Currency::from(CurrencyCode::Eur), Currency::EUR));
        assert!(matches!(Currency::from(CurrencyCode::Gbp), Currency::GBP));
        assert!(matches!(Currency::from(CurrencyCode::Jpy), Currency::JPY));
        assert!(matches!(Currency::from(CurrencyCode::Chf), Currency::CHF));
        assert!(matches!(Currency::from(CurrencyCode::Cad), Currency::CAD));
        assert!(matches!(Currency::from(CurrencyCode::Aud), Currency::AUD));
    }

    #[test]
    fn test_currency_code_from_currency() {
        assert!(matches!(CurrencyCode::from(Currency::USD), CurrencyCode::Usd));
        assert!(matches!(CurrencyCode::from(Currency::EUR), CurrencyCode::Eur));
        assert!(matches!(CurrencyCode::from(Currency::GBP), CurrencyCode::Gbp));
    }

    #[test]
    fn test_frequency_code_conversions() {
        assert!(matches!(
            Frequency::from(FrequencyCode::Annual),
            Frequency::Annual
        ));
        assert!(matches!(
            Frequency::from(FrequencyCode::SemiAnnual),
            Frequency::SemiAnnual
        ));
        assert!(matches!(
            Frequency::from(FrequencyCode::Quarterly),
            Frequency::Quarterly
        ));
        assert!(matches!(
            Frequency::from(FrequencyCode::Monthly),
            Frequency::Monthly
        ));
    }

    #[test]
    fn test_frequency_code_from_frequency() {
        assert!(matches!(
            FrequencyCode::from(Frequency::Annual),
            FrequencyCode::Annual
        ));
        assert!(matches!(
            FrequencyCode::from(Frequency::SemiAnnual),
            FrequencyCode::SemiAnnual
        ));
    }

    #[test]
    fn test_day_count_code_conversions() {
        assert!(matches!(
            DayCountConvention::from(DayCountCode::Act360),
            DayCountConvention::Act360
        ));
        assert!(matches!(
            DayCountConvention::from(DayCountCode::Act365Fixed),
            DayCountConvention::Act365Fixed
        ));
        assert!(matches!(
            DayCountConvention::from(DayCountCode::Thirty360Us),
            DayCountConvention::Thirty360US
        ));
    }

    #[test]
    fn test_default_currency_code() {
        let default = CurrencyCode::default();
        assert!(matches!(default, CurrencyCode::Usd));
    }

    #[test]
    fn test_default_frequency_code() {
        let default = FrequencyCode::default();
        assert!(matches!(default, FrequencyCode::SemiAnnual));
    }

    #[test]
    fn test_default_day_count_code() {
        let default = DayCountCode::default();
        assert!(matches!(default, DayCountCode::Thirty360Us));
    }

    #[test]
    fn test_date_input_serialization() {
        let input = DateInput {
            year: 2025,
            month: 12,
            day: 15,
        };
        let json = serde_json::to_string(&input).unwrap();
        assert!(json.contains("2025"));
        assert!(json.contains("12"));
        assert!(json.contains("15"));
    }

    #[test]
    fn test_date_input_deserialization() {
        let json = r#"{"year": 2025, "month": 6, "day": 15}"#;
        let input: DateInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.year, 2025);
        assert_eq!(input.month, 6);
        assert_eq!(input.day, 15);
    }
}
