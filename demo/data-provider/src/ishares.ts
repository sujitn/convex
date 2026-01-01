// =============================================================================
// iShares ETF Holdings Fetcher
// Fetches holdings from iShares website (CSV format)
// =============================================================================

import { ETFHolding, Env } from './types';

// iShares fund IDs and their product page paths
const ISHARES_FUNDS: Record<string, { productId: string; fundId: string }> = {
  'LQD': { productId: '239566', fundId: 'LQD' },
  'HYG': { productId: '239565', fundId: 'HYG' },
  'TLT': { productId: '239454', fundId: 'TLT' },
  'AGG': { productId: '239458', fundId: 'AGG' },
  'IEF': { productId: '239456', fundId: 'IEF' },
  'SHY': { productId: '239452', fundId: 'SHY' },
  'GOVT': { productId: '239468', fundId: 'GOVT' },
  'MBB': { productId: '239465', fundId: 'MBB' },
  'EMB': { productId: '239572', fundId: 'EMB' },
  'USIG': { productId: '239460', fundId: 'USIG' },
  'IGSB': { productId: '239451', fundId: 'IGSB' },
  'IGLB': { productId: '239423', fundId: 'IGLB' },
};

/**
 * Fetch ETF holdings from iShares website
 * iShares provides holdings as downloadable CSV files
 *
 * Note: In production, this would use proper API authentication.
 * For demo purposes, we attempt to fetch and parse the CSV,
 * falling back to demo data if unavailable.
 */
export async function fetchISharesHoldings(
  ticker: string,
  _env: Env
): Promise<ETFHolding[] | null> {
  const fundConfig = ISHARES_FUNDS[ticker.toUpperCase()];
  if (!fundConfig) {
    console.log(`iShares: ${ticker} not in supported funds list`);
    return null;
  }

  try {
    // iShares holdings CSV URL pattern
    // Note: This URL structure may change - in production, use official API
    const csvUrl = `https://www.ishares.com/us/products/${fundConfig.productId}/${fundConfig.fundId}/1467271812596.ajax?fileType=csv&fileName=${fundConfig.fundId}_holdings&dataType=fund`;

    console.log(`iShares: Fetching holdings for ${ticker} from ${csvUrl}`);

    const response = await fetch(csvUrl, {
      headers: {
        'User-Agent': 'ConvexDemo/1.0',
        'Accept': 'text/csv, application/csv, */*',
      },
    });

    if (!response.ok) {
      console.log(`iShares: HTTP ${response.status} for ${ticker}`);
      return null;
    }

    const csvText = await response.text();

    // iShares CSV has header rows before data
    const holdings = parseISharesCSV(csvText, ticker);

    if (holdings.length === 0) {
      console.log(`iShares: No holdings parsed for ${ticker}`);
      return null;
    }

    console.log(`iShares: Parsed ${holdings.length} holdings for ${ticker}`);
    return holdings;

  } catch (error) {
    console.error(`iShares: Error fetching ${ticker}:`, error);
    return null;
  }
}

/**
 * Parse iShares CSV format
 *
 * iShares CSV structure (varies by fund type):
 * - Header rows with fund info
 * - Column headers
 * - Data rows
 *
 * Common columns:
 * - Name, Ticker, CUSIP, ISIN, Sector, Asset Class
 * - Market Value, Weight (%), Notional Value
 * - Maturity, Coupon (%), YTM (%), Duration
 */
