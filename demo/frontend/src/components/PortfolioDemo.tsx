import { useState, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  PieChart,
  Pie,
  Cell,
} from 'recharts';
import { Wifi, WifiOff, RefreshCw, AlertCircle } from 'lucide-react';
import { formatNumber, formatBps, cn } from '../lib/utils';
import {
  priceBondWithDetails,
  calculatePortfolioAnalytics,
  calculateKeyRateDuration,
  runStandardStressTest,
  BondQuoteResponse,
  BondReferenceInput,
} from '../lib/api';

// Sample portfolio holdings with bond details
interface PortfolioHolding {
  id: string;
  issuer: string;
  cusip: string;
  coupon: number;
  maturity: string;
  issueDate: string;
  sector: string;
  rating: string;
  notional: number; // Face value held
  frequency: number;
  dayCount: string;
}

const SAMPLE_HOLDINGS: PortfolioHolding[] = [
  { id: 'AAPL-5.0-2030', issuer: 'Apple Inc', cusip: '037833AK6', coupon: 5.0, maturity: '2030-06-15', issueDate: '2020-06-15', sector: 'Technology', rating: 'AA+', notional: 5000000, frequency: 2, dayCount: '30/360' },
  { id: 'MSFT-4.5-2032', issuer: 'Microsoft Corp', cusip: '594918BG8', coupon: 4.5, maturity: '2032-03-15', issueDate: '2022-03-15', sector: 'Technology', rating: 'AAA', notional: 4500000, frequency: 2, dayCount: '30/360' },
  { id: 'JPM-5.25-2035', issuer: 'JPMorgan Chase', cusip: '46625HJE5', coupon: 5.25, maturity: '2035-09-01', issueDate: '2020-09-01', sector: 'Financials', rating: 'A-', notional: 6000000, frequency: 2, dayCount: '30/360' },
  { id: 'VZ-4.5-2033', issuer: 'Verizon', cusip: '92343VEP1', coupon: 4.5, maturity: '2033-08-10', issueDate: '2023-08-10', sector: 'Communications', rating: 'BBB+', notional: 4000000, frequency: 2, dayCount: '30/360' },
  { id: 'XOM-4.23-2039', issuer: 'Exxon Mobil', cusip: '30231GAV4', coupon: 4.23, maturity: '2039-03-01', issueDate: '2019-03-01', sector: 'Energy', rating: 'AA-', notional: 3500000, frequency: 2, dayCount: '30/360' },
  { id: 'CVS-5.05-2048', issuer: 'CVS Health', cusip: '126650CZ6', coupon: 5.05, maturity: '2048-03-25', issueDate: '2018-03-25', sector: 'Healthcare', rating: 'BBB', notional: 3000000, frequency: 2, dayCount: '30/360' },
  { id: 'T-4.85-2044', issuer: 'AT&T Inc', cusip: '00206RCJ3', coupon: 4.85, maturity: '2044-03-01', issueDate: '2014-03-01', sector: 'Communications', rating: 'BBB', notional: 4000000, frequency: 2, dayCount: '30/360' },
  { id: 'BRK-3.85-2052', issuer: 'Berkshire Hathaway', cusip: '084670BR5', coupon: 3.85, maturity: '2052-03-15', issueDate: '2022-03-15', sector: 'Financials', rating: 'AA', notional: 5000000, frequency: 2, dayCount: '30/360' },
  { id: 'CMCSA-4.15-2038', issuer: 'Comcast Corp', cusip: '20030NCQ0', coupon: 4.15, maturity: '2038-10-15', issueDate: '2018-10-15', sector: 'Communications', rating: 'A-', notional: 3500000, frequency: 2, dayCount: '30/360' },
  { id: 'PEP-3.9-2032', issuer: 'PepsiCo Inc', cusip: '713448EK1', coupon: 3.9, maturity: '2032-07-18', issueDate: '2022-07-18', sector: 'Consumer', rating: 'A+', notional: 4000000, frequency: 2, dayCount: '30/360' },
  { id: 'UNH-4.75-2035', issuer: 'UnitedHealth', cusip: '91324PDV5', coupon: 4.75, maturity: '2035-05-15', issueDate: '2020-05-15', sector: 'Healthcare', rating: 'A+', notional: 4500000, frequency: 2, dayCount: '30/360' },
  { id: 'GS-5.15-2030', issuer: 'Goldman Sachs', cusip: '38141GXZ1', coupon: 5.15, maturity: '2030-01-23', issueDate: '2020-01-23', sector: 'Financials', rating: 'A', notional: 3500000, frequency: 2, dayCount: '30/360' },
];

