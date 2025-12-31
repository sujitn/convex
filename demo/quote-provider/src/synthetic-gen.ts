// =============================================================================
// Synthetic Quote Generator
// Generates realistic bond prices from yield curves
// =============================================================================

import {
  BondInstrument,
  CurvePoint,
  YieldCurve,
  QuoteState,
} from './types';

// Volatility multipliers (daily price vol in %)
const VOLATILITY_MAP = {
  low: 0.002,    // ~0.2% daily vol
  medium: 0.005, // ~0.5% daily vol
  high: 0.01,    // ~1.0% daily vol
};

// Bid-ask spreads by rating (in price points)
const BID_ASK_SPREADS: Record<string, number> = {
  'AAA': 0.05,
  'AA+': 0.06,
  'AA': 0.07,
  'AA-': 0.08,
  'A+': 0.10,
  'A': 0.12,
  'A-': 0.15,
  'BBB+': 0.20,
  'BBB': 0.25,
  'BBB-': 0.30,
  'BB+': 0.40,
  'BB': 0.50,
  'BB-': 0.60,
  'B+': 0.75,
  'B': 1.00,
  'B-': 1.25,
  'CCC+': 1.50,
  'CCC': 2.00,
  'CCC-': 2.50,
  'CC': 3.00,
  'C': 4.00,
  'D': 5.00,
  'NR': 0.50,
};

// Sector spreads over Treasury (in basis points)
const SECTOR_SPREADS: Record<string, number> = {
  'Government': 0,
  'Agency MBS': 20,
  'Financials': 80,
  'Technology': 60,
  'Healthcare': 65,
  'Consumer': 70,
  'Energy': 90,
  'Industrials': 75,
  'Utilities': 55,
  'Telecom': 85,
  'Media': 80,
  'Real Estate': 95,
  'Materials': 85,
  'Transportation': 80,
  'Autos': 100,
  'Other': 75,
};

// Rating spread adders (in basis points)
const RATING_SPREADS: Record<string, number> = {
  'AAA': 0,
  'AA+': 10,
  'AA': 15,
  'AA-': 20,
  'A+': 30,
  'A': 40,
  'A-': 50,
  'BBB+': 80,
  'BBB': 100,
  'BBB-': 130,
  'BB+': 200,
  'BB': 250,
  'BB-': 300,
  'B+': 400,
  'B': 500,
  'B-': 600,
  'CCC+': 800,
  'CCC': 1000,
  'CCC-': 1200,
  'CC': 1500,
  'C': 2000,
  'D': 3000,
  'NR': 150,
};

/**
 * Synthetic Quote Generator
 * Generates realistic bond quotes from yield curves
 */
export class SyntheticQuoteGenerator {
  private treasuryCurve: YieldCurve | null = null;
  private sofrCurve: YieldCurve | null = null;
  private corporateIGSpread: number = 110; // Default IG spread in bps
  private corporateHYSpread: number = 350; // Default HY spread in bps

  /**
   * Update curves from market data
   */
  setCurves(curves: YieldCurve[]): void {
    for (const curve of curves) {
      if (curve.id.includes('TREASURY') || curve.name.includes('Treasury')) {
        this.treasuryCurve = curve;
      } else if (curve.id.includes('SOFR') || curve.name.includes('SOFR')) {
        this.sofrCurve = curve;
      }
    }
  }

  /**
   * Update corporate spreads
   */
  setSpreads(igSpread: number, hySpread: number): void {
    this.corporateIGSpread = igSpread;
    this.corporateHYSpread = hySpread;
  }

