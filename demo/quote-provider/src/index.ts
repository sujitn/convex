// =============================================================================
// Convex Demo Quote Provider
// Cloudflare Worker that generates synthetic bond quotes for demo
// =============================================================================

import {
  Env,
  SimulatorConfig,
  QuoteState,
  BondInstrument,
  MarketDataResponse,
  StressScenario,
  YieldCurve,
} from './types';
import {
  SyntheticQuoteGenerator,
  getSampleBonds,
} from './synthetic-gen';

// CORS headers for cross-origin requests
const corsHeaders = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
  'Access-Control-Allow-Headers': 'Content-Type, Authorization',
  'Access-Control-Max-Age': '86400',
};

// In-memory state (resets on worker restart)
// For production, use Durable Objects for persistent state
let simulatorState = {
  running: false,
  config: null as SimulatorConfig | null,
  currentPrices: new Map<string, QuoteState>(),
  lastTick: '',
  tickCount: 0,
  generator: new SyntheticQuoteGenerator(),
};

/**
 * Main worker entry point
 */
export default {
  async fetch(
    request: Request,
    env: Env,
    ctx: ExecutionContext
  ): Promise<Response> {
    // Handle CORS preflight
    if (request.method === 'OPTIONS') {
      return new Response(null, { headers: corsHeaders });
    }

    const url = new URL(request.url);
    const path = url.pathname;

    try {
      // Health check
      if (path === '/' || path === '/health') {
        return jsonResponse({
          status: 'ok',
          service: 'convex-quote-provider',
          version: '1.0.0',
          timestamp: new Date().toISOString(),
          simulator: {
            running: simulatorState.running,
            tickCount: simulatorState.tickCount,
            instrumentCount: simulatorState.currentPrices.size,
          },
          endpoints: [
            'POST /start - Start simulation',
            'POST /stop - Stop simulation',
            'GET /status - Get current state',
            'GET /quotes - Get current quotes',
            'POST /tick - Manual tick',
            'POST /stress - Apply stress scenario',
            'POST /refresh-curves - Refresh curves from data provider',
          ],
        });
      }

      // Start simulation
      if (path === '/start' && request.method === 'POST') {
        return await handleStart(request, env, ctx);
      }

      // Stop simulation
      if (path === '/stop' && request.method === 'POST') {
        return handleStop();
      }

      // Get status
      if (path === '/status') {
        return handleStatus();
      }

      // Get current quotes
      if (path === '/quotes') {
        return handleQuotes();
      }

      // Manual tick
      if (path === '/tick' && request.method === 'POST') {
        return await handleTick(env);
      }

      // Apply stress scenario
      if (path === '/stress' && request.method === 'POST') {
        return await handleStress(request, env);
      }

      // Refresh curves from data provider
      if (path === '/refresh-curves' && request.method === 'POST') {
        return await handleRefreshCurves(env);
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
 * Start the quote simulation
 */
async function handleStart(
  request: Request,
  env: Env,
  ctx: ExecutionContext
): Promise<Response> {
  // Parse config from request body
  let config: Partial<SimulatorConfig> & { force?: boolean } = {};
  try {
    config = await request.json();
  } catch {
    // Use defaults
  }

  // If already running, check for force flag or return error
  if (simulatorState.running) {
    if (config.force) {
      // Force stop and restart
      console.log('Force stopping existing simulation');
      simulatorState.running = false;
    } else {
      return jsonResponse({ error: 'Simulator already running', hint: 'Use force: true to restart' }, 400);
    }
  }

  // Get instruments (use sample bonds if not provided)
  const instruments: BondInstrument[] =
    config.instruments || getSampleBonds();

  // Build full config
  const fullConfig: SimulatorConfig = {
    instruments,
    interval_ms: config.interval_ms || 1000,
    volatility: config.volatility || 'medium',
    mode: config.mode || 'random_walk',
  };

  // Fetch curves from data provider
  try {
    const curvesResponse = await fetch(`${env.DATA_PROVIDER_URL}/api/curves`);
    if (curvesResponse.ok) {
      const curvesData: MarketDataResponse = await curvesResponse.json();
      simulatorState.generator.setCurves(curvesData.curves);
      console.log(`Loaded ${curvesData.curves.length} curves from data provider`);
    }
  } catch (error) {
    console.log('Could not fetch curves, using defaults:', error);
  }

  // Initialize prices
  simulatorState.currentPrices.clear();
  for (const instrument of instruments) {
    const quote = simulatorState.generator.generateInitialQuote(instrument);
    simulatorState.currentPrices.set(instrument.id, quote);
  }

  // Update state
  simulatorState.running = true;
  simulatorState.config = fullConfig;
  simulatorState.tickCount = 0;
  simulatorState.lastTick = new Date().toISOString();

  // Schedule automatic ticks using waitUntil
  // Note: Cloudflare Workers don't support setInterval directly
  // For continuous streaming, the frontend should poll /tick or use scheduled triggers
  console.log(
    `Simulator started: ${instruments.length} instruments, ${fullConfig.interval_ms}ms interval, ${fullConfig.mode} mode`
  );

  return jsonResponse({
    status: 'started',
    config: fullConfig,
    instruments: instruments.length,
    initial_quotes: Array.from(simulatorState.currentPrices.values()),
  });
}

/**
 * Stop the simulation
 */
function handleStop(): Response {
  if (!simulatorState.running) {
    return jsonResponse({ message: 'Simulator not running' });
  }

  simulatorState.running = false;
  const tickCount = simulatorState.tickCount;

  return jsonResponse({
    status: 'stopped',
    total_ticks: tickCount,
    final_quotes: Array.from(simulatorState.currentPrices.values()),
  });
}

/**
 * Get current status
 */
function handleStatus(): Response {
  return jsonResponse({
    running: simulatorState.running,
    config: simulatorState.config,
    tickCount: simulatorState.tickCount,
    lastTick: simulatorState.lastTick,
    instrumentCount: simulatorState.currentPrices.size,
    instruments: simulatorState.config?.instruments.map((i) => i.id) || [],
  });
}

/**
 * Get current quotes
 */
function handleQuotes(): Response {
  const quotes = Array.from(simulatorState.currentPrices.values());

  return jsonResponse({
    count: quotes.length,
    timestamp: new Date().toISOString(),
    quotes,
  });
}

/**
 * Execute a single tick and push to pricing server
 * Works statelessly - initializes if needed
 */
async function handleTick(env: Env): Promise<Response> {
  // Initialize if not running (stateless mode for Cloudflare Workers)
  if (!simulatorState.running || !simulatorState.config) {
    // Auto-initialize with defaults
    const instruments = getSampleBonds();
    simulatorState.config = {
      instruments,
      interval_ms: 1000,
      volatility: 'medium',
      mode: 'random_walk',
    };
    simulatorState.running = true;

    // Fetch curves for initial pricing
    try {
      const curves = await fetchCurvesFromProvider(env);
      if (curves) {
        simulatorState.generator.setCurves(curves);
      }
    } catch (e) {
      console.error('Failed to fetch curves:', e);
    }

    // Generate initial quotes
    for (const instrument of instruments) {
      const quote = simulatorState.generator.generateInitialQuote(instrument);
      simulatorState.currentPrices.set(instrument.id, quote);
    }
  }

  const config = simulatorState.config;
  const results: Array<{ instrument_id: string; quote: QuoteState; response?: unknown }> = [];

  // Update each quote
  for (const instrument of config.instruments) {
    const currentQuote = simulatorState.currentPrices.get(instrument.id);
    if (!currentQuote) continue;

    // Apply tick
    const newQuote = simulatorState.generator.tick(
      currentQuote,
      config.volatility,
      config.mode
    );

    simulatorState.currentPrices.set(instrument.id, newQuote);

    // Push to pricing server
    try {
      const quoteRequest = {
        bond: instrument.bond_reference,
        settlement_date: new Date().toISOString().split('T')[0],
        market_price: newQuote.mid,
      };

      const response = await fetch(`${env.CONVEX_API_URL}/api/v1/quote`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(quoteRequest),
      });

      if (response.ok) {
        const responseData = await response.json();
        results.push({
          instrument_id: instrument.id,
          quote: newQuote,
          response: responseData,
        });
      } else {
        results.push({
          instrument_id: instrument.id,
          quote: newQuote,
          response: { error: `HTTP ${response.status}` },
        });
      }
    } catch (error) {
      results.push({
        instrument_id: instrument.id,
        quote: newQuote,
        response: { error: String(error) },
      });
    }
  }

  simulatorState.tickCount++;
  simulatorState.lastTick = new Date().toISOString();

  return jsonResponse({
    tick: simulatorState.tickCount,
    timestamp: simulatorState.lastTick,
    results,
  });
}

/**
 * Apply a stress scenario
 */
async function handleStress(request: Request, env: Env): Promise<Response> {
  let body: { scenario?: StressScenario } = {};
  try {
    body = await request.json();
  } catch {
    return jsonResponse({ error: 'Invalid request body' }, 400);
  }

  const scenario = body.scenario;
  if (!scenario) {
    return jsonResponse({
      error: 'Missing scenario',
      available_scenarios: [
        'rates_up_100bp',
        'rates_down_100bp',
        'spreads_wide_50bp',
        'spreads_tight_50bp',
        'flight_to_quality',
        'risk_on',
      ],
    }, 400);
  }

  // Apply stress to generator
  simulatorState.generator.applyStress(scenario);

  // Regenerate all prices with new curves/spreads
  if (simulatorState.config) {
    for (const instrument of simulatorState.config.instruments) {
      const newQuote = simulatorState.generator.generateInitialQuote(instrument);
      simulatorState.currentPrices.set(instrument.id, newQuote);
    }
  }

  return jsonResponse({
    scenario_applied: scenario,
    timestamp: new Date().toISOString(),
    new_quotes: Array.from(simulatorState.currentPrices.values()),
  });
}

/**
 * Refresh curves from data provider
 */
async function handleRefreshCurves(env: Env): Promise<Response> {
  try {
    const response = await fetch(`${env.DATA_PROVIDER_URL}/api/curves`);
    if (!response.ok) {
      return jsonResponse(
        { error: `Data provider returned ${response.status}` },
        500
      );
    }

    const data: MarketDataResponse = await response.json();
    simulatorState.generator.setCurves(data.curves);

    // Also fetch spreads
    const spreadsResponse = await fetch(`${env.DATA_PROVIDER_URL}/api/spreads`);
    if (spreadsResponse.ok) {
      const spreadsData = await spreadsResponse.json() as {
        spreads?: {
          investment_grade?: { value: number };
          high_yield?: { value: number };
        };
      };
      if (spreadsData.spreads) {
        const igSpread = spreadsData.spreads.investment_grade?.value || 110;
        const hySpread = spreadsData.spreads.high_yield?.value || 350;
        simulatorState.generator.setSpreads(igSpread, hySpread);
      }
    }

    // Regenerate quotes with new curves
    if (simulatorState.config) {
      for (const instrument of simulatorState.config.instruments) {
        const newQuote = simulatorState.generator.generateInitialQuote(instrument);
        simulatorState.currentPrices.set(instrument.id, newQuote);
      }
    }

    return jsonResponse({
      status: 'curves_refreshed',
      curves_loaded: data.curves.length,
      source: data.source,
      timestamp: new Date().toISOString(),
    });
  } catch (error) {
    return jsonResponse({ error: String(error) }, 500);
  }
}

// =============================================================================
// Utilities
// =============================================================================

/**
 * Fetch curves from the data provider
 */
async function fetchCurvesFromProvider(env: Env): Promise<YieldCurve[] | null> {
  try {
    const response = await fetch(`${env.DATA_PROVIDER_URL}/api/curves`);
    if (response.ok) {
      const data: MarketDataResponse = await response.json();
      return data.curves;
    }
  } catch (error) {
    console.error('Failed to fetch curves from provider:', error);
  }
  return null;
}

/**
 * Create a JSON response with CORS headers
 */
function jsonResponse(
  data: unknown,
  status: number = 200
): Response {
  return new Response(JSON.stringify(data, null, 2), {
    status,
    headers: {
      ...corsHeaders,
      'Content-Type': 'application/json',
    },
  });
}
