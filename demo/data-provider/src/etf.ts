// =============================================================================
// ETF Holdings Data Provider
// Fetches ETF holdings from free sources (SEC EDGAR, iShares, etc.)
// =============================================================================

import { ETFHolding, ETFInfo, ETFHoldingsResponse, Env } from './types';

// Known ETF configurations
const ETF_CONFIG: Record<string, { issuer: string; fundType: string }> = {
  'LQD': { issuer: 'iShares', fundType: 'Investment Grade Corporate' },
  'HYG': { issuer: 'iShares', fundType: 'High Yield Corporate' },
  'TLT': { issuer: 'iShares', fundType: 'Long-Term Treasury' },
  'AGG': { issuer: 'iShares', fundType: 'Aggregate Bond' },
  'BND': { issuer: 'Vanguard', fundType: 'Total Bond Market' },
  'VCIT': { issuer: 'Vanguard', fundType: 'Intermediate Corporate' },
  'VCSH': { issuer: 'Vanguard', fundType: 'Short-Term Corporate' },
  'GOVT': { issuer: 'iShares', fundType: 'US Treasury' },
  'SHY': { issuer: 'iShares', fundType: 'Short Treasury' },
  'IEF': { issuer: 'iShares', fundType: 'Intermediate Treasury' },
};

/**
 * Fetch ETF holdings - currently uses curated demo data
 * In production, this would integrate with SEC EDGAR or fund provider APIs
 */
export async function fetchETFHoldings(
  ticker: string,
  _env: Env
): Promise<ETFHoldingsResponse | null> {
  const upperTicker = ticker.toUpperCase();

  // Check if we have data for this ETF
  const holdings = getETFHoldings(upperTicker);
  if (!holdings) {
    return null;
  }

  const info = getETFInfo(upperTicker);
  if (!info) {
    return null;
  }

  const today = new Date().toISOString().split('T')[0];

  return {
    etf: info,
    holdings,
    as_of_date: today,
    source: 'Demo Data Provider',
  };
}

/**
 * Get list of available ETFs
 */
export function getAvailableETFs(): string[] {
  return Object.keys(ETF_CONFIG);
}

// =============================================================================
// ETF Info Data
// =============================================================================

function getETFInfo(ticker: string): ETFInfo | null {
  const config = ETF_CONFIG[ticker];
  if (!config) return null;

  const infoMap: Record<string, ETFInfo> = {
    'LQD': {
      ticker: 'LQD',
      name: 'iShares iBoxx $ Investment Grade Corporate Bond ETF',
      description: 'Tracks an index of US investment grade corporate bonds',
      issuer: 'iShares',
      inception_date: '2002-07-22',
      expense_ratio: 0.14,
      aum: 32.98e9,
      shares_outstanding: 298500000,
      nav: 110.45,
      holdings_count: 2500,
    },
    'HYG': {
      ticker: 'HYG',
      name: 'iShares iBoxx $ High Yield Corporate Bond ETF',
      description: 'Tracks an index of US high yield corporate bonds',
      issuer: 'iShares',
      inception_date: '2007-04-04',
      expense_ratio: 0.48,
      aum: 18.39e9,
      shares_outstanding: 228300000,
      nav: 80.41,
      holdings_count: 1200,
    },
    'TLT': {
      ticker: 'TLT',
      name: 'iShares 20+ Year Treasury Bond ETF',
      description: 'Tracks an index of US Treasury bonds with 20+ years to maturity',
      issuer: 'iShares',
      inception_date: '2002-07-22',
      expense_ratio: 0.15,
      aum: 48.18e9,
      shares_outstanding: 548600000,
      nav: 87.84,
      holdings_count: 45,
    },
    'AGG': {
      ticker: 'AGG',
      name: 'iShares Core U.S. Aggregate Bond ETF',
      description: 'Tracks the Bloomberg US Aggregate Bond Index',
      issuer: 'iShares',
      inception_date: '2003-09-22',
      expense_ratio: 0.03,
      aum: 98.5e9,
      shares_outstanding: 980000000,
      nav: 100.52,
      holdings_count: 11000,
    },
    'BND': {
      ticker: 'BND',
      name: 'Vanguard Total Bond Market ETF',
      description: 'Tracks the Bloomberg US Aggregate Float Adjusted Index',
      issuer: 'Vanguard',
      inception_date: '2007-04-03',
      expense_ratio: 0.03,
      aum: 105.2e9,
      shares_outstanding: 1350000000,
      nav: 77.93,
      holdings_count: 10500,
    },
  };

  return infoMap[ticker] || null;
}

// =============================================================================
// ETF Holdings Data
// Representative holdings for demo purposes
// =============================================================================

