import { useState, useCallback } from 'react';
import { useMutation } from '@tanstack/react-query';
import { Calculator, RefreshCw, TrendingUp, ChevronDown, ChevronUp, AlertCircle } from 'lucide-react';
import { formatBps, cn } from '../lib/utils';
import { priceBondWithDetails, BondQuoteResponse } from '../lib/api';

// Helper to safely parse numeric values (API returns strings)
const toNum = (val: unknown): number => {
  if (val === null || val === undefined) return 0;
  const n = typeof val === 'string' ? parseFloat(val) : Number(val);
  return isNaN(n) ? 0 : n;
};

// Format number with specified decimals
const fmt = (val: unknown, decimals: number = 2): string => {
  return toNum(val).toFixed(decimals);
};

// Local fallback bond calculations when API is unavailable
function calculateBondLocally(
  coupon: number,
  maturityDate: string,
  price: number,
  frequency: number
): BondQuoteResponse {
  const today = new Date();
  const maturity = new Date(maturityDate);
  const yearsToMaturity = (maturity.getTime() - today.getTime()) / (365.25 * 24 * 60 * 60 * 1000);

  // Simple YTM approximation (Current Yield + Capital Gain Yield)
  const couponPayment = coupon;
  const currentYield = couponPayment / price;
  const capitalGainYield = (100 - price) / yearsToMaturity / price;
  const ytm = currentYield + capitalGainYield;

  // Modified Duration approximation
  const modDuration = (1 - Math.pow(1 + ytm / frequency, -yearsToMaturity * frequency)) / (ytm / frequency);

  // Macaulay Duration
  const macDuration = modDuration * (1 + ytm / frequency);

  // Convexity approximation
  const convexity = modDuration * modDuration / (1 + ytm / frequency);

  // DV01 = Modified Duration × Price × 0.0001
  const dv01 = modDuration * price * 0.0001;

  // Accrued interest (simplified - assume mid-period)
  const periodFraction = 0.5;
  const accruedInterest = (coupon / frequency) * periodFraction;

  // Z-spread approximation (using Treasury + 50bp for corporate)
  const treasuryRate = 0.04; // Approximate 10Y Treasury
  const zSpread = ytm - treasuryRate;

  return {
    instrument_id: 'CALC-LOCAL',
    currency: 'USD',
    settlement_date: today.toISOString().split('T')[0],
    clean_price_mid: price,
    accrued_interest: accruedInterest,
    ytm_mid: ytm,
    modified_duration: modDuration > 0 ? modDuration : yearsToMaturity * 0.9,
    macaulay_duration: macDuration > 0 ? macDuration : yearsToMaturity,
    convexity: convexity > 0 ? convexity : yearsToMaturity * 2,
    dv01: dv01 > 0 ? dv01 : modDuration * price / 10000,
    z_spread_mid: zSpread > 0 ? zSpread : 0.005,
  };
}

type SolveMode = 'price' | 'yield';
type BondTypeInput = 'fixed' | 'callable' | 'frn';
type DayCount = '30/360' | 'ACT/360' | 'ACT/365' | 'ACT/ACT';

interface BondInput {
  instrumentId: string;
  issuer: string;
  coupon: number;
  maturityDate: string;
  issueDate: string;
  frequency: number;
  dayCount: DayCount;
  bondType: BondTypeInput;
  // For callable
  callDate?: string;
  callPrice?: number;
  // For FRN
  spread?: number;
  index?: string;
}

const DEFAULT_BOND: BondInput = {
  instrumentId: 'DEMO-BOND-001',
  issuer: 'Demo Corp',
  coupon: 5.0,
  maturityDate: '2030-06-15',
  issueDate: '2020-06-15',
  frequency: 2,
  dayCount: '30/360',
  bondType: 'fixed',
};

