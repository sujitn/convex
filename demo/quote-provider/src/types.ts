// =============================================================================
// Quote Provider Types
// =============================================================================

export interface Env {
  CONVEX_API_URL: string;
  DATA_PROVIDER_URL: string;
}

// =============================================================================
// Bond Reference Types (matching convex-server)
// =============================================================================

export interface BondReference {
  // Required fields for server (matches BondReferenceData)
  instrument_id: string;
  description: string;
  bond_type: 'FixedBullet' | 'ZeroCoupon' | 'Callable' | 'Puttable' | 'FloatingRate' | 'InflationLinked';
  issuer_type: 'Sovereign' | 'Agency' | 'CorporateIG' | 'CorporateHY' | 'Municipal' | 'Financial';
  currency: string;
  issue_date: string;
  maturity_date: string;
  coupon_rate?: number;  // As decimal (e.g., 0.05 for 5%)
  frequency: number;     // Payments per year (e.g., 2 for semi-annual)
  day_count: string;
  face_value: number;
  // Issuer info (required by server)
  issuer_id: string;
  issuer_name: string;
  seniority: string;
  // Callable/Putable flags (required by server)
  is_callable: boolean;
  is_putable: boolean;
  is_sinkable: boolean;
  call_schedule: Array<{ call_date: string; call_price: number }>;
  // Additional required fields
  has_deflation_floor: boolean;
  country_of_risk: string;
  sector: string;
  last_updated: number;
  source: string;
  // Optional identifiers
  isin?: string;
  cusip?: string;
  sedol?: string;
  bbgid?: string;
  // Optional fields
  first_coupon_date?: string;
  amount_outstanding?: number;
  // FRN fields
  floating_terms?: {
    rate_index: string;
    spread: number;
    reset_frequency: number;
    cap?: number;
    floor?: number;
  };
  // Inflation-linked fields
  inflation_index?: string;
  inflation_base_index?: number;
}

export interface QuoteRequest {
  instrument_id: string;
  bond_reference?: BondReference;
  settlement_date?: string;
  price?: number;
  yield_to_maturity?: number;
  curve_id?: string;
}

export interface QuoteResponse {
  instrument_id: string;
  clean_price_mid?: number;
  dirty_price_mid?: number;
  ytm_mid?: number;
  modified_duration?: number;
  convexity?: number;
  z_spread_mid?: number;
  i_spread_mid?: number;
  dv01?: number;
  accrued_interest?: number;
  calculation_timestamp: string;
}

// =============================================================================
// Simulation Types
// =============================================================================

export interface SimulatorConfig {
  instruments: BondInstrument[];
  interval_ms: number;
  volatility: 'low' | 'medium' | 'high';
  mode: 'static' | 'random_walk' | 'mean_revert' | 'stress';
}

export interface BondInstrument {
  id: string;
  cusip: string;
  issuer: string;
  coupon: number;
  maturity: string;
  rating: string;
  sector: string;
  bond_reference: BondReference;
}

export interface SimulatorState {
  running: boolean;
  config: SimulatorConfig | null;
  currentPrices: Map<string, QuoteState>;
  lastTick: string;
  tickCount: number;
}

export interface QuoteState {
  instrument_id: string;
  bid: number;
  mid: number;
  ask: number;
  yield: number;
  last_update: string;
}

// =============================================================================
// Curve Types (from data provider)
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
// Stress Scenario Types
// =============================================================================

export type StressScenario =
  | 'rates_up_100bp'
  | 'rates_down_100bp'
  | 'spreads_wide_50bp'
  | 'spreads_tight_50bp'
  | 'flight_to_quality'
  | 'risk_on';

export interface StressConfig {
  scenario: StressScenario;
  duration_seconds?: number;
}
