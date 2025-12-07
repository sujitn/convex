import React, { useState, useCallback, useEffect } from 'react';
import Header from './components/Header';
import BondInput from './components/BondInput';
import YieldAnalysis from './components/YieldAnalysis';
import SpreadAnalysis from './components/SpreadAnalysis';
import RiskMetrics from './components/RiskMetrics';
import CashFlowTable from './components/CashFlowTable';
import PriceYieldChart from './components/PriceYieldChart';

// Default bond parameters
const getDefaultBond = () => {
  const today = new Date();
  const settlement = new Date(today);
  settlement.setDate(settlement.getDate() + 2); // T+2

  const maturity = new Date(settlement);
  maturity.setFullYear(maturity.getFullYear() + 5);

  const issue = new Date(settlement);
  issue.setFullYear(issue.getFullYear() - 1);

  return {
    name: 'Sample Corporate Bond 5% 2029',
    couponRate: 5.0,
    maturityDate: maturity.toISOString().split('T')[0],
    issueDate: issue.toISOString().split('T')[0],
    settlementDate: settlement.toISOString().split('T')[0],
    dayCount: '30/360',
    frequency: 2,
    faceValue: 100,
    currency: 'USD',
    firstCouponDate: '',
    callSchedule: [],
  };
};

// Default benchmark yield (comparable Treasury yield)
const DEFAULT_BENCHMARK_YIELD = 4.25;

function addYears(date, years) {
  const result = new Date(date);
  result.setFullYear(result.getFullYear() + Math.floor(years));
  result.setMonth(result.getMonth() + Math.round((years % 1) * 12));
  return result.toISOString().split('T')[0];
}

// Generate curve points from benchmark yield (flat curve at benchmark rate)
function generateCurveFromBenchmark(benchmarkYield, maturityDate) {
  const baseDate = new Date();
  const maturity = new Date(maturityDate);
  const yearsToMaturity = Math.max(1, (maturity - baseDate) / (365.25 * 24 * 60 * 60 * 1000));

  // Create a simple curve with points at key tenors up to maturity
  const tenors = [0.25, 0.5, 1, 2, 3, 5, 7, 10, 20, 30].filter(t => t <= yearsToMaturity + 1);
  if (tenors.length === 0) tenors.push(1);

  return tenors.map(years => ({
    date: addYears(baseDate, years),
    rate: benchmarkYield,
  }));
}

