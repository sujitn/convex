//! File-based reference data sources.

use async_trait::async_trait;

use convex_core::ids::*;
use convex_core::Date;
use convex_ports::error::TraitError;
use convex_ports::reference_data::*;

// =============================================================================
// EMPTY IMPLEMENTATIONS
// =============================================================================

/// Empty issuer reference source.
pub struct EmptyIssuerReferenceSource;

#[async_trait]
impl IssuerReferenceSource for EmptyIssuerReferenceSource {
    async fn get_issuer(&self, _issuer_id: &str) -> Result<Option<IssuerInfo>, TraitError> {
        Ok(None)
    }

    async fn get_by_lei(&self, _lei: &str) -> Result<Option<IssuerInfo>, TraitError> {
        Ok(None)
    }

    async fn search(&self, _query: &str, _limit: usize) -> Result<Vec<IssuerInfo>, TraitError> {
        Ok(vec![])
    }

    async fn get_by_sector(&self, _sector: &str) -> Result<Vec<IssuerInfo>, TraitError> {
        Ok(vec![])
    }
}

/// Empty rating source.
pub struct EmptyRatingSource;

#[async_trait]
impl RatingSource for EmptyRatingSource {
    async fn get_rating(
        &self,
        _issuer_id: &str,
        _agency: RatingAgency,
    ) -> Result<Option<CreditRating>, TraitError> {
        Ok(None)
    }

    async fn get_all_ratings(&self, _issuer_id: &str) -> Result<Vec<CreditRating>, TraitError> {
        Ok(vec![])
    }

    async fn get_composite_rating(
        &self,
        _issuer_id: &str,
    ) -> Result<Option<CreditRating>, TraitError> {
        Ok(None)
    }

    async fn get_rating_history(
        &self,
        _issuer_id: &str,
        _agency: RatingAgency,
        _limit: usize,
    ) -> Result<Vec<CreditRating>, TraitError> {
        Ok(vec![])
    }
}

/// Empty ETF holdings source.
pub struct EmptyEtfHoldingsSource;

#[async_trait]
impl EtfHoldingsSource for EmptyEtfHoldingsSource {
    async fn get_holdings(&self, _etf_id: &EtfId) -> Result<Option<EtfHoldings>, TraitError> {
        Ok(None)
    }

    async fn get_holdings_as_of(
        &self,
        _etf_id: &EtfId,
        _as_of_date: Date,
    ) -> Result<Option<EtfHoldings>, TraitError> {
        Ok(None)
    }

    async fn list_etfs(&self) -> Result<Vec<EtfId>, TraitError> {
        Ok(vec![])
    }

    async fn subscribe(&self, _etf_ids: &[EtfId]) -> Result<EtfHoldingsReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}
