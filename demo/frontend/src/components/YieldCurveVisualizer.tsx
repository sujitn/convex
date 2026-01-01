import { useState, useMemo, useCallback } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
  ReferenceLine,
  Area,
  ComposedChart,
} from 'recharts';
import { Wifi, WifiOff, RefreshCw } from 'lucide-react';
import { formatNumber, cn } from '../lib/utils';
import { fetchMarketDataFromProvider, DataProviderYieldCurve } from '../lib/api';

// Curve data types
interface CurvePoint {
  tenor: string;
  years: number;
  rate: number;
}

interface CurveData {
  id: string;
  name: string;
  color: string;
  points: CurvePoint[];
  description: string;
}

// Demo market curves (realistic Dec 2025 data)
// In production, these would come from a market data provider implementing PricingDataProvider trait
const DEMO_CURVES: CurveData[] = [
  {
    id: 'USD_GOVT',
    name: 'US Treasury',
    color: '#2563eb',
    description: 'US Government benchmark yields',
    points: [
      { tenor: '1M', years: 1/12, rate: 4.32 },
      { tenor: '3M', years: 0.25, rate: 4.30 },
      { tenor: '6M', years: 0.5, rate: 4.28 },
      { tenor: '1Y', years: 1, rate: 4.20 },
      { tenor: '2Y', years: 2, rate: 4.15 },
      { tenor: '3Y', years: 3, rate: 4.12 },
      { tenor: '5Y', years: 5, rate: 4.10 },
      { tenor: '7Y', years: 7, rate: 4.15 },
      { tenor: '10Y', years: 10, rate: 4.25 },
      { tenor: '20Y', years: 20, rate: 4.50 },
      { tenor: '30Y', years: 30, rate: 4.45 },
    ],
  },
  {
    id: 'USD_SOFR',
    name: 'SOFR OIS',
    color: '#16a34a',
    description: 'Secured Overnight Financing Rate OIS curve',
    points: [
      { tenor: 'ON', years: 1/365, rate: 4.33 },
      { tenor: '1W', years: 1/52, rate: 4.33 },
      { tenor: '1M', years: 1/12, rate: 4.32 },
      { tenor: '3M', years: 0.25, rate: 4.28 },
      { tenor: '6M', years: 0.5, rate: 4.20 },
      { tenor: '1Y', years: 1, rate: 4.05 },
      { tenor: '2Y', years: 2, rate: 3.85 },
      { tenor: '3Y', years: 3, rate: 3.75 },
      { tenor: '5Y', years: 5, rate: 3.70 },
      { tenor: '7Y', years: 7, rate: 3.75 },
      { tenor: '10Y', years: 10, rate: 3.85 },
      { tenor: '30Y', years: 30, rate: 4.00 },
    ],
  },
  {
    id: 'USD_SWAP',
    name: 'USD Swap',
    color: '#dc2626',
    description: 'USD Interest Rate Swap curve (vs SOFR)',
    points: [
      { tenor: '1Y', years: 1, rate: 4.08 },
      { tenor: '2Y', years: 2, rate: 3.90 },
      { tenor: '3Y', years: 3, rate: 3.82 },
      { tenor: '5Y', years: 5, rate: 3.78 },
      { tenor: '7Y', years: 7, rate: 3.83 },
      { tenor: '10Y', years: 10, rate: 3.92 },
      { tenor: '15Y', years: 15, rate: 4.05 },
      { tenor: '20Y', years: 20, rate: 4.12 },
      { tenor: '30Y', years: 30, rate: 4.08 },
    ],
  },
  {
    id: 'USD_CORP_IG',
    name: 'Corporate IG',
    color: '#9333ea',
    description: 'Investment Grade Corporate spread over Treasuries',
    points: [
      { tenor: '1Y', years: 1, rate: 4.70 },
      { tenor: '2Y', years: 2, rate: 4.75 },
      { tenor: '3Y', years: 3, rate: 4.82 },
      { tenor: '5Y', years: 5, rate: 4.95 },
      { tenor: '7Y', years: 7, rate: 5.10 },
      { tenor: '10Y', years: 10, rate: 5.25 },
      { tenor: '20Y', years: 20, rate: 5.55 },
      { tenor: '30Y', years: 30, rate: 5.50 },
    ],
  },
];

