// =============================================================================
// ETF Holdings Data Provider
// Fetches ETF holdings from free sources (SEC EDGAR, iShares, etc.)
// =============================================================================

import {
  ETFHolding,
  ETFInfo,
  ETFHoldingsResponse,
  Env,
  NAVHistoryPoint,
  NAVHistoryResponse,
  CreationBasket,
  CreationBasketResponse,
  BasketComponent,
} from './types';
import { fetchISharesHoldings, getSupportedISharesETFs } from './ishares';

// Known ETF configurations
const ETF_CONFIG: Record<string, { issuer: string; fundType: string; creationUnitSize: number }> = {
  'LQD': { issuer: 'iShares', fundType: 'Investment Grade Corporate', creationUnitSize: 50000 },
  'HYG': { issuer: 'iShares', fundType: 'High Yield Corporate', creationUnitSize: 50000 },
  'TLT': { issuer: 'iShares', fundType: 'Long-Term Treasury', creationUnitSize: 50000 },
  'AGG': { issuer: 'iShares', fundType: 'Aggregate Bond', creationUnitSize: 100000 },
  'BND': { issuer: 'Vanguard', fundType: 'Total Bond Market', creationUnitSize: 100000 },
  'VCIT': { issuer: 'Vanguard', fundType: 'Intermediate Corporate', creationUnitSize: 50000 },
  'VCSH': { issuer: 'Vanguard', fundType: 'Short-Term Corporate', creationUnitSize: 50000 },
  'GOVT': { issuer: 'iShares', fundType: 'US Treasury', creationUnitSize: 50000 },
  'SHY': { issuer: 'iShares', fundType: 'Short Treasury', creationUnitSize: 50000 },
  'IEF': { issuer: 'iShares', fundType: 'Intermediate Treasury', creationUnitSize: 50000 },
};

/**
 * Fetch ETF holdings - tries iShares first, falls back to curated demo data
 */
