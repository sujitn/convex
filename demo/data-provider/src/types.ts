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
// NAV History Types
// =============================================================================

export interface NAVHistoryPoint {
  date: string;
  nav: number;
  inav?: number;
  market_price: number;
  premium_discount: number;  // As percentage
  volume: number;
  shares_outstanding: number;
}

export interface NAVHistoryResponse {
  ticker: string;
  history: NAVHistoryPoint[];
  period: string;
  source: string;
}

// =============================================================================
// Creation/Redemption Basket Types
// =============================================================================

export interface BasketComponent {
  cusip: string;
  isin?: string;
  name: string;
  shares: number;
  weight: number;
  market_value: number;
  settlement_date?: string;
}

export interface CreationBasket {
  etf_ticker: string;
  basket_date: string;
  creation_unit_size: number;  // Typically 50,000 shares
  cash_component: number;
  total_value: number;
  nav_per_share: number;
  components: BasketComponent[];
  estimated_expenses: number;  // Creation/redemption fee
}

export interface CreationBasketResponse {
  basket: CreationBasket;
  as_of_date: string;
  source: string;
}

// =============================================================================
// Streaming Quote Types (for synthetic data generation)
// =============================================================================

export interface SyntheticQuote {
  instrument_id: string;
  cusip?: string;
  bid: number;
  mid: number;
  ask: number;
  bid_yield?: number;
  mid_yield?: number;
  ask_yield?: number;
  timestamp: string;
  source: string;
}

export interface QuoteStreamConfig {
  instruments: string[];
  interval_ms: number;
  volatility: 'low' | 'medium' | 'high';
  mode: 'static' | 'random_walk' | 'mean_revert' | 'stress';
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