  /**
   * Apply stress scenario to spreads and curves
   */
  applyStress(scenario: string): void {
    switch (scenario) {
      case 'rates_up_100bp':
        // Shift curves up 100bp
        if (this.treasuryCurve) {
          this.treasuryCurve.points = this.treasuryCurve.points.map(p => ({
            ...p,
            rate: p.rate + 1.0,
          }));
        }
        break;
      case 'rates_down_100bp':
        if (this.treasuryCurve) {
          this.treasuryCurve.points = this.treasuryCurve.points.map(p => ({
            ...p,
            rate: Math.max(0, p.rate - 1.0),
          }));
        }
        break;
      case 'spreads_wide_50bp':
        this.corporateIGSpread += 50;
        this.corporateHYSpread += 100;
        break;
      case 'spreads_tight_50bp':
        this.corporateIGSpread = Math.max(50, this.corporateIGSpread - 50);
        this.corporateHYSpread = Math.max(150, this.corporateHYSpread - 100);
        break;
      case 'flight_to_quality':
        // Treasury yields down, spreads wide
        if (this.treasuryCurve) {
          this.treasuryCurve.points = this.treasuryCurve.points.map(p => ({
            ...p,
            rate: Math.max(0, p.rate - 0.5),
          }));
        }
        this.corporateIGSpread += 75;
        this.corporateHYSpread += 200;
        break;
      case 'risk_on':
        // Spreads tight, rates up slightly
        if (this.treasuryCurve) {
          this.treasuryCurve.points = this.treasuryCurve.points.map(p => ({
            ...p,
            rate: p.rate + 0.25,
          }));
        }
        this.corporateIGSpread = Math.max(40, this.corporateIGSpread - 30);
        this.corporateHYSpread = Math.max(100, this.corporateHYSpread - 75);
        break;
    }
  }

  /**
   * Generate initial quote from curves
   */
  generateInitialQuote(instrument: BondInstrument): QuoteState {
    const yearsToMaturity = this.calculateYearsToMaturity(instrument.maturity);
    const yield_ = this.calculateYield(instrument, yearsToMaturity);
    const price = this.calculatePriceFromYield(
      instrument.coupon,
      yield_,
      yearsToMaturity
    );
    const bidAskHalf = this.getBidAskSpread(instrument.rating) / 2;

    return {
      instrument_id: instrument.id,
      bid: Math.round((price - bidAskHalf) * 1000) / 1000,
      mid: Math.round(price * 1000) / 1000,
      ask: Math.round((price + bidAskHalf) * 1000) / 1000,
      yield: Math.round(yield_ * 10000) / 10000,
      last_update: new Date().toISOString(),
    };
  }

  /**
   * Apply price tick (random walk)
   */
  tick(
    currentQuote: QuoteState,
    volatility: 'low' | 'medium' | 'high',
    mode: string
  ): QuoteState {
    const vol = VOLATILITY_MAP[volatility];
    let priceChange = 0;

    switch (mode) {
      case 'random_walk':
        // Simple random walk
        priceChange = this.normalRandom() * vol * currentQuote.mid;
        break;
      case 'mean_revert':
        // Mean-reverting with momentum
        const basePrice = 100; // Assume par
        const reversion = (basePrice - currentQuote.mid) * 0.01;
        priceChange = this.normalRandom() * vol * currentQuote.mid + reversion;
        break;
      case 'stress':
        // Higher volatility
        priceChange = this.normalRandom() * vol * 3 * currentQuote.mid;
        break;
      default: // static
        priceChange = 0;
    }

    const newMid = currentQuote.mid + priceChange;
    const bidAskHalf = (currentQuote.ask - currentQuote.bid) / 2;

    // Estimate yield change (inverse relationship with price)
    // Approximate: Δy ≈ -Δp / duration, assume duration ≈ 5 for simplicity
    const yieldChange = -priceChange / (5 * currentQuote.mid) * 100;

    return {
      instrument_id: currentQuote.instrument_id,
      bid: Math.round((newMid - bidAskHalf) * 1000) / 1000,
      mid: Math.round(newMid * 1000) / 1000,
      ask: Math.round((newMid + bidAskHalf) * 1000) / 1000,
      yield: Math.round((currentQuote.yield + yieldChange) * 10000) / 10000,
      last_update: new Date().toISOString(),
    };
  }

