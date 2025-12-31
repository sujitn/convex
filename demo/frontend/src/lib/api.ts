// Convex API Client

const API_BASE = import.meta.env.VITE_API_URL || '';
const DATA_PROVIDER_BASE = import.meta.env.VITE_DATA_PROVIDER_URL || 'https://convex-demo-data.sujitnair.workers.dev';

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
  bond_type: string;
  issuer_type: string;
  issuer_id: string;
  issuer_name: string;
  seniority: string;
  is_callable: boolean;
  call_schedule: Array<{ call_date: string; call_price: number }>;
  is_putable: boolean;
  is_sinkable: boolean;
  floating_terms?: {
    spread: number;
    index: string;
    reset_frequency: number;
    current_rate?: number | null;
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

// Portfolio
export async function calculatePortfolioAnalytics(holdings: Array<{
  instrument_id: string;
  quantity: number;
  market_value?: number;
}>) {
  return fetchJson<unknown>('/api/v1/portfolio/analytics', {
    method: 'POST',
    body: JSON.stringify({ holdings }),
  });
}

export async function calculateKeyRateDuration(holdings: Array<{
  instrument_id: string;
  quantity: number;
}>) {
  return fetchJson<unknown>('/api/v1/portfolio/key-rate-duration', {
    method: 'POST',
    body: JSON.stringify({ holdings }),
  });
}

// Stress Testing
export async function listStressScenarios() {
  return fetchJson<{ scenarios: Array<{ id: string; name: string; description: string }> }>(
    '/api/v1/stress/scenarios'
  );
}

export async function runStressTest(scenarioId: string, holdings: Array<{
  instrument_id: string;
  quantity: number;
}>) {
  return fetchJson<unknown>('/api/v1/stress/single', {
    method: 'POST',
    body: JSON.stringify({ scenario_id: scenarioId, holdings }),
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
