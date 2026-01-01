import { useState, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  PieChart,
  Pie,
  Cell,
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
} from 'recharts';
import { Wifi, WifiOff, RefreshCw, AlertCircle, Info } from 'lucide-react';
import { formatNumber, cn } from '../lib/utils';
import {
  priceBondWithDetails,
  BondQuoteResponse,
  BondReferenceInput,
  fetchETFHoldingsFromProvider,
  DataProviderETFResponse,
} from '../lib/api';

// Demo ETF data
interface ETFHolding {
  id: string;
  cusip: string;
  issuer: string;
  coupon: number;
  maturity: string;
  rating: string;
  sector: string;
  weight: number;
  price: number;
  yield: number;
  duration: number;
  quantity: number;
  marketValue: number;
}

interface ETFData {
  id: string;
  ticker: string;
  name: string;
  description: string;
  nav: number;
  inav: number;
  marketPrice: number;
  sharesOutstanding: number;
  aum: number;
  avgDuration: number;
  avgYield: number;
  expenseRatio: number;
  holdings: ETFHolding[];
}

// Sample ETF holdings data
const LQD_HOLDINGS: ETFHolding[] = [
  { id: '1', cusip: '037833AK6', issuer: 'Apple Inc', coupon: 4.65, maturity: '2046-02-23', rating: 'AA+', sector: 'Technology', weight: 0.52, price: 92.45, yield: 5.21, duration: 12.4, quantity: 15000, marketValue: 1386750 },
  { id: '2', cusip: '594918BG8', issuer: 'Microsoft Corp', coupon: 3.50, maturity: '2042-02-12', rating: 'AAA', sector: 'Technology', weight: 0.48, price: 88.12, yield: 4.85, duration: 11.2, quantity: 14500, marketValue: 1277740 },
  { id: '3', cusip: '46625HJE5', issuer: 'JPMorgan Chase', coupon: 5.25, maturity: '2034-07-15', rating: 'A-', sector: 'Financials', weight: 0.45, price: 99.85, yield: 5.28, duration: 7.1, quantity: 12000, marketValue: 1198200 },
  { id: '4', cusip: '92343VEP1', issuer: 'Verizon', coupon: 4.50, maturity: '2033-08-10', rating: 'BBB+', sector: 'Telecom', weight: 0.42, price: 95.23, yield: 5.12, duration: 6.8, quantity: 11800, marketValue: 1123714 },
  { id: '5', cusip: '30231GAV4', issuer: 'Exxon Mobil', coupon: 4.23, maturity: '2039-03-01', rating: 'AA-', sector: 'Energy', weight: 0.40, price: 91.67, yield: 5.02, duration: 9.5, quantity: 11600, marketValue: 1063372 },
  { id: '6', cusip: '126650CZ6', issuer: 'CVS Health', coupon: 5.05, maturity: '2048-03-25', rating: 'BBB', sector: 'Healthcare', weight: 0.38, price: 89.34, yield: 5.85, duration: 13.2, quantity: 11300, marketValue: 1009542 },
  { id: '7', cusip: '00206RCJ3', issuer: 'AT&T Inc', coupon: 4.85, maturity: '2044-03-01', rating: 'BBB', sector: 'Telecom', weight: 0.35, price: 87.56, yield: 5.78, duration: 11.8, quantity: 10600, marketValue: 928136 },
  { id: '8', cusip: '084670BR5', issuer: 'Berkshire Hathaway', coupon: 3.85, maturity: '2052-03-15', rating: 'AA', sector: 'Financials', weight: 0.33, price: 79.12, yield: 5.15, duration: 15.6, quantity: 11100, marketValue: 878232 },
  { id: '9', cusip: '20030NCQ0', issuer: 'Comcast Corp', coupon: 4.15, maturity: '2038-10-15', rating: 'A-', sector: 'Telecom', weight: 0.31, price: 90.45, yield: 5.05, duration: 9.8, quantity: 9100, marketValue: 823095 },
  { id: '10', cusip: '713448EK1', issuer: 'PepsiCo Inc', coupon: 3.90, maturity: '2032-07-18', rating: 'A+', sector: 'Consumer', weight: 0.29, price: 96.78, yield: 4.42, duration: 6.2, quantity: 8000, marketValue: 774240 },
];

