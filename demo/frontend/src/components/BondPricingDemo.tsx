import { useState } from 'react';
import { Calculator, List } from 'lucide-react';
import { formatNumber, formatBps, cn } from '../lib/utils';
import BondCalculator from './BondCalculator';

// Sample corporate bonds
const SAMPLE_BONDS = [
  {
    id: 'AAPL-5.0-2030',
    issuer: 'Apple Inc',
    coupon: 5.0,
    maturity: '2030-06-15',
    rating: 'AA+',
    sector: 'Technology',
    price: 102.345,
    ytm: 4.65,
    zSpread: 32,
    duration: 4.23,
    convexity: 21.5,
    type: 'fixed',
  },
  {
    id: 'MSFT-4.5-2032',
    issuer: 'Microsoft Corp',
    coupon: 4.5,
    maturity: '2032-03-15',
    rating: 'AAA',
    sector: 'Technology',
    price: 99.875,
    ytm: 4.52,
    zSpread: 25,
    duration: 5.67,
    convexity: 38.2,
    type: 'fixed',
  },
  {
    id: 'JPM-5.25-2035',
    issuer: 'JPMorgan Chase',
    coupon: 5.25,
    maturity: '2035-09-01',
    rating: 'A-',
    sector: 'Financials',
    price: 101.234,
    ytm: 5.12,
    zSpread: 95,
    duration: 7.89,
    convexity: 74.5,
    type: 'fixed',
  },
  {
    id: 'T-5.5-2035-CALL',
    issuer: 'AT&T Inc',
    coupon: 5.5,
    maturity: '2035-06-15',
    rating: 'BBB',
    sector: 'Communications',
    price: 98.567,
    ytm: 5.68,
    ytw: 5.42,
    zSpread: 145,
    oas: 132,
    duration: 7.23,
    effectiveDuration: 6.45,
    convexity: 65.3,
    type: 'callable',
    callDate: '2030-06-15',
    callPrice: 102.0,
  },
  {
    id: 'GS-FRN-2027',
    issuer: 'Goldman Sachs',
    spread: 95,
    maturity: '2027-08-15',
    rating: 'A',
    sector: 'Financials',
    price: 99.875,
    discountMargin: 92,
    spreadDuration: 2.34,
    type: 'frn',
    index: 'SOFR',
  },
];

type BondType = 'all' | 'fixed' | 'callable' | 'frn';
type ViewMode = 'calculator' | 'universe';

