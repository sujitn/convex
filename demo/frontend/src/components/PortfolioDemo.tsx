import { useMemo, useState } from 'react';
import { BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, PieChart, Pie, Cell } from 'recharts';
import { formatNumber, cn } from '../lib/utils';

// Illustrative supplied positions; these are not live holdings or market marks.
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

const COLORS = ['#3b82f6', '#22c55e', '#f59e0b', '#ef4444', '#8b5cf6', '#6b7280', '#ec4899', '#14b8a6'];
type Tab = 'overview' | 'allocation' | 'risk' | 'stress';

export default function PortfolioDemo() {
  const [activeTab, setActiveTab] = useState<Tab>('overview');
  const totalFace = SAMPLE_HOLDINGS.reduce((sum, holding) => sum + holding.notional, 0);
  const allocation = useMemo(() => {
    const sectors = new Map<string, number>();
    const ratings = new Map<string, number>();
    for (const holding of SAMPLE_HOLDINGS) {
      sectors.set(holding.sector, (sectors.get(holding.sector) ?? 0) + holding.notional);
      ratings.set(holding.rating, (ratings.get(holding.rating) ?? 0) + holding.notional);
    }
    return {
      sectors: Array.from(sectors, ([name, face]) => ({name, face, value: face / totalFace * 100})),
      ratings: Array.from(ratings, ([name, face]) => ({name, face, value: face / totalFace * 100})),
    };
  }, [totalFace]);
  return (
    <div className="space-y-6">
      <div className="card">
        <h3 className="font-semibold text-slate-900">Illustrative corporate bond portfolio</h3>
        <p className="text-sm text-slate-600 mt-2">
          {SAMPLE_HOLDINGS.length} supplied sample positions, ${formatNumber(totalFace / 1e6, 2)}M face amount.
          Face amount is not portfolio market value. No live market-data feed is selected.
        </p>
      </div>
      <div className="card flex flex-wrap gap-2">
        {(['overview', 'allocation', 'risk', 'stress'] as Tab[]).map(tab => (
          <button key={tab} onClick={() => setActiveTab(tab)} className={cn('px-4 py-2 rounded-lg text-sm font-medium',
            activeTab === tab ? 'bg-primary-100 text-primary-700' : 'text-slate-600 hover:bg-slate-100')}>
            {tab === 'overview' ? 'Positions' : tab === 'allocation' ? 'Face amount allocation' : tab === 'risk' ? 'Risk analytics' : 'Stress testing'}
          </button>
        ))}
      </div>
      {activeTab === 'overview' && (
        <div className="card overflow-x-auto">
          <h3 className="card-header">Supplied sample positions</h3>
          <table className="w-full text-sm">
            <thead><tr className="border-b border-slate-200">
              <th className="text-left py-3">Issuer</th><th className="text-left">CUSIP</th>
              <th className="text-right">Coupon</th><th className="text-right">Maturity</th>
              <th className="text-right">Face amount (USD)</th><th className="text-right">Rating</th>
            </tr></thead>
            <tbody>{SAMPLE_HOLDINGS.map(holding => (
              <tr key={holding.id} className="border-b border-slate-100">
                <td className="py-3">{holding.issuer}</td><td className="font-mono">{holding.cusip}</td>
                <td className="text-right">{formatNumber(holding.coupon, 2)}%</td>
                <td className="text-right">{holding.maturity}</td>
                <td className="text-right font-mono">{formatNumber(holding.notional, 0)}</td>
                <td className="text-right">{holding.rating}</td>
              </tr>
            ))}</tbody>
          </table>
        </div>
      )}
      {activeTab === 'allocation' && (
        <div className="grid md:grid-cols-2 gap-6">
          <div className="card">
            <h3 className="card-header">Sector allocation by face amount</h3>
            <div className="h-64"><ResponsiveContainer width="100%" height="100%"><PieChart>
              <Pie data={allocation.sectors} dataKey="value" nameKey="name" outerRadius={90}>
                {allocation.sectors.map((row, index) => <Cell key={row.name} fill={COLORS[index % COLORS.length]} />)}
              </Pie><Tooltip formatter={(value: number) => [`${formatNumber(value, 2)}%`, 'Face amount']} />
            </PieChart></ResponsiveContainer></div>
            <p className="text-sm text-slate-500">These weights use supplied face amounts; no market-value or risk weighting is implied.</p>
          </div>
          <div className="card">
            <h3 className="card-header">Rating allocation by face amount</h3>
            <div className="h-64"><ResponsiveContainer width="100%" height="100%"><BarChart data={allocation.ratings} layout="vertical">
              <CartesianGrid strokeDasharray="3 3" /><XAxis type="number" tickFormatter={value => `${value}%`} />
              <YAxis type="category" dataKey="name" width={50} />
              <Tooltip formatter={(value: number) => [`${formatNumber(value, 2)}%`, 'Face amount']} />
              <Bar dataKey="value" fill={COLORS[0]} />
            </BarChart></ResponsiveContainer></div>
          </div>
        </div>
      )}
      {activeTab === 'risk' && (
        <div className="card">
          <h3 className="card-header">Risk analytics unavailable</h3>
          <p className="text-sm text-slate-600">Supply complete positions, valuation context and compatible calculated sensitivities to display portfolio risk.</p>
          <div className="grid sm:grid-cols-3 gap-4 mt-5">
            {['Market value', 'Duration', 'Convexity', 'Key-rate risk', 'DV01 / CS01', 'Value at risk'].map(label => (
              <div key={label}><div className="stat-label">{label}</div><div className="text-slate-500 mt-1">Unavailable</div></div>
            ))}
          </div>
        </div>
      )}
      {activeTab === 'stress' && (
        <div className="card">
          <h3 className="card-header">Stress results unavailable</h3>
          <p className="text-sm text-slate-600">Supply complete valuation inputs, risk assumptions and scenario shocks to calculate P&amp;L.</p>
          <p className="text-sm text-slate-500 mt-3">No scenario result or concentration estimate has been calculated for these sample positions.</p>
        </div>
      )}
    </div>
  );
}