const HYG_HOLDINGS: ETFHolding[] = [
  { id: '1', cusip: '345370CQ2', issuer: 'Ford Motor Co', coupon: 6.10, maturity: '2032-08-19', rating: 'BB+', sector: 'Autos', weight: 0.65, price: 94.25, yield: 6.89, duration: 5.8, quantity: 18000, marketValue: 1696500 },
  { id: '2', cusip: '172967LS8', issuer: 'Occidental Petro', coupon: 6.45, maturity: '2036-09-15', rating: 'BB', sector: 'Energy', weight: 0.58, price: 96.12, yield: 6.92, duration: 7.2, quantity: 15800, marketValue: 1518696 },
  { id: '3', cusip: '29273VAM5', issuer: 'Carnival Corp', coupon: 7.00, maturity: '2030-08-15', rating: 'B+', sector: 'Leisure', weight: 0.52, price: 99.45, yield: 7.12, duration: 4.5, quantity: 13700, marketValue: 1362465 },
  { id: '4', cusip: '902494BL5', issuer: 'T-Mobile USA', coupon: 5.75, maturity: '2034-01-15', rating: 'BB+', sector: 'Telecom', weight: 0.48, price: 97.34, yield: 6.15, duration: 6.8, quantity: 12900, marketValue: 1255686 },
  { id: '5', cusip: '254687FS3', issuer: 'Dish Network', coupon: 7.75, maturity: '2027-07-01', rating: 'CCC+', sector: 'Media', weight: 0.42, price: 72.50, yield: 14.25, duration: 2.1, quantity: 15200, marketValue: 1102000 },
];

const TLT_HOLDINGS: ETFHolding[] = [
  { id: '1', cusip: '912810TN8', issuer: 'US Treasury', coupon: 1.875, maturity: '2051-02-15', rating: 'AAA', sector: 'Government', weight: 4.85, price: 62.45, yield: 4.52, duration: 18.2, quantity: 205000, marketValue: 12802250 },
  { id: '2', cusip: '912810TR9', issuer: 'US Treasury', coupon: 2.25, maturity: '2052-02-15', rating: 'AAA', sector: 'Government', weight: 4.62, price: 66.78, yield: 4.48, duration: 17.8, quantity: 183000, marketValue: 12220740 },
  { id: '3', cusip: '912810TT5', issuer: 'US Treasury', coupon: 3.00, maturity: '2052-08-15', rating: 'AAA', sector: 'Government', weight: 4.51, price: 73.25, yield: 4.45, duration: 16.9, quantity: 163000, marketValue: 11939750 },
  { id: '4', cusip: '912810TW8', issuer: 'US Treasury', coupon: 3.625, maturity: '2053-02-15', rating: 'AAA', sector: 'Government', weight: 4.38, price: 79.12, yield: 4.42, duration: 16.2, quantity: 147000, marketValue: 11630640 },
  { id: '5', cusip: '912810TY4', issuer: 'US Treasury', coupon: 3.875, maturity: '2043-08-15', rating: 'AAA', sector: 'Government', weight: 4.25, price: 85.67, yield: 4.38, duration: 13.5, quantity: 132000, marketValue: 11308440 },
];

// ETF configurations
const DEMO_ETFS: ETFData[] = [
  {
    id: 'LQD',
    ticker: 'LQD',
    name: 'iShares iBoxx $ Investment Grade Corporate Bond ETF',
    description: 'Tracks an index of US investment grade corporate bonds',
    nav: 110.45,
    inav: 110.52,
    marketPrice: 110.65,
    sharesOutstanding: 298500000,
    aum: 32.98e9,
    avgDuration: 8.45,
    avgYield: 5.12,
    expenseRatio: 0.14,
    holdings: LQD_HOLDINGS,
  },
  {
    id: 'HYG',
    ticker: 'HYG',
    name: 'iShares iBoxx $ High Yield Corporate Bond ETF',
    description: 'Tracks an index of US high yield corporate bonds',
    nav: 80.41,
    inav: 80.48,
    marketPrice: 80.71,
    sharesOutstanding: 228300000,
    aum: 18.39e9,
    avgDuration: 3.85,
    avgYield: 7.25,
    expenseRatio: 0.48,
    holdings: HYG_HOLDINGS,
  },
  {
    id: 'TLT',
    ticker: 'TLT',
    name: 'iShares 20+ Year Treasury Bond ETF',
    description: 'Tracks an index of US Treasury bonds with 20+ years to maturity',
    nav: 87.84,
    inav: 87.92,
    marketPrice: 87.87,
    sharesOutstanding: 548600000,
    aum: 48.18e9,
    avgDuration: 16.52,
    avgYield: 4.45,
    expenseRatio: 0.15,
    holdings: TLT_HOLDINGS,
  },
];

// Color palettes
const SECTOR_COLORS: Record<string, string> = {
  Technology: '#3b82f6',
  Financials: '#10b981',
  Telecom: '#f59e0b',
  Energy: '#ef4444',
  Healthcare: '#8b5cf6',
  Consumer: '#ec4899',
  Government: '#0ea5e9',
  Autos: '#f97316',
  Leisure: '#14b8a6',
  Media: '#6366f1',
};

