// Convex API Client

const API_BASE = import.meta.env.VITE_API_URL || '';
const DATA_PROVIDER_BASE = import.meta.env.VITE_DATA_PROVIDER_URL || 'https://convex-demo-data.sujitnair.workers.dev';
const QUOTE_PROVIDER_BASE = import.meta.env.VITE_QUOTE_PROVIDER_URL || 'https://convex-quote-provider.sujitnair.workers.dev';

class ConvexApiError extends Error {
  constructor(
    message: string,
    public status: number,
    public body?: unknown
  ) {
    super(message);
    this.name = 'ConvexApiError';
  }
}

// Bond Quote Response from API
export interface BondQuoteResponse {
  instrument_id: string;
  isin?: string;
  currency: string;
  settlement_date: string;

  // Prices
  clean_price_bid?: number;
  clean_price_mid?: number;
  clean_price_ask?: number;
  accrued_interest?: number;

  // Yields
  ytm_bid?: number;
  ytm_mid?: number;
  ytm_ask?: number;
  ytw?: number;
  ytc?: number;

  // Spreads
  z_spread_bid?: number;
  z_spread_mid?: number;
  z_spread_ask?: number;
  i_spread_bid?: number;
  i_spread_mid?: number;
  i_spread_ask?: number;
  g_spread_bid?: number;
  g_spread_mid?: number;
  g_spread_ask?: number;
  asw_bid?: number;
  asw_mid?: number;
  asw_ask?: number;
  oas_bid?: number;
  oas_mid?: number;
  oas_ask?: number;
  discount_margin_bid?: number;
  discount_margin_mid?: number;
  discount_margin_ask?: number;
  simple_margin_bid?: number;
  simple_margin_mid?: number;
  simple_margin_ask?: number;

  // Risk metrics
  modified_duration?: number;
  macaulay_duration?: number;
  effective_duration?: number;
  spread_duration?: number;
  convexity?: number;
  effective_convexity?: number;
  dv01?: number;

  // Workout info (for callable)
  workout_date?: string;
}

async function fetchJson<T>(path: string, options?: RequestInit): Promise<T> {
  const url = `${API_BASE}${path}`;
  const response = await fetch(url, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...options?.headers,
    },
  });

  if (!response.ok) {
    const body = await response.text();
    throw new ConvexApiError(
      `API error: ${response.statusText}`,
      response.status,
      body
    );
  }

  return response.json();
}

// Health
export async function checkHealth() {
  return fetchJson<{ status: string; version: string; timestamp: string }>(
    '/health'
  );
}

// Market Data Types
export interface MarketCurvePoint {
  tenor: string;
  years: number;
  rate: number;
}

export interface MarketCurve {
  id: string;
  name: string;
  description: string;
  currency: string;
  as_of_date: string;
  source: string;
  points: MarketCurvePoint[];
}

export interface MarketDataResponse {
  curves: MarketCurve[];
  last_updated: string;
  source: string;
}

// Market Data
export async function getMarketData() {
  return fetchJson<MarketDataResponse>('/api/v1/market-data');
}

// Curves
export async function listCurves() {
  return fetchJson<{ curves: Array<{ curve_id: string; currency: string; as_of_date: string }> }>(
    '/api/v1/curves'
  );
}

export async function getCurve(curveId: string) {
  return fetchJson<{ curve_id: string; currency: string; points: Array<[number, number]> }>(
    `/api/v1/curves/${curveId}`
  );
}

export async function createCurve(curve: {
  curve_id: string;
  currency: string;
  as_of_date: string;
  points: Array<{ tenor: string; rate: number }>;
  interpolation?: string;
}) {
  return fetchJson<{ curve_id: string; message: string }>(
    '/api/v1/curves',
    {
      method: 'POST',
      body: JSON.stringify(curve),
    }
  );
}