type ViewMode = 'zero' | 'forward' | 'discount';
type BumpType = 'parallel' | 'twist' | 'butterfly';

// Calculate forward rates from zero rates
function calculateForwardRates(points: CurvePoint[]): CurvePoint[] {
  const result: CurvePoint[] = [];

  for (let i = 0; i < points.length; i++) {
    if (i === 0) {
      result.push({ ...points[i] });
    } else {
      const t1 = points[i - 1].years;
      const t2 = points[i].years;
      const r1 = points[i - 1].rate / 100;
      const r2 = points[i].rate / 100;

      // Forward rate = (r2 * t2 - r1 * t1) / (t2 - t1)
      const forward = ((r2 * t2 - r1 * t1) / (t2 - t1)) * 100;

      result.push({
        tenor: points[i].tenor,
        years: points[i].years,
        rate: forward,
      });
    }
  }

  return result;
}

// Calculate discount factors from zero rates
function calculateDiscountFactors(points: CurvePoint[]): CurvePoint[] {
  return points.map(p => ({
    tenor: p.tenor,
    years: p.years,
    rate: Math.exp(-p.rate / 100 * p.years) * 100, // as percentage for display
  }));
}

// Apply bump to curve
function applyCurveBump(
  points: CurvePoint[],
  bumpType: BumpType,
  bumpSize: number
): CurvePoint[] {
  return points.map((p, idx, arr) => {
    let bump = 0;
    const midIdx = Math.floor(arr.length / 2);

    switch (bumpType) {
      case 'parallel':
        bump = bumpSize;
        break;
      case 'twist':
        // Linear from -bump at short end to +bump at long end
        bump = bumpSize * (idx / (arr.length - 1) * 2 - 1);
        break;
      case 'butterfly':
        // Negative at belly, positive at wings
        const distance = Math.abs(idx - midIdx) / midIdx;
        bump = bumpSize * (distance * 2 - 1);
        break;
    }

    return {
      ...p,
      rate: p.rate + bump,
    };
  });
}

// Custom tooltip
interface TooltipProps {
  active?: boolean;
  payload?: Array<{ name: string; value: number; color: string; dataKey: string }>;
  label?: string;
  viewMode: ViewMode;
}

function CustomTooltip({ active, payload, label, viewMode }: TooltipProps) {
  if (!active || !payload || payload.length === 0) return null;

  const getUnit = () => {
    switch (viewMode) {
      case 'discount':
        return '';
      default:
        return '%';
    }
  };

  return (
    <div className="bg-white border border-slate-200 rounded-lg shadow-lg p-3">
      <p className="font-semibold text-slate-700 mb-2">{label}</p>
      {payload.map((entry, idx) => (
        <div key={idx} className="flex items-center gap-2 text-sm">
          <div
            className="w-3 h-3 rounded-full"
            style={{ backgroundColor: entry.color }}
          />
          <span className="text-slate-600">{entry.name}:</span>
          <span className="font-mono font-medium" style={{ color: entry.color }}>
            {formatNumber(entry.value, 3)}{getUnit()}
          </span>
        </div>
      ))}
    </div>
  );
}

// Curve colors for display
const CURVE_COLORS: Record<string, string> = {
  'USD_GOVT': '#2563eb',
  'USD_SOFR': '#16a34a',
  'USD_SWAP': '#dc2626',
  'USD_CORP_IG': '#9333ea',
  'USD_CORP_HY': '#f59e0b',
};

