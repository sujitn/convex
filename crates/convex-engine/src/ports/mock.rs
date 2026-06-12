#![cfg(test)]
#![allow(unused_variables)]

use async_trait::async_trait;
use rust_decimal::Decimal;
use std::sync::Arc;

use crate::ports::error::TraitError;
use crate::ports::market_data::*;
use crate::ports::reference_data::*;
use convex_core::ids::*;
use convex_core::types::*;
use convex_core::Date;

pub struct EmptyQuoteSource;
#[async_trait]
impl QuoteSource for EmptyQuoteSource {
    fn source_type(&self) -> SourceType {
        SourceType::Manual
    }
    async fn get_quote(&self, instrument_id: &InstrumentId) -> Result<Option<RawQuote>, TraitError> {
        Ok(None)
    }
    async fn get_quotes(&self,
        instrument_ids: &[InstrumentId],) -> Result<Vec<RawQuote>, TraitError> {
        Ok(vec![])
    }
    async fn subscribe(&self, instrument_ids: &[InstrumentId]) -> Result<QuoteReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
    async fn unsubscribe(&self, instrument_ids: &[InstrumentId]) -> Result<(), TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}

pub struct EmptyCurveInputSource;
#[async_trait]
impl CurveInputSource for EmptyCurveInputSource {
    fn source_type(&self) -> SourceType {
        SourceType::Manual
    }
    async fn get_curve_inputs(&self, curve_id: &CurveId) -> Result<Vec<CurveInput>, TraitError> {
        Ok(vec![])
    }
    async fn subscribe(&self, curve_ids: &[CurveId]) -> Result<CurveInputReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}

pub struct EmptyIndexFixingSource;
#[async_trait]
impl IndexFixingSource for EmptyIndexFixingSource {
    fn source_type(&self) -> SourceType {
        SourceType::Manual
    }
    async fn get_fixing(&self,
        index: &FloatingRateIndex,
        date: Date,) -> Result<Option<IndexFixing>, TraitError> {
        Ok(None)
    }
    async fn get_fixings(&self,
        index: &FloatingRateIndex,
        from: Date,
        to: Date,) -> Result<Vec<IndexFixing>, TraitError> {
        Ok(vec![])
    }
    async fn subscribe(&self,
        indices: &[FloatingRateIndex],) -> Result<IndexFixingReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}

pub struct EmptyVolatilitySource;
#[async_trait]
impl VolatilitySource for EmptyVolatilitySource {
    async fn get_surface(&self,
        surface_id: &VolSurfaceId,) -> Result<Option<VolatilitySurface>, TraitError> {
        Ok(None)
    }
    async fn get_atm_vol(&self,
        surface_id: &VolSurfaceId,
        expiry: Tenor,
        underlying_tenor: Option<Tenor>,) -> Result<Option<Decimal>, TraitError> {
        Ok(None)
    }
    async fn get_vol(&self,
        surface_id: &VolSurfaceId,
        expiry: Tenor,
        underlying_tenor: Option<Tenor>,
        strike: Option<Decimal>,) -> Result<Option<Decimal>, TraitError> {
        Ok(None)
    }
    async fn subscribe(&self, surface_ids: &[VolSurfaceId]) -> Result<VolReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}

pub struct EmptyFxRateSource;
#[async_trait]
impl FxRateSource for EmptyFxRateSource {
    async fn get_spot(&self, pair: &CurrencyPair) -> Result<Option<FxRate>, TraitError> {
        Ok(None)
    }
    async fn get_spot_triangulated(&self,
        base: convex_core::Currency,
        quote: convex_core::Currency,) -> Result<Option<FxRate>, TraitError> {
        Ok(None)
    }
    async fn get_forward(&self,
        pair: &CurrencyPair,
        tenor: Tenor,) -> Result<Option<Decimal>, TraitError> {
        Ok(None)
    }
    async fn subscribe(&self, pairs: &[CurrencyPair]) -> Result<FxRateReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}

