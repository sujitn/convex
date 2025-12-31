// =============================================================================
// Convex Demo Data Provider
// Cloudflare Worker that provides market data from free sources
// =============================================================================

import { Env, MarketDataResponse, YieldCurve } from './types';
import {
  fetchTreasuryCurve,
  fetchSOFRCurve,
  fetchCorporateIGCurve,
  fetchHighYieldCurve,
  fetchCorporateSpreads,
} from './fred';
import {
  fetchETFHoldings,
  getAvailableETFs,
  calculatePortfolioMetrics,
  generateNAVHistory,
  generateCreationBasket,
  calculateArbitrage,
} from './etf';
import { debugISharesHoldings } from './ishares';

// CORS headers for cross-origin requests
const corsHeaders = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
  'Access-Control-Allow-Headers': 'Content-Type, Authorization',
  'Access-Control-Max-Age': '86400',
};

// Cache duration in seconds
const CACHE_DURATION = 300; // 5 minutes

/**
 * Main worker entry point
 */
export default {
  async fetch(request: Request, env: Env, _ctx: ExecutionContext): Promise<Response> {
    // Handle CORS preflight
    if (request.method === 'OPTIONS') {
      return new Response(null, { headers: corsHeaders });
    }

    const url = new URL(request.url);
    const path = url.pathname;

    try {
      // Route requests
      if (path === '/' || path === '/health') {
        return jsonResponse({
          status: 'ok',
          service: 'convex-demo-data-provider',
          version: '1.0.0',
          timestamp: new Date().toISOString(),
          endpoints: [
            '/api/curves',
            '/api/curves/treasury',
            '/api/curves/sofr',
            '/api/curves/corporate-ig',
            '/api/curves/corporate-hy',
            '/api/spreads',
            '/api/etf/list',
            '/api/etf/:ticker',
            '/api/etf/:ticker/holdings',
            '/api/etf/:ticker/nav-history',
            '/api/etf/:ticker/basket',
            '/api/etf/:ticker/arbitrage',
            '/api/etf/:ticker/debug',
            '/api/market-data',
          ],
        });
      }

      // Curves endpoints
      if (path === '/api/curves' || path === '/api/market-data') {
        return await handleMarketData(env);
      }

      if (path === '/api/curves/treasury') {
        return await handleTreasuryCurve(env);
      }

      if (path === '/api/curves/sofr') {
        return await handleSOFRCurve(env);
      }

      if (path === '/api/curves/corporate-ig') {
        return await handleCorporateIGCurve(env);
      }

      if (path === '/api/curves/corporate-hy') {
        return await handleHighYieldCurve(env);
      }

      // Spreads endpoint
      if (path === '/api/spreads') {
        return await handleSpreads(env);
      }

      // ETF endpoints
      if (path === '/api/etf/list') {
        return handleETFList();
      }

      // ETF NAV history: /api/etf/:ticker/nav-history
      const navHistoryMatch = path.match(/^\/api\/etf\/([A-Za-z]+)\/nav-history$/);
      if (navHistoryMatch) {
        const ticker = navHistoryMatch[1].toUpperCase();
        const daysParam = url.searchParams.get('days');
        const days = daysParam ? parseInt(daysParam, 10) : 30;
        return handleNAVHistory(ticker, days);
      }

      // ETF creation basket: /api/etf/:ticker/basket
      const basketMatch = path.match(/^\/api\/etf\/([A-Za-z]+)\/basket$/);
      if (basketMatch) {
        const ticker = basketMatch[1].toUpperCase();
        return await handleCreationBasket(ticker, env);
      }

      // ETF arbitrage: /api/etf/:ticker/arbitrage
      const arbMatch = path.match(/^\/api\/etf\/([A-Za-z]+)\/arbitrage$/);
      if (arbMatch) {
        const ticker = arbMatch[1].toUpperCase();
        const marketPriceParam = url.searchParams.get('market_price');
        return await handleArbitrage(ticker, marketPriceParam, env);
      }

      // ETF debug: /api/etf/:ticker/debug - show raw parsing info
      const debugMatch = path.match(/^\/api\/etf\/([A-Za-z]+)\/debug$/);
      if (debugMatch) {
        const ticker = debugMatch[1].toUpperCase();
        return await handleETFDebug(ticker, env);
      }

      // ETF holdings: /api/etf/:ticker or /api/etf/:ticker/holdings
      const etfMatch = path.match(/^\/api\/etf\/([A-Za-z]+)(\/holdings)?$/);
      if (etfMatch) {
        const ticker = etfMatch[1].toUpperCase();
        return await handleETFHoldings(ticker, env);
      }

      // 404 for unknown routes
      return jsonResponse({ error: 'Not found', path }, 404);

    } catch (error) {
      console.error('Request error:', error);
      return jsonResponse(
        { error: 'Internal server error', message: String(error) },
        500
      );
    }
  },
};