// Bonds
export async function listBonds(filters?: {
  currency?: string;
  rating?: string;
  is_callable?: boolean;
  is_floating?: boolean;
}) {
  const params = new URLSearchParams();
  if (filters?.currency) params.set('currency', filters.currency);
  if (filters?.rating) params.set('rating', filters.rating);
  if (filters?.is_callable !== undefined) params.set('is_callable', String(filters.is_callable));
  if (filters?.is_floating !== undefined) params.set('is_floating', String(filters.is_floating));

  const query = params.toString();
  return fetchJson<{ bonds: unknown[]; total: number; page: number; page_size: number }>(
    `/api/v1/bonds${query ? `?${query}` : ''}`
  );
}

export async function getBondQuote(instrumentId: string) {
  return fetchJson<unknown>(`/api/v1/quotes/${instrumentId}`);
}

export async function priceBond(bond: {
  instrument_id: string;
  coupon_rate: number;
  maturity_date: string;
  settlement_date: string;
  price?: number;
  yield_value?: number;
  currency?: string;
  face_value?: number;
  frequency?: number;
  day_count?: string;
}) {
  return fetchJson<unknown>('/api/v1/quote', {
    method: 'POST',
    body: JSON.stringify(bond),
  });
}

// Full bond pricing with detailed reference data
export interface BondReferenceInput {
  instrument_id: string;
  isin?: string | null;
  cusip?: string | null;
  sedol?: string | null;
  bbgid?: string | null;
  description: string;
  currency: string;
  issue_date: string;
  maturity_date: string;
  coupon_rate?: number | null;
  frequency: number;
  day_count: string;
  face_value: number;
  // Bond type: FixedBullet, FixedCallable, FixedPutable, FloatingRate, ZeroCoupon, InflationLinked, Amortizing, Convertible
  bond_type: string;
  // Issuer type: Sovereign, Agency, Supranational, CorporateIG, CorporateHY, Financial, Municipal
  issuer_type: string;
  issuer_id: string;
  issuer_name: string;
  seniority: string;
  is_callable: boolean;
  call_schedule: Array<{ call_date: string; call_price: number; is_make_whole?: boolean }>;
  is_putable: boolean;
  is_sinkable: boolean;
  floating_terms?: {
    spread: number;
    index: string;
    reset_frequency: number;
    cap?: number | null;
    floor?: number | null;
  } | null;
  inflation_index?: string | null;
  inflation_base_index?: number | null;
  has_deflation_floor: boolean;
  country_of_risk: string;
  sector: string;
  amount_outstanding?: number | null;
  first_coupon_date?: string | null;
  last_updated?: number;
  source?: string;
}

export interface SingleBondPricingRequest {
  bond: BondReferenceInput;
  settlement_date: string;
  market_price?: number | null;
}

export async function priceBondWithDetails(
  request: SingleBondPricingRequest
): Promise<BondQuoteResponse> {
  return fetchJson<BondQuoteResponse>('/api/v1/quote', {
    method: 'POST',
    body: JSON.stringify(request),
  });
}

export async function batchPrice(bonds: Array<{
  instrument_id: string;
  coupon_rate: number;
  maturity_date: string;
  settlement_date: string;
  price?: number;
  yield_value?: number;
}>) {
  return fetchJson<{ results: unknown[] }>('/api/v1/batch/price', {
    method: 'POST',
    body: JSON.stringify({ bonds }),
  });
}

// =============================================================================
// ETF ANALYTICS
// =============================================================================

// ETF Holding Entry for iNAV calculation
export interface EtfHoldingEntry {
  instrument_id: string;
  weight: number;
  shares: number;
  market_value?: number | null;
  notional_value?: number | null;
  accrued_interest?: number | null;
}

// ETF Holdings input for iNAV calculation
export interface EtfHoldingsInput {
  etf_id: string;
  name: string;
  currency?: string;
  as_of_date: string;
  holdings: EtfHoldingEntry[];
  total_market_value: number;
  shares_outstanding: number;
  nav_per_share?: number | null;
}