function App({ wasmModule }) {
  // Bond parameters
  const [bond, setBond] = useState(getDefaultBond);
  const [price, setPrice] = useState(100.0);
  const [benchmarkYield, setBenchmarkYield] = useState(DEFAULT_BENCHMARK_YIELD);

  // Analysis results
  const [analysis, setAnalysis] = useState(null);
  const [cashFlows, setCashFlows] = useState([]);
  const [priceYieldData, setPriceYieldData] = useState([]);
  const [isCalculating, setIsCalculating] = useState(false);
  const [error, setError] = useState(null);

  // Calculate bond analytics
  const calculate = useCallback(() => {
    if (!wasmModule) {
      setError('WASM module not loaded');
      return;
    }

    setIsCalculating(true);
    setError(null);

    try {
      // Prepare bond parameters
      const bondParams = {
        coupon_rate: bond.couponRate,
        maturity_date: bond.maturityDate,
        issue_date: bond.issueDate,
        settlement_date: bond.settlementDate,
        face_value: bond.faceValue,
        frequency: bond.frequency,
        day_count: bond.dayCount,
        currency: bond.currency,
        first_coupon_date: bond.firstCouponDate || null,
      };

      // Generate curve points from benchmark yield
      const curvePoints = generateCurveFromBenchmark(benchmarkYield, bond.maturityDate);

      // Call WASM analytics
      const result = wasmModule.analyze_bond(bondParams, price, curvePoints);

      if (result.error) {
        setError(result.error);
        setAnalysis(null);
      } else {
        setAnalysis(result);
        setError(null);
      }

      // Get cash flows
      const flows = wasmModule.get_cash_flows(bondParams);
      setCashFlows(flows || []);

      // Generate price/yield curve data
      generatePriceYieldCurve(bondParams, curvePoints);

    } catch (err) {
      console.error('Calculation error:', err);
      setError(err.message || 'Calculation failed');
    } finally {
      setIsCalculating(false);
    }
  }, [wasmModule, bond, price, benchmarkYield]);

  // Generate price/yield curve for chart
  const generatePriceYieldCurve = useCallback((bondParams, curvePoints) => {
    if (!wasmModule) return;

    const data = [];
    for (let p = 80; p <= 120; p += 2) {
      try {
        const result = wasmModule.analyze_bond(bondParams, p, curvePoints);
        if (!result.error && result.ytm !== null) {
          data.push({
            price: p,
            yield: result.ytm,
            duration: result.modified_duration,
          });
        }
      } catch (e) {
        // Skip invalid points
      }
    }
    setPriceYieldData(data);
  }, [wasmModule]);

  // Initial calculation
  useEffect(() => {
    const timer = setTimeout(() => {
      calculate();
    }, 100);
    return () => clearTimeout(timer);
  }, []);

  // Handle bond parameter changes
  const handleBondChange = useCallback((field, value) => {
    setBond(prev => ({ ...prev, [field]: value }));
  }, []);

  // Handle price change with auto-calculate
  const handlePriceChange = useCallback((newPrice) => {
    setPrice(newPrice);
  }, []);

  // Handle benchmark yield change
  const handleBenchmarkChange = useCallback((newYield) => {
    setBenchmarkYield(newYield);
  }, []);

  // Reset to defaults
  const handleReset = useCallback(() => {
    setBond(getDefaultBond());
    setPrice(100.0);
    setBenchmarkYield(DEFAULT_BENCHMARK_YIELD);
    setAnalysis(null);
    setCashFlows([]);
    setError(null);
  }, []);

  // Export results
  const handleExport = useCallback(() => {
    if (!analysis) return;

    const exportData = {
      bond,
      price,
      benchmarkYield,
      analysis,
      cashFlows,
      exportDate: new Date().toISOString(),
    };

    const blob = new Blob([JSON.stringify(exportData, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `bond-analysis-${bond.name.replace(/\s+/g, '-')}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }, [bond, price, benchmarkYield, analysis, cashFlows]);

  return (
    <div className="app">
      <Header
        bondName={bond.name}
        onBondNameChange={(name) => handleBondChange('name', name)}
        settlementDate={bond.settlementDate}
        onSettlementChange={(date) => handleBondChange('settlementDate', date)}
        currency={bond.currency}
        onCurrencyChange={(curr) => handleBondChange('currency', curr)}
        onCalculate={calculate}
        onReset={handleReset}
        onExport={handleExport}
        isCalculating={isCalculating}
      />

      {error && (
        <div className="error-banner">
          <span className="error-icon">!</span>
          <span>{error}</span>
        </div>
      )}

      <div className="main-content">
        <div className="left-panel">
          <BondInput
            bond={bond}
            onChange={handleBondChange}
            benchmarkYield={benchmarkYield}
            onBenchmarkChange={handleBenchmarkChange}
          />
        </div>

        <div className="center-panel">
          <YieldAnalysis
            analysis={analysis}
            price={price}
            onPriceChange={handlePriceChange}
            onCalculate={calculate}
            settlementDate={bond.settlementDate}
          />
          <SpreadAnalysis analysis={analysis} />
          <RiskMetrics analysis={analysis} />
        </div>

        <div className="right-panel">
          <CashFlowTable
            cashFlows={cashFlows}
            analysis={analysis}
          />
        </div>
      </div>

      <div className="bottom-panel">
        <PriceYieldChart
          data={priceYieldData}
          currentPrice={price}
          currentYield={analysis?.ytm}
        />
      </div>
    </div>
  );
}

export default App;
