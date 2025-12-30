//! File-based reference data sources.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use dashmap::DashMap;
use rust_decimal::Decimal;
use serde::Deserialize;

use convex_traits::error::TraitError;
use convex_traits::ids::*;
use convex_traits::reference_data::*;
use convex_core::{Currency, Date};

// =============================================================================
// CSV BOND REFERENCE SOURCE
// =============================================================================

/// CSV record for bonds.
#[derive(Debug, Deserialize)]
struct BondRecord {
    instrument_id: String,
    isin: Option<String>,
    cusip: Option<String>,
    description: String,
    currency: String,
    maturity_date: String,
    coupon_rate: Option<f64>,
    frequency: u32,
    issuer_name: String,
}

/// CSV-based bond reference source.
pub struct CsvBondReferenceSource {
    file_path: PathBuf,
    bonds: DashMap<InstrumentId, BondReferenceData>,
    by_isin: DashMap<String, InstrumentId>,
    by_cusip: DashMap<String, InstrumentId>,
}

impl CsvBondReferenceSource {
    /// Create a new CSV bond reference source.
    pub fn new(file_path: impl AsRef<Path>) -> Result<Self, TraitError> {
        let source = Self {
            file_path: file_path.as_ref().to_path_buf(),
            bonds: DashMap::new(),
            by_isin: DashMap::new(),
            by_cusip: DashMap::new(),
        };
        source.reload()?;
        Ok(source)
    }

    /// Reload bonds from file.
    pub fn reload(&self) -> Result<(), TraitError> {
        if !self.file_path.exists() {
            return Ok(()); // Empty source
        }

        let mut reader = csv::Reader::from_path(&self.file_path)
            .map_err(|e| TraitError::IoError(e.to_string()))?;

        for result in reader.deserialize() {
            let record: BondRecord = result.map_err(|e| TraitError::ParseError(e.to_string()))?;

            let instrument_id = InstrumentId::new(&record.instrument_id);

            // Parse maturity date
            let maturity_parts: Vec<&str> = record.maturity_date.split('-').collect();
            let maturity_date = if maturity_parts.len() == 3 {
                let year: i32 = maturity_parts[0].parse().unwrap_or(2030);
                let month: u32 = maturity_parts[1].parse().unwrap_or(1);
                let day: u32 = maturity_parts[2].parse().unwrap_or(1);
                Date::from_ymd(year, month, day).unwrap_or_else(|_| Date::from_ymd(2030, 1, 1).unwrap())
            } else {
                Date::from_ymd(2030, 1, 1).unwrap()
            };

            let currency = Currency::from_code(&record.currency).unwrap_or(Currency::USD);

            let bond = BondReferenceData {
                instrument_id: instrument_id.clone(),
                isin: record.isin.clone(),
                cusip: record.cusip.clone(),
                sedol: None,
                bbgid: None,
                description: record.description,
                currency,
                issue_date: Date::from_ymd(2020, 1, 1).unwrap(),
                maturity_date,
                coupon_rate: record.coupon_rate.map(|r| Decimal::try_from(r).unwrap_or_default()),
                frequency: record.frequency,
                day_count: "30/360".to_string(),
                face_value: Decimal::from(100),
                bond_type: BondType::FixedBullet,
                issuer_type: IssuerType::CorporateIG,
                issuer_id: "unknown".to_string(),
                issuer_name: record.issuer_name,
                seniority: "Senior".to_string(),
                is_callable: false,
                call_schedule: vec![],
                is_putable: false,
                is_sinkable: false,
                floating_terms: None,
                inflation_index: None,
                inflation_base_index: None,
                has_deflation_floor: false,
                country_of_risk: "US".to_string(),
                sector: "Corporate".to_string(),
                amount_outstanding: None,
                first_coupon_date: None,
                last_updated: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64,
                source: "file".to_string(),
            };

            // Index by ISIN and CUSIP
            if let Some(ref isin) = record.isin {
                self.by_isin.insert(isin.clone(), instrument_id.clone());
            }
            if let Some(ref cusip) = record.cusip {
                self.by_cusip.insert(cusip.clone(), instrument_id.clone());
            }

            self.bonds.insert(instrument_id, bond);
        }

        Ok(())
    }
}

#[async_trait]
impl BondReferenceSource for CsvBondReferenceSource {
    async fn get_by_isin(&self, isin: &str) -> Result<Option<BondReferenceData>, TraitError> {
        if let Some(id) = self.by_isin.get(isin) {
            return Ok(self.bonds.get(&id).map(|b| b.clone()));
        }
        Ok(None)
    }

    async fn get_by_cusip(&self, cusip: &str) -> Result<Option<BondReferenceData>, TraitError> {
        if let Some(id) = self.by_cusip.get(cusip) {
            return Ok(self.bonds.get(&id).map(|b| b.clone()));
        }
        Ok(None)
    }

    async fn get_by_id(
        &self,
        instrument_id: &InstrumentId,
    ) -> Result<Option<BondReferenceData>, TraitError> {
        Ok(self.bonds.get(instrument_id).map(|b| b.clone()))
    }