function parseISharesCSV(csvText: string, ticker: string): ETFHolding[] {
  const lines = csvText.split('\n');
  const holdings: ETFHolding[] = [];

  // Find the header row (contains "Name" or "Ticker")
  let headerIndex = -1;
  let headers: string[] = [];

  for (let i = 0; i < Math.min(lines.length, 20); i++) {
    const line = lines[i];
    if (line.includes('Name') && (line.includes('CUSIP') || line.includes('Ticker') || line.includes('Weight'))) {
      headerIndex = i;
      headers = parseCSVLine(line);
      break;
    }
  }

  if (headerIndex === -1) {
    console.log('iShares CSV: Could not find header row');
    return [];
  }

  // Build column index map
  const columnMap: Record<string, number> = {};
  headers.forEach((header, index) => {
    const normalized = header.toLowerCase().trim();
    columnMap[normalized] = index;
  });

  // Parse data rows
  for (let i = headerIndex + 1; i < lines.length; i++) {
    const line = lines[i].trim();
    if (!line || line.startsWith(',')) continue;

    const values = parseCSVLine(line);
    if (values.length < 3) continue;

    try {
      const holding = parseHoldingRow(values, columnMap, i - headerIndex, ticker);
      if (holding) {
        holdings.push(holding);
      }
    } catch (e) {
      // Skip malformed rows
      continue;
    }
  }

  return holdings;
}

/**
 * Parse a single CSV line, handling quoted values
 */
function parseCSVLine(line: string): string[] {
  const values: string[] = [];
  let current = '';
  let inQuotes = false;

  for (let i = 0; i < line.length; i++) {
    const char = line[i];

    if (char === '"') {
      inQuotes = !inQuotes;
    } else if (char === ',' && !inQuotes) {
      values.push(current.trim());
      current = '';
    } else {
      current += char;
    }
  }
  values.push(current.trim());

  return values;
}

/**
 * Parse a holding row from CSV values
 */
function parseHoldingRow(
  values: string[],
  columnMap: Record<string, number>,
  rowIndex: number,
  etfTicker: string
): ETFHolding | null {
  // Helper to get value by possible column names
  const getValue = (...names: string[]): string => {
    for (const name of names) {
      const idx = columnMap[name.toLowerCase()];
      if (idx !== undefined && values[idx]) {
        return values[idx].trim();
      }
    }
    return '';
  };

  const getNumber = (...names: string[]): number => {
    const val = getValue(...names).replace(/[,%$]/g, '');
    const num = parseFloat(val);
    return isNaN(num) ? 0 : num;
  };

  // Extract fields
  const name = getValue('name', 'security', 'issuer');
  const cusip = getValue('cusip');
  const isin = getValue('isin');
  const description = getValue('description', 'name');

  // Skip cash, derivatives, money market, and other non-bond entries
  const assetClass = getValue('asset class', 'assetclass').toLowerCase();
  const securityType = getValue('security type', 'securitytype', 'type').toLowerCase();
  const sectorRaw = getValue('sector').toLowerCase();

  if (
    assetClass.includes('cash') ||
    assetClass.includes('derivative') ||
    assetClass.includes('future') ||
    assetClass.includes('money market') ||
    securityType.includes('cash') ||
    securityType.includes('derivative') ||
    sectorRaw.includes('cash') ||
    sectorRaw.includes('derivative') ||
    name.toLowerCase().includes('cash') ||
    name.toLowerCase().includes('blackrock') ||
    name.toLowerCase().includes('blk csh fnd') ||
    cusip === '-' ||
    cusip === ''
  ) {
    return null;
  }

  const coupon = getNumber('coupon', 'coupon (%)', 'coupon rate', 'cpn');
  const maturity = getValue('maturity', 'maturity date', 'effective date');

  // Credit rating - iShares uses various column names
  // Note: iShares CSV typically doesn't include ratings, so we estimate
  const rawRating = getValue(
    'rating', 'credit rating',
    "moody's rating", 'moody rating', 'moodys rating',
    "s&p rating", 'sp rating', 's&p',
    'fitch rating', 'fitch',
    'composite rating', 'credit quality',
    'average credit quality', 'avg credit quality'
  );

  const sector = getValue('sector', 'industry', 'gics sector', 'bclass level 2', 'bclass level 3') || 'Other';
  const normalizedSector = normalizeSector(sector);

  // Get YTM for rating estimation
  const ytm = getNumber('ytm', 'ytm (%)', 'yield to maturity', 'yield');

  // Use provided rating or estimate from ETF type, sector, and spread
  const rating = rawRating
    ? normalizeRating(rawRating)
    : estimateRating(etfTicker, normalizedSector, ytm || undefined, name);

  // Weight handling - keep raw value, normalization happens in calculatePortfolioMetrics
  // iShares CSV formats vary: some have "0.52" meaning 0.52%, others may have different formats
  const weight = getNumber('weight', 'weight (%)', 'weight(%)', '% of net assets', 'market value weight');

  const marketValue = getNumber('market value', 'marketvalue', 'notional value', 'mv');
  const price = getNumber('price', 'market price', 'mid price');

  // For bond ETFs, "Par Value" is the notional in dollars, not number of shares
  // We need to calculate quantity from market value / price
  // This gives us the number of $100 face value units
  const parValue = getNumber('par value', 'notional', 'face amount', 'par/shares');

  // Calculate shares/quantity from market value and price
  // For bonds: quantity = market_value / price (where price is per $100 face)
  // Or if we have par value: quantity = par_value / 100 (convert to units)
  let shares: number;
  if (price && price > 0 && marketValue > 0) {
    // Best method: calculate from market value and price
    shares = Math.round(marketValue / price);
  } else if (parValue > 0) {
    // Fallback: use par value converted to units (assuming $100 face)
    shares = Math.round(parValue / 100);
  } else {
    shares = 0;
  }

  // Skip if missing critical fields
  if (!name || weight <= 0) {
    return null;
  }

  // Format maturity date
  let formattedMaturity = maturity;
  if (maturity && !maturity.includes('-')) {
    // Try to parse MM/DD/YYYY or other formats
    const parsed = new Date(maturity);
    if (!isNaN(parsed.getTime())) {
      formattedMaturity = parsed.toISOString().split('T')[0];
    }
  }

  return {
    id: `${rowIndex}`,
    cusip: cusip || `GEN${rowIndex}`,
    isin: isin || undefined,
    issuer: name.split(' ')[0] || name, // First word as issuer
    description: description || name,
    coupon,
    maturity: formattedMaturity || '2030-01-01',
    rating, // Already normalized or estimated
    sector: normalizedSector, // Already normalized
    weight,
    shares: shares || Math.round(marketValue / (price || 100)),
    market_value: marketValue,
    price: price || undefined,
  };
}

