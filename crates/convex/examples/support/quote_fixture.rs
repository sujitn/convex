//! Offline full original cash reference inputs, shared by consumer and tests.
#![allow(dead_code)]
use convex_analytics::{cash_calibration::*, quote_risk::*};
use convex_bonds::contracts::BondCalendars;
use convex_core::{
    calendars::{DynamicCalendar, WeekendType},
    contracts::*,
    Currency,
};
use convex_curves::contracts::*;
use serde_json::Value;
pub type Error = Box<dyn std::error::Error>;
pub fn failure(e: impl std::fmt::Display) -> QuoteRiskError {
    QuoteRiskError::Invalid {
        field: "original native pricing".into(),
        reason: e.to_string(),
    }
}
pub fn settings(bump: f64) -> Result<QuoteRiskSettings, Error> {
    Ok(QuoteRiskSettings {
        bump,
        stability: RiskTolerance {
            absolute: 0.05,
            relative: 1e-4,
        },
        policy_version: Version::new(1)?,
    })
}
pub fn snapshot() -> Result<SnapshotId, Error> {
    Ok(SnapshotId::new("synthetic-CVX024-original-snapshot")?)
}
pub fn position(
    quantity: &str,
    reference: &str,
    unit: RiskQuantityUnit,
) -> Result<RiskPosition, Error> {
    Ok(RiskPosition {
        quantity: ExactDecimal::parse(quantity)?,
        reference_quantity: ExactDecimal::parse(reference)?,
        quantity_unit: unit,
        currency: Currency::USD,
    })
}
pub fn key(
    q: QuoteType,
    coordinate: RiskCoordinate,
    native_unit: QuoteUnit,
    response: RiskResponse,
    tenor: &str,
) -> Result<QuoteRiskKey, Error> {
    let family = match q {
        QuoteType::CdsSpread
        | QuoteType::CdsUpfront
        | QuoteType::CdsDirtyUpfront
        | QuoteType::ZSpread
        | QuoteType::ResidualBasis
        | QuoteType::OasSpread => FactorFamily::CreditSpread,
        QuoteType::RecoveryRate => FactorFamily::Recovery,
        QuoteType::HazardRate => FactorFamily::Hazard,
        QuoteType::FuturesPrice => FactorFamily::FuturesPrice,
        QuoteType::CashCleanPrice | QuoteType::CashDirtyPrice => FactorFamily::CashPrice,
        _ => FactorFamily::InterestRate,
    };
    let target = match &coordinate {
        RiskCoordinate::Recovery { .. } => FactorTarget::Entity {
            entity_id: EntityId::new("legal-A")?,
        },
        RiskCoordinate::Raw { input_id, .. } => FactorTarget::Instrument {
            instrument_id: InstrumentId::new(input_id.clone())?,
        },
        RiskCoordinate::Quote { curve, .. }
        | RiskCoordinate::ParallelQuotes { curve, .. }
        | RiskCoordinate::Zero { curve, .. }
        | RiskCoordinate::Basis { curve, .. }
        | RiskCoordinate::Hazard { curve, .. } => FactorTarget::Curve {
            curve_id: CurveId::new(curve.id.clone())?,
        },
    };
    let basis = match coordinate {
        RiskCoordinate::Quote { .. } | RiskCoordinate::ParallelQuotes { .. } => {
            RiskBasis::MarketQuoteRecalibrated
        }
        RiskCoordinate::Zero { .. } | RiskCoordinate::Basis { .. } => RiskBasis::ZeroNode,
        RiskCoordinate::Recovery { .. } | RiskCoordinate::Hazard { .. } => {
            RiskBasis::ModelParameter
        }
        RiskCoordinate::Raw { .. } => RiskBasis::RawQuote,
    };
    let unit = match q {
        QuoteType::CdsUpfront | QuoteType::CdsDirtyUpfront | QuoteType::RecoveryRate => {
            QuoteRiskUnit::CurrencyPerFraction
        }
        QuoteType::FuturesPrice | QuoteType::CashCleanPrice | QuoteType::CashDirtyPrice => {
            QuoteRiskUnit::CurrencyPerPricePoint
        }
        QuoteType::HazardRate => QuoteRiskUnit::CurrencyPerAnnualRate,
        _ => QuoteRiskUnit::CurrencyPerBp,
    };
    Ok(QuoteRiskKey::new(QuoteRiskDefinition {
        factor: RiskFactorKey::new(FactorDefinition {
            family,
            target,
            currency: Currency::USD,
            tenor: FactorTenor::new(tenor)?,
            quote_type: q,
            basis,
            convention_version: Version::new(1)?,
            model_version: ModelId::new("synthetic.CVX024.original.risk.v1")?,
        })?,
        coordinate,
        native_unit,
        response,
        unit,
    })?)
}
pub struct Fixture {
    pub data: Value,
    pub helper: CashBondHelper,
    pub calendar: DynamicCalendar,
}
impl Fixture {
    pub fn load() -> Result<Self, Error> {
        let data: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/risk/quote-risk.json"
        ))?;
        let holidays: Vec<convex_core::Date> = serde_json::from_value(data["holidays"].clone())?;
        let calendar = DynamicCalendar::from_dates(
            "actual synthetic CVX024 original",
            WeekendType::SaturdaySunday,
            holidays,
        );
        let helper = serde_json::from_value(data["helper"].clone())?;
        Ok(Self {
            data,
            helper,
            calendar,
        })
    }
    pub fn resources(&self) -> CashHelperResources<'_> {
        CashHelperResources::Bond {
            ids: &self.helper.terms.calendars,
            calendars: BondCalendars {
                payment: &self.calendar,
                reset: &self.calendar,
                observation: &self.calendar,
                exercise: &self.calendar,
            },
        }
    }
    pub fn raw_yield(&self, quantity: &str, bump: f64) -> Result<QuoteSensitivity, Error> {
        let key = key(
            QuoteType::Yield,
            RiskCoordinate::Raw {
                input_id: self.helper.id.clone(),
                version: self.helper.quote_version,
            },
            self.helper.quote.unit,
            RiskResponse::FrozenCurves,
            "spot",
        )?;
        let face: f64 = self.helper.terms.face_value.to_string().parse()?;
        let scale = position(
            quantity,
            &self.helper.terms.face_value.to_string(),
            RiskQuantityUnit::Notional,
        )?;
        Ok(reference_quote_sensitivity(
            key.clone(),
            scale,
            snapshot()?,
            settings(bump)?,
            |d| {
                let mut q = self.helper.quote;
                q.value += key.native_move(d)?;
                let helper = self.helper.with_quote(q).map_err(failure)?;
                let quote = convert_cash_quote(
                    &helper,
                    &self.resources(),
                    convex_core::Date::parse("2026-10-07").map_err(failure)?,
                )
                .map_err(failure)?;
                risk_reference_pv(quote.dirty_per100 * face / 100.0, helper.terms.currency)
            },
        )?)
    }
}