// Demo fallback data
const DEMO_PORTFOLIO = {
  nav: 50_000_000,
  modifiedDuration: 5.23,
  convexity: 42.5,
  ytm: 5.12,
  zSpread: 68,
  dv01: 26_150,
};

const DEMO_KRD_DATA = [
  { tenor: 0.5, duration: 0.12, contribution_pct: 2.3 },
  { tenor: 1, duration: 0.28, contribution_pct: 5.4 },
  { tenor: 2, duration: 0.85, contribution_pct: 16.3 },
  { tenor: 3, duration: 0.95, contribution_pct: 18.2 },
  { tenor: 5, duration: 1.45, contribution_pct: 27.7 },
  { tenor: 7, duration: 0.78, contribution_pct: 14.9 },
  { tenor: 10, duration: 0.65, contribution_pct: 12.4 },
  { tenor: 20, duration: 0.12, contribution_pct: 2.3 },
  { tenor: 30, duration: 0.03, contribution_pct: 0.5 },
];

const DEMO_STRESS_RESULTS = [
  { scenario_name: 'Rates +100bp', initial_value: '50000000', stressed_value: '47385000', pnl: '-2615000', pnl_pct: '-5.23' },
  { scenario_name: 'Rates -100bp', initial_value: '50000000', stressed_value: '52615000', pnl: '2615000', pnl_pct: '5.23' },
  { scenario_name: 'Rates +50bp', initial_value: '50000000', stressed_value: '48690000', pnl: '-1310000', pnl_pct: '-2.62' },
  { scenario_name: 'Spread +50bp', initial_value: '50000000', stressed_value: '48695000', pnl: '-1305000', pnl_pct: '-2.61' },
  { scenario_name: 'Flattener', initial_value: '50000000', stressed_value: '49075000', pnl: '-925000', pnl_pct: '-1.85' },
  { scenario_name: 'Steepener', initial_value: '50000000', stressed_value: '50925000', pnl: '925000', pnl_pct: '1.85' },
];

const COLORS = ['#3b82f6', '#22c55e', '#f59e0b', '#ef4444', '#8b5cf6', '#6b7280', '#ec4899', '#14b8a6'];

type Tab = 'overview' | 'allocation' | 'risk' | 'stress';

// Convert holding to bond reference input
function holdingToBondReference(holding: PortfolioHolding): BondReferenceInput {
  const isHighYield = holding.rating.startsWith('BB') || holding.rating.startsWith('B') || holding.rating.startsWith('CCC');
  const issuerType = holding.sector === 'Financials' ? 'Financial' : isHighYield ? 'CorporateHY' : 'CorporateIG';

  return {
    instrument_id: holding.id,
    cusip: holding.cusip,
    isin: null,
    sedol: null,
    bbgid: null,
    description: `${holding.issuer} ${holding.coupon}% ${holding.maturity.slice(0, 4)}`,
    currency: 'USD',
    issue_date: holding.issueDate,
    maturity_date: holding.maturity,
    coupon_rate: holding.coupon / 100,
    frequency: holding.frequency,
    day_count: holding.dayCount,
    face_value: 100,
    bond_type: 'FixedBullet',
    issuer_type: issuerType,
    issuer_id: holding.id.split('-')[0].toLowerCase(),
    issuer_name: holding.issuer,
    seniority: 'Senior',
    is_callable: false,
    call_schedule: [],
    is_putable: false,
    is_sinkable: false,
    floating_terms: null,
    inflation_index: null,
    inflation_base_index: null,
    has_deflation_floor: false,
    country_of_risk: 'US',
    sector: holding.sector,
    amount_outstanding: null,
    first_coupon_date: null,
    last_updated: Math.floor(Date.now() / 1000),
    source: 'demo',
  };
}