// Curve descriptions
const CURVE_DESCRIPTIONS: Record<string, string> = {
  'USD_GOVT': 'US Government benchmark yields',
  'USD_SOFR': 'Secured Overnight Financing Rate OIS curve',
  'USD_SWAP': 'USD Interest Rate Swap curve',
  'USD_CORP_IG': 'Investment Grade Corporate spread over Treasuries',
  'USD_CORP_HY': 'High Yield Corporate spread over Treasuries',
};

// Transform data provider curve to internal format
function transformProviderCurve(curve: DataProviderYieldCurve): CurveData {
  return {
    id: curve.id,
    name: curve.name,
    color: CURVE_COLORS[curve.id] || '#64748b',
    description: CURVE_DESCRIPTIONS[curve.id] || curve.name,
    points: curve.points.map(p => ({
      tenor: p.tenor,
      years: p.years,
      rate: p.rate * 100, // Convert decimal to percentage
    })),
  };
}

export default function YieldCurveVisualizer() {
  // Fetch curves from data provider
  const {
    data: providerData,
    isLoading,
    isError,
    refetch,
    isFetching,
  } = useQuery({
    queryKey: ['market-curves'],
    queryFn: fetchMarketDataFromProvider,
    staleTime: 60000, // 1 minute
    retry: 1,
    refetchOnWindowFocus: false,
  });

  // Transform provider data or use fallback
  const allCurves = useMemo(() => {
    if (providerData?.curves && providerData.curves.length > 0) {
      return providerData.curves.map(transformProviderCurve);
    }
    // Fallback to demo curves
    return DEMO_CURVES;
  }, [providerData]);

  const isLive = providerData?.curves && providerData.curves.length > 0;
  const dataSource = providerData?.source || 'Demo Data';

  // State
  const [selectedCurves, setSelectedCurves] = useState<string[]>(['USD_GOVT', 'USD_SOFR']);
  const [viewMode, setViewMode] = useState<ViewMode>('zero');
  const [bumpEnabled, setBumpEnabled] = useState(false);
  const [bumpType, setBumpType] = useState<BumpType>('parallel');
  const [bumpSize, setBumpSize] = useState(0);
  const [showSpread, setShowSpread] = useState(false);
  const [spreadBase, setSpreadBase] = useState('USD_GOVT');

  // Toggle curve selection
  const toggleCurve = useCallback((curveId: string) => {
    setSelectedCurves(prev =>
      prev.includes(curveId)
        ? prev.filter(id => id !== curveId)
        : [...prev, curveId]
    );
  }, []);

  // Process curves based on view mode and bumps
  const processedCurves = useMemo(() => {
    return allCurves
      .filter(curve => selectedCurves.includes(curve.id))
      .map(curve => {
        let points = curve.points;

        // Apply bump if enabled
        if (bumpEnabled && bumpSize !== 0) {
          points = applyCurveBump(points, bumpType, bumpSize);
        }

        // Transform based on view mode
        switch (viewMode) {
          case 'forward':
            points = calculateForwardRates(points);
            break;
          case 'discount':
            points = calculateDiscountFactors(points);
            break;
        }

        return { ...curve, points };
      });
  }, [allCurves, selectedCurves, viewMode, bumpEnabled, bumpType, bumpSize]);

  // Chart data row type
  interface ChartDataRow {
    tenor: string;
    years: number;
    [curveId: string]: string | number; // curve rates indexed by ID
  }

  // Prepare chart data - merge all curves by tenor
  const chartData = useMemo((): ChartDataRow[] => {
    const tenorMap = new Map<string, ChartDataRow>();

    processedCurves.forEach(curve => {
      curve.points.forEach(point => {
        const existing = tenorMap.get(point.tenor) || { tenor: point.tenor, years: point.years };
        existing[curve.id] = point.rate;
        tenorMap.set(point.tenor, existing);
      });
    });

    // Sort by years
    return Array.from(tenorMap.values()).sort((a, b) => a.years - b.years);
  }, [processedCurves]);

  // Calculate spreads between curves
  const spreadData = useMemo(() => {
    if (!showSpread) return null;

    const baseCurve = processedCurves.find(c => c.id === spreadBase);
    if (!baseCurve) return null;

    const baseRates = new Map(baseCurve.points.map(p => [p.tenor, p.rate]));

    return processedCurves
      .filter(c => c.id !== spreadBase)
      .map(curve => ({
        ...curve,
        points: curve.points
          .filter(p => baseRates.has(p.tenor))
          .map(p => ({
            ...p,
            rate: (p.rate - (baseRates.get(p.tenor) || 0)) * 100, // bps
          })),
      }));
  }, [processedCurves, showSpread, spreadBase]);

  // Spread chart data
  const spreadChartData = useMemo((): ChartDataRow[] => {
    if (!spreadData) return [];

    const tenorMap = new Map<string, ChartDataRow>();

    spreadData.forEach(curve => {
      curve.points.forEach(point => {
        const existing = tenorMap.get(point.tenor) || { tenor: point.tenor, years: point.years };
        existing[curve.id] = point.rate;
        tenorMap.set(point.tenor, existing);
      });
    });

    return Array.from(tenorMap.values()).sort((a, b) => a.years - b.years);
  }, [spreadData]);

  // Key metrics
  const keyMetrics = useMemo(() => {
    const metrics: Array<{ label: string; value: string; curve: string; color: string }> = [];

    processedCurves.forEach(curve => {
      const points = curve.points;
      const y2 = points.find(p => p.years === 2)?.rate;
      const y10 = points.find(p => p.years === 10)?.rate;

      if (y2 !== undefined && y10 !== undefined) {
        const spread = (y10 - y2) * 100;
        metrics.push({
          label: '2Y-10Y Spread',
          value: `${spread >= 0 ? '+' : ''}${formatNumber(spread, 0)} bps`,
          curve: curve.name,
          color: curve.color,
        });
      }
    });

    return metrics;
  }, [processedCurves]);

  const getYAxisLabel = () => {
    switch (viewMode) {
      case 'zero':
        return 'Zero Rate (%)';
      case 'forward':
        return 'Forward Rate (%)';
      case 'discount':
        return 'Discount Factor (x100)';
    }
  };

  return (
    <div className="space-y-6">
      {/* Data Source Info */}
      <div className={cn(
        "card bg-gradient-to-r",
        isLive ? "from-green-50 to-emerald-50" : "from-blue-50 to-indigo-50"
      )}>
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div>
            <h3 className="text-lg font-semibold text-slate-800">Yield Curve Visualizer</h3>
            <p className="text-sm text-slate-600">
              {isLive ? (
                <>
                  Live curves from <span className="font-medium text-green-700">{dataSource}</span>
                  <span className="mx-2">•</span>
                  <span className="text-slate-500">
                    Updated {providerData?.last_updated ? new Date(providerData.last_updated).toLocaleTimeString() : 'just now'}
                  </span>
                </>
              ) : (
                <>
                  Demo curves for Treasury, SOFR OIS, Swap, and Corporate IG
                  <span className="mx-2">•</span>
                  <span className="text-slate-500">
                    Connect data provider for live rates
                  </span>
                </>
              )}
            </p>
          </div>
          <div className="flex items-center gap-3">
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
                  <span>Loading...</span>
                </>
              ) : isLive ? (
                <>
                  <Wifi className="w-4 h-4" />
                  <span>Live</span>
                </>
              ) : (
                <>
                  <WifiOff className="w-4 h-4" />
                  <span>Demo</span>
                </>
              )}
            </div>
            <button
              onClick={() => refetch()}
              disabled={isLoading || isFetching}
              className={cn(
                "p-2 rounded-lg border transition-colors",
                isLoading || isFetching
                  ? "border-slate-200 text-slate-400 cursor-not-allowed"
                  : "border-slate-300 text-slate-600 hover:bg-white hover:border-slate-400"
              )}
              title="Refresh curves"
            >
              <RefreshCw className={cn("w-4 h-4", (isLoading || isFetching) && "animate-spin")} />
            </button>
          </div>
        </div>
      </div>

      {/* Controls */}
      <div className="card">
        <div className="flex flex-wrap items-start gap-6">
          {/* Curve Selection */}
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-2">
              Curves
            </label>
            <div className="flex flex-wrap gap-2">
              {allCurves.map(curve => (
                <button
                  key={curve.id}
                  onClick={() => toggleCurve(curve.id)}
                  className={`px-3 py-1.5 rounded-lg text-sm font-medium transition-all ${
                    selectedCurves.includes(curve.id)
                      ? 'text-white'
                      : 'bg-slate-100 text-slate-600 hover:bg-slate-200'
                  }`}
                  style={
                    selectedCurves.includes(curve.id)
                      ? { backgroundColor: curve.color }
                      : undefined
                  }
                >
                  {curve.name}
                </button>
              ))}
            </div>
          </div>

          {/* View Mode */}
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-2">
              View
            </label>
            <div className="flex gap-1 bg-slate-100 rounded-lg p-1">
              {[
                { id: 'zero', label: 'Zero Rates' },
                { id: 'forward', label: 'Forward Rates' },
                { id: 'discount', label: 'Discount Factors' },
              ].map(mode => (
                <button
                  key={mode.id}
                  onClick={() => setViewMode(mode.id as ViewMode)}
                  className={`px-3 py-1.5 rounded-md text-sm font-medium transition-all ${
                    viewMode === mode.id
                      ? 'bg-white text-slate-900 shadow-sm'
                      : 'text-slate-600 hover:text-slate-900'
                  }`}
                >
                  {mode.label}
                </button>
              ))}
            </div>
          </div>

          {/* Spread Toggle */}
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-2">
              Spread Analysis
            </label>
            <div className="flex items-center gap-3">
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={showSpread}
                  onChange={e => setShowSpread(e.target.checked)}
                  className="w-4 h-4 rounded border-slate-300 text-primary-600 focus:ring-primary-500"
                />
                <span className="text-sm text-slate-600">Show vs</span>
              </label>
              <select
                value={spreadBase}
                onChange={e => setSpreadBase(e.target.value)}
                disabled={!showSpread}
                className="px-2 py-1 border border-slate-300 rounded text-sm disabled:opacity-50"
              >
                {allCurves.map(curve => (
                  <option key={curve.id} value={curve.id}>
                    {curve.name}
                  </option>
                ))}
              </select>
            </div>
          </div>
        </div>
      </div>

      {/* Curve Bump Tool */}
      <div className="card">
        <div className="flex items-center justify-between mb-4">
          <h3 className="text-lg font-semibold text-slate-800">
            Curve Scenario Analysis
          </h3>
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={bumpEnabled}
              onChange={e => setBumpEnabled(e.target.checked)}
              className="w-4 h-4 rounded border-slate-300 text-primary-600 focus:ring-primary-500"
            />
            <span className="text-sm font-medium text-slate-700">Enable Bumps</span>
          </label>
        </div>

        <div className={`flex flex-wrap gap-6 ${!bumpEnabled ? 'opacity-50 pointer-events-none' : ''}`}>
          <div>
            <label className="block text-sm font-medium text-slate-700 mb-2">
              Bump Type
            </label>
            <div className="flex gap-1 bg-slate-100 rounded-lg p-1">
              {[
                { id: 'parallel', label: 'Parallel' },
                { id: 'twist', label: 'Twist' },
                { id: 'butterfly', label: 'Butterfly' },
              ].map(type => (
                <button
                  key={type.id}
                  onClick={() => setBumpType(type.id as BumpType)}
                  className={`px-3 py-1.5 rounded-md text-sm font-medium transition-all ${
                    bumpType === type.id
                      ? 'bg-white text-slate-900 shadow-sm'
                      : 'text-slate-600 hover:text-slate-900'
                  }`}
                >
                  {type.label}
                </button>
              ))}
            </div>
          </div>

          <div className="flex-1 min-w-64">
            <label className="block text-sm font-medium text-slate-700 mb-2">
              Bump Size: <span className={bumpSize >= 0 ? 'text-loss' : 'text-gain'}>
                {bumpSize >= 0 ? '+' : ''}{bumpSize} bps
              </span>
            </label>
            <input
              type="range"
              min="-100"
              max="100"
              step="5"
              value={bumpSize}
              onChange={e => setBumpSize(Number(e.target.value))}
              className="w-full h-2 bg-slate-200 rounded-lg appearance-none cursor-pointer"
            />
            <div className="flex justify-between text-xs text-slate-500 mt-1">
              <span>-100 bps</span>
              <span>0</span>
              <span>+100 bps</span>
            </div>
          </div>

          <button
            onClick={() => setBumpSize(0)}
            className="btn btn-secondary self-end"
          >
            Reset
          </button>
        </div>
      </div>

      {/* Main Chart */}
      <div className="card">
        <h3 className="card-header">{getYAxisLabel()}</h3>
        <div className="h-96">
          <ResponsiveContainer width="100%" height="100%">
            <ComposedChart data={chartData} margin={{ top: 10, right: 30, left: 10, bottom: 10 }}>
              <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
              <XAxis
                dataKey="tenor"
                tick={{ fill: '#64748b', fontSize: 12 }}
                tickLine={{ stroke: '#e2e8f0' }}
              />
              <YAxis
                domain={viewMode === 'discount' ? [80, 105] : ['auto', 'auto']}
                tick={{ fill: '#64748b', fontSize: 12 }}
                tickLine={{ stroke: '#e2e8f0' }}
                tickFormatter={(v) => viewMode === 'discount' ? v.toFixed(0) : `${v.toFixed(2)}%`}
              />
              <Tooltip content={<CustomTooltip viewMode={viewMode} />} />
              <Legend />
              {bumpSize !== 0 && (
                <ReferenceLine y={0} stroke="#94a3b8" strokeDasharray="3 3" />
              )}
              {processedCurves.map(curve => (
                <Line
                  key={curve.id}
                  type="monotone"
                  dataKey={curve.id}
                  name={curve.name}
                  stroke={curve.color}
                  strokeWidth={2}
                  dot={{ fill: curve.color, r: 4 }}
                  activeDot={{ r: 6 }}
                  connectNulls
                />
              ))}
            </ComposedChart>
          </ResponsiveContainer>
        </div>
      </div>

      {/* Spread Chart */}
      {showSpread && spreadChartData.length > 0 && (
        <div className="card">
          <h3 className="card-header">
            Spread vs {allCurves.find(c => c.id === spreadBase)?.name} (bps)
          </h3>
          <div className="h-64">
            <ResponsiveContainer width="100%" height="100%">
              <ComposedChart data={spreadChartData} margin={{ top: 10, right: 30, left: 10, bottom: 10 }}>
                <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
                <XAxis
                  dataKey="tenor"
                  tick={{ fill: '#64748b', fontSize: 12 }}
                />
                <YAxis
                  tick={{ fill: '#64748b', fontSize: 12 }}
                  tickFormatter={(v) => `${v >= 0 ? '+' : ''}${v.toFixed(0)}`}
                />
                <Tooltip
                  formatter={(value: number, name: string) => [
                    `${value >= 0 ? '+' : ''}${formatNumber(value, 1)} bps`,
                    allCurves.find(c => c.id === name)?.name || name
                  ]}
                />
                <Legend />
                <ReferenceLine y={0} stroke="#94a3b8" strokeDasharray="3 3" />
                {spreadData?.map(curve => (
                  <Area
                    key={curve.id}
                    type="monotone"
                    dataKey={curve.id}
                    name={curve.name}
                    stroke={curve.color}
                    fill={curve.color}
                    fillOpacity={0.1}
                    strokeWidth={2}
                    connectNulls
                  />
                ))}
              </ComposedChart>
            </ResponsiveContainer>
          </div>
        </div>
      )}

      {/* Key Metrics and Data Table */}
      <div className="grid lg:grid-cols-3 gap-6">
        {/* Key Rate Metrics */}
        <div className="card">
          <h3 className="card-header">Key Metrics</h3>
          <div className="space-y-4">
            {keyMetrics.map((metric, idx) => (
              <div key={idx} className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <div
                    className="w-3 h-3 rounded-full"
                    style={{ backgroundColor: metric.color }}
                  />
                  <span className="text-sm text-slate-600">{metric.curve}</span>
                </div>
                <div>
                  <span className="text-xs text-slate-500 mr-2">{metric.label}</span>
                  <span className={`font-mono font-medium ${
                    metric.value.startsWith('+') ? 'text-gain' :
                    metric.value.startsWith('-') ? 'text-loss' : ''
                  }`}>
                    {metric.value}
                  </span>
                </div>
              </div>
            ))}

            {processedCurves.length > 0 && (
              <div className="pt-4 border-t border-slate-200">
                <div className="text-sm text-slate-500 mb-2">Curve Stats</div>
                {processedCurves.map(curve => {
                  const rates = curve.points.map(p => p.rate);
                  const min = Math.min(...rates);
                  const max = Math.max(...rates);
                  return (
                    <div key={curve.id} className="flex items-center justify-between text-sm mb-1">
                      <span style={{ color: curve.color }}>{curve.name}</span>
                      <span className="font-mono text-slate-600">
                        {formatNumber(min, 2)}% - {formatNumber(max, 2)}%
                      </span>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>

        {/* Data Table */}
        <div className="lg:col-span-2 card">
          <h3 className="card-header">Curve Data</h3>
          <div className="overflow-x-auto max-h-80">
            <table className="w-full text-sm">
              <thead className="sticky top-0 bg-white">
                <tr className="border-b border-slate-200">
                  <th className="text-left py-2 px-3 font-medium text-slate-600">Tenor</th>
                  <th className="text-right py-2 px-3 font-medium text-slate-600">Years</th>
                  {processedCurves.map(curve => (
                    <th
                      key={curve.id}
                      className="text-right py-2 px-3 font-medium"
                      style={{ color: curve.color }}
                    >
                      {curve.name}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {chartData.map((row) => (
                  <tr
                    key={row.tenor}
                    className="border-b border-slate-100 hover:bg-slate-50"
                  >
                    <td className="py-2 px-3 font-medium">{row.tenor}</td>
                    <td className="py-2 px-3 text-right font-mono text-slate-500">
                      {formatNumber(row.years, 2)}
                    </td>
                    {processedCurves.map(curve => (
                      <td
                        key={curve.id}
                        className="py-2 px-3 text-right font-mono"
                        style={{ color: curve.color }}
                      >
                        {row[curve.id] !== undefined
                          ? viewMode === 'discount'
                            ? formatNumber(row[curve.id] as number, 4)
                            : `${formatNumber(row[curve.id] as number, 3)}%`
                          : '-'}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>

      {/* Info Panel */}
      <div className="card bg-slate-50">
        <h3 className="card-header text-slate-700">Understanding Yield Curves</h3>
        <div className="grid md:grid-cols-3 gap-6 text-sm text-slate-600">
          <div>
            <h4 className="font-semibold text-slate-700 mb-2">Zero Rates</h4>
            <p>
              Spot rates for zero-coupon bonds. The rate earned from today until maturity
              with no intermediate cash flows.
            </p>
          </div>
          <div>
            <h4 className="font-semibold text-slate-700 mb-2">Forward Rates</h4>
            <p>
              Implied rates for future periods derived from the zero curve.
              Used for FRN projections and forward-starting instruments.
            </p>
          </div>
          <div>
            <h4 className="font-semibold text-slate-700 mb-2">Discount Factors</h4>
            <p>
              Present value of $1 received at maturity. Calculated as e^(-r*t)
              where r is the zero rate and t is time in years.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
