// =============================================================================
// Market Data Types
// =============================================================================

export interface CurvePoint {
  tenor: string;
  years: number;
  rate: number;
}

export interface YieldCurve {
  id: string;
  name: string;
  currency: string;
  as_of_date: string;
  source: string;
  points: CurvePoint[];
}

export interface MarketDataResponse {
  curves: YieldCurve[];
  last_updated: string;
  source: string;
}

// =============================================================================
// ETF Types
// =============================================================================

export interface ETFHolding {
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

export interface ETFInfo {
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

export interface ETFHoldingsResponse {
  etf: ETFInfo;
  holdings: ETFHolding[];
  as_of_date: string;
  source: string;
}

// =============================================================================
// FRED API Types
// =============================================================================

export interface FREDObservation {
  date: string;
  value: string;
}

export interface FREDSeriesResponse {
  observations: FREDObservation[];
}

// =============================================================================
// Environment Types
// =============================================================================

export interface Env {
  FRED_API_KEY?: string;
  CACHE?: KVNamespace;
}