// ETF iNAV Request
export interface EtfInavRequest {
  holdings: EtfHoldingsInput;
  bond_prices: BondQuoteResponse[];
  settlement_date: string;
}

// ETF Quote Output (iNAV response)
export interface EtfQuoteOutput {
  etf_id: string;
  inav: number;
  nav: number;
  premium_discount_pct: number;
  total_market_value: number;
  shares_outstanding: number;
  currency: string;
  as_of_date: string;
  settlement_date: string;
  holdings_count: number;
  weighted_duration: number;
  weighted_yield: number;
  weighted_spread: number;
  timestamp: number;
}

// Calculate iNAV for an ETF
export async function calculateEtfInav(request: EtfInavRequest): Promise<EtfQuoteOutput> {
  return fetchJson<EtfQuoteOutput>('/api/v1/etf/inav', {
    method: 'POST',
    body: JSON.stringify(request),
  });
}

// Batch iNAV calculation
export interface BatchEtfInavRequest {
  etfs: EtfHoldingsInput[];
  bond_prices: BondQuoteResponse[];
  settlement_date: string;
}

export interface BatchEtfInavResponse {
  results: EtfQuoteOutput[];
  errors: Array<{ etf_id: string; error: string }>;
}

export async function batchCalculateEtfInav(request: BatchEtfInavRequest): Promise<BatchEtfInavResponse> {
  return fetchJson<BatchEtfInavResponse>('/api/v1/etf/inav/batch', {
    method: 'POST',
    body: JSON.stringify(request),
  });
}

// SEC Yield Request
export interface SecYieldRequest {
  etf_id: string;
  net_investment_income: number;
  avg_shares_outstanding: number;
  max_offering_price: number;
  gross_expenses?: number | null;
  fee_waivers?: number | null;
  as_of_date: string;
}

// SEC Yield Response
export interface SecYieldResponse {
  etf_id: string;
  sec_30_day_yield: number;
  unsubsidized_yield?: number | null;
  fee_waiver_impact?: number | null;
  dividend_income: string;
  interest_income: string;
  total_income: string;
  avg_shares: string;
  max_offering_price: string;
  as_of_date: string;
  timestamp: number;
}

export async function calculateSecYield(request: SecYieldRequest): Promise<SecYieldResponse> {
  return fetchJson<SecYieldResponse>('/api/v1/etf/sec-yield', {
    method: 'POST',
    body: JSON.stringify(request),
  });
}

// Batch price multiple bonds with full reference data
export async function batchPriceBondsWithDetails(
  requests: SingleBondPricingRequest[]
): Promise<{ results: BondQuoteResponse[]; errors: Array<{ index: number; error: string }> }> {
  // Use Promise.allSettled to handle partial failures
  const results = await Promise.allSettled(
    requests.map(req => priceBondWithDetails(req))
  );

  const successResults: BondQuoteResponse[] = [];
  const errors: Array<{ index: number; error: string }> = [];

  results.forEach((result, index) => {
    if (result.status === 'fulfilled') {
      successResults.push(result.value);
    } else {
      errors.push({ index, error: result.reason?.message || 'Unknown error' });
    }
  });

  return { results: successResults, errors };
}

// =============================================================================
// PORTFOLIO ANALYTICS
// =============================================================================

// Position input for portfolio
export interface PositionInput {
  instrument_id: string;
  notional: number;
  sector?: string;
  rating?: string;
}

// Portfolio input
export interface PortfolioInput {
  portfolio_id: string;
  name: string;
  currency?: string;
  positions: PositionInput[];
}

// Portfolio analytics request
export interface PortfolioAnalyticsRequest {
  portfolio: PortfolioInput;
  bond_prices: BondQuoteResponse[];
}