export default function BondCalculator() {
  const [solveMode, setSolveMode] = useState<SolveMode>('yield');
  const [bondInput, setBondInput] = useState<BondInput>(DEFAULT_BOND);
  const [priceInput, setPriceInput] = useState<number>(100.0);
  const [yieldInput, setYieldInput] = useState<number>(5.0);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [quote, setQuote] = useState<BondQuoteResponse | null>(null);
  const [usingFallback, setUsingFallback] = useState(false);

  const pricingMutation = useMutation({
    mutationFn: async (): Promise<BondQuoteResponse> => {
      const today = new Date().toISOString().split('T')[0];

      // Build the bond reference data for the API
      // Generate a placeholder CUSIP (9 chars) based on instrument_id
      const generateCusip = (id: string): string => {
        return id.replace(/[^A-Z0-9]/gi, '').toUpperCase().padEnd(9, '0').slice(0, 9);
      };

      const bondData = {
        bond: {
          instrument_id: bondInput.instrumentId,
          isin: null,
          cusip: generateCusip(bondInput.instrumentId),
          sedol: null,
          bbgid: null,
          description: `${bondInput.issuer} ${bondInput.coupon}% ${bondInput.maturityDate.slice(0, 4)}`,
          currency: 'USD',
          issue_date: bondInput.issueDate,
          maturity_date: bondInput.maturityDate,
          coupon_rate: bondInput.bondType === 'frn' ? null : bondInput.coupon / 100,
          frequency: bondInput.frequency,
          day_count: bondInput.dayCount,
          face_value: 100,
          bond_type: bondInput.bondType === 'fixed' ? 'FixedBullet' :
                     bondInput.bondType === 'callable' ? 'FixedCallable' : 'FloatingRate',
          issuer_type: 'CorporateIG',
          issuer_id: bondInput.instrumentId.split('-')[0],
          issuer_name: bondInput.issuer,
          seniority: 'Senior',
          is_callable: bondInput.bondType === 'callable',
          call_schedule: bondInput.bondType === 'callable' && bondInput.callDate ? [
            { call_date: bondInput.callDate, call_price: bondInput.callPrice || 100, is_make_whole: false }
          ] : [],
          is_putable: false,
          is_sinkable: false,
          floating_terms: bondInput.bondType === 'frn' ? {
            spread: (bondInput.spread || 0) / 10000,
            index: bondInput.index || 'SOFR',
            reset_frequency: bondInput.frequency,
            current_rate: null,
            cap: null,
            floor: null,
          } : null,
          inflation_index: null,
          inflation_base_index: null,
          has_deflation_floor: false,
          country_of_risk: 'USA',
          sector: 'Corporate',
          amount_outstanding: null,
          first_coupon_date: null,
          last_updated: Math.floor(Date.now() / 1000),
          source: 'Demo',
        },
        settlement_date: today,
        market_price: solveMode === 'yield' ? priceInput : null,
      };

      try {
        const result = await priceBondWithDetails(bondData);
        setUsingFallback(false);
        return result;
      } catch {
        // API failed - use local fallback calculations
        console.log('API unavailable, using local fallback calculations');
        setUsingFallback(true);
        return calculateBondLocally(
          bondInput.coupon,
          bondInput.maturityDate,
          priceInput,
          bondInput.frequency
        );
      }
    },
    onSuccess: (data) => {
      setQuote(data);
      // If solving for price, update the price input from the result
      if (solveMode === 'price' && data.clean_price_mid) {
        setPriceInput(data.clean_price_mid);
      }
      // If solving for yield, update the yield input from the result
      if (solveMode === 'yield' && data.ytm_mid) {
        setYieldInput(data.ytm_mid * 100);
      }
    },
  });

  const handleCalculate = useCallback(() => {
    pricingMutation.mutate();
  }, [pricingMutation]);

  const handleBondInputChange = (field: keyof BondInput, value: string | number) => {
    setBondInput(prev => ({ ...prev, [field]: value }));
  };

  const handlePresetSelect = (preset: 'apple' | 'treasury' | 'callable' | 'frn') => {
    switch (preset) {
      case 'apple':
        setBondInput({
          instrumentId: 'AAPL-5.0-2030',
          issuer: 'Apple Inc',
          coupon: 5.0,
          maturityDate: '2030-06-15',
          issueDate: '2020-06-15',
          frequency: 2,
          dayCount: '30/360',
          bondType: 'fixed',
        });
        setPriceInput(102.345);
        break;
      case 'treasury':
        setBondInput({
          instrumentId: 'UST-10Y-2034',
          issuer: 'US Treasury',
          coupon: 4.0,
          maturityDate: '2034-12-15',
          issueDate: '2024-12-15',
          frequency: 2,
          dayCount: 'ACT/ACT',
          bondType: 'fixed',
        });
        setPriceInput(99.5);
        break;
      case 'callable':
        setBondInput({
          instrumentId: 'ATT-5.5-2035-CALL',
          issuer: 'AT&T Inc',
          coupon: 5.5,
          maturityDate: '2035-06-15',
          issueDate: '2020-06-15',
          frequency: 2,
          dayCount: '30/360',
          bondType: 'callable',
          callDate: '2030-06-15',
          callPrice: 102.0,
        });
        setPriceInput(98.567);
        break;
      case 'frn':
        setBondInput({
          instrumentId: 'GS-FRN-2027',
          issuer: 'Goldman Sachs',
          coupon: 0,
          maturityDate: '2027-08-15',
          issueDate: '2024-08-15',
          frequency: 4,
          dayCount: 'ACT/360',
          bondType: 'frn',
          spread: 95,
          index: 'Sofr',
        });
        setPriceInput(99.875);
        break;
    }
  };

  // Calculate sensitivity grid
  const sensitivityGrid = quote ? generateSensitivityGrid(
    toNum(quote.clean_price_mid) || priceInput,
    quote.ytm_mid ? toNum(quote.ytm_mid) * 100 : yieldInput,
    toNum(quote.modified_duration),
    toNum(quote.convexity)
  ) : null;

  return (
    <div className="space-y-6">
      {/* Preset Bonds */}
      <div className="card">
        <div className="text-xs text-slate-500 uppercase mb-3">Quick Presets</div>
        <div className="flex flex-wrap gap-2">
          <button
            onClick={() => handlePresetSelect('apple')}
            className="btn btn-secondary text-sm"
          >
            Apple 5% 2030
          </button>
          <button
            onClick={() => handlePresetSelect('treasury')}
            className="btn btn-secondary text-sm"
          >
            10Y Treasury
          </button>
          <button
            onClick={() => handlePresetSelect('callable')}
            className="btn btn-secondary text-sm"
          >
            AT&T Callable
          </button>
          <button
            onClick={() => handlePresetSelect('frn')}
            className="btn btn-secondary text-sm"
          >
            Goldman FRN
          </button>
        </div>
      </div>

      <div className="grid lg:grid-cols-2 gap-6">
        {/* Input Panel */}
        <div className="card">
          <h3 className="card-header flex items-center gap-2">
            <Calculator className="w-5 h-5 text-primary-600" />
            Bond Parameters
          </h3>

          <div className="space-y-4">
            {/* Solve Mode Toggle */}
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-2">Solve For</label>
              <div className="flex gap-2">
                <button
                  onClick={() => setSolveMode('yield')}
                  className={cn(
                    'flex-1 py-2 px-4 rounded-lg text-sm font-medium transition-colors',
                    solveMode === 'yield'
                      ? 'bg-primary-600 text-white'
                      : 'bg-slate-100 text-slate-700 hover:bg-slate-200'
                  )}
                >
                  Yield (from Price)
                </button>
                <button
                  onClick={() => setSolveMode('price')}
                  className={cn(
                    'flex-1 py-2 px-4 rounded-lg text-sm font-medium transition-colors',
                    solveMode === 'price'
                      ? 'bg-primary-600 text-white'
                      : 'bg-slate-100 text-slate-700 hover:bg-slate-200'
                  )}
                >
                  Price (from Yield)
                </button>
              </div>
            </div>

            {/* Price/Yield Input */}
            <div className="grid grid-cols-2 gap-4">
              <div>
                <label className="block text-sm font-medium text-slate-700 mb-1">
                  Clean Price
                </label>
                <input
                  type="number"
                  value={priceInput}
                  onChange={(e) => setPriceInput(parseFloat(e.target.value) || 0)}
                  disabled={solveMode === 'price'}
                  step="0.001"
                  className={cn(
                    'input w-full',
                    solveMode === 'price' && 'bg-slate-50 text-slate-500'
                  )}
                />
              </div>
              <div>
                <label className="block text-sm font-medium text-slate-700 mb-1">
                  Yield (%)
                </label>
                <input
                  type="number"
                  value={yieldInput}
                  onChange={(e) => setYieldInput(parseFloat(e.target.value) || 0)}
                  disabled={solveMode === 'yield'}
                  step="0.01"
                  className={cn(
                    'input w-full',
                    solveMode === 'yield' && 'bg-slate-50 text-slate-500'
                  )}
                />
              </div>
            </div>

            {/* Bond Type */}
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-1">Bond Type</label>
              <select
                value={bondInput.bondType}
                onChange={(e) => handleBondInputChange('bondType', e.target.value as BondTypeInput)}
                className="input w-full"
              >
                <option value="fixed">Fixed Rate</option>
                <option value="callable">Callable</option>
                <option value="frn">Floating Rate (FRN)</option>
              </select>
            </div>

            {/* Coupon / Spread */}
            <div className="grid grid-cols-2 gap-4">
              {bondInput.bondType === 'frn' ? (
                <>
                  <div>
                    <label className="block text-sm font-medium text-slate-700 mb-1">
                      Index
                    </label>
                    <select
                      value={bondInput.index || 'Sofr'}
                      onChange={(e) => handleBondInputChange('index', e.target.value)}
                      className="input w-full"
                    >
                      <option value="Sofr">SOFR</option>
                      <option value="Estr">ESTR</option>
                      <option value="Sonia">SONIA</option>
                    </select>
                  </div>
                  <div>
                    <label className="block text-sm font-medium text-slate-700 mb-1">
                      Spread (bps)
                    </label>
                    <input
                      type="number"
                      value={bondInput.spread || 0}
                      onChange={(e) => handleBondInputChange('spread', parseFloat(e.target.value) || 0)}
                      className="input w-full"
                    />
                  </div>
                </>
              ) : (
                <>
                  <div>
                    <label className="block text-sm font-medium text-slate-700 mb-1">
                      Coupon (%)
                    </label>
                    <input
                      type="number"
                      value={bondInput.coupon}
                      onChange={(e) => handleBondInputChange('coupon', parseFloat(e.target.value) || 0)}
                      step="0.125"
                      className="input w-full"
                    />
                  </div>
                  <div>
                    <label className="block text-sm font-medium text-slate-700 mb-1">
                      Frequency
                    </label>
                    <select
                      value={bondInput.frequency}
                      onChange={(e) => handleBondInputChange('frequency', parseInt(e.target.value))}
                      className="input w-full"
                    >
                      <option value={1}>Annual</option>
                      <option value={2}>Semi-Annual</option>
                      <option value={4}>Quarterly</option>
                    </select>
                  </div>
                </>
              )}
            </div>

            {/* Maturity Date */}
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-1">Maturity Date</label>
              <input
                type="date"
                value={bondInput.maturityDate}
                onChange={(e) => handleBondInputChange('maturityDate', e.target.value)}
                className="input w-full"
              />
            </div>

            {/* Callable Options */}
            {bondInput.bondType === 'callable' && (
              <div className="grid grid-cols-2 gap-4 p-3 bg-amber-50 rounded-lg">
                <div>
                  <label className="block text-sm font-medium text-amber-800 mb-1">
                    First Call Date
                  </label>
                  <input
                    type="date"
                    value={bondInput.callDate || ''}
                    onChange={(e) => handleBondInputChange('callDate', e.target.value)}
                    className="input w-full"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-amber-800 mb-1">
                    Call Price
                  </label>
                  <input
                    type="number"
                    value={bondInput.callPrice || 100}
                    onChange={(e) => handleBondInputChange('callPrice', parseFloat(e.target.value) || 100)}
                    step="0.01"
                    className="input w-full"
                  />
                </div>
              </div>
            )}

            {/* Advanced Options Toggle */}
            <button
              onClick={() => setShowAdvanced(!showAdvanced)}
              className="flex items-center gap-2 text-sm text-slate-600 hover:text-slate-900"
            >
              {showAdvanced ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
              Advanced Options
            </button>

            {showAdvanced && (
              <div className="space-y-4 p-3 bg-slate-50 rounded-lg">
                <div className="grid grid-cols-2 gap-4">
                  <div>
                    <label className="block text-sm font-medium text-slate-700 mb-1">
                      Issue Date
                    </label>
                    <input
                      type="date"
                      value={bondInput.issueDate}
                      onChange={(e) => handleBondInputChange('issueDate', e.target.value)}
                      className="input w-full"
                    />
                  </div>
                  <div>
                    <label className="block text-sm font-medium text-slate-700 mb-1">
                      Day Count
                    </label>
                    <select
                      value={bondInput.dayCount}
                      onChange={(e) => handleBondInputChange('dayCount', e.target.value as DayCount)}
                      className="input w-full"
                    >
                      <option value="30/360">30/360</option>
                      <option value="ACT/360">ACT/360</option>
                      <option value="ACT/365">ACT/365</option>
                      <option value="ACT/ACT">ACT/ACT</option>
                    </select>
                  </div>
                </div>
                <div>
                  <label className="block text-sm font-medium text-slate-700 mb-1">
                    Issuer Name
                  </label>
                  <input
                    type="text"
                    value={bondInput.issuer}
                    onChange={(e) => handleBondInputChange('issuer', e.target.value)}
                    className="input w-full"
                  />
                </div>
              </div>
            )}

            {/* Calculate Button */}
            <button
              onClick={handleCalculate}
              disabled={pricingMutation.isPending}
              className="btn btn-primary w-full flex items-center justify-center gap-2"
            >
              {pricingMutation.isPending ? (
                <RefreshCw className="w-4 h-4 animate-spin" />
              ) : (
                <Calculator className="w-4 h-4" />
              )}
              {pricingMutation.isPending ? 'Calculating...' : 'Calculate Analytics'}
            </button>

          </div>
        </div>

        {/* Results Panel */}
        <div className="space-y-6">
          {quote ? (
            <>
              {/* Fallback Mode Warning */}
              {usingFallback && (
                <div className="flex items-start gap-3 p-4 bg-amber-50 border border-amber-200 rounded-lg">
                  <AlertCircle className="w-5 h-5 text-amber-600 flex-shrink-0 mt-0.5" />
                  <div>
                    <div className="font-medium text-amber-800">Using Local Calculations</div>
                    <div className="text-sm text-amber-700 mt-1">
                      The pricing API is unavailable. Results shown are simplified approximations
                      calculated in the browser. For production-grade analytics, please ensure
                      the backend service is running.
                    </div>
                  </div>
                </div>
              )}

              {/* Price & Yield */}
              <div className="card">
                <h3 className="card-header flex items-center gap-2">
                  <TrendingUp className="w-5 h-5 text-primary-600" />
                  Pricing Results
                </h3>
                <div className="grid grid-cols-2 gap-4">
                  <MetricCard
                    label="Clean Price"
                    value={fmt(quote.clean_price_mid, 3)}
                    highlight={solveMode === 'price'}
                  />
                  <MetricCard
                    label="Dirty Price"
                    value={fmt(toNum(quote.clean_price_mid) + toNum(quote.accrued_interest), 3)}
                  />
                  <MetricCard
                    label={bondInput.bondType === 'frn' ? 'YTM (N/A for FRN)' : 'YTM'}
                    value={quote.ytm_mid ? `${fmt(toNum(quote.ytm_mid) * 100, 3)}%` :
                           quote.discount_margin_mid ? `DM: ${fmt(toNum(quote.discount_margin_mid) * 10000, 1)} bps` : 'N/A'}
                    highlight={solveMode === 'yield'}
                  />
                  <MetricCard
                    label="Accrued Interest"
                    value={fmt(quote.accrued_interest, 4)}
                  />
                  {quote.ytw && (
                    <MetricCard
                      label="YTW"
                      value={`${fmt(toNum(quote.ytw) * 100, 3)}%`}
                      variant="warning"
                    />
                  )}
                  {quote.ytc && (
                    <MetricCard
                      label="YTC"
                      value={`${fmt(toNum(quote.ytc) * 100, 3)}%`}
                    />
                  )}
                </div>
              </div>

              {/* Risk Metrics */}
              <div className="card">
                <h3 className="card-header">Risk Metrics</h3>
                <div className="grid grid-cols-2 gap-4">
                  <MetricCard
                    label="Modified Duration"
                    value={fmt(quote.modified_duration, 2)}
                  />
                  {quote.macaulay_duration && (
                    <MetricCard
                      label="Macaulay Duration"
                      value={fmt(quote.macaulay_duration, 2)}
                    />
                  )}
                  <MetricCard
                    label="Convexity"
                    value={fmt(quote.convexity, 2)}
                  />
                  {quote.dv01 && (
                    <MetricCard
                      label="DV01"
                      value={`$${fmt(quote.dv01, 4)}`}
                    />
                  )}
                  {quote.effective_duration && (
                    <MetricCard
                      label="Effective Duration"
                      value={fmt(quote.effective_duration, 2)}
                      variant="warning"
                    />
                  )}
                  {quote.spread_duration && (
                    <MetricCard
                      label="Spread Duration"
                      value={fmt(quote.spread_duration, 2)}
                    />
                  )}
                </div>
              </div>

              {/* Spreads */}
              <div className="card">
                <h3 className="card-header">Spread Analysis</h3>
                <div className="grid grid-cols-2 gap-4">
                  {quote.z_spread_mid !== null && quote.z_spread_mid !== undefined && (
                    <MetricCard
                      label="Z-Spread"
                      value={formatBps(toNum(quote.z_spread_mid))}
                    />
                  )}
                  {quote.i_spread_mid !== null && quote.i_spread_mid !== undefined && (
                    <MetricCard
                      label="I-Spread"
                      value={formatBps(toNum(quote.i_spread_mid))}
                    />
                  )}
                  {quote.g_spread_mid !== null && quote.g_spread_mid !== undefined && (
                    <MetricCard
                      label="G-Spread"
                      value={formatBps(toNum(quote.g_spread_mid))}
                    />
                  )}
                  {quote.asw_mid !== null && quote.asw_mid !== undefined && (
                    <MetricCard
                      label="ASW"
                      value={formatBps(toNum(quote.asw_mid))}
                    />
                  )}
                  {quote.oas_mid !== null && quote.oas_mid !== undefined && (
                    <MetricCard
                      label="OAS"
                      value={formatBps(toNum(quote.oas_mid))}
                      variant="warning"
                    />
                  )}
                  {quote.discount_margin_mid !== null && quote.discount_margin_mid !== undefined && (
                    <MetricCard
                      label="Discount Margin"
                      value={formatBps(toNum(quote.discount_margin_mid) * 10000)}
                      variant="info"
                    />
                  )}
                  {quote.simple_margin_mid !== null && quote.simple_margin_mid !== undefined && (
                    <MetricCard
                      label="Simple Margin"
                      value={formatBps(toNum(quote.simple_margin_mid))}
                      variant="info"
                    />
                  )}
                </div>
              </div>

              {/* Sensitivity Grid */}
              {sensitivityGrid && (
                <div className="card">
                  <h3 className="card-header">Price Sensitivity</h3>
                  <div className="overflow-x-auto">
                    <table className="w-full text-sm">
                      <thead>
                        <tr className="bg-slate-50">
                          <th className="py-2 px-3 text-left font-medium text-slate-600">Yield Change</th>
                          <th className="py-2 px-3 text-right font-medium text-slate-600">New Yield</th>
                          <th className="py-2 px-3 text-right font-medium text-slate-600">New Price</th>
                          <th className="py-2 px-3 text-right font-medium text-slate-600">Price Change</th>
                          <th className="py-2 px-3 text-right font-medium text-slate-600">% Change</th>
                        </tr>
                      </thead>
                      <tbody>
                        {sensitivityGrid.map((row, i) => (
                          <tr key={i} className={cn(
                            'border-t border-slate-100',
                            row.yieldChange === 0 && 'bg-primary-50 font-medium'
                          )}>
                            <td className="py-2 px-3">{row.yieldChange > 0 ? '+' : ''}{row.yieldChange} bps</td>
                            <td className="py-2 px-3 text-right font-mono">{fmt(row.newYield, 3)}%</td>
                            <td className="py-2 px-3 text-right font-mono">{fmt(row.newPrice, 3)}</td>
                            <td className={cn(
                              'py-2 px-3 text-right font-mono',
                              row.priceChange > 0 ? 'text-gain' : row.priceChange < 0 ? 'text-loss' : ''
                            )}>
                              {row.priceChange > 0 ? '+' : ''}{fmt(row.priceChange, 3)}
                            </td>
                            <td className={cn(
                              'py-2 px-3 text-right font-mono',
                              row.pctChange > 0 ? 'text-gain' : row.pctChange < 0 ? 'text-loss' : ''
                            )}>
                              {row.pctChange > 0 ? '+' : ''}{fmt(row.pctChange, 2)}%
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </div>
              )}
            </>
          ) : (
            <div className="card text-center py-12">
              <Calculator className="w-12 h-12 text-slate-300 mx-auto mb-4" />
              <p className="text-slate-500">
                Enter bond parameters and click Calculate to see analytics
              </p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function MetricCard({
  label,
  value,
  highlight = false,
  variant = 'default',
}: {
  label: string;
  value: string;
  highlight?: boolean;
  variant?: 'default' | 'warning' | 'info';
}) {
  return (
    <div className={cn(
      'p-3 rounded-lg',
      highlight ? 'bg-primary-50 ring-2 ring-primary-200' :
      variant === 'warning' ? 'bg-amber-50' :
      variant === 'info' ? 'bg-blue-50' :
      'bg-slate-50'
    )}>
      <div className={cn(
        'text-xs uppercase mb-1',
        highlight ? 'text-primary-600' :
        variant === 'warning' ? 'text-amber-600' :
        variant === 'info' ? 'text-blue-600' :
        'text-slate-500'
      )}>
        {label}
      </div>
      <div className={cn(
        'text-lg font-mono font-semibold',
        highlight ? 'text-primary-900' : 'text-slate-900'
      )}>
        {value}
      </div>
    </div>
  );
}

function generateSensitivityGrid(
  basePrice: number,
  baseYield: number,
  duration: number,
  convexity: number
) {
  const shifts = [-50, -25, -10, -1, 0, 1, 10, 25, 50];

  return shifts.map(bps => {
    const yieldChange = bps / 100; // Convert bps to %
    const newYield = baseYield + yieldChange;

    // Price change using duration + convexity approximation
    // ΔP/P ≈ -D × Δy + 0.5 × C × (Δy)²
    const dy = yieldChange / 100; // Convert to decimal
    const pctChange = (-duration * dy + 0.5 * convexity * dy * dy) * 100;
    const priceChange = basePrice * pctChange / 100;
    const newPrice = basePrice + priceChange;

    return {
      yieldChange: bps,
      newYield,
      newPrice,
      priceChange,
      pctChange,
    };
  });
}