const RATING_COLORS: Record<string, string> = {
  'AAA': '#059669',
  'AA+': '#10b981',
  'AA': '#34d399',
  'AA-': '#6ee7b7',
  'A+': '#3b82f6',
  'A': '#60a5fa',
  'A-': '#93c5fd',
  'BBB+': '#f59e0b',
  'BBB': '#fbbf24',
  'BB+': '#f97316',
  'BB': '#fb923c',
  'B+': '#ef4444',
  'CCC+': '#dc2626',
};

// Helper to calculate premium/discount
function getPremiumDiscount(price: number, nav: number): { value: number; label: string; color: string } {
  const diff = ((price - nav) / nav) * 100;
  return {
    value: diff,
    label: diff >= 0 ? 'Premium' : 'Discount',
    color: Math.abs(diff) < 0.1 ? 'text-slate-600' : diff > 0 ? 'text-loss' : 'text-gain',
  };
}

// Get today's date in YYYY-MM-DD format
function getTodayDate(): string {
  return new Date().toISOString().split('T')[0];
}

// Convert ETF holding to bond reference input for API pricing
function holdingToBondReference(holding: ETFHolding, etfId: string): BondReferenceInput {
  // Determine issuer type based on sector
  // Server expects: Sovereign, Agency, Supranational, CorporateIG, CorporateHY, Financial, Municipal
  let issuerType: string;
  if (holding.sector === 'Government') {
    issuerType = 'Sovereign';
  } else if (holding.sector === 'Financials') {
    issuerType = 'Financial';
  } else if (holding.rating.startsWith('BB') || holding.rating.startsWith('B') || holding.rating.startsWith('CCC')) {
    issuerType = 'CorporateHY';
  } else {
    issuerType = 'CorporateIG';
  }

  // Determine bond type
  // Server expects: FixedBullet, FixedCallable, FixedPutable, FloatingRate, ZeroCoupon, InflationLinked, Amortizing, Convertible
  const bondType = holding.coupon === 0 ? 'ZeroCoupon' : 'FixedBullet';

  return {
    instrument_id: `${etfId}-${holding.id}`,
    cusip: holding.cusip,
    isin: null,
    sedol: null,
    bbgid: null,
    description: `${holding.issuer} ${holding.coupon}% ${holding.maturity}`,
    currency: 'USD',
    issue_date: '2020-01-15', // Placeholder
    maturity_date: holding.maturity,
    coupon_rate: holding.coupon / 100, // Convert to decimal
    frequency: 2, // Semi-annual
    day_count: holding.sector === 'Government' ? 'ActAct' : '30360',
    face_value: 100,
    bond_type: bondType,
    issuer_type: issuerType,
    issuer_id: holding.issuer.toLowerCase().replace(/\s+/g, '-'),
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
    last_updated: Math.floor(Date.now() / 1000), // Unix timestamp in seconds
    source: 'demo-provider',
  };
}

// Price holdings via API and return updated analytics
async function priceHoldings(
  holdings: ETFHolding[],
  etfId: string,
  settlementDate: string
): Promise<{ prices: Map<string, BondQuoteResponse>; errors: string[] }> {
  const prices = new Map<string, BondQuoteResponse>();
  const errors: string[] = [];

  // Price each holding (limit to avoid overwhelming the API)
  const pricingPromises = holdings.slice(0, 10).map(async (holding) => {
    try {
      const bondRef = holdingToBondReference(holding, etfId);
      const quote = await priceBondWithDetails({
        bond: bondRef,
        settlement_date: settlementDate,
        market_price: holding.price,
      });
      return { holdingId: holding.id, quote };
    } catch (err) {
      errors.push(`Failed to price ${holding.issuer}: ${err}`);
      return null;
    }
  });

  const results = await Promise.allSettled(pricingPromises);

  results.forEach((result) => {
    if (result.status === 'fulfilled' && result.value) {
      prices.set(result.value.holdingId, result.value.quote);
    }
  });

  return { prices, errors };
}

// Calculate iNAV from priced holdings
function calculateInavFromPrices(
  etf: ETFData,
  prices: Map<string, BondQuoteResponse>
): { inav: number; weightedDuration: number; weightedYield: number } {
  let totalValue = 0;
  let weightedDuration = 0;
  let weightedYield = 0;
  let totalWeight = 0;

  etf.holdings.forEach((holding) => {
    const quote = prices.get(holding.id);
    if (quote) {
      // Use API-calculated values
      const price = quote.clean_price_mid ?? holding.price;
      const duration = quote.modified_duration ?? holding.duration;
      const yieldVal = (quote.ytm_mid ?? holding.yield / 100) * 100;

      totalValue += price * holding.quantity;
      weightedDuration += duration * holding.weight;
      weightedYield += yieldVal * holding.weight;
      totalWeight += holding.weight;
    } else {
      // Use demo values
      totalValue += holding.price * holding.quantity;
      weightedDuration += holding.duration * holding.weight;
      weightedYield += holding.yield * holding.weight;
      totalWeight += holding.weight;
    }
  });

  // Normalize
  if (totalWeight > 0) {
    weightedDuration /= totalWeight;
    weightedYield /= totalWeight;
  }

  // Calculate iNAV per share
  const inav = totalValue / etf.sharesOutstanding;

  return { inav, weightedDuration, weightedYield };
}