// Portfolio analytics output
export interface PortfolioAnalyticsOutput {
  portfolio_id: string;
  name: string;
  currency: string;
  as_of_date: string;
  // Summary metrics
  total_market_value: number;
  total_par_value: number;
  holdings_count: number;
  // Duration metrics
  modified_duration: number;
  effective_duration?: number;
  macaulay_duration?: number;
  spread_duration?: number;
  // Yield and spread
  weighted_yield: number;
  weighted_spread?: number;
  weighted_oas?: number;
  // Risk metrics
  dv01: number;
  convexity: number;
  cs01?: number;
  // Credit quality
  weighted_rating?: string;
  investment_grade_pct?: number;
  // Sector breakdown
  sector_weights?: Record<string, number>;
  rating_weights?: Record<string, number>;
  // Timestamp
  timestamp: number;
}

// Calculate portfolio analytics
export async function calculatePortfolioAnalytics(
  request: PortfolioAnalyticsRequest
): Promise<PortfolioAnalyticsOutput> {
  return fetchJson<PortfolioAnalyticsOutput>('/api/v1/portfolio/analytics', {
    method: 'POST',
    body: JSON.stringify(request),
  });
}

// =============================================================================
// KEY RATE DURATION
// =============================================================================

// Position with key rate duration data
export interface KeyRatePosition {
  instrument_id: string;
  notional: number;
  market_price?: number;
  key_rate_durations?: Array<[number, number]>; // [tenor, duration] pairs
}

// Key rate duration request
export interface KeyRateDurationRequest {
  portfolio_id: string;
  name: string;
  positions: KeyRatePosition[];
  tenors?: number[]; // Custom tenor points
}

// Key rate point output
export interface KeyRatePointOutput {
  tenor: number;
  duration: number;
  contribution_pct: number;
}

// Key rate duration response
export interface KeyRateDurationResponse {
  portfolio_id: string;
  profile: KeyRatePointOutput[];
  total_duration: number;
  short_duration: number;
  intermediate_duration: number;
  long_duration: number;
  coverage: number;
  total_holdings: number;
  coverage_pct: number;
  timestamp: number;
}

// Calculate key rate duration profile
export async function calculateKeyRateDuration(
  request: KeyRateDurationRequest
): Promise<KeyRateDurationResponse> {
  return fetchJson<KeyRateDurationResponse>('/api/v1/portfolio/key-rate-duration', {
    method: 'POST',
    body: JSON.stringify(request),
  });
}

// =============================================================================
// STRESS TESTING
// =============================================================================

// Stress scenario
export interface StressScenario {
  id: string;
  name: string;
  description: string;
  category: string;
}

// List available stress scenarios
export async function listStressScenarios(): Promise<{ scenarios: StressScenario[] }> {
  return fetchJson<{ scenarios: StressScenario[] }>('/api/v1/stress/scenarios');
}

// Standard stress test request
export interface StandardStressRequest {
  portfolio: PortfolioInput;
  bond_prices: BondQuoteResponse[];
}

// Stress result output
export interface StressResultOutput {
  scenario_name: string;
  initial_value: string;
  stressed_value: string;
  pnl: string;
  pnl_pct: string;
  duration_impact?: string;
  spread_impact?: string;
}

// Standard stress test response
export interface StandardStressResponse {
  portfolio_id: string;
  results: StressResultOutput[];
  timestamp: number;
}

// Run standard stress tests
export async function runStandardStressTest(
  request: StandardStressRequest
): Promise<StandardStressResponse> {
  return fetchJson<StandardStressResponse>('/api/v1/stress/standard', {
    method: 'POST',
    body: JSON.stringify(request),
  });
}