/**
 * Normalize credit rating to standard format
 */
function normalizeRating(rating: string): string {
  const upper = rating.toUpperCase().replace(/\s/g, '');

  // Already standard format
  if (/^(AAA|AA\+|AA|AA-|A\+|A|A-|BBB\+|BBB|BBB-|BB\+|BB|BB-|B\+|B|B-|CCC\+|CCC|CCC-|CC|C|D|NR)$/.test(upper)) {
    return upper;
  }

  // Moody's to S&P mapping
  const moodyMap: Record<string, string> = {
    'AAA': 'AAA',
    'AA1': 'AA+',
    'AA2': 'AA',
    'AA3': 'AA-',
    'A1': 'A+',
    'A2': 'A',
    'A3': 'A-',
    'BAA1': 'BBB+',
    'BAA2': 'BBB',
    'BAA3': 'BBB-',
    'BA1': 'BB+',
    'BA2': 'BB',
    'BA3': 'BB-',
    'B1': 'B+',
    'B2': 'B',
    'B3': 'B-',
    'CAA1': 'CCC+',
    'CAA2': 'CCC',
    'CAA3': 'CCC-',
    'CA': 'CC',
    'C': 'C',
  };

  return moodyMap[upper] || 'NR';
}

/**
 * Estimate credit rating from ETF type, sector, and spread
 * This is an APPROXIMATION since iShares doesn't provide ratings
 *
 * Based on:
 * - ETF type (IG vs HY vs Treasury)
 * - Sector (financials, utilities tend to be higher rated)
 * - YTM spread vs Treasury (wider = lower rating)
 */