  /**
   * Calculate yield for a bond based on curves
   */
  private calculateYield(
    instrument: BondInstrument,
    yearsToMaturity: number
  ): number {
    // Get base Treasury rate
    const baseRate = this.interpolateRate(this.treasuryCurve, yearsToMaturity);

    // Add sector spread
    const sectorSpread = (SECTOR_SPREADS[instrument.sector] || 75) / 100;

    // Add rating spread
    const ratingSpread = (RATING_SPREADS[instrument.rating] || 150) / 100;

    // Add base corporate spread based on rating
    let corpSpread = 0;
    if (RATING_SPREADS[instrument.rating] !== undefined) {
      const ratingNum = RATING_SPREADS[instrument.rating];
      if (ratingNum >= 200) {
        // High yield
        corpSpread = this.corporateHYSpread / 100;
      } else {
        // Investment grade
        corpSpread = this.corporateIGSpread / 100;
      }
    }

    return baseRate + sectorSpread + ratingSpread + corpSpread;
  }

  /**
   * Interpolate rate from curve
   */
  private interpolateRate(curve: YieldCurve | null, years: number): number {
    if (!curve || curve.points.length === 0) {
      // Default fallback curve
      return 4.0 + years * 0.1; // Simple upward sloping
    }

    const points = curve.points.sort((a, b) => a.years - b.years);

    // Below curve
    if (years <= points[0].years) {
      return points[0].rate;
    }

    // Above curve
    if (years >= points[points.length - 1].years) {
      return points[points.length - 1].rate;
    }

    // Linear interpolation
    for (let i = 0; i < points.length - 1; i++) {
      if (years >= points[i].years && years <= points[i + 1].years) {
        const t =
          (years - points[i].years) / (points[i + 1].years - points[i].years);
        return points[i].rate + t * (points[i + 1].rate - points[i].rate);
      }
    }

    return 4.5; // Fallback
  }

  /**
   * Calculate price from yield (simplified bond math)
   */
  private calculatePriceFromYield(
    couponRate: number,
    yield_: number,
    yearsToMaturity: number
  ): number {
    // Simplified price calculation
    // P = C * [1 - (1+y)^-n] / y + 100 / (1+y)^n
    // Where C = annual coupon, y = yield (as decimal), n = years

    if (yearsToMaturity <= 0) return 100;

    const c = couponRate; // Annual coupon per 100 face
    const y = yield_ / 100; // Convert to decimal
    const n = yearsToMaturity;

    if (y <= 0) {
      // Avoid division by zero
      return 100 + c * n;
    }

    // PV of coupons
    const pvCoupons = c * (1 - Math.pow(1 + y, -n)) / y;

    // PV of principal
    const pvPrincipal = 100 / Math.pow(1 + y, n);

    return pvCoupons + pvPrincipal;
  }

  /**
   * Calculate years to maturity
   */
  private calculateYearsToMaturity(maturityDate: string): number {
    const maturity = new Date(maturityDate);
    const today = new Date();
    const years = (maturity.getTime() - today.getTime()) / (365.25 * 24 * 60 * 60 * 1000);
    return Math.max(0.01, years); // Minimum 0.01 years
  }

  /**
   * Get bid-ask spread for rating
   */
  private getBidAskSpread(rating: string): number {
    return BID_ASK_SPREADS[rating] || 0.5;
  }

  /**
   * Generate normally distributed random number (Box-Muller)
   */
  private normalRandom(): number {
    let u = 0, v = 0;
    while (u === 0) u = Math.random();
    while (v === 0) v = Math.random();
    return Math.sqrt(-2.0 * Math.log(u)) * Math.cos(2.0 * Math.PI * v);
  }
}

/**
 * Get sample bonds for simulation
 */