// Price all holdings
async function priceAllHoldings(holdings: PortfolioHolding[]): Promise<Map<string, BondQuoteResponse>> {
  const settlementDate = new Date().toISOString().split('T')[0];
  const priceMap = new Map<string, BondQuoteResponse>();

  const promises = holdings.map(async (holding) => {
    try {
      const bondRef = holdingToBondReference(holding);
      const quote = await priceBondWithDetails({
        bond: bondRef,
        settlement_date: settlementDate,
        market_price: null, // Let server calculate
      });
      return { id: holding.id, quote };
    } catch (error) {
      console.error(`Failed to price ${holding.id}:`, error);
      return null;
    }
  });

  const results = await Promise.allSettled(promises);

  results.forEach((result) => {
    if (result.status === 'fulfilled' && result.value) {
      priceMap.set(result.value.id, result.value.quote);
    }
  });

  return priceMap;
}

export default function PortfolioDemo() {
  const [activeTab, setActiveTab] = useState<Tab>('overview');

  // Price all holdings first
  const {
    data: bondPrices,
    isLoading: isPricingLoading,
    isError: isPricingError,
    refetch: refetchPricing,
    isFetching: isFetchingPricing,
  } = useQuery({
    queryKey: ['portfolio-bond-prices'],
    queryFn: () => priceAllHoldings(SAMPLE_HOLDINGS),
    staleTime: 60000,
    retry: 1,
    refetchOnWindowFocus: false,
  });

  // Calculate portfolio analytics once we have prices
  const {
    data: portfolioAnalytics,
    isLoading: isAnalyticsLoading,
    isError: isAnalyticsError,
  } = useQuery({
    queryKey: ['portfolio-analytics', bondPrices?.size],
    queryFn: async () => {
      if (!bondPrices || bondPrices.size === 0) {
        throw new Error('No bond prices available');
      }

      const quotes: BondQuoteResponse[] = Array.from(bondPrices.values());

      return calculatePortfolioAnalytics({
        portfolio: {
          portfolio_id: 'demo-portfolio',
          name: 'Sample IG Corporate Portfolio',
          currency: 'USD',
          positions: SAMPLE_HOLDINGS.map((h) => ({
            instrument_id: h.id,
            notional: h.notional,
            sector: h.sector,
            rating: h.rating,
          })),
        },
        bond_prices: quotes,
      });
    },
    enabled: !!bondPrices && bondPrices.size > 0,
    staleTime: 60000,
    retry: 1,
    refetchOnWindowFocus: false,
  });

  // Calculate key rate durations
  const {
    data: krdData,
    isLoading: isKrdLoading,
  } = useQuery({
    queryKey: ['portfolio-krd', bondPrices?.size],
    queryFn: async () => {
      if (!bondPrices || bondPrices.size === 0) {
        return null;
      }

      // Build positions with KRD data from bond quotes
      const positions = SAMPLE_HOLDINGS.map((h) => {
        const quote = bondPrices.get(h.id);
        // Approximate KRD based on modified duration and maturity
        const maturityYears = (new Date(h.maturity).getTime() - Date.now()) / (365.25 * 24 * 60 * 60 * 1000);
        const modDur = quote?.modified_duration || maturityYears * 0.9;

        // Distribute duration across tenors based on maturity
        const krd: Array<[number, number]> = [];
        const tenors = [0.5, 1, 2, 3, 5, 7, 10, 20, 30];
        for (const t of tenors) {
          if (t <= maturityYears + 2) {
            // Simple triangular distribution around maturity
            const weight = Math.max(0, 1 - Math.abs(t - maturityYears) / maturityYears);
            krd.push([t, modDur * weight]);
          }
        }

        return {
          instrument_id: h.id,
          notional: h.notional,
          market_price: quote?.clean_price_mid || 100,
          key_rate_durations: krd,
        };
      });

      return calculateKeyRateDuration({
        portfolio_id: 'demo-portfolio',
        name: 'Sample IG Corporate Portfolio',
        positions,
      });
    },
    enabled: !!bondPrices && bondPrices.size > 0,
    staleTime: 60000,
    retry: 1,
    refetchOnWindowFocus: false,
  });

  // Run stress tests
  const {
    data: stressData,
    isLoading: isStressLoading,
  } = useQuery({
    queryKey: ['portfolio-stress', bondPrices?.size],
    queryFn: async () => {
      if (!bondPrices || bondPrices.size === 0) {
        return null;
      }

      const quotes: BondQuoteResponse[] = Array.from(bondPrices.values());

      return runStandardStressTest({
        portfolio: {
          portfolio_id: 'demo-portfolio',
          name: 'Sample IG Corporate Portfolio',
          currency: 'USD',
          positions: SAMPLE_HOLDINGS.map((h) => ({
            instrument_id: h.id,
            notional: h.notional,
            sector: h.sector,
            rating: h.rating,
          })),
        },
        bond_prices: quotes,
      });
    },
    enabled: !!bondPrices && bondPrices.size > 0,
    staleTime: 60000,
    retry: 1,
    refetchOnWindowFocus: false,
  });

  // Determine live status
  const isLive = !!portfolioAnalytics && !isAnalyticsError;
  const isAnyLoading = isPricingLoading || isAnalyticsLoading || isFetchingPricing;
  const pricedCount = bondPrices?.size || 0;

  // Use live data or fallback to demo
  const navValue = portfolioAnalytics?.total_market_value || DEMO_PORTFOLIO.nav;
  const modDuration = portfolioAnalytics?.modified_duration || DEMO_PORTFOLIO.modifiedDuration;
  const convexity = portfolioAnalytics?.convexity || DEMO_PORTFOLIO.convexity;
  const ytm = portfolioAnalytics?.weighted_yield ? portfolioAnalytics.weighted_yield * 100 : DEMO_PORTFOLIO.ytm;
  const dv01 = portfolioAnalytics?.dv01 || DEMO_PORTFOLIO.dv01;
  const zSpread = portfolioAnalytics?.weighted_spread ? portfolioAnalytics.weighted_spread * 10000 : DEMO_PORTFOLIO.zSpread;

  // Use live KRD or demo
  const krdProfile = krdData?.profile?.length ? krdData.profile : DEMO_KRD_DATA;

  // Use live stress results or demo
  const stressResults = stressData?.results?.length ? stressData.results : DEMO_STRESS_RESULTS;

  // Sector allocation from holdings
  const sectorData = useMemo(() => {
    const sectors: Record<string, { value: number; notional: number }> = {};
    const totalNotional = SAMPLE_HOLDINGS.reduce((sum, h) => sum + h.notional, 0);

    SAMPLE_HOLDINGS.forEach((h) => {
      if (!sectors[h.sector]) {
        sectors[h.sector] = { value: 0, notional: 0 };
      }
      sectors[h.sector].notional += h.notional;
      sectors[h.sector].value += (h.notional / totalNotional) * 100;
    });

    return Object.entries(sectors).map(([name, data], i) => ({
      name,
      value: parseFloat(data.value.toFixed(2)),
      color: COLORS[i % COLORS.length],
    }));
  }, []);

  // Rating distribution from holdings
  const ratingData = useMemo(() => {
    const ratings: Record<string, number> = {};
    const totalNotional = SAMPLE_HOLDINGS.reduce((sum, h) => sum + h.notional, 0);

    SAMPLE_HOLDINGS.forEach((h) => {
      const bucket = h.rating.startsWith('AAA') ? 'AAA' :
                     h.rating.startsWith('AA') ? 'AA' :
                     h.rating.startsWith('A') ? 'A' : 'BBB';
      ratings[bucket] = (ratings[bucket] || 0) + (h.notional / totalNotional) * 100;
    });

    const ratingColors: Record<string, string> = {
      'AAA': '#22c55e',
      'AA': '#84cc16',
      'A': '#3b82f6',
      'BBB': '#f59e0b',
    };

    return Object.entries(ratings)
      .map(([rating, value]) => ({
        rating,
        value: parseFloat(value.toFixed(1)),
        color: ratingColors[rating] || '#64748b',
      }))
      .sort((a, b) => {
        const order = ['AAA', 'AA', 'A', 'BBB'];
        return order.indexOf(a.rating) - order.indexOf(b.rating);
      });
  }, []);

  return (
    <div className="space-y-6">
      {/* Status Bar */}
      <div className="card">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div>
            <h3 className="font-semibold text-slate-900">Sample IG Corporate Portfolio</h3>
            <p className="text-sm text-slate-500">{SAMPLE_HOLDINGS.length} holdings</p>
          </div>

          <div className="flex items-center gap-3">
            {/* Live Status */}
            <div className={cn(
              "flex items-center gap-2 px-3 py-1.5 rounded-full text-sm font-medium",
              isAnyLoading ? "bg-yellow-100 text-yellow-700" :
              isLive ? "bg-green-100 text-green-700" :
              isPricingError || isAnalyticsError ? "bg-red-100 text-red-700" :
              "bg-slate-100 text-slate-600"
            )}>
              {isAnyLoading ? (
                <>
                  <RefreshCw className="w-4 h-4 animate-spin" />
                  <span>Loading...</span>
                </>
              ) : isLive ? (
                <>
                  <Wifi className="w-4 h-4" />
                  <span>Live ({pricedCount} priced)</span>
                </>
              ) : (
                <>
                  <WifiOff className="w-4 h-4" />
                  <span>Demo Data</span>
                </>
              )}
            </div>

            <button
              onClick={() => refetchPricing()}
              disabled={isAnyLoading}
              className={cn(
                "p-2 rounded-lg border transition-colors",
                isAnyLoading
                  ? "border-slate-200 text-slate-400 cursor-not-allowed"
                  : "border-slate-300 text-slate-600 hover:bg-slate-50"
              )}
              title="Refresh data"
            >
              <RefreshCw className={cn("w-4 h-4", isAnyLoading && "animate-spin")} />
            </button>
          </div>
        </div>

        {/* Error message */}
        {(isPricingError || isAnalyticsError) && (
          <div className="mt-3 p-3 bg-red-50 border border-red-200 rounded-lg text-sm text-red-700 flex items-start gap-2">
            <AlertCircle className="w-4 h-4 mt-0.5 flex-shrink-0" />
            <span>Failed to fetch live data. Showing demo values.</span>
          </div>
        )}
      </div>

      {/* Tab Navigation */}
      <div className="card">
        <div className="flex flex-wrap gap-2">
          {(['overview', 'allocation', 'risk', 'stress'] as Tab[]).map((tab) => (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={cn(
                'btn',
                activeTab === tab ? 'btn-primary' : 'btn-secondary'
              )}
            >
              {tab === 'overview' ? 'Overview' :
               tab === 'allocation' ? 'Allocation' :
               tab === 'risk' ? 'Risk Analytics' : 'Stress Testing'}
            </button>
          ))}
        </div>
      </div>

      {/* Overview Tab */}
      {activeTab === 'overview' && (
        <>
          {/* Portfolio Summary */}
          <div className="grid md:grid-cols-2 lg:grid-cols-4 gap-4">
            <div className={cn("card", isLive && "ring-1 ring-green-200")}>
              <div className="stat-label">Portfolio NAV</div>
              <div className={cn("stat-value", isLive && "text-green-700")}>
                ${formatNumber(navValue / 1e6, 2)}M
              </div>
              <p className="text-sm text-slate-500 mt-1">
                {SAMPLE_HOLDINGS.length} holdings
                {isLive && <span className="text-green-600 ml-1">(live)</span>}
              </p>
            </div>
            <div className={cn("card", isLive && "ring-1 ring-green-200")}>
              <div className="stat-label">Modified Duration</div>
              <div className={cn("stat-value text-primary-700", isLive && "text-green-700")}>
                {formatNumber(modDuration, 2)}
              </div>
              <p className="text-sm text-slate-500 mt-1">years</p>
            </div>
            <div className={cn("card", isLive && "ring-1 ring-green-200")}>
              <div className="stat-label">Portfolio YTM</div>
              <div className={cn("stat-value", isLive && "text-green-700")}>
                {formatNumber(ytm, 2)}%
              </div>
              <p className="text-sm text-slate-500 mt-1">yield to maturity</p>
            </div>
            <div className={cn("card", isLive && "ring-1 ring-green-200")}>
              <div className="stat-label">DV01</div>
              <div className={cn("stat-value", isLive && "text-green-700")}>
                ${formatNumber(dv01, 0)}
              </div>
              <p className="text-sm text-slate-500 mt-1">dollar duration per bp</p>
            </div>
          </div>

          {/* Additional Metrics */}
          <div className="grid md:grid-cols-3 gap-4">
            <div className={cn("card", isLive && "ring-1 ring-green-200")}>
              <div className="stat-label">Convexity</div>
              <div className={cn("stat-value", isLive && "text-green-700")}>
                {formatNumber(convexity, 1)}
              </div>
            </div>
            <div className={cn("card", isLive && "ring-1 ring-green-200")}>
              <div className="stat-label">Avg Z-Spread</div>
              <div className={cn("stat-value", isLive && "text-green-700")}>
                {formatBps(zSpread)}
              </div>
            </div>
            <div className="card">
              <div className="stat-label">Avg Rating</div>
              <div className="stat-value">A</div>
            </div>
          </div>
        </>
      )}

      {/* Allocation Tab */}
      {activeTab === 'allocation' && (
        <div className="grid lg:grid-cols-2 gap-6">
          {/* Sector Allocation */}
          <div className="card">
            <h3 className="card-header">Sector Allocation</h3>
            <div className="h-64">
              <ResponsiveContainer width="100%" height="100%">
                <PieChart>
                  <Pie
                    data={sectorData}
                    cx="50%"
                    cy="50%"
                    outerRadius={80}
                    dataKey="value"
                    label={({ name, value }) => `${name} ${value}%`}
                  >
                    {sectorData.map((entry, index) => (
                      <Cell key={index} fill={entry.color} />
                    ))}
                  </Pie>
                  <Tooltip formatter={(value: number) => [`${value}%`, 'Weight']} />
                </PieChart>
              </ResponsiveContainer>
            </div>
            <div className="mt-4">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-slate-200">
                    <th className="text-left py-2 font-medium text-slate-600">Sector</th>
                    <th className="text-right py-2 font-medium text-slate-600">Weight</th>
                  </tr>
                </thead>
                <tbody>
                  {sectorData.map((sector, i) => (
                    <tr key={sector.name} className="border-b border-slate-100">
                      <td className="py-2 flex items-center gap-2">
                        <div
                          className="w-3 h-3 rounded"
                          style={{ backgroundColor: COLORS[i % COLORS.length] }}
                        />
                        {sector.name}
                      </td>
                      <td className="py-2 text-right font-mono">{sector.value}%</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>

          {/* Rating Distribution */}
          <div className="card">
            <h3 className="card-header">Rating Distribution</h3>
            <div className="h-64">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={ratingData} layout="vertical">
                  <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
                  <XAxis type="number" domain={[0, 50]} tickFormatter={(v) => `${v}%`} />
                  <YAxis type="category" dataKey="rating" width={50} />
                  <Tooltip formatter={(value: number) => [`${value}%`, 'Weight']} />
                  <Bar dataKey="value" radius={[0, 4, 4, 0]}>
                    {ratingData.map((entry) => (
                      <Cell key={entry.rating} fill={entry.color} />
                    ))}
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
            </div>
            <div className="mt-4 grid grid-cols-4 gap-2 text-center">
              {ratingData.map((r) => (
                <div key={r.rating}>
                  <div className="text-lg font-bold" style={{ color: r.color }}>
                    {r.value}%
                  </div>
                  <div className="text-xs text-slate-500">{r.rating}</div>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* Risk Analytics Tab */}
      {activeTab === 'risk' && (
        <div className="space-y-6">
          {/* Key Rate Durations */}
          <div className="card">
            <div className="flex items-center justify-between mb-4">
              <h3 className="card-header mb-0">Key Rate Durations</h3>
              {krdData && !isKrdLoading && (
                <span className="text-xs px-2 py-1 bg-green-100 text-green-700 rounded">
                  Live ({krdData.coverage_pct?.toFixed(0)}% coverage)
                </span>
              )}
            </div>
            <div className="h-72">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={krdProfile.map((k) => ({ tenor: `${k.tenor}Y`, krd: k.duration }))}>
                  <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
                  <XAxis dataKey="tenor" />
                  <YAxis tickFormatter={(v) => formatNumber(v, 2)} />
                  <Tooltip
                    formatter={(value: number) => [formatNumber(value, 3), 'KRD']}
                  />
                  <Bar dataKey="krd" fill="#3b82f6" radius={[4, 4, 0, 0]} />
                </BarChart>
              </ResponsiveContainer>
            </div>
            <p className="text-sm text-slate-500 mt-4">
              Key rate duration measures sensitivity to changes at specific points on the yield curve.
              Total duration: {formatNumber(krdProfile.reduce((sum, k) => sum + k.duration, 0), 2)}.
            </p>
          </div>

          {/* Risk Summary */}
          <div className="grid md:grid-cols-3 gap-4">
            <div className="card">
              <div className="stat-label">Duration Breakdown</div>
              <div className="space-y-2 mt-2">
                <div className="flex justify-between text-sm">
                  <span className="text-slate-600">Short-end (&lt;2Y)</span>
                  <span className="font-mono">{formatNumber(krdData?.short_duration || 0.4, 2)}</span>
                </div>
                <div className="flex justify-between text-sm">
                  <span className="text-slate-600">Intermediate (2-10Y)</span>
                  <span className="font-mono">{formatNumber(krdData?.intermediate_duration || 4.2, 2)}</span>
                </div>
                <div className="flex justify-between text-sm">
                  <span className="text-slate-600">Long-end (&gt;10Y)</span>
                  <span className="font-mono">{formatNumber(krdData?.long_duration || 0.6, 2)}</span>
                </div>
              </div>
            </div>
            <div className="card">
              <div className="stat-label">Spread Risk</div>
              <div className="space-y-2 mt-2">
                <div className="flex justify-between text-sm">
                  <span className="text-slate-600">Spread Duration</span>
                  <span className="font-mono">{formatNumber(portfolioAnalytics?.spread_duration || 5.15, 2)}</span>
                </div>
                <div className="flex justify-between text-sm">
                  <span className="text-slate-600">CS01 (per bp)</span>
                  <span className="font-mono">${formatNumber(portfolioAnalytics?.cs01 || 25750, 0)}</span>
                </div>
              </div>
            </div>
            <div className="card">
              <div className="stat-label">VaR (95%, 1-day)</div>
              <div className="stat-value text-loss">-${formatNumber(navValue * 0.00365, 0)}</div>
              <p className="text-sm text-slate-500 mt-1">-0.37% of NAV</p>
            </div>
          </div>
        </div>
      )}

      {/* Stress Testing Tab */}
      {activeTab === 'stress' && (
        <div className="card">
          <div className="flex items-center justify-between mb-4">
            <h3 className="card-header mb-0">Stress Test Results</h3>
            {stressData && !isStressLoading && (
              <span className="text-xs px-2 py-1 bg-green-100 text-green-700 rounded">Live</span>
            )}
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="bg-slate-50 border-b border-slate-200">
                  <th className="text-left py-3 px-4 font-medium text-slate-600">Scenario</th>
                  <th className="text-right py-3 px-4 font-medium text-slate-600">P&L Impact (%)</th>
                  <th className="text-right py-3 px-4 font-medium text-slate-600">P&L Impact ($)</th>
                </tr>
              </thead>
              <tbody>
                {stressResults.map((scenario) => {
                  const pnlPct = parseFloat(scenario.pnl_pct);
                  const pnl = parseFloat(scenario.pnl);
                  return (
                    <tr key={scenario.scenario_name} className="border-b border-slate-100 hover:bg-slate-50">
                      <td className="py-3 px-4 font-medium">{scenario.scenario_name}</td>
                      <td className={cn(
                        'py-3 px-4 text-right font-mono',
                        pnlPct >= 0 ? 'text-gain' : 'text-loss'
                      )}>
                        {pnlPct >= 0 ? '+' : ''}{formatNumber(pnlPct, 2)}%
                      </td>
                      <td className={cn(
                        'py-3 px-4 text-right font-mono',
                        pnl >= 0 ? 'text-gain' : 'text-loss'
                      )}>
                        {pnl >= 0 ? '+' : ''}${formatNumber(Math.abs(pnl) / 1000, 0)}K
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>

          <div className="mt-6 p-4 bg-slate-50 rounded-lg">
            <h4 className="font-medium text-slate-900 mb-2">Scenario Analysis</h4>
            <p className="text-sm text-slate-600">
              The portfolio shows moderate rate sensitivity (duration ~{formatNumber(modDuration, 1)}) with most risk concentrated
              in the 3-5 year sector. A 100bp parallel shift in rates would result in approximately
              ±${formatNumber(navValue * modDuration / 100 / 1e6, 1)}M P&L impact. Spread risk is also significant, with a 50bp spread widening
              causing ~${formatNumber(navValue * modDuration / 200 / 1e6, 1)}M loss.
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