export default function BondPricingDemo() {
  const [viewMode, setViewMode] = useState<ViewMode>('calculator');
  const [bondType, setBondType] = useState<BondType>('all');
  const [selectedBond, setSelectedBond] = useState<typeof SAMPLE_BONDS[0] | null>(null);

  const filteredBonds = bondType === 'all'
    ? SAMPLE_BONDS
    : SAMPLE_BONDS.filter((b) => b.type === bondType);

  return (
    <div className="space-y-6">
      {/* View Mode Toggle */}
      <div className="card">
        <div className="flex flex-wrap gap-2">
          <button
            onClick={() => setViewMode('calculator')}
            className={cn(
              'btn flex items-center gap-2',
              viewMode === 'calculator' ? 'btn-primary' : 'btn-secondary'
            )}
          >
            <Calculator className="w-4 h-4" />
            Interactive Calculator
          </button>
          <button
            onClick={() => setViewMode('universe')}
            className={cn(
              'btn flex items-center gap-2',
              viewMode === 'universe' ? 'btn-primary' : 'btn-secondary'
            )}
          >
            <List className="w-4 h-4" />
            Bond Universe
          </button>
        </div>
      </div>

      {viewMode === 'calculator' ? (
        <BondCalculator />
      ) : (
        <>
          {/* Bond Type Filter */}
          <div className="card">
            <div className="flex flex-wrap gap-2">
              {(['all', 'fixed', 'callable', 'frn'] as BondType[]).map((type) => (
                <button
                  key={type}
                  onClick={() => setBondType(type)}
                  className={cn(
                    'btn',
                    bondType === type ? 'btn-primary' : 'btn-secondary'
                  )}
                >
                  {type === 'all' ? 'All Bonds' :
                  type === 'fixed' ? 'Fixed Rate' :
                  type === 'callable' ? 'Callable' : 'Floating Rate'}
                </button>
              ))}
            </div>
          </div>

      {/* Bonds Table */}
      <div className="card overflow-hidden">
        <h3 className="card-header">Corporate Bond Universe</h3>
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="bg-slate-50 border-b border-slate-200">
                <th className="text-left py-3 px-4 font-medium text-slate-600">Bond ID</th>
                <th className="text-left py-3 px-4 font-medium text-slate-600">Issuer</th>
                <th className="text-center py-3 px-4 font-medium text-slate-600">Rating</th>
                <th className="text-right py-3 px-4 font-medium text-slate-600">Coupon/Spread</th>
                <th className="text-right py-3 px-4 font-medium text-slate-600">Price</th>
                <th className="text-right py-3 px-4 font-medium text-slate-600">Yield/DM</th>
                <th className="text-right py-3 px-4 font-medium text-slate-600">Z-Spread</th>
                <th className="text-right py-3 px-4 font-medium text-slate-600">Duration</th>
              </tr>
            </thead>
            <tbody>
              {filteredBonds.map((bond) => (
                <tr
                  key={bond.id}
                  onClick={() => setSelectedBond(bond)}
                  className={cn(
                    'border-b border-slate-100 cursor-pointer transition-colors',
                    selectedBond?.id === bond.id ? 'bg-primary-50' : 'hover:bg-slate-50'
                  )}
                >
                  <td className="py-3 px-4 font-mono text-xs">{bond.id}</td>
                  <td className="py-3 px-4">
                    <div className="font-medium">{bond.issuer}</div>
                    <div className="text-xs text-slate-500">{bond.sector}</div>
                  </td>
                  <td className="py-3 px-4 text-center">
                    <span className={cn(
                      'badge',
                      bond.rating.startsWith('A') ? 'badge-green' :
                      bond.rating.startsWith('B') ? 'badge-blue' : 'bg-slate-100 text-slate-700'
                    )}>
                      {bond.rating}
                    </span>
                  </td>
                  <td className="py-3 px-4 text-right font-mono">
                    {bond.type === 'frn'
                      ? `${bond.index}+${bond.spread}bp`
                      : `${formatNumber(bond.coupon, 2)}%`}
                  </td>
                  <td className="py-3 px-4 text-right font-mono">
                    {formatNumber(bond.price, 3)}
                  </td>
                  <td className="py-3 px-4 text-right font-mono">
                    {bond.type === 'frn'
                      ? formatBps(bond.discountMargin)
                      : `${formatNumber(bond.ytm, 2)}%`}
                  </td>
                  <td className="py-3 px-4 text-right font-mono">
                    {bond.type === 'frn' ? '-' : formatBps(bond.zSpread)}
                  </td>
                  <td className="py-3 px-4 text-right font-mono">
                    {bond.type === 'frn'
                      ? formatNumber(bond.spreadDuration, 2)
                      : formatNumber(bond.duration, 2)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {/* Bond Details Panel */}
      {selectedBond && (
        <div className="card">
          <h3 className="card-header flex items-center justify-between">
            <span>{selectedBond.issuer} - {selectedBond.id}</span>
            <button
              onClick={() => setSelectedBond(null)}
              className="text-sm text-slate-500 hover:text-slate-700"
            >
              Close
            </button>
          </h3>

          <div className="grid md:grid-cols-2 lg:grid-cols-4 gap-6">
            {/* Price & Yield */}
            <div>
              <div className="text-xs text-slate-500 uppercase mb-2">Price & Yield</div>
              <div className="space-y-2">
                <div className="flex justify-between">
                  <span className="text-slate-600">Clean Price</span>
                  <span className="font-mono">{formatNumber(selectedBond.price, 4)}</span>
                </div>
                {selectedBond.type !== 'frn' && (
                  <div className="flex justify-between">
                    <span className="text-slate-600">YTM</span>
                    <span className="font-mono">{formatNumber(selectedBond.ytm, 3)}%</span>
                  </div>
                )}
                {selectedBond.type === 'callable' && (
                  <>
                    <div className="flex justify-between">
                      <span className="text-slate-600">YTW</span>
                      <span className="font-mono text-loss">{formatNumber(selectedBond.ytw, 3)}%</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-slate-600">Call Date</span>
                      <span className="font-mono">{selectedBond.callDate}</span>
                    </div>
                  </>
                )}
                {selectedBond.type === 'frn' && (
                  <>
                    <div className="flex justify-between">
                      <span className="text-slate-600">Index</span>
                      <span className="font-mono">{selectedBond.index}</span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-slate-600">Spread</span>
                      <span className="font-mono">+{selectedBond.spread}bp</span>
                    </div>
                  </>
                )}
              </div>
            </div>

            {/* Spreads */}
            <div>
              <div className="text-xs text-slate-500 uppercase mb-2">Spreads</div>
              <div className="space-y-2">
                {selectedBond.type !== 'frn' && (
                  <div className="flex justify-between">
                    <span className="text-slate-600">Z-Spread</span>
                    <span className="font-mono">{formatBps(selectedBond.zSpread)}</span>
                  </div>
                )}
                {selectedBond.type === 'callable' && selectedBond.oas && (
                  <div className="flex justify-between">
                    <span className="text-slate-600">OAS</span>
                    <span className="font-mono">{formatBps(selectedBond.oas)}</span>
                  </div>
                )}
                {selectedBond.type === 'frn' && (
                  <div className="flex justify-between">
                    <span className="text-slate-600">Discount Margin</span>
                    <span className="font-mono">{formatBps(selectedBond.discountMargin)}</span>
                  </div>
                )}
              </div>
            </div>

            {/* Risk Metrics */}
            <div>
              <div className="text-xs text-slate-500 uppercase mb-2">Risk Metrics</div>
              <div className="space-y-2">
                <div className="flex justify-between">
                  <span className="text-slate-600">
                    {selectedBond.type === 'frn' ? 'Spread Duration' : 'Modified Duration'}
                  </span>
                  <span className="font-mono">
                    {selectedBond.type === 'frn'
                      ? formatNumber(selectedBond.spreadDuration, 2)
                      : formatNumber(selectedBond.duration, 2)}
                  </span>
                </div>
                {selectedBond.type === 'callable' && selectedBond.effectiveDuration && (
                  <div className="flex justify-between">
                    <span className="text-slate-600">Effective Duration</span>
                    <span className="font-mono">{formatNumber(selectedBond.effectiveDuration, 2)}</span>
                  </div>
                )}
                {selectedBond.convexity && (
                  <div className="flex justify-between">
                    <span className="text-slate-600">Convexity</span>
                    <span className="font-mono">{formatNumber(selectedBond.convexity, 1)}</span>
                  </div>
                )}
              </div>
            </div>

            {/* Bond Info */}
            <div>
              <div className="text-xs text-slate-500 uppercase mb-2">Bond Info</div>
              <div className="space-y-2">
                <div className="flex justify-between">
                  <span className="text-slate-600">Type</span>
                  <span className="badge badge-blue">
                    {selectedBond.type === 'fixed' ? 'Fixed Rate' :
                     selectedBond.type === 'callable' ? 'Callable' : 'FRN'}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-600">Maturity</span>
                  <span className="font-mono">{selectedBond.maturity}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-600">Sector</span>
                  <span>{selectedBond.sector}</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      )}
        </>
      )}
    </div>
  );
}