// WebSocket connection
export function createWebSocket(
  onMessage: (msg: unknown) => void,
  onOpen?: () => void,
  onClose?: () => void,
  onError?: (error: Event) => void
) {
  const wsUrl = import.meta.env.VITE_WS_URL ||
    `${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}/ws`;

  const ws = new WebSocket(wsUrl);

  ws.onopen = () => {
    console.log('WebSocket connected');
    onOpen?.();
  };

  ws.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);
      onMessage(data);
    } catch (e) {
      console.error('Failed to parse WebSocket message:', e);
    }
  };

  ws.onclose = () => {
    console.log('WebSocket disconnected');
    onClose?.();
  };

  ws.onerror = (error) => {
    console.error('WebSocket error:', error);
    onError?.(error);
  };

  return {
    subscribe: (channel: string, instrumentIds?: string[]) => {
      ws.send(JSON.stringify({
        type: 'subscribe',
        channel,
        instrument_ids: instrumentIds,
      }));
    },
    unsubscribe: (channel: string) => {
      ws.send(JSON.stringify({
        type: 'unsubscribe',
        channel,
      }));
    },
    close: () => ws.close(),
    ws,
  };
}

// =============================================================================
// DEMO DATA PROVIDER API
// Fetches market data from the demo data provider service
// =============================================================================

// Curve point from data provider
export interface DataProviderCurvePoint {
  tenor: string;
  years: number;
  rate: number;
}

// Yield curve from data provider
export interface DataProviderYieldCurve {
  id: string;
  name: string;
  currency: string;
  as_of_date: string;
  source: string;
  points: DataProviderCurvePoint[];
}

// Market data response from data provider
export interface DataProviderMarketData {
  curves: DataProviderYieldCurve[];
  last_updated: string;
  source: string;
}

// ETF holding from data provider
export interface DataProviderETFHolding {
  id: string;
  cusip: string;
  isin?: string;
  issuer: string;
  description: string;
  coupon: number;
  maturity: string;
  rating: string;
  sector: string;
  weight: number;
  shares: number;
  market_value: number;
  price?: number;
}

// ETF info from data provider
export interface DataProviderETFInfo {
  ticker: string;
  name: string;
  description: string;
  issuer: string;
  inception_date: string;
  expense_ratio: number;
  aum: number;
  shares_outstanding: number;
  nav: number;
  holdings_count: number;
}

// ETF holdings response from data provider
export interface DataProviderETFResponse {
  etf: DataProviderETFInfo;
  holdings: DataProviderETFHolding[];
  as_of_date: string;
  source: string;
  metrics: {
    weighted_duration: number;
    weighted_yield: number;
    weighted_coupon: number;
    total_market_value: number;
  };
}

// Corporate spreads response
export interface DataProviderSpreadsResponse {
  spreads: Record<string, number>;
  as_of_date: string;
  source: string;
  description: string;
}

/**
 * Fetch market data (curves) from the demo data provider
 */
export async function fetchMarketDataFromProvider(): Promise<DataProviderMarketData> {
  const response = await fetch(`${DATA_PROVIDER_BASE}/api/market-data`);
  if (!response.ok) {
    throw new Error(`Failed to fetch market data: ${response.statusText}`);
  }
  return response.json();
}

/**
 * Fetch Treasury curve from the demo data provider
 */
export async function fetchTreasuryCurveFromProvider(): Promise<DataProviderYieldCurve> {
  const response = await fetch(`${DATA_PROVIDER_BASE}/api/curves/treasury`);
  if (!response.ok) {
    throw new Error(`Failed to fetch Treasury curve: ${response.statusText}`);
  }
  return response.json();
}

/**
 * Fetch SOFR curve from the demo data provider
 */
export async function fetchSOFRCurveFromProvider(): Promise<DataProviderYieldCurve> {
  const response = await fetch(`${DATA_PROVIDER_BASE}/api/curves/sofr`);
  if (!response.ok) {
    throw new Error(`Failed to fetch SOFR curve: ${response.statusText}`);
  }
  return response.json();
}

/**
 * Fetch corporate spreads from the demo data provider
 */