function getETFHoldings(ticker: string): ETFHolding[] | null {
  const holdingsMap: Record<string, ETFHolding[]> = {
    'LQD': [
      { id: '1', cusip: '037833AK6', isin: 'US037833AK68', issuer: 'Apple Inc', description: 'AAPL 4.65% 02/23/2046', coupon: 4.65, maturity: '2046-02-23', rating: 'AA+', sector: 'Technology', weight: 0.52, shares: 15000, market_value: 1386750, price: 92.45 },
      { id: '2', cusip: '594918BG8', isin: 'US594918BG81', issuer: 'Microsoft Corp', description: 'MSFT 3.50% 02/12/2042', coupon: 3.50, maturity: '2042-02-12', rating: 'AAA', sector: 'Technology', weight: 0.48, shares: 14500, market_value: 1277740, price: 88.12 },
      { id: '3', cusip: '46625HJE5', isin: 'US46625HJE53', issuer: 'JPMorgan Chase', description: 'JPM 5.25% 07/15/2034', coupon: 5.25, maturity: '2034-07-15', rating: 'A-', sector: 'Financials', weight: 0.45, shares: 12000, market_value: 1198200, price: 99.85 },
      { id: '4', cusip: '92343VEP1', isin: 'US92343VEP13', issuer: 'Verizon', description: 'VZ 4.50% 08/10/2033', coupon: 4.50, maturity: '2033-08-10', rating: 'BBB+', sector: 'Telecom', weight: 0.42, shares: 11800, market_value: 1123714, price: 95.23 },
      { id: '5', cusip: '30231GAV4', isin: 'US30231GAV41', issuer: 'Exxon Mobil', description: 'XOM 4.23% 03/01/2039', coupon: 4.23, maturity: '2039-03-01', rating: 'AA-', sector: 'Energy', weight: 0.40, shares: 11600, market_value: 1063372, price: 91.67 },
      { id: '6', cusip: '126650CZ6', isin: 'US126650CZ62', issuer: 'CVS Health', description: 'CVS 5.05% 03/25/2048', coupon: 5.05, maturity: '2048-03-25', rating: 'BBB', sector: 'Healthcare', weight: 0.38, shares: 11300, market_value: 1009542, price: 89.34 },
      { id: '7', cusip: '00206RCJ3', isin: 'US00206RCJ32', issuer: 'AT&T Inc', description: 'T 4.85% 03/01/2044', coupon: 4.85, maturity: '2044-03-01', rating: 'BBB', sector: 'Telecom', weight: 0.35, shares: 10600, market_value: 928136, price: 87.56 },
      { id: '8', cusip: '084670BR5', isin: 'US084670BR54', issuer: 'Berkshire Hathaway', description: 'BRK 3.85% 03/15/2052', coupon: 3.85, maturity: '2052-03-15', rating: 'AA', sector: 'Financials', weight: 0.33, shares: 11100, market_value: 878232, price: 79.12 },
      { id: '9', cusip: '20030NCQ0', isin: 'US20030NCQ02', issuer: 'Comcast Corp', description: 'CMCSA 4.15% 10/15/2038', coupon: 4.15, maturity: '2038-10-15', rating: 'A-', sector: 'Media', weight: 0.31, shares: 9100, market_value: 823095, price: 90.45 },
      { id: '10', cusip: '713448EK1', isin: 'US713448EK11', issuer: 'PepsiCo Inc', description: 'PEP 3.90% 07/18/2032', coupon: 3.90, maturity: '2032-07-18', rating: 'A+', sector: 'Consumer', weight: 0.29, shares: 8000, market_value: 774240, price: 96.78 },
      { id: '11', cusip: '459200JN3', isin: 'US459200JN39', issuer: 'IBM Corp', description: 'IBM 4.00% 06/20/2042', coupon: 4.00, maturity: '2042-06-20', rating: 'A-', sector: 'Technology', weight: 0.28, shares: 8500, market_value: 748225, price: 88.03 },
      { id: '12', cusip: '931142EK1', isin: 'US931142EK19', issuer: 'Walmart Inc', description: 'WMT 4.05% 06/29/2048', coupon: 4.05, maturity: '2048-06-29', rating: 'AA', sector: 'Consumer', weight: 0.26, shares: 7800, market_value: 695760, price: 89.20 },
    ],
    'HYG': [
      { id: '1', cusip: '345370CQ2', isin: 'US345370CQ23', issuer: 'Ford Motor Co', description: 'F 6.10% 08/19/2032', coupon: 6.10, maturity: '2032-08-19', rating: 'BB+', sector: 'Autos', weight: 0.65, shares: 18000, market_value: 1696500, price: 94.25 },
      { id: '2', cusip: '172967LS8', isin: 'US172967LS85', issuer: 'Occidental Petroleum', description: 'OXY 6.45% 09/15/2036', coupon: 6.45, maturity: '2036-09-15', rating: 'BB', sector: 'Energy', weight: 0.58, shares: 15800, market_value: 1518696, price: 96.12 },
      { id: '3', cusip: '29273VAM5', isin: 'US29273VAM55', issuer: 'Carnival Corp', description: 'CCL 7.00% 08/15/2030', coupon: 7.00, maturity: '2030-08-15', rating: 'B+', sector: 'Leisure', weight: 0.52, shares: 13700, market_value: 1362465, price: 99.45 },
      { id: '4', cusip: '902494BL5', isin: 'US902494BL59', issuer: 'T-Mobile USA', description: 'TMUS 5.75% 01/15/2034', coupon: 5.75, maturity: '2034-01-15', rating: 'BB+', sector: 'Telecom', weight: 0.48, shares: 12900, market_value: 1255686, price: 97.34 },
      { id: '5', cusip: '254687FS3', isin: 'US254687FS30', issuer: 'Dish Network', description: 'DISH 7.75% 07/01/2027', coupon: 7.75, maturity: '2027-07-01', rating: 'CCC+', sector: 'Media', weight: 0.42, shares: 15200, market_value: 1102000, price: 72.50 },
      { id: '6', cusip: '00287YAZ3', isin: 'US00287YAZ34', issuer: 'AbbVie Inc', description: 'ABBV 5.40% 03/15/2054', coupon: 5.40, maturity: '2054-03-15', rating: 'BBB+', sector: 'Healthcare', weight: 0.40, shares: 11500, market_value: 1058500, price: 92.04 },
      { id: '7', cusip: '136375BN7', isin: 'US136375BN75', issuer: 'Canadian Natural', description: 'CNQ 6.25% 03/15/2038', coupon: 6.25, maturity: '2038-03-15', rating: 'BBB', sector: 'Energy', weight: 0.38, shares: 10200, market_value: 989400, price: 97.00 },
      { id: '8', cusip: '532457BM3', isin: 'US532457BM30', issuer: 'Liberty Media', description: 'LMCA 6.875% 08/15/2029', coupon: 6.875, maturity: '2029-08-15', rating: 'BB-', sector: 'Media', weight: 0.35, shares: 9500, market_value: 921250, price: 96.97 },
    ],
    'TLT': [
      { id: '1', cusip: '912810TN8', isin: 'US912810TN81', issuer: 'US Treasury', description: 'T 1.875% 02/15/2051', coupon: 1.875, maturity: '2051-02-15', rating: 'AAA', sector: 'Government', weight: 4.85, shares: 205000, market_value: 12802250, price: 62.45 },
      { id: '2', cusip: '912810TR9', isin: 'US912810TR96', issuer: 'US Treasury', description: 'T 2.25% 02/15/2052', coupon: 2.25, maturity: '2052-02-15', rating: 'AAA', sector: 'Government', weight: 4.62, shares: 183000, market_value: 12220740, price: 66.78 },
      { id: '3', cusip: '912810TT5', isin: 'US912810TT54', issuer: 'US Treasury', description: 'T 3.00% 08/15/2052', coupon: 3.00, maturity: '2052-08-15', rating: 'AAA', sector: 'Government', weight: 4.51, shares: 163000, market_value: 11939750, price: 73.25 },
      { id: '4', cusip: '912810TW8', isin: 'US912810TW83', issuer: 'US Treasury', description: 'T 3.625% 02/15/2053', coupon: 3.625, maturity: '2053-02-15', rating: 'AAA', sector: 'Government', weight: 4.38, shares: 147000, market_value: 11630640, price: 79.12 },
      { id: '5', cusip: '912810TY4', isin: 'US912810TY40', issuer: 'US Treasury', description: 'T 3.875% 08/15/2043', coupon: 3.875, maturity: '2043-08-15', rating: 'AAA', sector: 'Government', weight: 4.25, shares: 132000, market_value: 11308440, price: 85.67 },
      { id: '6', cusip: '912810TA6', isin: 'US912810TA60', issuer: 'US Treasury', description: 'T 4.00% 11/15/2042', coupon: 4.00, maturity: '2042-11-15', rating: 'AAA', sector: 'Government', weight: 4.12, shares: 125000, market_value: 10912500, price: 87.30 },
      { id: '7', cusip: '912810TB4', isin: 'US912810TB44', issuer: 'US Treasury', description: 'T 4.25% 05/15/2044', coupon: 4.25, maturity: '2044-05-15', rating: 'AAA', sector: 'Government', weight: 3.98, shares: 118000, market_value: 10502000, price: 89.00 },
      { id: '8', cusip: '912810TC2', isin: 'US912810TC27', issuer: 'US Treasury', description: 'T 4.50% 02/15/2045', coupon: 4.50, maturity: '2045-02-15', rating: 'AAA', sector: 'Government', weight: 3.85, shares: 112000, market_value: 10192000, price: 91.00 },
    ],
    'AGG': [
      { id: '1', cusip: '912810TN8', isin: 'US912810TN81', issuer: 'US Treasury', description: 'T 1.875% 02/15/2051', coupon: 1.875, maturity: '2051-02-15', rating: 'AAA', sector: 'Government', weight: 2.15, shares: 95000, market_value: 5932750, price: 62.45 },
      { id: '2', cusip: '3135G0V34', isin: 'US3135G0V349', issuer: 'Fannie Mae', description: 'FNMA 3.00% 01/01/2052', coupon: 3.00, maturity: '2052-01-01', rating: 'AA+', sector: 'Agency MBS', weight: 1.85, shares: 82000, market_value: 4879000, price: 59.50 },
      { id: '3', cusip: '3137EAEL8', isin: 'US3137EAEL87', issuer: 'Freddie Mac', description: 'FHLMC 2.50% 05/01/2051', coupon: 2.50, maturity: '2051-05-01', rating: 'AA+', sector: 'Agency MBS', weight: 1.65, shares: 75000, market_value: 4312500, price: 57.50 },
      { id: '4', cusip: '037833AK6', isin: 'US037833AK68', issuer: 'Apple Inc', description: 'AAPL 4.65% 02/23/2046', coupon: 4.65, maturity: '2046-02-23', rating: 'AA+', sector: 'Technology', weight: 0.35, shares: 12000, market_value: 1109400, price: 92.45 },
      { id: '5', cusip: '594918BG8', isin: 'US594918BG81', issuer: 'Microsoft Corp', description: 'MSFT 3.50% 02/12/2042', coupon: 3.50, maturity: '2042-02-12', rating: 'AAA', sector: 'Technology', weight: 0.32, shares: 11000, market_value: 969320, price: 88.12 },
    ],
    'BND': [
      { id: '1', cusip: '912810TN8', isin: 'US912810TN81', issuer: 'US Treasury', description: 'T 1.875% 02/15/2051', coupon: 1.875, maturity: '2051-02-15', rating: 'AAA', sector: 'Government', weight: 2.25, shares: 100000, market_value: 6245000, price: 62.45 },
      { id: '2', cusip: '3135G0V34', isin: 'US3135G0V349', issuer: 'Fannie Mae', description: 'FNMA 3.00% 01/01/2052', coupon: 3.00, maturity: '2052-01-01', rating: 'AA+', sector: 'Agency MBS', weight: 1.95, shares: 87000, market_value: 5176500, price: 59.50 },
      { id: '3', cusip: '3137EAEL8', isin: 'US3137EAEL87', issuer: 'Freddie Mac', description: 'FHLMC 2.50% 05/01/2051', coupon: 2.50, maturity: '2051-05-01', rating: 'AA+', sector: 'Agency MBS', weight: 1.75, shares: 80000, market_value: 4600000, price: 57.50 },
      { id: '4', cusip: '46625HJE5', isin: 'US46625HJE53', issuer: 'JPMorgan Chase', description: 'JPM 5.25% 07/15/2034', coupon: 5.25, maturity: '2034-07-15', rating: 'A-', sector: 'Financials', weight: 0.28, shares: 9500, market_value: 948575, price: 99.85 },
    ],
  };

  return holdingsMap[ticker] || null;
}

