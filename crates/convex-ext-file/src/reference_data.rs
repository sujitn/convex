//! File-based reference data sources.

use async_trait::async_trait;
use dashmap::DashMap;
use rust_decimal::Decimal;
use serde::Deserialize;

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

// =============================================================================
// IN-MEMORY PORTFOLIO STORE
// =============================================================================

use serde::Serialize;

/// Stored position in a portfolio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPosition {
    /// Instrument ID
    pub instrument_id: String,
    /// Notional/face value
    pub notional: Decimal,
    /// Sector classification
    pub sector: Option<String>,
    /// Credit rating
    pub rating: Option<String>,
}

/// Stored portfolio for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPortfolio {
    /// Portfolio identifier
    pub portfolio_id: String,
    /// Portfolio name
    pub name: String,
    /// Reporting currency (USD, EUR, GBP, etc.)
    pub currency: String,
    /// Description
    pub description: Option<String>,
    /// Positions
    pub positions: Vec<StoredPosition>,
    /// Created timestamp
    pub created_at: i64,
    /// Last updated timestamp
    pub updated_at: i64,
}

/// Filter for portfolio queries.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PortfolioFilter {
    /// Currency filter
    pub currency: Option<String>,
    /// Text search query
    pub text_search: Option<String>,
}

/// In-memory mutable portfolio store.
///
/// Supports CRUD operations for portfolios, used by the REST API.
pub struct InMemoryPortfolioStore {
    portfolios: DashMap<String, StoredPortfolio>,
}

impl InMemoryPortfolioStore {
    /// Create a new empty in-memory portfolio store.
    pub fn new() -> Self {
        Self {
            portfolios: DashMap::new(),
        }
    }

    /// Insert or update a portfolio.
    pub fn upsert(&self, portfolio: StoredPortfolio) -> StoredPortfolio {
        let id = portfolio.portfolio_id.clone();
        self.portfolios.insert(id, portfolio.clone());
        portfolio
    }

    /// Get a portfolio by ID.
    pub fn get(&self, portfolio_id: &str) -> Option<StoredPortfolio> {
        self.portfolios.get(portfolio_id).map(|p| p.clone())
    }

    /// Delete a portfolio by ID.
    pub fn delete(&self, portfolio_id: &str) -> Option<StoredPortfolio> {
        self.portfolios.remove(portfolio_id).map(|(_, p)| p)
    }

    /// List all portfolios with optional filtering and pagination.
    pub fn list(
        &self,
        filter: &PortfolioFilter,
        limit: usize,
        offset: usize,
    ) -> Vec<StoredPortfolio> {
        let mut results: Vec<StoredPortfolio> = self
            .portfolios
            .iter()
            .filter(|r| {
                let portfolio = r.value();

                // Apply filters
                if let Some(ref currency) = filter.currency {
                    if portfolio.currency != *currency {
                        return false;
                    }
                }
                if let Some(ref text_search) = filter.text_search {
                    let search_lower = text_search.to_lowercase();
                    let matches = portfolio.name.to_lowercase().contains(&search_lower)
                        || portfolio
                            .portfolio_id
                            .to_lowercase()
                            .contains(&search_lower)
                        || portfolio
                            .description
                            .as_ref()
                            .is_some_and(|d| d.to_lowercase().contains(&search_lower));
                    if !matches {
                        return false;
                    }
                }

                true
            })
            .map(|r| r.value().clone())
            .collect();

        // Sort by portfolio ID for consistent ordering
        results.sort_by(|a, b| a.portfolio_id.cmp(&b.portfolio_id));

        // Apply pagination
        results.into_iter().skip(offset).take(limit).collect()
    }

    /// Count portfolios matching filter.
    pub fn count(&self, filter: &PortfolioFilter) -> usize {
        self.portfolios
            .iter()
            .filter(|r| {
                let portfolio = r.value();

                if let Some(ref currency) = filter.currency {
                    if portfolio.currency != *currency {
                        return false;
                    }
                }
                if let Some(ref text_search) = filter.text_search {
                    let search_lower = text_search.to_lowercase();
                    let matches = portfolio.name.to_lowercase().contains(&search_lower)
                        || portfolio
                            .portfolio_id
                            .to_lowercase()
                            .contains(&search_lower)
                        || portfolio
                            .description
                            .as_ref()
                            .is_some_and(|d| d.to_lowercase().contains(&search_lower));
                    if !matches {
                        return false;
                    }
                }

                true
            })
            .count()
    }

    /// Get total count of portfolios.
    pub fn len(&self) -> usize {
        self.portfolios.len()
    }

    /// Check if the store is empty.
    pub fn is_empty(&self) -> bool {
        self.portfolios.is_empty()
    }

    /// Clear all portfolios.
    pub fn clear(&self) {
        self.portfolios.clear();
    }

    /// Add a position to a portfolio.
    pub fn add_position(
        &self,
        portfolio_id: &str,
        position: StoredPosition,
    ) -> Option<StoredPortfolio> {
        if let Some(mut entry) = self.portfolios.get_mut(portfolio_id) {
            entry.positions.push(position);
            entry.updated_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            Some(entry.clone())
        } else {
            None
        }
    }

    /// Remove a position from a portfolio.
    pub fn remove_position(
        &self,
        portfolio_id: &str,
        instrument_id: &str,
    ) -> Option<StoredPortfolio> {
        if let Some(mut entry) = self.portfolios.get_mut(portfolio_id) {
            entry.positions.retain(|p| p.instrument_id != instrument_id);
            entry.updated_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            Some(entry.clone())
        } else {
            None
        }
    }

    /// Update a position in a portfolio.
    pub fn update_position(
        &self,
        portfolio_id: &str,
        position: StoredPosition,
    ) -> Option<StoredPortfolio> {
        if let Some(mut entry) = self.portfolios.get_mut(portfolio_id) {
            if let Some(existing) = entry
                .positions
                .iter_mut()
                .find(|p| p.instrument_id == position.instrument_id)
            {
                *existing = position;
                entry.updated_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64;
                Some(entry.clone())
            } else {
                None
            }
        } else {
            None
        }
    }
}

impl Default for InMemoryPortfolioStore {
    fn default() -> Self {
        Self::new()
    }
}
