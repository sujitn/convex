// =============================================================================
// FRED API Integration
// Federal Reserve Economic Data - Free API for Treasury rates and spreads
// https://fred.stlouisfed.org/docs/api/fred/
// =============================================================================

import { CurvePoint, YieldCurve, FREDSeriesResponse, Env } from './types';

// FRED Series IDs for Treasury rates
const TREASURY_SERIES: Record<string, { id: string; years: number }> = {
  '1M': { id: 'DGS1MO', years: 1 / 12 },
  '3M': { id: 'DGS3MO', years: 0.25 },
  '6M': { id: 'DGS6MO', years: 0.5 },
  '1Y': { id: 'DGS1', years: 1 },
  '2Y': { id: 'DGS2', years: 2 },
  '3Y': { id: 'DGS3', years: 3 },
  '5Y': { id: 'DGS5', years: 5 },
  '7Y': { id: 'DGS7', years: 7 },
  '10Y': { id: 'DGS10', years: 10 },
  '20Y': { id: 'DGS20', years: 20 },
  '30Y': { id: 'DGS30', years: 30 },
};

// FRED Series IDs for SOFR and other rates
const SOFR_SERIES = 'SOFR';
const SOFR_30D_AVG = 'SOFR30DAYAVG';
const SOFR_90D_AVG = 'SOFR90DAYAVG';
const SOFR_180D_AVG = 'SOFR180DAYAVG';

// Corporate spread series
const SPREAD_SERIES: Record<string, string> = {
  'AAA': 'BAMLC0A1CAAAEY', // ICE BofA AAA US Corporate Index Effective Yield
  'AA': 'BAMLC0A2CAAEY',   // ICE BofA AA US Corporate Index Effective Yield
  'A': 'BAMLC0A3CAEY',     // ICE BofA A US Corporate Index Effective Yield
  'BBB': 'BAMLC0A4CBBBEY', // ICE BofA BBB US Corporate Index Effective Yield
  'BB': 'BAMLH0A1HYBBEY',  // ICE BofA BB US High Yield Index Effective Yield
  'B': 'BAMLH0A2HYBEY',    // ICE BofA B US High Yield Index Effective Yield
  'CCC': 'BAMLH0A3HYCEY',  // ICE BofA CCC & Lower US High Yield Index
};

// OAS Spread series (Option-Adjusted Spread)
const OAS_SERIES: Record<string, string> = {
  'IG': 'BAMLC0A0CM',      // ICE BofA US Corporate Index OAS
  'HY': 'BAMLH0A0HYM2',    // ICE BofA US High Yield Index OAS
  'AAA': 'BAMLC0A1CAAA',   // ICE BofA AAA US Corporate Index OAS
  'BBB': 'BAMLC0A4CBBB',   // ICE BofA BBB US Corporate Index OAS
};

/**
 * Fetch a single FRED series
 */
async function fetchFREDSeries(
  seriesId: string,
  apiKey: string,
  limit: number = 1
): Promise<FREDSeriesResponse | null> {
  const url = new URL('https://api.stlouisfed.org/fred/series/observations');
  url.searchParams.set('series_id', seriesId);
  url.searchParams.set('api_key', apiKey);
  url.searchParams.set('file_type', 'json');
  url.searchParams.set('sort_order', 'desc');
  url.searchParams.set('limit', limit.toString());

  try {
    const response = await fetch(url.toString());
    if (!response.ok) {
      console.error(`FRED API error for ${seriesId}: ${response.status}`);
      return null;
    }
    return await response.json();
  } catch (error) {
    console.error(`Failed to fetch FRED series ${seriesId}:`, error);
    return null;
  }
}

/**
 * Get the latest value from a FRED series
 */
function getLatestValue(response: FREDSeriesResponse | null): number | null {
  if (!response || !response.observations || response.observations.length === 0) {
    return null;
  }

  // Find most recent non-empty value
  for (const obs of response.observations) {
    if (obs.value && obs.value !== '.') {
      return parseFloat(obs.value);
    }
  }
  return null;
}