export async function fetchETFHoldings(
  ticker: string,
  env: Env
): Promise<ETFHoldingsResponse | null> {
  const upperTicker = ticker.toUpperCase();

  const info = getETFInfo(upperTicker);
  if (!info) {
    return null;
  }

  const today = new Date().toISOString().split('T')[0];

  // Try fetching from iShares first (for supported ETFs)
  if (getSupportedISharesETFs().includes(upperTicker)) {
    try {
      const liveHoldings = await fetchISharesHoldings(upperTicker, env);
      if (liveHoldings && liveHoldings.length > 0) {
        return {
          etf: { ...info, holdings_count: liveHoldings.length },
          holdings: liveHoldings,
          as_of_date: today,
          source: 'iShares (Live)',
        };
      }
    } catch (error) {
      console.log(`iShares fetch failed for ${upperTicker}, using demo data`);
    }
  }

  // Fallback to curated demo data
  const holdings = getETFHoldings(upperTicker);
  if (!holdings) {
    return null;
  }

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
 * Also normalizes weights if they don't sum to ~1.0
 */
export function calculatePortfolioMetrics(holdings: ETFHolding[]): {
  weighted_duration: number;
  weighted_yield: number;
  weighted_coupon: number;
  total_market_value: number;
  total_weight: number;
  nav_per_share?: number;
} {
  let totalWeight = 0;
  let weightedYield = 0;
  let weightedCoupon = 0;
  let totalMarketValue = 0;
  let weightedDuration = 0;

  // First pass: calculate total weight
  for (const holding of holdings) {
    totalWeight += holding.weight;
    totalMarketValue += holding.market_value;
  }

  // Log for debugging
  console.log(`Portfolio metrics: ${holdings.length} holdings, totalWeight=${totalWeight}, totalMV=${totalMarketValue}`);

  // Normalize weights if they don't sum to approximately 1.0
  // iShares weights might be in percentage form (summing to ~100) or decimal form (summing to ~1)
  let weightMultiplier = 1;
  if (totalWeight > 50) {
    // Weights are in percentage form (e.g., 0.52 means 0.52%, sum to ~100)
    weightMultiplier = 1 / 100;
    console.log('Weights appear to be percentages, normalizing by /100');
  } else if (totalWeight < 0.5 && totalWeight > 0) {
    // Weights are too small (e.g., 0.0052 for 0.52%), need to scale up
    // This happens if weights were already divided by 100 incorrectly
    weightMultiplier = 100;
    console.log('Weights appear too small, normalizing by *100');
  }

  // Second pass: calculate weighted metrics with normalized weights
  let normalizedTotalWeight = 0;
  for (const holding of holdings) {
    const maturityDate = new Date(holding.maturity);
    const yearsToMaturity = Math.max(0, (maturityDate.getTime() - Date.now()) / (365.25 * 24 * 60 * 60 * 1000));

    // Simplified duration estimate
    const estimatedDuration = Math.min(yearsToMaturity * 0.85, yearsToMaturity);

    // Estimate yield from coupon and price
    const estimatedYield = holding.price && holding.price > 0
      ? (holding.coupon / holding.price) * 100
      : holding.coupon;

    const normalizedWeight = holding.weight * weightMultiplier;
    normalizedTotalWeight += normalizedWeight;

    weightedDuration += estimatedDuration * normalizedWeight;
    weightedYield += estimatedYield * normalizedWeight;
    weightedCoupon += holding.coupon * normalizedWeight;
  }

  if (normalizedTotalWeight > 0) {
    return {
      weighted_duration: weightedDuration / normalizedTotalWeight,
      weighted_yield: weightedYield / normalizedTotalWeight,
      weighted_coupon: weightedCoupon / normalizedTotalWeight,
      total_market_value: totalMarketValue,
      total_weight: normalizedTotalWeight,
    };
  }

  return {
    weighted_duration: 0,
    weighted_yield: 0,
    weighted_coupon: 0,
    total_market_value: 0,
    total_weight: 0,
  };
}

// =============================================================================
// NAV History Functions
// =============================================================================

/**
 * Generate synthetic NAV history for demo purposes
 * In production, this would fetch from Bloomberg, Refinitiv, or fund provider APIs
 */
export function generateNAVHistory(
  ticker: string,
  days: number = 30
): NAVHistoryResponse | null {
  const info = getETFInfo(ticker.toUpperCase());
  if (!info) return null;

  const history: NAVHistoryPoint[] = [];
  const baseNAV = info.nav;
  const baseVolume = 5000000; // 5M shares daily volume

  // Generate synthetic historical data with realistic patterns
  const today = new Date();
  let nav = baseNAV;

  for (let i = days; i >= 0; i--) {
    const date = new Date(today);
    date.setDate(date.getDate() - i);

    // Skip weekends
    if (date.getDay() === 0 || date.getDay() === 6) continue;

    // Random walk for NAV (typical daily vol ~0.3% for bond ETFs)
    const dailyReturn = (Math.random() - 0.5) * 0.006;
    nav = nav * (1 + dailyReturn);

    // Market price with small premium/discount
    const premiumDiscount = (Math.random() - 0.5) * 0.004; // +/- 0.2%
    const marketPrice = nav * (1 + premiumDiscount);

    // Volume with some randomness
    const volume = Math.round(baseVolume * (0.5 + Math.random()));

    history.push({
      date: date.toISOString().split('T')[0],
      nav: Math.round(nav * 100) / 100,
      inav: Math.round(nav * 100) / 100, // iNAV approximates NAV at end of day
      market_price: Math.round(marketPrice * 100) / 100,
      premium_discount: Math.round(premiumDiscount * 10000) / 100, // basis points to %
      volume,
      shares_outstanding: info.shares_outstanding,
    });
  }

  return {
    ticker: ticker.toUpperCase(),
    history,
    period: `${days}D`,
    source: 'Synthetic Demo Data',
  };
}

// =============================================================================
// Creation/Redemption Basket Functions
// =============================================================================

/**
 * Generate creation basket from holdings
 * A creation basket is the portfolio of securities needed to create new ETF shares
 */
export function generateCreationBasket(
  ticker: string,
  holdings: ETFHolding[]
): CreationBasketResponse | null {
  const upperTicker = ticker.toUpperCase();
  const config = ETF_CONFIG[upperTicker];
  const info = getETFInfo(upperTicker);

  if (!config || !info) return null;

  const creationUnitSize = config.creationUnitSize;
  const navPerShare = info.nav;
  const totalValue = navPerShare * creationUnitSize;

  // Calculate basket components proportional to holdings
  const components: BasketComponent[] = holdings
    .filter(h => h.weight > 0.001) // Only include material positions
    .slice(0, 100) // Limit to top 100 for demo
    .map(holding => {
      const componentValue = totalValue * holding.weight;
      const shares = Math.round(componentValue / (holding.price || 100));

      return {
        cusip: holding.cusip,
        isin: holding.isin,
        name: holding.description || holding.issuer,
        shares,
        weight: holding.weight,
        market_value: Math.round(componentValue * 100) / 100,
      };
    });

  // Calculate actual securities value from components
  const securitiesValue = components.reduce((sum, c) => sum + c.market_value, 0);

  // Cash component is the difference (for rounding, accrued interest, etc.)
  const cashComponent = Math.round((totalValue - securitiesValue) * 100) / 100;

  // Typical creation fee is ~$500-1500 per creation unit
  const estimatedExpenses = creationUnitSize <= 50000 ? 500 : 1000;

  const basket: CreationBasket = {
    etf_ticker: upperTicker,
    basket_date: new Date().toISOString().split('T')[0],
    creation_unit_size: creationUnitSize,
    cash_component: cashComponent,
    total_value: Math.round(totalValue * 100) / 100,
    nav_per_share: navPerShare,
    components,
    estimated_expenses: estimatedExpenses,
  };

  return {
    basket,
    as_of_date: new Date().toISOString().split('T')[0],
    source: 'Demo Data Provider',
  };
}

/**
 * Calculate arbitrage opportunity
 * Returns premium/discount and estimated profit/loss for creation/redemption
 */
export function calculateArbitrage(
  navPerShare: number,
  marketPrice: number,
  creationUnitSize: number,
  creationFee: number = 500
): {
  premium_discount_pct: number;
  premium_discount_bps: number;
  action: 'create' | 'redeem' | 'none';
  gross_profit: number;
  net_profit: number;
  profitable: boolean;
} {
  const premiumDiscountPct = ((marketPrice - navPerShare) / navPerShare) * 100;
  const premiumDiscountBps = premiumDiscountPct * 100;

  // Gross profit per creation unit
  const grossProfit = (marketPrice - navPerShare) * creationUnitSize;

  // Net profit after fees
  const netProfit = Math.abs(grossProfit) - creationFee;

  // Typical threshold is 10-20 bps to cover transaction costs
  const threshold = 15; // bps

  let action: 'create' | 'redeem' | 'none' = 'none';
  if (premiumDiscountBps > threshold) {
    action = 'create'; // Create ETF shares and sell at premium
  } else if (premiumDiscountBps < -threshold) {
    action = 'redeem'; // Buy ETF shares and redeem at NAV
  }

  return {
    premium_discount_pct: Math.round(premiumDiscountPct * 100) / 100,
    premium_discount_bps: Math.round(premiumDiscountBps),
    action,
    gross_profit: Math.round(grossProfit * 100) / 100,
    net_profit: Math.round(netProfit * 100) / 100,
    profitable: netProfit > 0 && action !== 'none',
  };
}
