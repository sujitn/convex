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
