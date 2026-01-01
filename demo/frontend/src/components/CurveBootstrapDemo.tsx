// =============================================================================
// Curve Bootstrapping Demo
// Interactive demo showcasing Global Fit vs Piecewise bootstrapping methods
// =============================================================================

import { useState, useMemo, useCallback } from 'react';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
  BarChart,
  Bar,
  ReferenceLine,
} from 'recharts';
import {
  Play,
  RotateCcw,
  CheckCircle,
  AlertCircle,
  Plus,
  Trash2,
  ChevronDown,
  ChevronUp,
  GitBranch,
  Zap,
  Activity,
} from 'lucide-react';

import {
  CalibrationInstrument,
  CalibrationResult,
  InterpolationType,
  formatTenor,
  formatFRA,
  getInstrumentTenor,
  PRESET_OPTIONS,
  getPreset,
  generateCurvePoints,
  generateForwardCurvePoints,
} from '../lib/bootstrap';
import {
  fetchSOFRCurveFromProvider,
  fetchTreasuryCurveFromProvider,
  bootstrapCurve as bootstrapCurveApi,
  BootstrapInstrumentInput,
} from '../lib/api';

// View modes for curve visualization
type ViewMode = 'zero' | 'forward' | 'discount';
type BootstrapMethod = 'both' | 'global' | 'piecewise';