// =============================================================================
// Route Handlers
// =============================================================================

/**
 * Get all market data (curves)
 */
async function handleMarketData(env: Env): Promise<Response> {
  const treasuryCurve = await fetchTreasuryCurve(env);
  const sofrCurve = await fetchSOFRCurve(env);

  const curves: YieldCurve[] = [];

  if (treasuryCurve) {
    curves.push(treasuryCurve);

    // Build corporate curves from Treasury + spread
    const corpIGCurve = await fetchCorporateIGCurve(treasuryCurve, env);
    const corpHYCurve = await fetchHighYieldCurve(treasuryCurve, env);
    curves.push(corpIGCurve);
    curves.push(corpHYCurve);
  }

  if (sofrCurve) {
    curves.push(sofrCurve);
  }

  const response: MarketDataResponse = {
    curves,
    last_updated: new Date().toISOString(),
    source: env.FRED_API_KEY ? 'FRED API' : 'Fallback Data',
  };

  return jsonResponse(response, 200, CACHE_DURATION);
}

/**
 * Get Treasury curve
 */
async function handleTreasuryCurve(env: Env): Promise<Response> {
  const curve = await fetchTreasuryCurve(env);

  if (!curve) {
    return jsonResponse({ error: 'Failed to fetch Treasury curve' }, 500);
  }

  return jsonResponse(curve, 200, CACHE_DURATION);
}

/**
 * Get SOFR curve
 */
async function handleSOFRCurve(env: Env): Promise<Response> {
  const curve = await fetchSOFRCurve(env);

  if (!curve) {
    return jsonResponse({ error: 'Failed to fetch SOFR curve' }, 500);
  }

  return jsonResponse(curve, 200, CACHE_DURATION);
}

/**
 * Get Corporate IG curve
 */
async function handleCorporateIGCurve(env: Env): Promise<Response> {
  const treasuryCurve = await fetchTreasuryCurve(env);

  if (!treasuryCurve) {
    return jsonResponse({ error: 'Failed to fetch base Treasury curve' }, 500);
  }

  const curve = await fetchCorporateIGCurve(treasuryCurve, env);
  return jsonResponse(curve, 200, CACHE_DURATION);
}

/**
 * Get High Yield curve
 */
async function handleHighYieldCurve(env: Env): Promise<Response> {
  const treasuryCurve = await fetchTreasuryCurve(env);

  if (!treasuryCurve) {
    return jsonResponse({ error: 'Failed to fetch base Treasury curve' }, 500);
  }

  const curve = await fetchHighYieldCurve(treasuryCurve, env);
  return jsonResponse(curve, 200, CACHE_DURATION);
}

/**
 * Get corporate spreads
 */
async function handleSpreads(env: Env): Promise<Response> {
  const spreads = await fetchCorporateSpreads(env);

  return jsonResponse({
    spreads,
    as_of_date: new Date().toISOString().split('T')[0],
    source: env.FRED_API_KEY ? 'FRED API' : 'Fallback Data',
    description: 'Option-Adjusted Spreads (OAS) in basis points',
  }, 200, CACHE_DURATION);
}

/**
 * Get list of available ETFs
 */
function handleETFList(): Response {
  const etfs = getAvailableETFs();

  return jsonResponse({
    etfs,
    count: etfs.length,
    source: 'Demo Data Provider',
  });
}

/**
 * Get ETF holdings
 */
async function handleETFHoldings(ticker: string, env: Env): Promise<Response> {
  const data = await fetchETFHoldings(ticker, env);

  if (!data) {
    return jsonResponse(
      {
        error: 'ETF not found',
        ticker,
        available_etfs: getAvailableETFs(),
      },
      404
    );
  }

  // Add calculated metrics
  const metrics = calculatePortfolioMetrics(data.holdings);

  return jsonResponse({
    ...data,
    metrics,
  }, 200, CACHE_DURATION);
}

/**
 * Get ETF NAV history
 */