// Transform data provider ETF response to internal format
function transformProviderETF(data: DataProviderETFResponse): ETFData {
  return {
    id: data.etf.ticker,
    ticker: data.etf.ticker,
    name: data.etf.name,
    description: data.etf.description,
    nav: data.etf.nav,
    inav: data.etf.nav * 1.001, // Slight premium for demo
    marketPrice: data.etf.nav * 1.002,
    sharesOutstanding: data.etf.shares_outstanding,
    aum: data.etf.aum,
    avgDuration: data.metrics.weighted_duration,
    avgYield: data.metrics.weighted_yield,
    expenseRatio: data.etf.expense_ratio,
    holdings: data.holdings.map(h => ({
      id: h.id,
      cusip: h.cusip,
      issuer: h.issuer,
      coupon: h.coupon,
      maturity: h.maturity,
      rating: h.rating,
      sector: h.sector,
      weight: h.weight,
      price: h.price || 100,
      yield: (h.coupon / (h.price || 100)) * 100,
      duration: estimateDuration(h.maturity, h.coupon),
      quantity: h.shares,
      marketValue: h.market_value,
    })),
  };
}

// Estimate duration from maturity (simplified)
function estimateDuration(maturity: string, coupon: number): number {
  const maturityDate = new Date(maturity);
  const yearsToMaturity = (maturityDate.getTime() - Date.now()) / (365.25 * 24 * 60 * 60 * 1000);
  // Simplified Macaulay duration approximation
  const modifiedDuration = yearsToMaturity * 0.9 / (1 + coupon / 200);
  return Math.max(0, Math.min(modifiedDuration, yearsToMaturity));
}