export function CurveBootstrapDemo() {
  // State
  const [instruments, setInstruments] = useState<CalibrationInstrument[]>(
    () => getPreset('usd-sofr') || []
  );
  const [selectedPreset, setSelectedPreset] = useState('usd-sofr');
  const [selectedMethod, setSelectedMethod] = useState<BootstrapMethod>('both');
  const [interpolation, setInterpolation] = useState<InterpolationType>('Linear');
  const [viewMode, setViewMode] = useState<ViewMode>('zero');
  const [isCalibrating, setIsCalibrating] = useState(false);
  const [isLoadingLiveRates, setIsLoadingLiveRates] = useState(false);
  const [globalResult, setGlobalResult] = useState<CalibrationResult | null>(null);
  const [piecewiseResult, setPiecewiseResult] = useState<CalibrationResult | null>(null);
  const [expandedSections, setExpandedSections] = useState({
    instruments: true,
    convergence: true,
    residuals: true,
  });

  // Handle preset change
  const handlePresetChange = useCallback((presetId: string) => {
    setSelectedPreset(presetId);
    const preset = getPreset(presetId);
    if (preset) {
      setInstruments(preset);
      setGlobalResult(null);
      setPiecewiseResult(null);
    }
  }, []);

  // Load live rates from data provider
  const handleLoadLiveRates = useCallback(async (curveType: 'sofr' | 'treasury') => {
    setIsLoadingLiveRates(true);
    try {
      const curve = curveType === 'sofr'
        ? await fetchSOFRCurveFromProvider()
        : await fetchTreasuryCurveFromProvider();

      // Convert curve points to calibration instruments
      // Rates from provider are already in decimal (e.g., 0.0435 for 4.35%)
      const newInstruments: CalibrationInstrument[] = curve.points.map(point => {
        // Use OIS for short-end, Swap for longer tenors
        const type = point.years <= 2 ? 'OIS' : 'Swap';
        return {
          type,
          tenor: point.years,
          quote: point.rate, // Already in decimal
          description: `${point.tenor} ${curveType.toUpperCase()}`,
        } as CalibrationInstrument;
      });

      setInstruments(newInstruments);
      setSelectedPreset('');
      setGlobalResult(null);
      setPiecewiseResult(null);
    } catch (error) {
      console.error('Failed to load live rates:', error);
      alert(`Failed to load live rates: ${error instanceof Error ? error.message : 'Unknown error'}`);
    } finally {
      setIsLoadingLiveRates(false);
    }
  }, []);

  // Convert frontend instruments to API format
  const toApiInstruments = (insts: CalibrationInstrument[]): BootstrapInstrumentInput[] => {
    return insts.map(inst => {
      const base = {
        type: inst.type,
        quote: inst.type === 'Bond' ? 0 : inst.quote, // Bond uses price, not quote
        description: inst.description,
      };

      if (inst.type === 'FRA') {
        return { ...base, start_tenor: inst.startTenor, end_tenor: inst.endTenor };
      } else if (inst.type === 'Bond') {
        // Bond pricing not fully supported yet - skip
        return { ...base, tenor: inst.maturity };
      } else {
        return { ...base, tenor: inst.tenor };
      }
    });
  };

  // Convert API response to frontend CalibrationResult format
  const apiToCalibrationResult = (
    response: Awaited<ReturnType<typeof bootstrapCurveApi>>,
    method: 'GlobalFit' | 'Piecewise',
    durationMs: number
  ): CalibrationResult => {
    // Build curve from response points
    const curvePoints = response.points.map(([tenor, rate]) => ({ tenor, rate }));

    return {
      curve: {
        id: response.curve_id,
        referenceDate: response.reference_date,
        points: curvePoints,
        interpolation,
        valueType: 'ZeroRate',
      },
      residuals: response.calibration.residuals_bps.map(bps => bps / 10000),
      residualsBps: response.calibration.residuals_bps,
      iterations: response.calibration.iterations,
      rmsError: response.calibration.rms_error,
      maxError: response.calibration.max_error_bps / 10000,
      converged: response.calibration.converged,
      method,
      durationMs,
    };
  };

  // Run calibration using Convex server API
  const handleCalibrate = useCallback(async () => {
    if (instruments.length === 0) return;

    setIsCalibrating(true);

    try {
      const apiInstruments = toApiInstruments(instruments);
      const referenceDate = new Date().toISOString().split('T')[0];

      if (selectedMethod === 'both' || selectedMethod === 'global') {
        const startTime = performance.now();
        const response = await bootstrapCurveApi({
          curve_id: 'demo-global-fit',
          reference_date: referenceDate,
          instruments: apiInstruments,
          interpolation,
          method: 'GlobalFit',
        });
        const durationMs = performance.now() - startTime;
        setGlobalResult(apiToCalibrationResult(response, 'GlobalFit', durationMs));
      } else {
        setGlobalResult(null);
      }

      if (selectedMethod === 'both' || selectedMethod === 'piecewise') {
        const startTime = performance.now();
        const response = await bootstrapCurveApi({
          curve_id: 'demo-piecewise',
          reference_date: referenceDate,
          instruments: apiInstruments,
          interpolation,
          method: 'Piecewise',
        });
        const durationMs = performance.now() - startTime;
        setPiecewiseResult(apiToCalibrationResult(response, 'Piecewise', durationMs));
      } else {
        setPiecewiseResult(null);
      }
    } catch (error) {
      console.error('Calibration error:', error);
      alert(`Calibration failed: ${error instanceof Error ? error.message : 'Unknown error'}`);
    } finally {
      setIsCalibrating(false);
    }
  }, [instruments, selectedMethod, interpolation]);

  // Delete instrument
  const handleDeleteInstrument = useCallback((index: number) => {
    setInstruments(prev => prev.filter((_, i) => i !== index));
    // Clear stale results
    setGlobalResult(null);
    setPiecewiseResult(null);
  }, []);

  // Update instrument quote/price
  const handleUpdateQuote = useCallback((index: number, value: number) => {
    setInstruments(prev => {
      const updated = [...prev];
      const inst = updated[index];
      if (inst.type === 'Bond') {
        updated[index] = { ...inst, price: value };
      } else {
        updated[index] = { ...inst, quote: value / 100 }; // Convert from % to decimal
      }
      return updated;
    });
    // Clear stale results
    setGlobalResult(null);
    setPiecewiseResult(null);
  }, []);

  // Update instrument tenor
  const handleUpdateTenor = useCallback((index: number, value: number) => {
    setInstruments(prev => {
      const updated = [...prev];
      const inst = updated[index];
      if (inst.type === 'Bond') {
        updated[index] = { ...inst, maturity: value };
      } else if (inst.type === 'FRA') {
        // For FRA, update end tenor and keep duration same
        const duration = inst.endTenor - inst.startTenor;
        updated[index] = { ...inst, startTenor: value, endTenor: value + duration };
      } else {
        updated[index] = { ...inst, tenor: value };
      }
      return updated;
    });
    // Clear stale results
    setGlobalResult(null);
    setPiecewiseResult(null);
  }, []);

  // Add instrument
  const handleAddInstrument = useCallback((type: CalibrationInstrument['type']) => {
    const lastTenor = instruments.length > 0
      ? Math.max(...instruments.map(getInstrumentTenor))
      : 0;

    let newInst: CalibrationInstrument;
    switch (type) {
      case 'Deposit':
        newInst = { type: 'Deposit', tenor: lastTenor + 0.25, quote: 0.04, description: 'New Deposit' };
        break;
      case 'FRA':
        newInst = { type: 'FRA', startTenor: lastTenor, endTenor: lastTenor + 0.25, quote: 0.04, description: 'New FRA' };
        break;
      case 'Swap':
        newInst = { type: 'Swap', tenor: lastTenor + 1, quote: 0.04, description: 'New Swap' };
        break;
      case 'OIS':
        newInst = { type: 'OIS', tenor: lastTenor + 1, quote: 0.04, description: 'New OIS' };
        break;
      case 'Bond':
        newInst = { type: 'Bond', coupon: 0.04, maturity: lastTenor + 2, price: 100, frequency: 2, description: 'New Bond' };
        break;
      default:
        // Fallback - shouldn't happen but ensures newInst is always assigned
        newInst = { type: 'Deposit', tenor: lastTenor + 0.25, quote: 0.04, description: 'New Deposit' };
    }
    setInstruments(prev => [...prev, newInst]);
    // Clear stale results
    setGlobalResult(null);
    setPiecewiseResult(null);
  }, [instruments]);

  // Generate chart data
  const chartData = useMemo(() => {
    if (!globalResult && !piecewiseResult) return [];

    const maxTenor = Math.max(...instruments.map(getInstrumentTenor), 30);
    const data: any[] = [];

    for (let t = 0; t <= maxTenor; t += 0.25) {
      const point: any = { tenor: t };

      if (globalResult) {
        if (viewMode === 'zero') {
          const pts = generateCurvePoints(globalResult.curve, t, t, 1);
          point.global = pts[0]?.rate * 100;
        } else if (viewMode === 'forward') {
          const pts = generateForwardCurvePoints(globalResult.curve, t, t + 0.25, 0.25, 1);
          point.global = pts[0]?.rate * 100;
        }
      }

      if (piecewiseResult) {
        if (viewMode === 'zero') {
          const pts = generateCurvePoints(piecewiseResult.curve, t, t, 1);
          point.piecewise = pts[0]?.rate * 100;
        } else if (viewMode === 'forward') {
          const pts = generateForwardCurvePoints(piecewiseResult.curve, t, t + 0.25, 0.25, 1);
          point.piecewise = pts[0]?.rate * 100;
        }
      }

      data.push(point);
    }

    return data;
  }, [globalResult, piecewiseResult, viewMode, instruments]);

  // Convergence chart data
  const convergenceData = useMemo(() => {
    if (!globalResult?.convergenceHistory) return [];
    return globalResult.convergenceHistory.map((rms, i) => ({
      iteration: i + 1,
      rms: rms * 10000, // Convert to bps
    }));
  }, [globalResult]);

  // Residual chart data
  const residualData = useMemo(() => {
    return instruments.map((inst, i) => {
      const label = inst.type === 'FRA'
        ? `FRA ${formatFRA(inst.startTenor, inst.endTenor)}`
        : `${inst.type} ${formatTenor(getInstrumentTenor(inst))}`;

      return {
        instrument: label,
        global: globalResult ? globalResult.residualsBps[i] : null,
        piecewise: piecewiseResult ? piecewiseResult.residualsBps[i] : null,
      };
    });
  }, [instruments, globalResult, piecewiseResult]);

  const toggleSection = (section: keyof typeof expandedSections) => {
    setExpandedSections(prev => ({ ...prev, [section]: !prev[section] }));
  };

  return (
    <div className="p-6 space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-900 flex items-center gap-2">
            <GitBranch className="h-6 w-6" />
            Curve Bootstrapping
          </h2>
          <p className="text-gray-600">
            Compare Global Fit (Levenberg-Marquardt) vs Piecewise (Brent) bootstrapping methods
          </p>
        </div>
      </div>

      {/* Control Panel */}
      <div className="bg-white rounded-lg shadow-sm border p-4">
        <div className="flex flex-wrap items-center gap-4">
          {/* Preset Selection */}
          <div className="flex items-center gap-2">
            <label className="text-sm font-medium text-gray-700">Preset:</label>
            <select
              value={selectedPreset}
              onChange={(e) => handlePresetChange(e.target.value)}
              className="border rounded-lg px-3 py-2 text-sm"
            >
              <option value="">-- Select --</option>
              {PRESET_OPTIONS.map(opt => (
                <option key={opt.id} value={opt.id}>{opt.name}</option>
              ))}
            </select>
          </div>

          {/* Live Rates Buttons */}
          <div className="flex items-center gap-2">
            <span className="text-sm text-gray-500">or load:</span>
            <button
              onClick={() => handleLoadLiveRates('sofr')}
              disabled={isLoadingLiveRates}
              className="px-3 py-2 text-sm bg-blue-50 text-blue-700 rounded-lg hover:bg-blue-100 disabled:opacity-50"
            >
              {isLoadingLiveRates ? 'Loading...' : 'Live SOFR'}
            </button>
            <button
              onClick={() => handleLoadLiveRates('treasury')}
              disabled={isLoadingLiveRates}
              className="px-3 py-2 text-sm bg-green-50 text-green-700 rounded-lg hover:bg-green-100 disabled:opacity-50"
            >
              {isLoadingLiveRates ? 'Loading...' : 'Live Treasury'}
            </button>
          </div>

          {/* Method Selection */}
          <div className="flex items-center gap-2">
            <label className="text-sm font-medium text-gray-700">Method:</label>
            <select
              value={selectedMethod}
              onChange={(e) => setSelectedMethod(e.target.value as BootstrapMethod)}
              className="border rounded-lg px-3 py-2 text-sm"
            >
              <option value="both">Both (Comparison)</option>
              <option value="global">Global Fit Only</option>
              <option value="piecewise">Piecewise Only</option>
            </select>
          </div>

          {/* Interpolation Selection */}
          <div className="flex items-center gap-2">
            <label className="text-sm font-medium text-gray-700">Interpolation:</label>
            <select
              value={interpolation}
              onChange={(e) => setInterpolation(e.target.value as InterpolationType)}
              className="border rounded-lg px-3 py-2 text-sm"
            >
              <option value="Linear">Linear</option>
              <option value="LogLinear">Log-Linear</option>
              <option value="CubicSpline">Cubic Spline</option>
            </select>
          </div>

          {/* Calibrate Button */}
          <button
            onClick={handleCalibrate}
            disabled={isCalibrating || instruments.length === 0}
            className="flex items-center gap-2 px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50"
          >
            {isCalibrating ? (
              <RotateCcw className="h-4 w-4 animate-spin" />
            ) : (
              <Play className="h-4 w-4" />
            )}
            {isCalibrating ? 'Calibrating...' : 'Calibrate'}
          </button>
        </div>
      </div>

      {/* Results Summary */}
      {(globalResult || piecewiseResult) && (
        <div className="grid grid-cols-2 gap-4">
          {globalResult && (
            <div className="bg-white rounded-lg shadow-sm border p-4">
              <div className="flex items-center justify-between mb-2">
                <h3 className="font-semibold text-blue-700 flex items-center gap-2">
                  <Zap className="h-4 w-4" />
                  Global Fit (Levenberg-Marquardt)
                </h3>
                {globalResult.converged ? (
                  <span className="flex items-center gap-1 text-green-600 text-sm">
                    <CheckCircle className="h-4 w-4" />
                    Converged
                  </span>
                ) : (
                  <span className="flex items-center gap-1 text-amber-600 text-sm">
                    <AlertCircle className="h-4 w-4" />
                    Not Converged
                  </span>
                )}
              </div>
              <div className="grid grid-cols-3 gap-4 text-sm">
                <div>
                  <div className="text-gray-500">RMS Error</div>
                  <div className="font-mono">{(globalResult.rmsError * 10000).toFixed(4)} bp</div>
                </div>
                <div>
                  <div className="text-gray-500">Iterations</div>
                  <div className="font-mono">{globalResult.iterations}</div>
                </div>
                <div>
                  <div className="text-gray-500">Time</div>
                  <div className="font-mono">{globalResult.durationMs.toFixed(1)} ms</div>
                </div>
              </div>
            </div>
          )}
          {piecewiseResult && (
            <div className="bg-white rounded-lg shadow-sm border p-4">
              <div className="flex items-center justify-between mb-2">
                <h3 className="font-semibold text-green-700 flex items-center gap-2">
                  <Activity className="h-4 w-4" />
                  Piecewise (Brent)
                </h3>
                {piecewiseResult.converged ? (
                  <span className="flex items-center gap-1 text-green-600 text-sm">
                    <CheckCircle className="h-4 w-4" />
                    Converged
                  </span>
                ) : (
                  <span className="flex items-center gap-1 text-amber-600 text-sm">
                    <AlertCircle className="h-4 w-4" />
                    Not Converged
                  </span>
                )}
              </div>
              <div className="grid grid-cols-3 gap-4 text-sm">
                <div>
                  <div className="text-gray-500">RMS Error</div>
                  <div className="font-mono">{(piecewiseResult.rmsError * 10000).toFixed(4)} bp</div>
                </div>
                <div>
                  <div className="text-gray-500">Iterations</div>
                  <div className="font-mono">{piecewiseResult.iterations}</div>
                </div>
                <div>
                  <div className="text-gray-500">Time</div>
                  <div className="font-mono">{piecewiseResult.durationMs.toFixed(1)} ms</div>
                </div>
              </div>
            </div>
          )}
        </div>
      )}

      {/* Curve Visualization */}
      {(globalResult || piecewiseResult) && (
        <div className="bg-white rounded-lg shadow-sm border p-6">
          <div className="flex items-center justify-between mb-4">
            <h3 className="text-lg font-semibold">Bootstrapped Curves</h3>
            <div className="flex gap-1 bg-gray-100 rounded-lg p-1">
              {(['zero', 'forward'] as ViewMode[]).map(mode => (
                <button
                  key={mode}
                  onClick={() => setViewMode(mode)}
                  className={`px-3 py-1.5 rounded-md text-sm font-medium transition-colors ${
                    viewMode === mode
                      ? 'bg-white text-gray-900 shadow-sm'
                      : 'text-gray-600 hover:text-gray-900'
                  }`}
                >
                  {mode === 'zero' ? 'Zero Rates' : 'Forward Rates'}
                </button>
              ))}
            </div>
          </div>
          <div className="h-80">
            <ResponsiveContainer width="100%" height="100%">
              <LineChart data={chartData}>
                <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
                <XAxis
                  dataKey="tenor"
                  label={{ value: 'Tenor (Years)', position: 'bottom', offset: -5 }}
                />
                <YAxis
                  tickFormatter={(v) => `${v.toFixed(2)}%`}
                  label={{ value: viewMode === 'zero' ? 'Zero Rate' : 'Forward Rate', angle: -90, position: 'insideLeft' }}
                />
                <Tooltip
                  formatter={(value: number) => [`${value.toFixed(4)}%`, '']}
                  labelFormatter={(label) => `Tenor: ${label}Y`}
                />
                <Legend />
                {globalResult && (
                  <Line
                    type="monotone"
                    dataKey="global"
                    name="Global Fit"
                    stroke="#2563eb"
                    strokeWidth={2}
                    dot={false}
                  />
                )}
                {piecewiseResult && (
                  <Line
                    type="monotone"
                    dataKey="piecewise"
                    name="Piecewise"
                    stroke="#16a34a"
                    strokeWidth={2}
                    dot={false}
                  />
                )}
              </LineChart>
            </ResponsiveContainer>
          </div>
        </div>
      )}

      {/* Convergence Monitor (Global Fit only) */}
      {globalResult && convergenceData.length > 1 && (
        <div className="bg-white rounded-lg shadow-sm border overflow-hidden">
          <button
            className="w-full px-6 py-4 flex items-center justify-between bg-gray-50 hover:bg-gray-100"
            onClick={() => toggleSection('convergence')}
          >
            <h3 className="text-lg font-semibold">Global Fit Convergence</h3>
            {expandedSections.convergence ? (
              <ChevronUp className="h-5 w-5" />
            ) : (
              <ChevronDown className="h-5 w-5" />
            )}
          </button>
          {expandedSections.convergence && (
            <div className="p-6">
              <div className="h-48">
                <ResponsiveContainer width="100%" height="100%">
                  <LineChart data={convergenceData}>
                    <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
                    <XAxis dataKey="iteration" label={{ value: 'Iteration', position: 'bottom', offset: -5 }} />
                    <YAxis
                      scale="log"
                      domain={['auto', 'auto']}
                      tickFormatter={(v) => `${v.toFixed(2)}`}
                      label={{ value: 'RMS Error (bp)', angle: -90, position: 'insideLeft' }}
                    />
                    <Tooltip formatter={(v: number) => [`${v.toFixed(4)} bp`, 'RMS Error']} />
                    <Line type="monotone" dataKey="rms" stroke="#2563eb" strokeWidth={2} dot={false} />
                    <ReferenceLine y={0.01} stroke="#16a34a" strokeDasharray="3 3" />
                  </LineChart>
                </ResponsiveContainer>
              </div>
            </div>
          )}
        </div>
      )}

      {/* Residual Analysis */}
      {(globalResult || piecewiseResult) && residualData.length > 0 && (
        <div className="bg-white rounded-lg shadow-sm border overflow-hidden">
          <button
            className="w-full px-6 py-4 flex items-center justify-between bg-gray-50 hover:bg-gray-100"
            onClick={() => toggleSection('residuals')}
          >
            <h3 className="text-lg font-semibold">Residual Errors by Instrument</h3>
            {expandedSections.residuals ? (
              <ChevronUp className="h-5 w-5" />
            ) : (
              <ChevronDown className="h-5 w-5" />
            )}
          </button>
          {expandedSections.residuals && (
            <div className="p-6">
              <div className="h-64">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart data={residualData} layout="vertical">
                    <CartesianGrid strokeDasharray="3 3" stroke="#e5e7eb" />
                    <XAxis
                      type="number"
                      tickFormatter={(v) => `${v.toFixed(2)}`}
                      domain={[-1, 1]}
                      label={{ value: 'Error (bp)', position: 'bottom', offset: -5 }}
                    />
                    <YAxis type="category" dataKey="instrument" width={100} />
                    <Tooltip formatter={(v: number) => [`${v.toFixed(4)} bp`, '']} />
                    <Legend />
                    <ReferenceLine x={0} stroke="#94a3b8" />
                    {globalResult && (
                      <Bar dataKey="global" name="Global Fit" fill="#2563eb" barSize={8} />
                    )}
                    {piecewiseResult && (
                      <Bar dataKey="piecewise" name="Piecewise" fill="#16a34a" barSize={8} />
                    )}
                  </BarChart>
                </ResponsiveContainer>
              </div>
            </div>
          )}
        </div>
      )}

      {/* Instruments Table */}
      <div className="bg-white rounded-lg shadow-sm border overflow-hidden">
        <button
          className="w-full px-6 py-4 flex items-center justify-between bg-gray-50 hover:bg-gray-100"
          onClick={() => toggleSection('instruments')}
        >
          <h3 className="text-lg font-semibold">Market Instruments ({instruments.length})</h3>
          {expandedSections.instruments ? (
            <ChevronUp className="h-5 w-5" />
          ) : (
            <ChevronDown className="h-5 w-5" />
          )}
        </button>
        {expandedSections.instruments && (
          <div className="p-6">
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead className="bg-gray-50">
                  <tr>
                    <th className="px-4 py-2 text-left">Type</th>
                    <th className="px-4 py-2 text-left">Tenor</th>
                    <th className="px-4 py-2 text-right">Quote (%)</th>
                    <th className="px-4 py-2 text-left">Description</th>
                    {globalResult && <th className="px-4 py-2 text-right">Global Error (bp)</th>}
                    {piecewiseResult && <th className="px-4 py-2 text-right">Piecewise Error (bp)</th>}
                    <th className="px-4 py-2 text-center">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {instruments.map((inst, i) => (
                    <tr key={i} className="border-t">
                      <td className="px-4 py-2">
                        <span className={`px-2 py-1 rounded text-xs font-medium ${
                          inst.type === 'Deposit' ? 'bg-blue-100 text-blue-700' :
                          inst.type === 'FRA' ? 'bg-purple-100 text-purple-700' :
                          inst.type === 'Swap' ? 'bg-green-100 text-green-700' :
                          inst.type === 'OIS' ? 'bg-amber-100 text-amber-700' :
                          'bg-gray-100 text-gray-700'
                        }`}>
                          {inst.type}
                        </span>
                      </td>
                      <td className="px-4 py-2 font-mono">
                        {inst.type === 'FRA' ? (
                          <span>{formatFRA(inst.startTenor, inst.endTenor)}</span>
                        ) : (
                          <input
                            type="number"
                            step="0.25"
                            min="0.01"
                            max="50"
                            value={getInstrumentTenor(inst)}
                            onChange={(e) => handleUpdateTenor(i, parseFloat(e.target.value) || 0)}
                            className="w-20 px-2 py-1 text-right border rounded focus:outline-none focus:ring-1 focus:ring-blue-500"
                          />
                        )}
                      </td>
                      <td className="px-4 py-2 text-right font-mono">
                        <input
                          type="number"
                          step={inst.type === 'Bond' ? '0.01' : '0.0001'}
                          value={inst.type === 'Bond' ? inst.price : (inst.quote * 100)}
                          onChange={(e) => handleUpdateQuote(i, parseFloat(e.target.value) || 0)}
                          className="w-24 px-2 py-1 text-right border rounded focus:outline-none focus:ring-1 focus:ring-blue-500"
                        />
                      </td>
                      <td className="px-4 py-2 text-gray-500">{inst.description}</td>
                      {globalResult && (
                        <td className={`px-4 py-2 text-right font-mono ${
                          globalResult.residualsBps[i] !== undefined && Math.abs(globalResult.residualsBps[i]) < 0.01 ? 'text-green-600' : 'text-amber-600'
                        }`}>
                          {globalResult.residualsBps[i]?.toFixed(4) ?? '-'}
                        </td>
                      )}
                      {piecewiseResult && (
                        <td className={`px-4 py-2 text-right font-mono ${
                          piecewiseResult.residualsBps[i] !== undefined && Math.abs(piecewiseResult.residualsBps[i]) < 0.01 ? 'text-green-600' : 'text-amber-600'
                        }`}>
                          {piecewiseResult.residualsBps[i]?.toFixed(4) ?? '-'}
                        </td>
                      )}
                      <td className="px-4 py-2 text-center">
                        <button
                          onClick={() => handleDeleteInstrument(i)}
                          className="text-red-500 hover:text-red-700"
                        >
                          <Trash2 className="h-4 w-4" />
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className="mt-4 flex gap-2">
              <button
                onClick={() => handleAddInstrument('Deposit')}
                className="flex items-center gap-1 px-3 py-1.5 bg-blue-100 text-blue-700 rounded text-sm hover:bg-blue-200"
              >
                <Plus className="h-3 w-3" /> Deposit
              </button>
              <button
                onClick={() => handleAddInstrument('FRA')}
                className="flex items-center gap-1 px-3 py-1.5 bg-purple-100 text-purple-700 rounded text-sm hover:bg-purple-200"
              >
                <Plus className="h-3 w-3" /> FRA
              </button>
              <button
                onClick={() => handleAddInstrument('Swap')}
                className="flex items-center gap-1 px-3 py-1.5 bg-green-100 text-green-700 rounded text-sm hover:bg-green-200"
              >
                <Plus className="h-3 w-3" /> Swap
              </button>
              <button
                onClick={() => handleAddInstrument('OIS')}
                className="flex items-center gap-1 px-3 py-1.5 bg-amber-100 text-amber-700 rounded text-sm hover:bg-amber-200"
              >
                <Plus className="h-3 w-3" /> OIS
              </button>
              <button
                onClick={() => handleAddInstrument('Bond')}
                className="flex items-center gap-1 px-3 py-1.5 bg-gray-100 text-gray-700 rounded text-sm hover:bg-gray-200"
              >
                <Plus className="h-3 w-3" /> Bond
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export default CurveBootstrapDemo;
