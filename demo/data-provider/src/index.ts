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
} from './etf';

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
