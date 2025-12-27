//! Portfolio builder for fluent construction.

use crate::types::{CashPosition, Holding};
use crate::{Portfolio, PortfolioError, PortfolioResult};
use convex_core::types::{Currency, Date};
use rust_decimal::Decimal;

/// Builder for constructing a [`Portfolio`].
///
/// # Example
///
/// ```rust,ignore
/// use convex_portfolio::prelude::*;
///
/// let portfolio = PortfolioBuilder::new()
///     .id("PORT001")
///     .name("My Portfolio")
///     .base_currency(Currency::USD)
///     .as_of_date(Date::from_ymd(2025, 1, 15)?)
///     .add_holding(holding1)
///     .add_holding(holding2)
///     .add_cash(CashPosition::new(dec!(1_000_000), Currency::USD))
///     .build()?;
/// ```
#[derive(Debug, Clone, Default)]
pub struct PortfolioBuilder {
    id: Option<String>,
    name: Option<String>,
    base_currency: Currency,
    as_of_date: Option<Date>,
    holdings: Vec<Holding>,
    cash: Vec<CashPosition>,
    shares_outstanding: Option<Decimal>,
    liabilities: Option<Decimal>,
}

impl PortfolioBuilder {
    /// Creates a new portfolio builder with defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            base_currency: Currency::USD,
            ..Self::default()
        }
    }

    /// Sets the portfolio ID.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the portfolio name.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the base currency for reporting.
    #[must_use]
    pub fn base_currency(mut self, currency: Currency) -> Self {
        self.base_currency = currency;
        self
    }

    /// Sets the as-of date.
    #[must_use]
    pub fn as_of_date(mut self, date: Date) -> Self {
        self.as_of_date = Some(date);
        self
    }

    /// Adds a holding to the portfolio.
    #[must_use]
    pub fn add_holding(mut self, holding: Holding) -> Self {
        self.holdings.push(holding);
        self
    }

    /// Adds multiple holdings to the portfolio.
    #[must_use]
    pub fn add_holdings(mut self, holdings: impl IntoIterator<Item = Holding>) -> Self {
        self.holdings.extend(holdings);
        self
    }

    /// Sets all holdings (replacing any existing).
    #[must_use]
    pub fn holdings(mut self, holdings: Vec<Holding>) -> Self {
        self.holdings = holdings;
        self
    }

    /// Adds a cash position to the portfolio.
    #[must_use]
    pub fn add_cash(mut self, cash: CashPosition) -> Self {
        self.cash.push(cash);
        self
    }

    /// Adds multiple cash positions.
    #[must_use]
    pub fn add_cash_positions(mut self, positions: impl IntoIterator<Item = CashPosition>) -> Self {
        self.cash.extend(positions);
        self
    }

    /// Sets all cash positions (replacing any existing).
    #[must_use]
    pub fn cash(mut self, cash: Vec<CashPosition>) -> Self {
        self.cash = cash;
        self
    }

    /// Sets the shares outstanding (for ETF NAV calculation).
    #[must_use]
    pub fn shares_outstanding(mut self, shares: Decimal) -> Self {
        self.shares_outstanding = Some(shares);
        self
    }

    /// Sets the liabilities.
    #[must_use]
    pub fn liabilities(mut self, liabilities: Decimal) -> Self {
        self.liabilities = Some(liabilities);
        self
    }

    /// Builds the portfolio.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Required fields (name, as_of_date) are missing
    /// - Validation fails
    pub fn build(self) -> PortfolioResult<Portfolio> {
        let name = self
            .name
            .ok_or_else(|| PortfolioError::missing_field("name"))?;

        let as_of_date = self
            .as_of_date
            .ok_or_else(|| PortfolioError::missing_field("as_of_date"))?;

        // Generate ID from name if not provided
        let id = self.id.unwrap_or_else(|| {
            name.chars()
                .filter(|c| c.is_alphanumeric())
                .take(20)
                .collect::<String>()
                .to_uppercase()
        });

        let portfolio = Portfolio {
            id,
            name,
            base_currency: self.base_currency,
            as_of_date,
            holdings: self.holdings,
            cash: self.cash,
            shares_outstanding: self.shares_outstanding,
            liabilities: self.liabilities,
        };

        // Validate the portfolio
        portfolio.validate()?;

        Ok(portfolio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::HoldingAnalytics;
    use convex_bonds::types::BondIdentifiers;
    use rust_decimal_macros::dec;

    fn create_test_holding(id: &str) -> Holding {
        Holding::builder()
            .id(id)
            .identifiers(BondIdentifiers::from_isin_str("US912828Z229").unwrap())
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .analytics(HoldingAnalytics::new())
            .build()
            .unwrap()
    }

    #[test]
    fn test_basic_build() {
        let portfolio = PortfolioBuilder::new()
            .id("TEST")
            .name("Test Portfolio")
            .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
            .build()
            .unwrap();

        assert_eq!(portfolio.id, "TEST");
        assert_eq!(portfolio.name, "Test Portfolio");
        assert_eq!(portfolio.base_currency, Currency::USD);
        assert!(portfolio.holdings.is_empty());
    }

    #[test]
    fn test_with_holdings() {
        let portfolio = PortfolioBuilder::new()
            .name("Test")
            .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
            .add_holding(create_test_holding("BOND1"))
            .add_holding(create_test_holding("BOND2"))
            .build()
            .unwrap();

        assert_eq!(portfolio.holding_count(), 2);
    }

    #[test]
    fn test_add_holdings_batch() {
        let holdings = vec![
            create_test_holding("BOND1"),
            create_test_holding("BOND2"),
            create_test_holding("BOND3"),
        ];

        let portfolio = PortfolioBuilder::new()
            .name("Test")
            .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
            .add_holdings(holdings)
            .build()
            .unwrap();

        assert_eq!(portfolio.holding_count(), 3);
    }

    #[test]
    fn test_with_cash() {
        let portfolio = PortfolioBuilder::new()
            .name("Test")
            .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
            .add_cash(CashPosition::new(dec!(1_000_000), Currency::USD))
            .add_cash(CashPosition::with_fx_rate(
                dec!(500_000),
                Currency::EUR,
                dec!(1.10),
            ))
            .build()
            .unwrap();

        assert_eq!(portfolio.cash.len(), 2);
        assert_eq!(portfolio.total_cash(), dec!(1_550_000));
    }

    #[test]
    fn test_missing_name() {
        let result = PortfolioBuilder::new()
            .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
            .build();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("name"));
    }

    #[test]
    fn test_missing_date() {
        let result = PortfolioBuilder::new().name("Test").build();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("as_of_date"));
    }

    #[test]
    fn test_auto_generated_id() {
        let portfolio = PortfolioBuilder::new()
            .name("My Test Portfolio 123")
            .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
            .build()
            .unwrap();

        // ID should be alphanumeric, uppercase, max 20 chars
        assert_eq!(portfolio.id, "MYTESTPORTFOLIO123");
    }

    #[test]
    fn test_etf_fields() {
        let portfolio = PortfolioBuilder::new()
            .name("Test ETF")
            .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
            .shares_outstanding(dec!(10_000_000))
            .liabilities(dec!(50_000))
            .add_holding(create_test_holding("BOND1"))
            .add_cash(CashPosition::new(dec!(100_000), Currency::USD))
            .build()
            .unwrap();

        assert_eq!(portfolio.shares_outstanding, Some(dec!(10_000_000)));
        assert_eq!(portfolio.liabilities, Some(dec!(50_000)));

        // NAV = 1,000,000 (MV) + 0 (accrued) + 100,000 (cash) - 50,000 (liabilities)
        assert_eq!(portfolio.nav(), dec!(1_050_000));

        // NAV per share = 1,050,000 / 10,000,000 = 0.105
        assert_eq!(portfolio.nav_per_share(), Some(dec!(0.105)));
    }

    #[test]
    fn test_multi_currency() {
        let eur_holding = Holding::builder()
            .id("EUR_BOND")
            .identifiers(BondIdentifiers::from_isin_str("DE0001102481").unwrap())
            .par_amount(dec!(1_000_000))
            .market_price(dec!(100))
            .currency(Currency::EUR)
            .fx_rate(dec!(1.10))
            .analytics(HoldingAnalytics::new())
            .build()
            .unwrap();

        let portfolio = PortfolioBuilder::new()
            .name("Multi-Currency")
            .base_currency(Currency::USD)
            .as_of_date(Date::from_ymd(2025, 1, 15).unwrap())
            .add_holding(create_test_holding("USD_BOND"))
            .add_holding(eur_holding)
            .build()
            .unwrap();

        assert!(portfolio.is_multi_currency());
        assert_eq!(portfolio.currencies().len(), 2);

        // USD: 1,000,000
        // EUR: 1,000,000 × 1.10 = 1,100,000
        // Total: 2,100,000
        assert_eq!(portfolio.securities_market_value(), dec!(2_100_000));
    }
}