/**
 * Fetch Treasury yield curve from FRED
 */
export async function fetchTreasuryCurve(env: Env): Promise<YieldCurve | null> {
  const apiKey = env.FRED_API_KEY;

  if (!apiKey) {
    console.log('No FRED API key configured, using fallback data');
    return getFallbackTreasuryCurve();
  }

  const points: CurvePoint[] = [];
  const today = new Date().toISOString().split('T')[0];

  // Fetch all Treasury rates in parallel
  const fetchPromises = Object.entries(TREASURY_SERIES).map(async ([tenor, config]) => {
    const data = await fetchFREDSeries(config.id, apiKey, 5);
    const rate = getLatestValue(data);

    if (rate !== null) {
      return {
        tenor,
        years: config.years,
        rate: rate / 100, // Convert from percentage to decimal
      };
    }
    return null;
  });

  const results = await Promise.all(fetchPromises);

  for (const result of results) {
    if (result) {
      points.push(result);
    }
  }

  // Sort by years
  points.sort((a, b) => a.years - b.years);

  if (points.length === 0) {
    return getFallbackTreasuryCurve();
  }

  return {
    id: 'USD_GOVT',
    name: 'US Treasury',
    currency: 'USD',
    as_of_date: today,
    source: 'FRED',
    points,
  };
}

/**
 * Fetch SOFR curve from FRED
 */
export async function fetchSOFRCurve(env: Env): Promise<YieldCurve | null> {
  const apiKey = env.FRED_API_KEY;

  if (!apiKey) {
    return getFallbackSOFRCurve();
  }

  const today = new Date().toISOString().split('T')[0];
  const points: CurvePoint[] = [];

  // Fetch SOFR rates
  const [sofr, sofr30d, sofr90d, sofr180d] = await Promise.all([
    fetchFREDSeries(SOFR_SERIES, apiKey, 5),
    fetchFREDSeries(SOFR_30D_AVG, apiKey, 5),
    fetchFREDSeries(SOFR_90D_AVG, apiKey, 5),
    fetchFREDSeries(SOFR_180D_AVG, apiKey, 5),
  ]);

  const sofrRate = getLatestValue(sofr);
  const sofr30dRate = getLatestValue(sofr30d);
  const sofr90dRate = getLatestValue(sofr90d);
  const sofr180dRate = getLatestValue(sofr180d);

  if (sofrRate !== null) {
    points.push({ tenor: 'ON', years: 1 / 365, rate: sofrRate / 100 });
  }
  if (sofr30dRate !== null) {
    points.push({ tenor: '1M', years: 1 / 12, rate: sofr30dRate / 100 });
  }
  if (sofr90dRate !== null) {
    points.push({ tenor: '3M', years: 0.25, rate: sofr90dRate / 100 });
  }
  if (sofr180dRate !== null) {
    points.push({ tenor: '6M', years: 0.5, rate: sofr180dRate / 100 });
  }

  if (points.length === 0) {
    return getFallbackSOFRCurve();
  }

  // For longer tenors, we'd need SOFR swap rates which require a different source
  // For now, extrapolate from Treasury curve with a small spread adjustment

  return {
    id: 'USD_SOFR',
    name: 'SOFR OIS',
    currency: 'USD',
    as_of_date: today,
    source: 'FRED',
    points,
  };
}

/**
 * Fetch corporate spread data from FRED
 */
export async function fetchCorporateSpreads(env: Env): Promise<Record<string, number>> {
  const apiKey = env.FRED_API_KEY;
  const spreads: Record<string, number> = {};

  if (!apiKey) {
    // Return fallback spreads
    return {
      'IG_OAS': 100,  // 100 bps
      'HY_OAS': 350,  // 350 bps
      'AAA_OAS': 50,
      'BBB_OAS': 130,
    };
  }

  // Fetch OAS spreads in parallel
  const fetchPromises = Object.entries(OAS_SERIES).map(async ([rating, seriesId]) => {
    const data = await fetchFREDSeries(seriesId, apiKey, 5);
    const spread = getLatestValue(data);
    return { rating, spread };
  });

  const results = await Promise.all(fetchPromises);

  for (const { rating, spread } of results) {
    if (spread !== null) {
      spreads[`${rating}_OAS`] = spread * 100; // Convert to bps
    }
  }

  return spreads;
}