    async fn get_many_by_isin(&self, isins: &[&str]) -> Result<Vec<BondReferenceData>, TraitError> {
        let mut results = Vec::new();
        for isin in isins {
            if let Some(bond) = self.get_by_isin(isin).await? {
                results.push(bond);
            }
        }
        Ok(results)
    }

    async fn search(
        &self,
        _filter: &BondFilter,
        limit: usize,
        _offset: usize,
    ) -> Result<Vec<BondReferenceData>, TraitError> {
        Ok(self
            .bonds
            .iter()
            .take(limit)
            .map(|r| r.value().clone())
            .collect())
    }

    async fn count(&self, _filter: &BondFilter) -> Result<u64, TraitError> {
        Ok(self.bonds.len() as u64)
    }

    async fn subscribe(&self, _filter: &BondFilter) -> Result<BondRefDataReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable(
            "File source does not support streaming".into(),
        ))
    }
}

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

/// Empty bond reference source for testing.
pub struct EmptyBondReferenceSource;

#[async_trait]
impl BondReferenceSource for EmptyBondReferenceSource {
    async fn get_by_isin(&self, _isin: &str) -> Result<Option<BondReferenceData>, TraitError> {
        Ok(None)
    }

    async fn get_by_cusip(&self, _cusip: &str) -> Result<Option<BondReferenceData>, TraitError> {
        Ok(None)
    }

    async fn get_by_id(
        &self,
        _instrument_id: &InstrumentId,
    ) -> Result<Option<BondReferenceData>, TraitError> {
        Ok(None)
    }

    async fn get_many_by_isin(&self, _isins: &[&str]) -> Result<Vec<BondReferenceData>, TraitError> {
        Ok(vec![])
    }

    async fn search(
        &self,
        _filter: &BondFilter,
        _offset: usize,
        _limit: usize,
    ) -> Result<Vec<BondReferenceData>, TraitError> {
        Ok(vec![])
    }

    async fn count(&self, _filter: &BondFilter) -> Result<u64, TraitError> {
        Ok(0)
    }

    async fn subscribe(&self, _filter: &BondFilter) -> Result<BondRefDataReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable("Not implemented".into()))
    }
}

// =============================================================================
// IN-MEMORY MUTABLE BOND STORE
// =============================================================================

/// In-memory mutable bond reference store.
///
/// Supports CRUD operations for bond reference data, used by the REST API.
pub struct InMemoryBondStore {
    bonds: DashMap<InstrumentId, BondReferenceData>,
    by_isin: DashMap<String, InstrumentId>,
    by_cusip: DashMap<String, InstrumentId>,
}

impl InMemoryBondStore {
    /// Create a new empty in-memory bond store.
    pub fn new() -> Self {
        Self {
            bonds: DashMap::new(),
            by_isin: DashMap::new(),
            by_cusip: DashMap::new(),
        }
    }

    /// Insert or update a bond.
    pub fn upsert(&self, bond: BondReferenceData) -> BondReferenceData {
        let instrument_id = bond.instrument_id.clone();

        // Update index maps
        if let Some(ref isin) = bond.isin {
            self.by_isin.insert(isin.clone(), instrument_id.clone());
        }
        if let Some(ref cusip) = bond.cusip {
            self.by_cusip.insert(cusip.clone(), instrument_id.clone());
        }

        self.bonds.insert(instrument_id, bond.clone());
        bond
    }

    /// Delete a bond by instrument ID.
    pub fn delete(&self, instrument_id: &InstrumentId) -> Option<BondReferenceData> {
        if let Some((_, bond)) = self.bonds.remove(instrument_id) {
            // Remove from index maps
            if let Some(ref isin) = bond.isin {
                self.by_isin.remove(isin);
            }
            if let Some(ref cusip) = bond.cusip {
                self.by_cusip.remove(cusip);
            }
            Some(bond)
        } else {
            None
        }
    }

    /// List all bonds with pagination.
    pub fn list(&self, limit: usize, offset: usize) -> Vec<BondReferenceData> {
        self.bonds
            .iter()
            .skip(offset)
            .take(limit)
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get total count of bonds.
    pub fn len(&self) -> usize {
        self.bonds.len()
    }

    /// Check if the store is empty.
    pub fn is_empty(&self) -> bool {
        self.bonds.is_empty()
    }

    /// Clear all bonds.
    pub fn clear(&self) {
        self.bonds.clear();
        self.by_isin.clear();
        self.by_cusip.clear();
    }
}

impl Default for InMemoryBondStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BondReferenceSource for InMemoryBondStore {
    async fn get_by_isin(&self, isin: &str) -> Result<Option<BondReferenceData>, TraitError> {
        if let Some(id) = self.by_isin.get(isin) {
            return Ok(self.bonds.get(&id).map(|b| b.clone()));
        }
        Ok(None)
    }