function estimateRating(
  etfTicker: string,
  sector: string,
  ytm: number | undefined,
  issuerName: string
): string {
  const ticker = etfTicker.toUpperCase();
  const sectorLower = sector.toLowerCase();
  const issuerLower = issuerName.toLowerCase();

  // Treasury ETFs - all AAA
  if (['TLT', 'IEF', 'SHY', 'GOVT'].includes(ticker)) {
    return 'AAA';
  }

  // Check if issuer is US Government or Agency
  if (issuerLower.includes('treasury') || issuerLower.includes('u.s.') ||
      issuerLower.includes('united states') || issuerLower.includes('gnma') ||
      issuerLower.includes('fnma') || issuerLower.includes('fhlmc')) {
    return 'AAA';
  }

  // High Yield ETF - estimate BB range
  if (ticker === 'HYG') {
    if (!ytm) return 'BB';
    // Higher yield = lower rating
    if (ytm > 9) return 'CCC';
    if (ytm > 8) return 'B';
    if (ytm > 7) return 'B+';
    if (ytm > 6) return 'BB-';
    return 'BB';
  }

  // Investment Grade Corporate (LQD, USIG, IGSB, IGLB)
  // Estimate based on sector and spread
  if (['LQD', 'USIG', 'IGSB', 'IGLB'].includes(ticker)) {
    // Sector-based rating estimates for IG corporates
    // Tech giants (AAPL, MSFT, GOOG) tend to be AA+/AA
    if (sectorLower === 'technology') {
      if (issuerLower.includes('apple') || issuerLower.includes('microsoft') ||
          issuerLower.includes('google') || issuerLower.includes('alphabet')) {
        return 'AA+';
      }
      return 'A';
    }

    // Large banks are typically A- to A+
    if (sectorLower === 'financials') {
      if (issuerLower.includes('jpmorgan') || issuerLower.includes('bank of america') ||
          issuerLower.includes('wells fargo') || issuerLower.includes('goldman')) {
        return 'A-';
      }
      return 'BBB+';
    }

    // Utilities tend to be stable, BBB+ to A-
    if (sectorLower === 'utilities') return 'BBB+';

    // Energy varies widely
    if (sectorLower === 'energy') return 'BBB';

    // Consumer can vary
    if (sectorLower === 'consumer') {
      // Premium consumer brands
      if (issuerLower.includes('coca-cola') || issuerLower.includes('pepsi') ||
          issuerLower.includes('procter') || issuerLower.includes('johnson')) {
        return 'A+';
      }
      return 'BBB+';
    }

    // Healthcare/Pharma tend to be higher rated
    if (sectorLower === 'healthcare') return 'A-';

    // Communications varies
    if (sectorLower === 'communications') return 'BBB+';

    // Industrials
    if (sectorLower === 'industrials') return 'BBB';

    // If we have YTM, use spread-based estimation
    if (ytm) {
      // Current 10Y Treasury ~4.5%, so IG spread is ytm - 4.5
      const spread = (ytm - 4.5) * 100; // Convert to bps
      if (spread < 50) return 'AA-';
      if (spread < 80) return 'A+';
      if (spread < 110) return 'A';
      if (spread < 140) return 'A-';
      if (spread < 170) return 'BBB+';
      if (spread < 200) return 'BBB';
      return 'BBB-';
    }

    // Default for IG
    return 'BBB+';
  }

  // Aggregate Bond ETFs (AGG, BND) - mix of govt and corp
  if (['AGG', 'BND'].includes(ticker)) {
    if (sectorLower.includes('government') || sectorLower.includes('agency')) {
      return 'AAA';
    }
    return 'A'; // Default for aggregate
  }

  // Default fallback
  return 'BBB';
}

/**
 * Normalize sector names into ~8 broad categories for cleaner charts
 */
