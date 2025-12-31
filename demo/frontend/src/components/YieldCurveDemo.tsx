import { useState } from 'react';
import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts';
import { formatNumber } from '../lib/utils';

// Sample Treasury curve data (Dec 2025)
const SAMPLE_TREASURY_CURVE = [
  { tenor: '1M', days: 30, rate: 3.69 },
  { tenor: '2M', days: 60, rate: 3.51 },
  { tenor: '3M', days: 90, rate: 3.48 },
  { tenor: '6M', days: 180, rate: 3.45 },
  { tenor: '1Y', days: 365, rate: 3.45 },
  { tenor: '2Y', days: 730, rate: 3.80 },
  { tenor: '3Y', days: 1095, rate: 3.92 },
  { tenor: '5Y', days: 1825, rate: 4.03 },
  { tenor: '7Y', days: 2555, rate: 4.17 },
  { tenor: '10Y', days: 3650, rate: 4.12 },
  { tenor: '20Y', days: 7300, rate: 4.51 },
  { tenor: '30Y', days: 10950, rate: 4.80 },
];

// Sample SOFR OIS curve
const SAMPLE_SOFR_CURVE = [
  { tenor: 'ON', days: 1, rate: 3.71 },
  { tenor: '1M', days: 30, rate: 3.68 },
  { tenor: '3M', days: 90, rate: 3.52 },
  { tenor: '6M', days: 180, rate: 3.40 },
  { tenor: '1Y', days: 365, rate: 3.32 },
  { tenor: '2Y', days: 730, rate: 3.28 },
  { tenor: '3Y', days: 1095, rate: 3.30 },
  { tenor: '5Y', days: 1825, rate: 3.36 },
  { tenor: '10Y', days: 3650, rate: 3.65 },
];

export default function YieldCurveDemo() {
  const [selectedCurve, setSelectedCurve] = useState<'treasury' | 'sofr'>('treasury');

  const chartData = selectedCurve === 'treasury' ? SAMPLE_TREASURY_CURVE : SAMPLE_SOFR_CURVE;

  return (
    <div className="space-y-6">
      {/* Curve Selector */}
      <div className="card">
        <div className="flex flex-wrap gap-4 mb-6">
          <button
            onClick={() => setSelectedCurve('treasury')}
            className={`btn ${selectedCurve === 'treasury' ? 'btn-primary' : 'btn-secondary'}`}
          >
            US Treasury Curve
          </button>
          <button
            onClick={() => setSelectedCurve('sofr')}
            className={`btn ${selectedCurve === 'sofr' ? 'btn-primary' : 'btn-secondary'}`}
          >
            SOFR OIS Curve
          </button>
        </div>

        {/* Curve Chart */}
        <div className="h-80">
          <ResponsiveContainer width="100%" height="100%">
            <LineChart data={chartData} margin={{ top: 5, right: 30, left: 20, bottom: 5 }}>
              <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
              <XAxis
                dataKey="tenor"
                tick={{ fill: '#64748b', fontSize: 12 }}
                tickLine={{ stroke: '#e2e8f0' }}
              />
              <YAxis
                domain={['auto', 'auto']}
                tick={{ fill: '#64748b', fontSize: 12 }}
                tickLine={{ stroke: '#e2e8f0' }}
                tickFormatter={(v) => `${v.toFixed(1)}%`}
              />
              <Tooltip
                contentStyle={{
                  backgroundColor: '#fff',
                  border: '1px solid #e2e8f0',
                  borderRadius: '8px',
                }}
                formatter={(value: number) => [`${value.toFixed(3)}%`, 'Rate']}
              />
              <Line
                type="monotone"
                dataKey="rate"
                stroke="#2563eb"
                strokeWidth={2}
                dot={{ fill: '#2563eb', r: 4 }}
                activeDot={{ r: 6 }}
              />
            </LineChart>
          </ResponsiveContainer>
        </div>
      </div>

      {/* Curve Data Table */}
      <div className="card">
        <h3 className="card-header">
          {selectedCurve === 'treasury' ? 'US Treasury Yields' : 'SOFR OIS Rates'}
          <span className="text-sm font-normal text-slate-500 ml-2">
            As of Dec 29, 2025
          </span>
        </h3>

        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-slate-200">
                <th className="text-left py-2 px-3 font-medium text-slate-600">Tenor</th>
                <th className="text-right py-2 px-3 font-medium text-slate-600">Days</th>
                <th className="text-right py-2 px-3 font-medium text-slate-600">Rate (%)</th>
                <th className="text-right py-2 px-3 font-medium text-slate-600">Rate (decimal)</th>
              </tr>
            </thead>
            <tbody>
              {chartData.map((point) => (
                <tr key={point.tenor} className="border-b border-slate-100 hover:bg-slate-50">
                  <td className="py-2 px-3 font-medium">{point.tenor}</td>
                  <td className="py-2 px-3 text-right font-mono">{point.days}</td>
                  <td className="py-2 px-3 text-right font-mono text-primary-700">
                    {formatNumber(point.rate, 3)}%
                  </td>
                  <td className="py-2 px-3 text-right font-mono text-slate-500">
                    {formatNumber(point.rate / 100, 5)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {/* Curve Analysis */}
      <div className="grid md:grid-cols-3 gap-4">
        <div className="card">
          <div className="stat-label">Curve Shape</div>
          <div className="stat-value text-primary-700">
            {selectedCurve === 'treasury' ? 'Inverted (Short End)' : 'Inverted'}
          </div>
          <p className="text-sm text-slate-500 mt-2">
            Short-term rates above long-term rates
          </p>
        </div>

        <div className="card">
          <div className="stat-label">2Y-10Y Spread</div>
          <div className="stat-value text-loss">
            {selectedCurve === 'treasury'
              ? formatNumber((4.12 - 3.80) * 100, 0)
              : formatNumber((3.65 - 3.28) * 100, 0)} bps
          </div>
          <p className="text-sm text-slate-500 mt-2">
            {selectedCurve === 'treasury' ? '10Y - 2Y Treasury spread' : '10Y - 2Y SOFR spread'}
          </p>
        </div>

        <div className="card">
          <div className="stat-label">Fed Funds Target</div>
          <div className="stat-value">3.50-3.75%</div>
          <p className="text-sm text-slate-500 mt-2">
            After 25bp cut on Dec 10, 2025
          </p>
        </div>
      </div>
    </div>
  );
}