    async fn get_by_cusip(&self, cusip: &str) -> Result<Option<BondReferenceData>, TraitError> {
        if let Some(id) = self.by_cusip.get(cusip) {
            return Ok(self.bonds.get(&id).map(|b| b.clone()));
        }
        Ok(None)
    }

    async fn get_by_id(
        &self,
        instrument_id: &InstrumentId,
    ) -> Result<Option<BondReferenceData>, TraitError> {
        Ok(self.bonds.get(instrument_id).map(|b| b.clone()))
    }

    async fn get_many_by_isin(&self, isins: &[&str]) -> Result<Vec<BondReferenceData>, TraitError> {
        let mut results = Vec::new();
        for isin in isins {
            if let Some(bond) = self.get_by_isin(isin).await? {
                results.push(bond);
            }
        }
        Ok(results)
    }

    async fn search(
        &self,
        filter: &BondFilter,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<BondReferenceData>, TraitError> {
        let mut results: Vec<BondReferenceData> = self
            .bonds
            .iter()
            .filter(|r| {
                let bond = r.value();

                // Apply filters
                if let Some(ref currency) = filter.currency {
                    if bond.currency != *currency {
                        return false;
                    }
                }
                if let Some(ref issuer_type) = filter.issuer_type {
                    if bond.issuer_type != *issuer_type {
                        return false;
                    }
                }
                if let Some(ref bond_type) = filter.bond_type {
                    if bond.bond_type != *bond_type {
                        return false;
                    }
                }
                if let Some(ref maturity_from) = filter.maturity_from {
                    if bond.maturity_date < *maturity_from {
                        return false;
                    }
                }
                if let Some(ref maturity_to) = filter.maturity_to {
                    if bond.maturity_date > *maturity_to {
                        return false;
                    }
                }
                if let Some(ref country) = filter.country {
                    if bond.country_of_risk != *country {
                        return false;
                    }
                }
                if let Some(ref sector) = filter.sector {
                    if bond.sector != *sector {
                        return false;
                    }
                }
                if let Some(is_callable) = filter.is_callable {
                    if bond.is_callable != is_callable {
                        return false;
                    }
                }
                if let Some(is_floating) = filter.is_floating {
                    let is_frn = bond.floating_terms.is_some();
                    if is_frn != is_floating {
                        return false;
                    }
                }
                if let Some(is_inflation_linked) = filter.is_inflation_linked {
                    let is_linker = bond.inflation_index.is_some();
                    if is_linker != is_inflation_linked {
                        return false;
                    }
                }
                if let Some(ref issuer_id) = filter.issuer_id {
                    if bond.issuer_id != *issuer_id {
                        return false;
                    }
                }
                if let Some(ref text_search) = filter.text_search {
                    let search_lower = text_search.to_lowercase();
                    let matches = bond.description.to_lowercase().contains(&search_lower)
                        || bond.issuer_name.to_lowercase().contains(&search_lower)
                        || bond.isin.as_ref().map_or(false, |s| s.to_lowercase().contains(&search_lower))
                        || bond.cusip.as_ref().map_or(false, |s| s.to_lowercase().contains(&search_lower));
                    if !matches {
                        return false;
                    }
                }

                true
            })
            .map(|r| r.value().clone())
            .collect();

        // Sort by instrument ID for consistent ordering
        results.sort_by(|a, b| a.instrument_id.as_str().cmp(b.instrument_id.as_str()));

        // Apply pagination
        Ok(results.into_iter().skip(offset).take(limit).collect())
    }

    async fn count(&self, filter: &BondFilter) -> Result<u64, TraitError> {
        // For count, we reuse search but without limit
        let results = self.search(filter, usize::MAX, 0).await?;
        Ok(results.len() as u64)
    }

    async fn subscribe(&self, _filter: &BondFilter) -> Result<BondRefDataReceiver, TraitError> {
        Err(TraitError::SourceNotAvailable(
            "In-memory store does not support streaming".into(),
        ))
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
    pub fn list(&self, filter: &PortfolioFilter, limit: usize, offset: usize) -> Vec<StoredPortfolio> {
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
                        || portfolio.portfolio_id.to_lowercase().contains(&search_lower)
                        || portfolio.description.as_ref().map_or(false, |d| d.to_lowercase().contains(&search_lower));
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
                        || portfolio.portfolio_id.to_lowercase().contains(&search_lower)
                        || portfolio.description.as_ref().map_or(false, |d| d.to_lowercase().contains(&search_lower));
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
    pub fn add_position(&self, portfolio_id: &str, position: StoredPosition) -> Option<StoredPortfolio> {
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
    pub fn remove_position(&self, portfolio_id: &str, instrument_id: &str) -> Option<StoredPortfolio> {
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
    pub fn update_position(&self, portfolio_id: &str, position: StoredPosition) -> Option<StoredPortfolio> {
        if let Some(mut entry) = self.portfolios.get_mut(portfolio_id) {
            if let Some(existing) = entry.positions.iter_mut().find(|p| p.instrument_id == position.instrument_id) {
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