function normalizeSector(sector: string): string {
  const lower = sector.toLowerCase();

  // Technology
  if (lower.includes('tech')) return 'Technology';

  // Financials (including banking, insurance, brokers)
  if (lower.includes('financ') || lower.includes('bank') ||
      lower.includes('insurance') || lower.includes('brokerage') ||
      lower.includes('asset manager') || lower.includes('exchange')) return 'Financials';

  // Healthcare (including pharma)
  if (lower.includes('health') || lower.includes('pharma')) return 'Healthcare';

  // Energy
  if (lower.includes('energy') || lower.includes('oil') || lower.includes('gas')) return 'Energy';

  // Consumer (cyclical and non-cyclical)
  if (lower.includes('consumer') || lower.includes('retail') ||
      lower.includes('food') || lower.includes('beverage')) return 'Consumer';

  // Communications (telecom and media)
  if (lower.includes('telecom') || lower.includes('communication') ||
      lower.includes('media') || lower.includes('entertainment')) return 'Communications';

  // Industrials (including capital goods, transportation, basic industry)
  if (lower.includes('industrial') || lower.includes('manufacturing') ||
      lower.includes('capital goods') || lower.includes('transport') ||
      lower.includes('airline') || lower.includes('auto') ||
      lower.includes('basic industry') || lower.includes('material') ||
      lower.includes('mining')) return 'Industrials';

  // Utilities (including electric)
  if (lower.includes('utility') || lower.includes('utilities') ||
      lower.includes('electric') || lower.includes('power')) return 'Utilities';

  // Government & Agency
  if (lower.includes('government') || lower.includes('treasury') ||
      lower.includes('sovereign') || lower.includes('agency') ||
      lower.includes('mbs') || lower.includes('owned no guarantee')) return 'Government';

  // Real Estate
  if (lower.includes('real estate') || lower.includes('reit')) return 'Real Estate';

  return 'Other';
}

/**
 * Get list of supported iShares ETFs
 */
export function getSupportedISharesETFs(): string[] {
  return Object.keys(ISHARES_FUNDS);
}

/**
 * Debug function to fetch and analyze iShares CSV structure
 */