export async function fetchSpreadsFromProvider(): Promise<DataProviderSpreadsResponse> {
  const response = await fetch(`${DATA_PROVIDER_BASE}/api/spreads`);
  if (!response.ok) {
    throw new Error(`Failed to fetch spreads: ${response.statusText}`);
  }
  return response.json();
}

/**
 * Fetch list of available ETFs from the demo data provider
 */
export async function fetchAvailableETFs(): Promise<{ etfs: string[]; count: number }> {
  const response = await fetch(`${DATA_PROVIDER_BASE}/api/etf/list`);
  if (!response.ok) {
    throw new Error(`Failed to fetch ETF list: ${response.statusText}`);
  }
  return response.json();
}

/**
 * Fetch ETF holdings from the demo data provider
 */
export async function fetchETFHoldingsFromProvider(ticker: string): Promise<DataProviderETFResponse> {
  const response = await fetch(`${DATA_PROVIDER_BASE}/api/etf/${ticker}`);
  if (!response.ok) {
    if (response.status === 404) {
      throw new Error(`ETF ${ticker} not found`);
    }
    throw new Error(`Failed to fetch ETF holdings: ${response.statusText}`);
  }
  return response.json();
}

/**
 * Fetch NAV history from the demo data provider
 */
export async function fetchNAVHistory(ticker: string, days = 30): Promise<{
  ticker: string;
  history: Array<{
    date: string;
    nav: number;
    inav?: number;
    market_price: number;
    premium_discount: number;
    volume: number;
    shares_outstanding: number;
  }>;
  period: string;
  source: string;
}> {
  const response = await fetch(`${DATA_PROVIDER_BASE}/api/etf/${ticker}/nav-history?days=${days}`);
  if (!response.ok) {
    throw new Error(`Failed to fetch NAV history: ${response.statusText}`);
  }
  return response.json();
}

/**
 * Fetch creation basket from the demo data provider
 */
export async function fetchCreationBasket(ticker: string): Promise<{
  basket: {
    etf_ticker: string;
    basket_date: string;
    creation_unit_size: number;
    cash_component: number;
    total_value: number;
    nav_per_share: number;
    components: Array<{
      cusip: string;
      isin?: string;
      name: string;
      shares: number;
      weight: number;
      market_value: number;
    }>;
    estimated_expenses: number;
  };
  as_of_date: string;
  source: string;
}> {
  const response = await fetch(`${DATA_PROVIDER_BASE}/api/etf/${ticker}/basket`);
  if (!response.ok) {
    throw new Error(`Failed to fetch creation basket: ${response.statusText}`);
  }
  return response.json();
}

/**
 * Fetch arbitrage calculation from the demo data provider
 */
export async function fetchArbitrage(ticker: string, marketPrice?: number): Promise<{
  ticker: string;
  nav_per_share: number;
  market_price: number;
  creation_unit_size: number;
  creation_fee: number;
  arbitrage: {
    premium_discount_pct: number;
    premium_discount_bps: number;
    action: 'create' | 'redeem' | 'none';
    gross_profit: number;
    net_profit: number;
    profitable: boolean;
  };
  timestamp: string;
  source: string;
}> {
  const url = marketPrice
    ? `${DATA_PROVIDER_BASE}/api/etf/${ticker}/arbitrage?market_price=${marketPrice}`
    : `${DATA_PROVIDER_BASE}/api/etf/${ticker}/arbitrage`;
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`Failed to fetch arbitrage: ${response.statusText}`);
  }
  return response.json();
}

// =============================================================================
// QUOTE PROVIDER API
// Controls the synthetic quote generator for streaming demos
// =============================================================================

// Quote provider status
export interface QuoteProviderStatus {
  running: boolean;
  tickCount: number;
  instrumentCount: number;
  config?: {
    interval_ms: number;
    volatility: 'low' | 'medium' | 'high';
    mode: 'static' | 'random_walk' | 'mean_revert' | 'stress';
  };
  lastTick?: string;
  instruments?: string[];
}