export default function ETFAnalyticsDemo() {
  const [selectedETF, setSelectedETF] = useState<string>('LQD');
  const [searchTerm, setSearchTerm] = useState('');
  const [sortField, setSortField] = useState<keyof ETFHolding>('weight');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('desc');

  const settlementDate = getTodayDate();

  // Fetch ETF holdings from data provider
  const {
    data: providerETFData,
    isLoading: isLoadingProvider,
    isError: isProviderError,
    refetch: refetchProvider,
    isFetching: isFetchingProvider,
  } = useQuery({
    queryKey: ['etf-provider-holdings', selectedETF],
    queryFn: () => fetchETFHoldingsFromProvider(selectedETF),
    staleTime: 60000, // 1 minute
    retry: 1,
    refetchOnWindowFocus: false,
  });

  // Use provider data if available, otherwise fall back to local demo data
  const etf = useMemo(() => {
    if (providerETFData) {
      return transformProviderETF(providerETFData);
    }
    return DEMO_ETFS.find(e => e.id === selectedETF) || DEMO_ETFS[0];
  }, [providerETFData, selectedETF]);

  const isFromProvider = !!providerETFData;
  const dataSource = providerETFData?.source || 'Local Demo Data';

  // Fetch live prices from convex-server API
  const {
    data: liveData,
    isLoading,
    isError,
    error,
    refetch,
    isFetching,
  } = useQuery({
    queryKey: ['etf-holdings-prices', selectedETF, etf.holdings.length],
    queryFn: async () => {
      const { prices, errors } = await priceHoldings(etf.holdings, etf.id, settlementDate);

      if (prices.size === 0 && errors.length > 0) {
        throw new Error(errors[0]);
      }

      // Calculate iNAV from live prices
      const { inav, weightedDuration, weightedYield } = calculateInavFromPrices(etf, prices);

      return {
        prices,
        inav,
        weightedDuration,
        weightedYield,
        errors,
        pricedCount: prices.size,
        timestamp: Date.now(),
      };
    },
    staleTime: 30000, // Consider data stale after 30 seconds
    retry: 1,
    refetchOnWindowFocus: false,
    enabled: etf.holdings.length > 0, // Only run if we have holdings
  });

  // Use live data if available, otherwise fall back to demo data
  const isLive = liveData && liveData.pricedCount > 0;
  const displayInav = isLive ? liveData.inav : etf.inav;
  const displayDuration = isLive ? liveData.weightedDuration : etf.avgDuration;
  const displayYield = isLive ? liveData.weightedYield : etf.avgYield;

  const premiumDiscount = getPremiumDiscount(etf.marketPrice, etf.nav);

  // Combined loading/fetching state
  const isAnyLoading = isLoading || isLoadingProvider || isFetching || isFetchingProvider;

  // Filter and sort holdings
  const filteredHoldings = useMemo(() => {
    let holdings = etf.holdings.filter(h =>
      h.issuer.toLowerCase().includes(searchTerm.toLowerCase()) ||
      h.cusip.toLowerCase().includes(searchTerm.toLowerCase()) ||
      h.sector.toLowerCase().includes(searchTerm.toLowerCase())
    );

    holdings.sort((a, b) => {
      const aVal = a[sortField];
      const bVal = b[sortField];
      if (typeof aVal === 'number' && typeof bVal === 'number') {
        return sortDirection === 'asc' ? aVal - bVal : bVal - aVal;
      }
      return sortDirection === 'asc'
        ? String(aVal).localeCompare(String(bVal))
        : String(bVal).localeCompare(String(aVal));
    });

    return holdings;
  }, [etf.holdings, searchTerm, sortField, sortDirection]);

  // Sector breakdown
  const sectorData = useMemo(() => {
    const sectors: Record<string, number> = {};
    etf.holdings.forEach(h => {
      sectors[h.sector] = (sectors[h.sector] || 0) + h.weight;
    });
    return Object.entries(sectors).map(([name, value]) => ({
      name,
      value: parseFloat(value.toFixed(2)),
      color: SECTOR_COLORS[name] || '#64748b',
    }));
  }, [etf.holdings]);

  // Rating breakdown
  const ratingData = useMemo(() => {
    const ratings: Record<string, number> = {};
    etf.holdings.forEach(h => {
      ratings[h.rating] = (ratings[h.rating] || 0) + h.weight;
    });
    return Object.entries(ratings)
      .map(([name, value]) => ({
        name,
        value: parseFloat(value.toFixed(2)),
        color: RATING_COLORS[name] || '#64748b',
      }))
      .sort((a, b) => {
        const order = ['AAA', 'AA+', 'AA', 'AA-', 'A+', 'A', 'A-', 'BBB+', 'BBB', 'BBB-', 'BB+', 'BB', 'BB-', 'B+', 'B', 'B-', 'CCC+', 'CCC'];
        return order.indexOf(a.name) - order.indexOf(b.name);
      });
  }, [etf.holdings]);

  // Duration distribution
  const durationData = useMemo(() => {
    const buckets = [
      { name: '0-2Y', min: 0, max: 2, value: 0 },
      { name: '2-5Y', min: 2, max: 5, value: 0 },
      { name: '5-7Y', min: 5, max: 7, value: 0 },
      { name: '7-10Y', min: 7, max: 10, value: 0 },
      { name: '10-15Y', min: 10, max: 15, value: 0 },
      { name: '15Y+', min: 15, max: 100, value: 0 },
    ];

    etf.holdings.forEach(h => {
      const bucket = buckets.find(b => h.duration >= b.min && h.duration < b.max);
      if (bucket) bucket.value += h.weight;
    });

    return buckets.map(b => ({ name: b.name, value: parseFloat(b.value.toFixed(2)) }));
  }, [etf.holdings]);

  const handleSort = (field: keyof ETFHolding) => {
    if (field === sortField) {
      setSortDirection(prev => prev === 'asc' ? 'desc' : 'asc');
    } else {
      setSortField(field);
      setSortDirection('desc');
    }
  };

  return (
    <div className="space-y-6">
      {/* ETF Selector with Live Status */}
      <div className="card">
        <div className="flex flex-wrap items-center justify-between gap-4 mb-4">
          <div className="flex flex-wrap gap-3">
            {DEMO_ETFS.map(e => (
              <button
                key={e.id}
                onClick={() => setSelectedETF(e.id)}
                className={`px-4 py-3 rounded-lg border-2 transition-all text-left ${
                  selectedETF === e.id
                    ? 'border-primary-500 bg-primary-50'
                    : 'border-slate-200 hover:border-slate-300 bg-white'
                }`}
              >
                <div className="font-bold text-lg">{e.ticker}</div>
                <div className="text-sm text-slate-600 truncate max-w-48">{e.name.split(' ').slice(0, 4).join(' ')}</div>
              </button>
            ))}
          </div>

          {/* Data Source Status Indicators */}
          <div className="flex items-center gap-3">
            {/* Data Provider Status */}
            <div className={cn(
              "flex items-center gap-2 px-3 py-1.5 rounded-full text-sm font-medium",
              isLoadingProvider || isFetchingProvider ? "bg-yellow-100 text-yellow-700" :
              isFromProvider ? "bg-blue-100 text-blue-700" :
              "bg-slate-100 text-slate-600"
            )}>
              {isLoadingProvider || isFetchingProvider ? (
                <>
                  <RefreshCw className="w-4 h-4 animate-spin" />
                  <span>Loading...</span>
                </>
              ) : isFromProvider ? (
                <>
                  <Wifi className="w-4 h-4" />
                  <span>Data: {dataSource}</span>
                </>
              ) : (
                <>
                  <WifiOff className="w-4 h-4" />
                  <span>Local Data</span>
                </>
              )}
            </div>
            {/* Pricing API Status */}
            <div className={cn(
              "flex items-center gap-2 px-3 py-1.5 rounded-full text-sm font-medium",
              isLoading || isFetching ? "bg-yellow-100 text-yellow-700" :
              isLive ? "bg-green-100 text-green-700" :
              isError ? "bg-red-100 text-red-700" :
              "bg-slate-100 text-slate-600"
            )}>
              {isLoading || isFetching ? (
                <>
                  <RefreshCw className="w-4 h-4 animate-spin" />
                  <span>Pricing...</span>
                </>
              ) : isLive ? (
                <>
                  <Wifi className="w-4 h-4" />
                  <span>Live ({liveData?.pricedCount} priced)</span>
                </>
              ) : isError ? (
                <>
                  <AlertCircle className="w-4 h-4" />
                  <span>Error</span>
                </>
              ) : (
                <>
                  <WifiOff className="w-4 h-4" />
                  <span>Demo</span>
                </>
              )}
            </div>
            <button
              onClick={() => {
                refetchProvider();
                refetch();
              }}
              disabled={isAnyLoading}
              className={cn(
                "p-2 rounded-lg border transition-colors",
                isAnyLoading
                  ? "border-slate-200 text-slate-400 cursor-not-allowed"
                  : "border-slate-300 text-slate-600 hover:bg-slate-50 hover:border-slate-400"
              )}
              title="Refresh all data"
            >
              <RefreshCw className={cn("w-4 h-4", isAnyLoading && "animate-spin")} />
            </button>
          </div>
        </div>

        {/* Error message */}
        {(isError || isProviderError) && (
          <div className="mt-2 p-3 bg-red-50 border border-red-200 rounded-lg text-sm text-red-700">
            <span className="font-medium">Error:</span> {(error as Error)?.message || 'Failed to fetch data'}
            <span className="ml-2 text-red-600">— Using demo data</span>
          </div>
        )}
      </div>

      {/* NAV / iNAV / Price Panel */}
      <div className="grid md:grid-cols-4 gap-4">
        <div className="card bg-gradient-to-br from-blue-50 to-indigo-50">
          <div className="text-sm font-medium text-slate-600 mb-1">NAV</div>
          <div className="text-3xl font-bold text-slate-900">${formatNumber(etf.nav, 2)}</div>
          <div className="text-sm text-slate-500 mt-1">Net Asset Value</div>
        </div>

        <div className={cn(
          "card bg-gradient-to-br",
          isLive ? "from-green-50 to-emerald-50 ring-2 ring-green-200" : "from-green-50 to-emerald-50"
        )}>
          <div className="flex items-center gap-2 text-sm font-medium text-slate-600 mb-1">
            <span>iNAV</span>
            {isLive && <span className="text-xs px-1.5 py-0.5 bg-green-100 text-green-700 rounded">LIVE</span>}
          </div>
          <div className={cn(
            "text-3xl font-bold",
            isLive ? "text-green-700" : "text-slate-900"
          )}>
            ${formatNumber(displayInav, 2)}
          </div>
          <div className="text-sm text-slate-500 mt-1">
            Indicative NAV
            <span className={cn("ml-1", displayInav >= etf.nav ? "text-gain" : "text-loss")}>
              ({displayInav >= etf.nav ? '+' : ''}{formatNumber(displayInav - etf.nav, 2)})
            </span>
          </div>
        </div>

        <div className="card">
          <div className="text-sm font-medium text-slate-600 mb-1">Market Price</div>
          <div className="text-3xl font-bold text-slate-900">${formatNumber(etf.marketPrice, 2)}</div>
          <div className={`text-sm mt-1 ${premiumDiscount.color}`}>
            {premiumDiscount.label}: {premiumDiscount.value >= 0 ? '+' : ''}{formatNumber(premiumDiscount.value, 2)}%
          </div>
        </div>

        <div className="card">
          <div className="text-sm font-medium text-slate-600 mb-1">AUM</div>
          <div className="text-3xl font-bold text-slate-900">${formatNumber(etf.aum / 1e9, 2)}B</div>
          <div className="text-sm text-slate-500 mt-1">
            {formatNumber(etf.sharesOutstanding / 1e6, 1)}M shares
          </div>
        </div>
      </div>

      {/* Key Metrics */}
      <div className="grid md:grid-cols-4 gap-4">
        <div className={cn("card text-center", isLive && "ring-1 ring-green-200")}>
          <div className="stat-label">Avg Duration</div>
          <div className={cn("stat-value", isLive && "text-green-700")}>{formatNumber(displayDuration, 2)}</div>
          <div className="text-sm text-slate-500">years {isLive && <span className="text-green-600">(live)</span>}</div>
        </div>
        <div className={cn("card text-center", isLive && "ring-1 ring-green-200")}>
          <div className="stat-label">Avg Yield</div>
          <div className={cn("stat-value", isLive && "text-green-700")}>{formatNumber(displayYield, 2)}%</div>
          <div className="text-sm text-slate-500">yield to maturity {isLive && <span className="text-green-600">(live)</span>}</div>
        </div>
        <div className="card text-center">
          <div className="stat-label">Holdings</div>
          <div className="stat-value">{etf.holdings.length}</div>
          <div className="text-sm text-slate-500">bonds</div>
        </div>
        <div className="card text-center">
          <div className="stat-label">Expense Ratio</div>
          <div className="stat-value">{formatNumber(etf.expenseRatio, 2)}%</div>
          <div className="text-sm text-slate-500">annual</div>
        </div>
      </div>

      {/* Charts Row */}
      <div className="grid lg:grid-cols-3 gap-6">
        {/* Sector Breakdown */}
        <div className="card">
          <h3 className="card-header">Sector Breakdown</h3>
          <div className="h-64">
            <ResponsiveContainer width="100%" height="100%">
              <PieChart>
                <Pie
                  data={sectorData}
                  dataKey="value"
                  nameKey="name"
                  cx="50%"
                  cy="50%"
                  innerRadius={40}
                  outerRadius={80}
                  paddingAngle={2}
                >
                  {sectorData.map((entry, index) => (
                    <Cell key={`cell-${index}`} fill={entry.color} />
                  ))}
                </Pie>
                <Tooltip
                  formatter={(value: number) => [`${formatNumber(value, 2)}%`, 'Weight']}
                />
                <Legend />
              </PieChart>
            </ResponsiveContainer>
          </div>
        </div>

        {/* Rating Breakdown */}
        <div className="card">
          <h3 className="card-header flex items-center gap-2">
            Credit Quality
            <span className="group relative">
              <Info className="h-4 w-4 text-slate-400 cursor-help" />
              <span className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 px-3 py-2 bg-slate-800 text-white text-xs rounded-lg opacity-0 group-hover:opacity-100 transition-opacity whitespace-nowrap z-10">
                Ratings are estimated based on ETF type, sector, and yield spread.
                <br />
                Actual ratings require licensed data from S&P, Moody's, or Fitch.
              </span>
            </span>
          </h3>
          <div className="h-64">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={ratingData} layout="vertical">
                <CartesianGrid strokeDasharray="3 3" />
                <XAxis type="number" tickFormatter={(v) => `${v}%`} />
                <YAxis type="category" dataKey="name" width={50} />
                <Tooltip formatter={(value: number) => [`${formatNumber(value, 2)}%`, 'Weight']} />
                <Bar dataKey="value" radius={[0, 4, 4, 0]}>
                  {ratingData.map((entry, index) => (
                    <Cell key={`cell-${index}`} fill={entry.color} />
                  ))}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          </div>
        </div>

        {/* Duration Profile */}
        <div className="card">
          <h3 className="card-header">Duration Profile</h3>
          <div className="h-64">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={durationData}>
                <CartesianGrid strokeDasharray="3 3" />
                <XAxis dataKey="name" />
                <YAxis tickFormatter={(v) => `${v}%`} />
                <Tooltip formatter={(value: number) => [`${formatNumber(value, 2)}%`, 'Weight']} />
                <Bar dataKey="value" fill="#3b82f6" radius={[4, 4, 0, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </div>
      </div>

      {/* Holdings Grid */}
      <div className="card">
        <div className="flex flex-wrap items-center justify-between gap-4 mb-4">
          <h3 className="card-header mb-0">Holdings</h3>
          <input
            type="text"
            placeholder="Search issuer, CUSIP, sector..."
            value={searchTerm}
            onChange={(e) => setSearchTerm(e.target.value)}
            className="px-3 py-2 border border-slate-300 rounded-lg text-sm w-64 focus:outline-none focus:ring-2 focus:ring-primary-500"
          />
        </div>

        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-slate-200">
                <th
                  className="text-left py-3 px-3 font-medium text-slate-600 cursor-pointer hover:text-slate-900"
                  onClick={() => handleSort('issuer')}
                >
                  Issuer {sortField === 'issuer' && (sortDirection === 'asc' ? '↑' : '↓')}
                </th>
                <th className="text-left py-3 px-3 font-medium text-slate-600">CUSIP</th>
                <th
                  className="text-right py-3 px-3 font-medium text-slate-600 cursor-pointer hover:text-slate-900"
                  onClick={() => handleSort('coupon')}
                >
                  Coupon {sortField === 'coupon' && (sortDirection === 'asc' ? '↑' : '↓')}
                </th>
                <th className="text-center py-3 px-3 font-medium text-slate-600">Maturity</th>
                <th className="text-center py-3 px-3 font-medium text-slate-600">
                  <span className="inline-flex items-center gap-1">
                    Rating
                    <span className="group relative">
                      <Info className="h-3 w-3 text-slate-400 cursor-help" />
                      <span className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 px-2 py-1 bg-slate-800 text-white text-xs rounded opacity-0 group-hover:opacity-100 transition-opacity whitespace-nowrap z-10">
                        Estimated ratings
                      </span>
                    </span>
                  </span>
                </th>
                <th
                  className="text-right py-3 px-3 font-medium text-slate-600 cursor-pointer hover:text-slate-900"
                  onClick={() => handleSort('price')}
                >
                  Price {sortField === 'price' && (sortDirection === 'asc' ? '↑' : '↓')}
                </th>
                <th
                  className="text-right py-3 px-3 font-medium text-slate-600 cursor-pointer hover:text-slate-900"
                  onClick={() => handleSort('yield')}
                >
                  Yield {sortField === 'yield' && (sortDirection === 'asc' ? '↑' : '↓')}
                </th>
                <th
                  className="text-right py-3 px-3 font-medium text-slate-600 cursor-pointer hover:text-slate-900"
                  onClick={() => handleSort('duration')}
                >
                  Duration {sortField === 'duration' && (sortDirection === 'asc' ? '↑' : '↓')}
                </th>
                <th
                  className="text-right py-3 px-3 font-medium text-slate-600 cursor-pointer hover:text-slate-900"
                  onClick={() => handleSort('weight')}
                >
                  Weight {sortField === 'weight' && (sortDirection === 'asc' ? '↑' : '↓')}
                </th>
              </tr>
            </thead>
            <tbody>
              {filteredHoldings.map((holding) => (
                <tr key={holding.id} className="border-b border-slate-100 hover:bg-slate-50">
                  <td className="py-3 px-3">
                    <div className="font-medium text-slate-900">{holding.issuer}</div>
                    <div className="text-xs text-slate-500">{holding.sector}</div>
                  </td>
                  <td className="py-3 px-3 font-mono text-slate-600">{holding.cusip}</td>
                  <td className="py-3 px-3 text-right font-mono">{formatNumber(holding.coupon, 3)}%</td>
                  <td className="py-3 px-3 text-center font-mono text-slate-600">{holding.maturity}</td>
                  <td className="py-3 px-3 text-center">
                    <span
                      className="px-2 py-1 rounded text-xs font-medium"
                      style={{
                        backgroundColor: `${RATING_COLORS[holding.rating] || '#64748b'}20`,
                        color: RATING_COLORS[holding.rating] || '#64748b',
                      }}
                    >
                      {holding.rating}
                    </span>
                  </td>
                  <td className="py-3 px-3 text-right font-mono">${formatNumber(holding.price, 2)}</td>
                  <td className="py-3 px-3 text-right font-mono text-primary-700">{formatNumber(holding.yield, 2)}%</td>
                  <td className="py-3 px-3 text-right font-mono">{formatNumber(holding.duration, 1)}</td>
                  <td className="py-3 px-3 text-right">
                    <div className="font-medium">{formatNumber(holding.weight, 2)}%</div>
                    <div className="w-full bg-slate-200 rounded-full h-1.5 mt-1">
                      <div
                        className="bg-primary-500 h-1.5 rounded-full"
                        style={{ width: `${Math.min(holding.weight * 20, 100)}%` }}
                      />
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        {filteredHoldings.length === 0 && (
          <div className="text-center py-8 text-slate-500">
            No holdings match your search criteria
          </div>
        )}
      </div>

      {/* Info Panel */}
      <div className="card bg-slate-50">
        <h3 className="card-header text-slate-700">About ETF Analytics</h3>
        <div className="grid md:grid-cols-3 gap-6 text-sm text-slate-600">
          <div>
            <h4 className="font-semibold text-slate-700 mb-2">iNAV (Indicative NAV)</h4>
            <p>
              Real-time estimate of NAV calculated every 15 seconds during market hours.
              Uses live bond prices to estimate the current value of the portfolio.
            </p>
          </div>
          <div>
            <h4 className="font-semibold text-slate-700 mb-2">Premium/Discount</h4>
            <p>
              Difference between market price and NAV. A premium means the ETF trades
              above its NAV; a discount means it trades below. Typically stays within 0.5%.
            </p>
          </div>
          <div>
            <h4 className="font-semibold text-slate-700 mb-2">Duration Profile</h4>
            <p>
              Distribution of holdings by modified duration. Longer duration means
              higher interest rate sensitivity. Useful for matching liability profiles.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