export async function debugISharesHoldings(
  ticker: string,
  _env: Env
): Promise<{
  success: boolean;
  csvUrl: string;
  headers: string[];
  sampleRows: string[][];
  firstParsedHoldings: Array<{
    name: string;
    cusip: string;
    weight: number;
    weight_column: string;
    rating: string;
    rating_column: string;
    market_value: number;
    price: number;
    shares: number;
  }>;
  totalWeight: number;
  holdingsCount: number;
} | null> {
  const fundConfig = ISHARES_FUNDS[ticker.toUpperCase()];
  if (!fundConfig) {
    return null;
  }

  try {
    const csvUrl = `https://www.ishares.com/us/products/${fundConfig.productId}/${fundConfig.fundId}/1467271812596.ajax?fileType=csv&fileName=${fundConfig.fundId}_holdings&dataType=fund`;

    const response = await fetch(csvUrl, {
      headers: {
        'User-Agent': 'ConvexDemo/1.0',
        'Accept': 'text/csv, application/csv, */*',
      },
    });

    if (!response.ok) {
      return {
        success: false,
        csvUrl,
        headers: [],
        sampleRows: [],
        firstParsedHoldings: [],
        totalWeight: 0,
        holdingsCount: 0,
      };
    }

    const csvText = await response.text();
    const lines = csvText.split('\n');

    // Find headers
    let headerIndex = -1;
    let headers: string[] = [];
    for (let i = 0; i < Math.min(lines.length, 20); i++) {
      const line = lines[i];
      if (line.includes('Name') && (line.includes('CUSIP') || line.includes('Ticker') || line.includes('Weight'))) {
        headerIndex = i;
        headers = parseCSVLineDebug(line);
        break;
      }
    }

    // Get sample data rows
    const sampleRows: string[][] = [];
    for (let i = headerIndex + 1; i < Math.min(headerIndex + 6, lines.length); i++) {
      const line = lines[i].trim();
      if (line && !line.startsWith(',')) {
        sampleRows.push(parseCSVLineDebug(line));
      }
    }

    // Parse first few holdings with debug info
    const columnMap: Record<string, number> = {};
    headers.forEach((header, index) => {
      columnMap[header.toLowerCase().trim()] = index;
    });

    const firstParsedHoldings: Array<{
      name: string;
      cusip: string;
      weight: number;
      weight_column: string;
      rating: string;
      rating_column: string;
      market_value: number;
      price: number;
      shares: number;
    }> = [];

    let totalWeight = 0;
    let holdingsCount = 0;

    for (let i = headerIndex + 1; i < lines.length && firstParsedHoldings.length < 10; i++) {
      const line = lines[i].trim();
      if (!line || line.startsWith(',')) continue;

      const values = parseCSVLineDebug(line);
      if (values.length < 3) continue;

      // Helper to get value
      const getValue = (...names: string[]): string => {
        for (const name of names) {
          const idx = columnMap[name.toLowerCase()];
          if (idx !== undefined && values[idx]) {
            return values[idx].trim();
          }
        }
        return '';
      };

      const getNumber = (...names: string[]): number => {
        const val = getValue(...names).replace(/[,%$]/g, '');
        const num = parseFloat(val);
        return isNaN(num) ? 0 : num;
      };

      // Find which columns have values
      const name = getValue('name', 'security', 'issuer');
      const cusip = getValue('cusip');

      // Find weight column
      let weightValue = 0;
      let weightColumn = '';
      const weightColumns = ['weight', 'weight (%)', 'weight(%)', '% of net assets', 'market value weight'];
      for (const col of weightColumns) {
        const idx = columnMap[col.toLowerCase()];
        if (idx !== undefined && values[idx]) {
          const val = parseFloat(values[idx].replace(/[,%$]/g, ''));
          if (!isNaN(val) && val > 0) {
            weightValue = val;
            weightColumn = col;
            break;
          }
        }
      }

      // Find rating column
      let ratingValue = 'NR';
      let ratingColumn = '';
      const ratingColumns = [
        'rating', 'credit rating',
        "moody's rating", 'moody rating', 'moodys rating',
        "s&p rating", 'sp rating', 's&p',
        'fitch rating', 'fitch',
        'composite rating', 'credit quality',
        'average credit quality', 'avg credit quality'
      ];
      for (const col of ratingColumns) {
        const idx = columnMap[col.toLowerCase()];
        if (idx !== undefined && values[idx] && values[idx].trim() !== '-' && values[idx].trim() !== '') {
          ratingValue = values[idx].trim();
          ratingColumn = col;
          break;
        }
      }

      const marketValue = getNumber('market value', 'marketvalue', 'notional value', 'mv');
      const price = getNumber('price', 'market price', 'mid price');
      const parValue = getNumber('par value', 'notional', 'face amount', 'par/shares');

      // Calculate shares correctly from market value / price
      let shares: number;
      if (price && price > 0 && marketValue > 0) {
        shares = Math.round(marketValue / price);
      } else if (parValue > 0) {
        shares = Math.round(parValue / 100);
      } else {
        shares = 0;
      }

      // Skip cash/derivatives/money market
      const assetClass = getValue('asset class', 'assetclass').toLowerCase();
      const sector = getValue('sector').toLowerCase();
      if (
        assetClass.includes('cash') ||
        assetClass.includes('derivative') ||
        assetClass.includes('money market') ||
        sector.includes('cash') ||
        sector.includes('derivative') ||
        name.toLowerCase().includes('blk csh fnd') ||
        cusip === '-' ||
        cusip === ''
      ) {
        continue;
      }

      totalWeight += weightValue;
      holdingsCount++;

      if (firstParsedHoldings.length < 10) {
        firstParsedHoldings.push({
          name,
          cusip,
          weight: weightValue,
          weight_column: weightColumn,
          rating: ratingValue,
          rating_column: ratingColumn,
          market_value: marketValue,
          price,
          shares,
        });
      }
    }

    return {
      success: true,
      csvUrl,
      headers,
      sampleRows,
      firstParsedHoldings,
      totalWeight,
      holdingsCount,
    };

  } catch (error) {
    console.error(`iShares debug error for ${ticker}:`, error);
    return null;
  }
}

function parseCSVLineDebug(line: string): string[] {
  const values: string[] = [];
  let current = '';
  let inQuotes = false;

  for (let i = 0; i < line.length; i++) {
    const char = line[i];

    if (char === '"') {
      inQuotes = !inQuotes;
    } else if (char === ',' && !inQuotes) {
      values.push(current.trim());
      current = '';
    } else {
      current += char;
    }
  }
  values.push(current.trim());

  return values;
}