pub struct EmptyInflationFixingSource;
#[async_trait]
impl InflationFixingSource for EmptyInflationFixingSource {
    async fn get_fixing(&self,
        index: &InflationIndex,
        month: YearMonth,) -> Result<Option<InflationFixing>, TraitError> {
        Ok(None)
    }
    async fn get_latest_fixing(&self,
        index: &InflationIndex,) -> Result<Option<InflationFixing>, TraitError> {
        Ok(None)
    }
    async fn get_fixings_range(&self,
        index: &InflationIndex,
        from: YearMonth,
        to: YearMonth,) -> Result<Vec<InflationFixing>, TraitError> {
        Ok(vec![])
    }
    async fn get_interpolated_index(&self,
        index: &InflationIndex,
        date: Date,
        interpolation: InflationInterpolation,) -> Result<Option<Decimal>, TraitError> {
        Ok(None)
    }
    async fn subscribe(&self, indices: &[InflationIndex]) -> Result<InflationReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}

pub struct EmptyEtfQuoteSource;
#[async_trait]
impl EtfQuoteSource for EmptyEtfQuoteSource {
    async fn get_quote(&self, etf_id: &EtfId) -> Result<Option<EtfQuote>, TraitError> {
        Ok(None)
    }
    async fn subscribe(&self, etf_ids: &[EtfId]) -> Result<EtfQuoteReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}

pub struct EmptyBondReferenceSource;
#[async_trait]
impl BondReferenceSource for EmptyBondReferenceSource {
    async fn get_by_isin(&self, isin: &str) -> Result<Option<BondReferenceData>, TraitError> {
        Ok(None)
    }
    async fn get_by_cusip(&self, cusip: &str) -> Result<Option<BondReferenceData>, TraitError> {
        Ok(None)
    }
    async fn get_by_id(&self,
        instrument_id: &InstrumentId,) -> Result<Option<BondReferenceData>, TraitError> {
        Ok(None)
    }
    async fn get_many_by_isin(&self, isins: &[&str]) -> Result<Vec<BondReferenceData>, TraitError> {
        Ok(vec![])
    }
    async fn search(&self,
        filter: &BondFilter,
        limit: usize,
        offset: usize,) -> Result<Vec<BondReferenceData>, TraitError> {
        Ok(vec![])
    }
    async fn count(&self, filter: &BondFilter) -> Result<u64, TraitError> {
        Ok(0)
    }
    async fn subscribe(&self, filter: &BondFilter) -> Result<BondRefDataReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}

pub struct EmptyIssuerReferenceSource;
#[async_trait]
impl IssuerReferenceSource for EmptyIssuerReferenceSource {
    async fn get_issuer(&self, issuer_id: &str) -> Result<Option<IssuerInfo>, TraitError> {
        Ok(None)
    }
    async fn get_by_lei(&self, lei: &str) -> Result<Option<IssuerInfo>, TraitError> {
        Ok(None)
    }
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<IssuerInfo>, TraitError> {
        Ok(vec![])
    }
    async fn get_by_sector(&self, sector: &str) -> Result<Vec<IssuerInfo>, TraitError> {
        Ok(vec![])
    }
}

pub struct EmptyRatingSource;
#[async_trait]
impl RatingSource for EmptyRatingSource {
    async fn get_rating(&self,
        issuer_id: &str,
        agency: RatingAgency,) -> Result<Option<CreditRating>, TraitError> {
        Ok(None)
    }
    async fn get_all_ratings(&self, issuer_id: &str) -> Result<Vec<CreditRating>, TraitError> {
        Ok(vec![])
    }
    async fn get_composite_rating(&self,
        issuer_id: &str,) -> Result<Option<CreditRating>, TraitError> {
        Ok(None)
    }
    async fn get_rating_history(&self,
        issuer_id: &str,
        agency: RatingAgency,
        limit: usize,) -> Result<Vec<CreditRating>, TraitError> {
        Ok(vec![])
    }
}

pub struct EmptyEtfHoldingsSource;
#[async_trait]
impl EtfHoldingsSource for EmptyEtfHoldingsSource {
    async fn get_holdings(&self, etf_id: &EtfId) -> Result<Option<EtfHoldings>, TraitError> {
        Ok(None)
    }
    async fn get_holdings_as_of(&self,
        etf_id: &EtfId,
        as_of_date: Date,) -> Result<Option<EtfHoldings>, TraitError> {
        Ok(None)
    }
    async fn list_etfs(&self) -> Result<Vec<EtfId>, TraitError> {
        Ok(vec![])
    }
    async fn subscribe(&self, etf_ids: &[EtfId]) -> Result<EtfHoldingsReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}