// Quote state from provider
export interface QuoteState {
  instrument_id: string;
  bid: number;
  mid: number;
  ask: number;
  yield: number;
  last_update: string;
}

// Start simulation config
export interface SimulationConfig {
  interval_ms?: number;
  volatility?: 'low' | 'medium' | 'high';
  mode?: 'static' | 'random_walk' | 'mean_revert' | 'stress';
  force?: boolean; // Force restart if already running
}

// Start response
export interface StartSimulationResponse {
  status: string;
  config: {
    instruments: unknown[];
    interval_ms: number;
    volatility: string;
    mode: string;
  };
  instruments: number;
  initial_quotes: QuoteState[];
}

// Tick response
export interface TickResponse {
  tick: number;
  timestamp: string;
  results: Array<{
    instrument_id: string;
    quote: QuoteState;
    response?: unknown;
  }>;
}

// Stress response
export interface StressResponse {
  scenario_applied: string;
  timestamp: string;
  new_quotes: QuoteState[];
}

/**
 * Get quote provider status
 */
export async function getQuoteProviderStatus(): Promise<QuoteProviderStatus> {
  const response = await fetch(`${QUOTE_PROVIDER_BASE}/status`);
  if (!response.ok) {
    throw new Error(`Failed to get quote provider status: ${response.statusText}`);
  }
  return response.json();
}

/**
 * Start the quote simulation
 */
export async function startQuoteSimulation(config?: SimulationConfig): Promise<StartSimulationResponse> {
  const response = await fetch(`${QUOTE_PROVIDER_BASE}/start`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(config || {}),
  });
  if (!response.ok) {
    const error = await response.json();
    throw new Error(error.error || `Failed to start simulation: ${response.statusText}`);
  }
  return response.json();
}

/**
 * Stop the quote simulation
 */
export async function stopQuoteSimulation(): Promise<{ status: string; total_ticks: number }> {
  const response = await fetch(`${QUOTE_PROVIDER_BASE}/stop`, {
    method: 'POST',
  });
  if (!response.ok) {
    throw new Error(`Failed to stop simulation: ${response.statusText}`);
  }
  return response.json();
}

/**
 * Get current quotes from the quote provider
 */
export async function getQuoteProviderQuotes(): Promise<{ count: number; timestamp: string; quotes: QuoteState[] }> {
  const response = await fetch(`${QUOTE_PROVIDER_BASE}/quotes`);
  if (!response.ok) {
    throw new Error(`Failed to get quotes: ${response.statusText}`);
  }
  return response.json();
}

/**
 * Trigger a single tick (for manual control)
 */
export async function triggerQuoteTick(): Promise<TickResponse> {
  const response = await fetch(`${QUOTE_PROVIDER_BASE}/tick`, {
    method: 'POST',
  });
  if (!response.ok) {
    const error = await response.json();
    throw new Error(error.error || `Failed to trigger tick: ${response.statusText}`);
  }
  return response.json();
}

/**
 * Apply a stress scenario
 */
export async function applyStressScenario(
  scenario: 'rates_up_100bp' | 'rates_down_100bp' | 'spreads_wide_50bp' | 'spreads_tight_50bp' | 'flight_to_quality' | 'risk_on'
): Promise<StressResponse> {
  const response = await fetch(`${QUOTE_PROVIDER_BASE}/stress`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ scenario }),
  });
  if (!response.ok) {
    const error = await response.json();
    throw new Error(error.error || `Failed to apply stress: ${response.statusText}`);
  }
  return response.json();
}

/**
 * Refresh curves in the quote provider
 */
export async function refreshQuoteProviderCurves(): Promise<{ status: string; curves_loaded: number }> {
  const response = await fetch(`${QUOTE_PROVIDER_BASE}/refresh-curves`, {
    method: 'POST',
  });
  if (!response.ok) {
    throw new Error(`Failed to refresh curves: ${response.statusText}`);
  }
  return response.json();
}