/**
 * Calculate portfolio metrics from holdings
 */
export function calculatePortfolioMetrics(holdings: ETFHolding[]): {
  weighted_duration: number;
  weighted_yield: number;
  weighted_coupon: number;
  total_market_value: number;
} {
  let totalWeight = 0;
  let weightedYield = 0;
  let weightedCoupon = 0;
  let totalMarketValue = 0;

  // Estimate duration from maturity (simplified)
  let weightedDuration = 0;

  for (const holding of holdings) {
    const maturityDate = new Date(holding.maturity);
    const yearsToMaturity = (maturityDate.getTime() - Date.now()) / (365.25 * 24 * 60 * 60 * 1000);

    // Simplified duration estimate (actual would use modified duration)
    const estimatedDuration = Math.min(yearsToMaturity * 0.85, yearsToMaturity);

    // Estimate yield from coupon and price
    const estimatedYield = (holding.coupon / (holding.price || 100)) * 100;

    totalWeight += holding.weight;
    weightedDuration += estimatedDuration * holding.weight;
    weightedYield += estimatedYield * holding.weight;
    weightedCoupon += holding.coupon * holding.weight;
    totalMarketValue += holding.market_value;
  }

  if (totalWeight > 0) {
    return {
      weighted_duration: weightedDuration / totalWeight,
      weighted_yield: weightedYield / totalWeight,
      weighted_coupon: weightedCoupon / totalWeight,
      total_market_value: totalMarketValue,
    };
  }

  return {
    weighted_duration: 0,
    weighted_yield: 0,
    weighted_coupon: 0,
    total_market_value: 0,
  };
}