/**
 * Build corporate IG curve from Treasury + spread
 */
export async function fetchCorporateIGCurve(
  treasuryCurve: YieldCurve,
  env: Env
): Promise<YieldCurve> {
  const spreads = await fetchCorporateSpreads(env);
  const igSpread = (spreads['IG_OAS'] || 100) / 10000; // Convert bps to decimal

  const today = new Date().toISOString().split('T')[0];

  return {
    id: 'USD_CORP_IG',
    name: 'Corporate IG',
    currency: 'USD',
    as_of_date: today,
    source: 'FRED (Treasury + OAS)',
    points: treasuryCurve.points.map(p => ({
      ...p,
      rate: p.rate + igSpread,
    })),
  };
}

/**
 * Build high yield curve from Treasury + spread
 */
export async function fetchHighYieldCurve(
  treasuryCurve: YieldCurve,
  env: Env
): Promise<YieldCurve> {
  const spreads = await fetchCorporateSpreads(env);
  const hySpread = (spreads['HY_OAS'] || 350) / 10000; // Convert bps to decimal

  const today = new Date().toISOString().split('T')[0];

  return {
    id: 'USD_CORP_HY',
    name: 'Corporate HY',
    currency: 'USD',
    as_of_date: today,
    source: 'FRED (Treasury + OAS)',
    points: treasuryCurve.points.map(p => ({
      ...p,
      rate: p.rate + hySpread,
    })),
  };
}

// =============================================================================
// Fallback Data (when API is unavailable)
// =============================================================================

function getFallbackTreasuryCurve(): YieldCurve {
  const today = new Date().toISOString().split('T')[0];

  return {
    id: 'USD_GOVT',
    name: 'US Treasury',
    currency: 'USD',
    as_of_date: today,
    source: 'Fallback',
    points: [
      { tenor: '1M', years: 1 / 12, rate: 0.0430 },
      { tenor: '3M', years: 0.25, rate: 0.0435 },
      { tenor: '6M', years: 0.5, rate: 0.0440 },
      { tenor: '1Y', years: 1, rate: 0.0425 },
      { tenor: '2Y', years: 2, rate: 0.0420 },
      { tenor: '3Y', years: 3, rate: 0.0415 },
      { tenor: '5Y', years: 5, rate: 0.0410 },
      { tenor: '7Y', years: 7, rate: 0.0415 },
      { tenor: '10Y', years: 10, rate: 0.0425 },
      { tenor: '20Y', years: 20, rate: 0.0455 },
      { tenor: '30Y', years: 30, rate: 0.0445 },
    ],
  };
}

function getFallbackSOFRCurve(): YieldCurve {
  const today = new Date().toISOString().split('T')[0];

  return {
    id: 'USD_SOFR',
    name: 'SOFR OIS',
    currency: 'USD',
    as_of_date: today,
    source: 'Fallback',
    points: [
      { tenor: 'ON', years: 1 / 365, rate: 0.0430 },
      { tenor: '1M', years: 1 / 12, rate: 0.0432 },
      { tenor: '3M', years: 0.25, rate: 0.0435 },
      { tenor: '6M', years: 0.5, rate: 0.0438 },
      { tenor: '1Y', years: 1, rate: 0.0420 },
      { tenor: '2Y', years: 2, rate: 0.0400 },
      { tenor: '3Y', years: 3, rate: 0.0385 },
      { tenor: '5Y', years: 5, rate: 0.0375 },
      { tenor: '7Y', years: 7, rate: 0.0375 },
      { tenor: '10Y', years: 10, rate: 0.0380 },
    ],
  };
}