export function getSampleBonds(): BondInstrument[] {
  return [
    {
      id: 'AAPL-4.65-2046',
      cusip: '037833AK6',
      issuer: 'Apple Inc',
      coupon: 4.65,
      maturity: '2046-02-23',
      rating: 'AA+',
      sector: 'Technology',
      bond_reference: {
        instrument_id: 'AAPL-4.65-2046',
        description: 'Apple Inc 4.65% 2046',
        bond_type: 'FixedBullet',
        issuer_type: 'CorporateIG',
        cusip: '037833AK6',
        coupon_rate: 0.0465,
        frequency: 2,
        maturity_date: '2046-02-23',
        issue_date: '2016-02-23',
        day_count: 'Thirty360',
        currency: 'USD',
        face_value: 100,
        issuer_id: 'AAPL',
        issuer_name: 'Apple Inc',
        seniority: 'Senior',
        is_callable: false,
        is_putable: false,
        is_sinkable: false,
        call_schedule: [],
        has_deflation_floor: false,
        country_of_risk: 'US',
        sector: 'Technology',
        last_updated: Date.now(),
        source: 'Demo',
      },
    },
    {
      id: 'MSFT-3.50-2042',
      cusip: '594918BG8',
      issuer: 'Microsoft Corp',
      coupon: 3.50,
      maturity: '2042-02-12',
      rating: 'AAA',
      sector: 'Technology',
      bond_reference: {
        instrument_id: 'MSFT-3.50-2042',
        description: 'Microsoft Corp 3.50% 2042',
        bond_type: 'FixedBullet',
        issuer_type: 'CorporateIG',
        cusip: '594918BG8',
        coupon_rate: 0.035,
        frequency: 2,
        maturity_date: '2042-02-12',
        issue_date: '2012-02-12',
        day_count: 'Thirty360',
        currency: 'USD',
        face_value: 100,
        issuer_id: 'MSFT',
        issuer_name: 'Microsoft Corp',
        seniority: 'Senior',
        is_callable: false,
        is_putable: false,
        is_sinkable: false,
        call_schedule: [],
        has_deflation_floor: false,
        country_of_risk: 'US',
        sector: 'Technology',
        last_updated: Date.now(),
        source: 'Demo',
      },
    },
    {
      id: 'JPM-5.25-2034',
      cusip: '46625HJE5',
      issuer: 'JPMorgan Chase',
      coupon: 5.25,
      maturity: '2034-07-15',
      rating: 'A-',
      sector: 'Financials',
      bond_reference: {
        instrument_id: 'JPM-5.25-2034',
        description: 'JPMorgan Chase 5.25% 2034',
        bond_type: 'FixedBullet',
        issuer_type: 'Financial',
        cusip: '46625HJE5',
        coupon_rate: 0.0525,
        frequency: 2,
        maturity_date: '2034-07-15',
        issue_date: '2024-07-15',
        day_count: 'Thirty360',
        currency: 'USD',
        face_value: 100,
        issuer_id: 'JPM',
        issuer_name: 'JPMorgan Chase',
        seniority: 'Senior',
        is_callable: false,
        is_putable: false,
        is_sinkable: false,
        call_schedule: [],
        has_deflation_floor: false,
        country_of_risk: 'US',
        sector: 'Financials',
        last_updated: Date.now(),
        source: 'Demo',
      },
    },
    {
      id: 'VZ-4.50-2033',
      cusip: '92343VEP1',
      issuer: 'Verizon',
      coupon: 4.50,
      maturity: '2033-08-10',
      rating: 'BBB+',
      sector: 'Telecom',
      bond_reference: {
        instrument_id: 'VZ-4.50-2033',
        description: 'Verizon 4.50% 2033',
        bond_type: 'FixedBullet',
        issuer_type: 'CorporateIG',
        cusip: '92343VEP1',
        coupon_rate: 0.045,
        frequency: 2,
        maturity_date: '2033-08-10',
        issue_date: '2023-08-10',
        day_count: 'Thirty360',
        currency: 'USD',
        face_value: 100,
        issuer_id: 'VZ',
        issuer_name: 'Verizon',
        seniority: 'Senior',
        is_callable: false,
        is_putable: false,
        is_sinkable: false,
        call_schedule: [],
        has_deflation_floor: false,
        country_of_risk: 'US',
        sector: 'Telecom',
        last_updated: Date.now(),
        source: 'Demo',
      },
    },
    {
      id: 'F-6.10-2032',
      cusip: '345370CQ2',
      issuer: 'Ford Motor Co',
      coupon: 6.10,
      maturity: '2032-08-19',
      rating: 'BB+',
      sector: 'Autos',
      bond_reference: {
        instrument_id: 'F-6.10-2032',
        description: 'Ford Motor Co 6.10% 2032',
        bond_type: 'FixedBullet',
        issuer_type: 'CorporateHY',
        cusip: '345370CQ2',
        coupon_rate: 0.061,
        frequency: 2,
        maturity_date: '2032-08-19',
        issue_date: '2022-08-19',
        day_count: 'Thirty360',
        currency: 'USD',
        face_value: 100,
        issuer_id: 'F',
        issuer_name: 'Ford Motor Co',
        seniority: 'Senior',
        is_callable: false,
        is_putable: false,
        is_sinkable: false,
        call_schedule: [],
        has_deflation_floor: false,
        country_of_risk: 'US',
        sector: 'Autos',
        last_updated: Date.now(),
        source: 'Demo',
      },
    },
    {
      id: 'OXY-6.45-2036',
      cusip: '172967LS8',
      issuer: 'Occidental Petroleum',
      coupon: 6.45,
      maturity: '2036-09-15',
      rating: 'BB',
      sector: 'Energy',
      bond_reference: {
        instrument_id: 'OXY-6.45-2036',
        description: 'Occidental Petroleum 6.45% 2036',
        bond_type: 'FixedBullet',
        issuer_type: 'CorporateHY',
        cusip: '172967LS8',
        coupon_rate: 0.0645,
        frequency: 2,
        maturity_date: '2036-09-15',
        issue_date: '2021-09-15',
        day_count: 'Thirty360',
        currency: 'USD',
        face_value: 100,
        issuer_id: 'OXY',
        issuer_name: 'Occidental Petroleum',
        seniority: 'Senior',
        is_callable: false,
        is_putable: false,
        is_sinkable: false,
        call_schedule: [],
        has_deflation_floor: false,
        country_of_risk: 'US',
        sector: 'Energy',
        last_updated: Date.now(),
        source: 'Demo',
      },
    },
    {
      id: 'T-4.00-2042',
      cusip: '912810TA6',
      issuer: 'US Treasury',
      coupon: 4.00,
      maturity: '2042-11-15',
      rating: 'AAA',
      sector: 'Government',
      bond_reference: {
        instrument_id: 'T-4.00-2042',
        description: 'US Treasury 4.00% 2042',
        bond_type: 'FixedBullet',
        issuer_type: 'Sovereign',
        cusip: '912810TA6',
        coupon_rate: 0.04,
        frequency: 2,
        maturity_date: '2042-11-15',
        issue_date: '2012-11-15',
        day_count: 'ActualActual',
        currency: 'USD',
        face_value: 100,
        issuer_id: 'UST',
        issuer_name: 'US Treasury',
        seniority: 'Senior',
        is_callable: false,
        is_putable: false,
        is_sinkable: false,
        call_schedule: [],
        has_deflation_floor: false,
        country_of_risk: 'US',
        sector: 'Government',
        last_updated: Date.now(),
        source: 'Demo',
      },
    },
    {
      id: 'CVS-5.05-2048',
      cusip: '126650CZ6',
      issuer: 'CVS Health',
      coupon: 5.05,
      maturity: '2048-03-25',
      rating: 'BBB',
      sector: 'Healthcare',
      bond_reference: {
        instrument_id: 'CVS-5.05-2048',
        description: 'CVS Health 5.05% 2048',
        bond_type: 'FixedBullet',
        issuer_type: 'CorporateIG',
        cusip: '126650CZ6',
        coupon_rate: 0.0505,
        frequency: 2,
        maturity_date: '2048-03-25',
        issue_date: '2018-03-25',
        day_count: 'Thirty360',
        currency: 'USD',
        face_value: 100,
        issuer_id: 'CVS',
        issuer_name: 'CVS Health',
        seniority: 'Senior',
        is_callable: false,
        is_putable: false,
        is_sinkable: false,
        call_schedule: [],
        has_deflation_floor: false,
        country_of_risk: 'US',
        sector: 'Healthcare',
        last_updated: Date.now(),
        source: 'Demo',
      },
    },
  ];
}
