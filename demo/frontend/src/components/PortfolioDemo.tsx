import { useState } from 'react';
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
import { formatNumber, formatBps, cn } from '../lib/utils';

// Sample portfolio data
const SAMPLE_PORTFOLIO = {
  id: 'demo-portfolio',
  name: 'Sample IG Corporate Portfolio',
  nav: 50_000_000,
  modifiedDuration: 5.23,
  convexity: 42.5,
  ytm: 5.12,
  zSpread: 68,
  dv01: 26_150,
  holdingsCount: 15,
};

// Sector allocation
const SECTOR_DATA = [
  { name: 'Financials', value: 28, duration: 4.8, spread: 85 },
  { name: 'Technology', value: 22, duration: 5.1, spread: 42 },
  { name: 'Healthcare', value: 18, duration: 5.5, spread: 62 },
  { name: 'Communications', value: 15, duration: 6.2, spread: 95 },
  { name: 'Consumer', value: 12, duration: 4.9, spread: 58 },
  { name: 'Other', value: 5, duration: 5.0, spread: 70 },
];

// Rating distribution
const RATING_DATA = [
  { rating: 'AAA', value: 8, color: '#22c55e' },
  { rating: 'AA', value: 18, color: '#84cc16' },
  { rating: 'A', value: 42, color: '#3b82f6' },
  { rating: 'BBB', value: 32, color: '#f59e0b' },
];

// Key rate durations
const KRD_DATA = [
  { tenor: '6M', krd: 0.12 },
  { tenor: '1Y', krd: 0.28 },
  { tenor: '2Y', krd: 0.85 },
  { tenor: '3Y', krd: 0.95 },
  { tenor: '5Y', krd: 1.45 },
  { tenor: '7Y', krd: 0.78 },
  { tenor: '10Y', krd: 0.65 },
  { tenor: '20Y', krd: 0.12 },
  { tenor: '30Y', krd: 0.03 },
];

// Stress scenarios
const STRESS_SCENARIOS = [
  { name: 'Rates +100bp', impact: -5.23, description: 'Parallel shift up 100bp' },
  { name: 'Rates -100bp', impact: 5.23, description: 'Parallel shift down 100bp' },
  { name: 'Rates +50bp', impact: -2.62, description: 'Parallel shift up 50bp' },
  { name: 'Spread +50bp', impact: -2.61, description: 'Credit spreads widen 50bp' },
  { name: 'Flattener', impact: -1.85, description: '2s10s flattening 50bp' },
  { name: 'Steepener', impact: 1.85, description: '2s10s steepening 50bp' },
];

const COLORS = ['#3b82f6', '#22c55e', '#f59e0b', '#ef4444', '#8b5cf6', '#6b7280'];

type Tab = 'overview' | 'allocation' | 'risk' | 'stress';

