// Convex API Client

const API_BASE = import.meta.env.VITE_API_URL || '';

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

// ETF
export async function calculateInav(holdings: Array<{
  instrument_id: string;
  quantity: number;
}>) {
  return fetchJson<unknown>('/api/v1/etf/inav', {
    method: 'POST',
    body: JSON.stringify({ holdings }),
  });
}

export async function calculateSecYield(etfId: string, holdings: Array<{
  instrument_id: string;
  quantity: number;
  market_value: number;
}>) {
  return fetchJson<unknown>('/api/v1/etf/sec-yield', {
    method: 'POST',
    body: JSON.stringify({ etf_id: etfId, holdings }),
  });
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
