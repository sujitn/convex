// Convex API Types

export interface HealthResponse {
  status: string;
  version: string;
  timestamp: string;
}

export interface BondQuote {
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
  i_spread_mid?: number;
  g_spread_mid?: number;
  oas_mid?: number;

  // Risk
  modified_duration?: number;
  macaulay_duration?: number;
  effective_duration?: number;
  convexity?: number;
  dv01?: number;

  // Metadata
  timestamp: number;
}

export interface BondInput {
  instrument_id: string;
  isin?: string;
  cusip?: string;
  issuer_name?: string;
  coupon_rate: number;
  maturity_date: string;
  issue_date?: string;
  settlement_date: string;
  price?: number;
  yield_value?: number;
  frequency?: number; // 1, 2, 4, 12
  day_count?: string; // "Act365", "30360", etc.
  currency?: string;
  face_value?: number;

  // For callable bonds
  is_callable?: boolean;
  call_dates?: string[];
  call_prices?: number[];

  // For FRNs
  is_floating?: boolean;
  spread?: number;
  index?: string;
}

export interface CurveInput {
  curve_id: string;
  currency: string;
  as_of_date: string;
  points: Array<{
    tenor: string;
    rate: number;
  }>;
  interpolation?: string;
}

export interface CurveResponse {
  curve_id: string;
  currency: string;
  as_of_date: string;
  points: Array<[number, number]>;
  timestamp: number;
}

export interface PortfolioHolding {
  instrument_id: string;
  quantity: number;
  weight?: number;
}

export interface PortfolioAnalytics {
  portfolio_id: string;
  nav: number;

  // Duration
  modified_duration: number;
  macaulay_duration?: number;
  effective_duration?: number;

  // Convexity
  convexity: number;

  // Yield & Spread
  ytm: number;
  z_spread?: number;
  oas?: number;

  // DV01
  dv01: number;

  // Holdings count
  holdings_count: number;

  timestamp: number;
}

export interface EtfAnalytics {
  etf_id: string;
  name: string;
  currency: string;

  nav?: number;
  inav?: number;
  price?: number;
  premium_discount?: number;

  num_holdings: number;
  coverage: number;

  duration?: number;
  yield_value?: number;
  spread?: number;

  timestamp: number;
}

export interface StressScenario {
  id: string;
  name: string;
  description: string;
  rate_shift_bps?: number;
  spread_shift_bps?: number;
}

export interface StressTestResult {
  scenario_id: string;
  scenario_name: string;
  portfolio_id: string;

  base_nav: number;
  stressed_nav: number;
  pnl: number;
  pnl_percent: number;

  duration_contribution: number;
  convexity_contribution: number;
  spread_contribution?: number;
}

// WebSocket message types
export interface WsSubscription {
  type: 'subscribe' | 'unsubscribe';
  channel: string;
  instrument_ids?: string[];
}

export interface WsMessage {
  type: 'quote' | 'curve' | 'etf' | 'portfolio' | 'error';
  data: BondQuote | CurveResponse | EtfAnalytics | PortfolioAnalytics | { message: string };
  timestamp: number;
}
