import { useState, useMemo } from 'react';
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
import { formatNumber } from '../lib/utils';

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

export default function ETFAnalyticsDemo() {
  const [selectedETF, setSelectedETF] = useState<string>('LQD');
  const [searchTerm, setSearchTerm] = useState('');
  const [sortField, setSortField] = useState<keyof ETFHolding>('weight');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('desc');

  const etf = useMemo(() => {
    return DEMO_ETFS.find(e => e.id === selectedETF) || DEMO_ETFS[0];
  }, [selectedETF]);

  const premiumDiscount = getPremiumDiscount(etf.marketPrice, etf.nav);

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
      {/* ETF Selector */}
      <div className="card">
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
      </div>

      {/* NAV / iNAV / Price Panel */}
      <div className="grid md:grid-cols-4 gap-4">
        <div className="card bg-gradient-to-br from-blue-50 to-indigo-50">
          <div className="text-sm font-medium text-slate-600 mb-1">NAV</div>
          <div className="text-3xl font-bold text-slate-900">${formatNumber(etf.nav, 2)}</div>
          <div className="text-sm text-slate-500 mt-1">Net Asset Value</div>
        </div>

        <div className="card bg-gradient-to-br from-green-50 to-emerald-50">
          <div className="text-sm font-medium text-slate-600 mb-1">iNAV</div>
          <div className="text-3xl font-bold text-slate-900">${formatNumber(etf.inav, 2)}</div>
          <div className="text-sm text-slate-500 mt-1">
            Indicative NAV
            <span className="ml-1 text-gain">
              (+${formatNumber(etf.inav - etf.nav, 2)})
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
        <div className="card text-center">
          <div className="stat-label">Avg Duration</div>
          <div className="stat-value">{formatNumber(etf.avgDuration, 2)}</div>
          <div className="text-sm text-slate-500">years</div>
        </div>
        <div className="card text-center">
          <div className="stat-label">Avg Yield</div>
          <div className="stat-value">{formatNumber(etf.avgYield, 2)}%</div>
          <div className="text-sm text-slate-500">yield to maturity</div>
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
          <h3 className="card-header">Credit Quality</h3>
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
                <th className="text-center py-3 px-3 font-medium text-slate-600">Rating</th>
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