export default function PortfolioDemo() {
  const [activeTab, setActiveTab] = useState<Tab>('overview');

  return (
    <div className="space-y-6">
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
            <div className="card">
              <div className="stat-label">Portfolio NAV</div>
              <div className="stat-value">$50.0M</div>
              <p className="text-sm text-slate-500 mt-1">15 holdings</p>
            </div>
            <div className="card">
              <div className="stat-label">Modified Duration</div>
              <div className="stat-value text-primary-700">{formatNumber(SAMPLE_PORTFOLIO.modifiedDuration, 2)}</div>
              <p className="text-sm text-slate-500 mt-1">years</p>
            </div>
            <div className="card">
              <div className="stat-label">Portfolio YTM</div>
              <div className="stat-value">{formatNumber(SAMPLE_PORTFOLIO.ytm, 2)}%</div>
              <p className="text-sm text-slate-500 mt-1">yield to maturity</p>
            </div>
            <div className="card">
              <div className="stat-label">DV01</div>
              <div className="stat-value">${formatNumber(SAMPLE_PORTFOLIO.dv01, 0)}</div>
              <p className="text-sm text-slate-500 mt-1">dollar duration per bp</p>
            </div>
          </div>

          {/* Additional Metrics */}
          <div className="grid md:grid-cols-3 gap-4">
            <div className="card">
              <div className="stat-label">Convexity</div>
              <div className="stat-value">{formatNumber(SAMPLE_PORTFOLIO.convexity, 1)}</div>
            </div>
            <div className="card">
              <div className="stat-label">Avg Z-Spread</div>
              <div className="stat-value">{formatBps(SAMPLE_PORTFOLIO.zSpread)}</div>
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
                    data={SECTOR_DATA}
                    cx="50%"
                    cy="50%"
                    outerRadius={80}
                    dataKey="value"
                    label={({ name, value }) => `${name} ${value}%`}
                  >
                    {SECTOR_DATA.map((_, index) => (
                      <Cell key={index} fill={COLORS[index % COLORS.length]} />
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
                    <th className="text-right py-2 font-medium text-slate-600">Duration</th>
                    <th className="text-right py-2 font-medium text-slate-600">Spread</th>
                  </tr>
                </thead>
                <tbody>
                  {SECTOR_DATA.map((sector, i) => (
                    <tr key={sector.name} className="border-b border-slate-100">
                      <td className="py-2 flex items-center gap-2">
                        <div
                          className="w-3 h-3 rounded"
                          style={{ backgroundColor: COLORS[i] }}
                        />
                        {sector.name}
                      </td>
                      <td className="py-2 text-right font-mono">{sector.value}%</td>
                      <td className="py-2 text-right font-mono">{formatNumber(sector.duration, 1)}</td>
                      <td className="py-2 text-right font-mono">{sector.spread}bp</td>
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
                <BarChart data={RATING_DATA} layout="vertical">
                  <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
                  <XAxis type="number" domain={[0, 50]} tickFormatter={(v) => `${v}%`} />
                  <YAxis type="category" dataKey="rating" width={50} />
                  <Tooltip formatter={(value: number) => [`${value}%`, 'Weight']} />
                  <Bar dataKey="value" radius={[0, 4, 4, 0]}>
                    {RATING_DATA.map((entry) => (
                      <Cell key={entry.rating} fill={entry.color} />
                    ))}
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
            </div>
            <div className="mt-4 grid grid-cols-4 gap-2 text-center">
              {RATING_DATA.map((r) => (
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
            <h3 className="card-header">Key Rate Durations</h3>
            <div className="h-72">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={KRD_DATA}>
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
              Sum of KRDs equals modified duration ({formatNumber(KRD_DATA.reduce((sum, k) => sum + k.krd, 0), 2)}).
            </p>
          </div>

          {/* Risk Summary */}
          <div className="grid md:grid-cols-3 gap-4">
            <div className="card">
              <div className="stat-label">Duration Contribution</div>
              <div className="space-y-2 mt-2">
                {SECTOR_DATA.slice(0, 4).map((sector) => (
                  <div key={sector.name} className="flex justify-between text-sm">
                    <span className="text-slate-600">{sector.name}</span>
                    <span className="font-mono">
                      {formatNumber((sector.value / 100) * sector.duration, 2)}
                    </span>
                  </div>
                ))}
              </div>
            </div>
            <div className="card">
              <div className="stat-label">Spread Risk</div>
              <div className="space-y-2 mt-2">
                <div className="flex justify-between text-sm">
                  <span className="text-slate-600">Spread Duration</span>
                  <span className="font-mono">5.15</span>
                </div>
                <div className="flex justify-between text-sm">
                  <span className="text-slate-600">CS01 (per bp)</span>
                  <span className="font-mono">$25,750</span>
                </div>
              </div>
            </div>
            <div className="card">
              <div className="stat-label">VaR (95%, 1-day)</div>
              <div className="stat-value text-loss">-$182,500</div>
              <p className="text-sm text-slate-500 mt-1">-0.37% of NAV</p>
            </div>
          </div>
        </div>
      )}

      {/* Stress Testing Tab */}
      {activeTab === 'stress' && (
        <div className="card">
          <h3 className="card-header">Stress Test Results</h3>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="bg-slate-50 border-b border-slate-200">
                  <th className="text-left py-3 px-4 font-medium text-slate-600">Scenario</th>
                  <th className="text-left py-3 px-4 font-medium text-slate-600">Description</th>
                  <th className="text-right py-3 px-4 font-medium text-slate-600">P&L Impact (%)</th>
                  <th className="text-right py-3 px-4 font-medium text-slate-600">P&L Impact ($)</th>
                </tr>
              </thead>
              <tbody>
                {STRESS_SCENARIOS.map((scenario) => (
                  <tr key={scenario.name} className="border-b border-slate-100 hover:bg-slate-50">
                    <td className="py-3 px-4 font-medium">{scenario.name}</td>
                    <td className="py-3 px-4 text-slate-600">{scenario.description}</td>
                    <td className={cn(
                      'py-3 px-4 text-right font-mono',
                      scenario.impact >= 0 ? 'text-gain' : 'text-loss'
                    )}>
                      {scenario.impact >= 0 ? '+' : ''}{formatNumber(scenario.impact, 2)}%
                    </td>
                    <td className={cn(
                      'py-3 px-4 text-right font-mono',
                      scenario.impact >= 0 ? 'text-gain' : 'text-loss'
                    )}>
                      {scenario.impact >= 0 ? '+' : ''}
                      ${formatNumber(Math.abs(scenario.impact) / 100 * SAMPLE_PORTFOLIO.nav / 1000, 0)}K
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div className="mt-6 p-4 bg-slate-50 rounded-lg">
            <h4 className="font-medium text-slate-900 mb-2">Scenario Analysis</h4>
            <p className="text-sm text-slate-600">
              The portfolio shows moderate rate sensitivity (duration ~5.2) with most risk concentrated
              in the 3-5 year sector. A 100bp parallel shift in rates would result in approximately
              ±$2.6M P&L impact. Spread risk is also significant, with a 50bp spread widening
              causing ~$1.3M loss.
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