function handleNAVHistory(ticker: string, days: number): Response {
  const data = generateNAVHistory(ticker, days);

  if (!data) {
    return jsonResponse(
      {
        error: 'ETF not found',
        ticker,
        available_etfs: getAvailableETFs(),
      },
      404
    );
  }

  return jsonResponse(data, 200, CACHE_DURATION);
}

/**
 * Get ETF creation basket
 */
async function handleCreationBasket(ticker: string, env: Env): Promise<Response> {
  const holdingsData = await fetchETFHoldings(ticker, env);

  if (!holdingsData) {
    return jsonResponse(
      {
        error: 'ETF not found',
        ticker,
        available_etfs: getAvailableETFs(),
      },
      404
    );
  }

  const basketData = generateCreationBasket(ticker, holdingsData.holdings);

  if (!basketData) {
    return jsonResponse(
      { error: 'Failed to generate creation basket', ticker },
      500
    );
  }

  return jsonResponse(basketData, 200, CACHE_DURATION);
}

/**
 * Debug ETF holdings parsing
 */
async function handleETFDebug(ticker: string, env: Env): Promise<Response> {
  const debugData = await debugISharesHoldings(ticker, env);

  if (!debugData) {
    return jsonResponse(
      {
        error: 'ETF not found or not supported for debug',
        ticker,
        available_etfs: getAvailableETFs(),
      },
      404
    );
  }

  return jsonResponse({
    ticker,
    debug: debugData,
    interpretation: {
      weight_format: debugData.totalWeight > 50
        ? 'Weights appear to be percentages (sum > 50, e.g., 0.52 = 0.52%)'
        : debugData.totalWeight < 0.5
          ? 'Weights appear to be very small decimals (sum < 0.5)'
          : 'Weights appear to be decimals (sum ~= 1.0, e.g., 0.0052 = 0.52%)',
      rating_columns_found: debugData.firstParsedHoldings
        .filter(h => h.rating_column)
        .map(h => h.rating_column)
        .filter((v, i, a) => a.indexOf(v) === i),
    },
    timestamp: new Date().toISOString(),
  });
}

/**
 * Calculate ETF arbitrage opportunity
 */
async function handleArbitrage(
  ticker: string,
  marketPriceParam: string | null,
  env: Env
): Promise<Response> {
  const holdingsData = await fetchETFHoldings(ticker, env);

  if (!holdingsData) {
    return jsonResponse(
      {
        error: 'ETF not found',
        ticker,
        available_etfs: getAvailableETFs(),
      },
      404
    );
  }

  const navPerShare = holdingsData.etf.nav;

  // Use provided market price or simulate small premium/discount
  let marketPrice: number;
  if (marketPriceParam) {
    marketPrice = parseFloat(marketPriceParam);
    if (isNaN(marketPrice)) {
      return jsonResponse({ error: 'Invalid market_price parameter' }, 400);
    }
  } else {
    // Simulate small random premium/discount for demo
    const premiumDiscount = (Math.random() - 0.5) * 0.004; // +/- 0.2%
    marketPrice = navPerShare * (1 + premiumDiscount);
  }

  // Get creation unit size (default 50000)
  const basketData = generateCreationBasket(ticker, holdingsData.holdings);
  const creationUnitSize = basketData?.basket.creation_unit_size || 50000;
  const creationFee = basketData?.basket.estimated_expenses || 500;

  const arbitrage = calculateArbitrage(
    navPerShare,
    marketPrice,
    creationUnitSize,
    creationFee
  );

  return jsonResponse({
    ticker,
    nav_per_share: navPerShare,
    market_price: Math.round(marketPrice * 100) / 100,
    creation_unit_size: creationUnitSize,
    creation_fee: creationFee,
    arbitrage,
    timestamp: new Date().toISOString(),
    source: 'Demo Data Provider',
  }, 200, 60); // Short cache for arbitrage
}

// =============================================================================
// Utilities
// =============================================================================

/**
 * Create a JSON response with CORS headers and optional caching
 */
function jsonResponse(
  data: unknown,
  status: number = 200,
  cacheDuration: number = 0
): Response {
  const headers: Record<string, string> = {
    ...corsHeaders,
    'Content-Type': 'application/json',
  };

  if (cacheDuration > 0) {
    headers['Cache-Control'] = `public, max-age=${cacheDuration}`;
  }

  return new Response(JSON.stringify(data, null, 2), {
    status,
    headers,
  });
}
