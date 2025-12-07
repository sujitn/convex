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

// Default Treasury curve (approximate rates)
const DEFAULT_TREASURY_CURVE = {
  '1M': 4.30,
  '3M': 4.32,
  '6M': 4.28,
  '1Y': 4.15,
  '2Y': 4.00,
  '3Y': 3.95,
  '5Y': 3.95,
  '7Y': 4.00,
  '10Y': 4.10,
  '20Y': 4.40,
  '30Y': 4.30,
};

// Tenor to years mapping
const TENOR_YEARS = {
  '1M': 1/12,
  '3M': 0.25,
  '6M': 0.5,
  '1Y': 1,
  '2Y': 2,
  '3Y': 3,
  '5Y': 5,
  '7Y': 7,
  '10Y': 10,
  '20Y': 20,
  '30Y': 30,
};

function addYears(date, years) {
  const result = new Date(date);
  result.setFullYear(result.getFullYear() + Math.floor(years));
  result.setMonth(result.getMonth() + Math.round((years % 1) * 12));
  return result.toISOString().split('T')[0];
}

// Generate curve points from Treasury curve object
function generateCurvePoints(treasuryCurve, maturityDate) {
  const baseDate = new Date();
  const maturity = new Date(maturityDate);
  const yearsToMaturity = Math.max(0.5, (maturity - baseDate) / (365.25 * 24 * 60 * 60 * 1000));

  // Filter tenors up to maturity + buffer, keep at least a few points
  const relevantTenors = Object.entries(TENOR_YEARS)
    .filter(([_, years]) => years <= yearsToMaturity + 2)
    .sort((a, b) => a[1] - b[1]);

  if (relevantTenors.length < 2) {
    // Ensure at least 2 points
    return [
      { date: addYears(baseDate, 0.25), rate: treasuryCurve['3M'] || 4.0 },
      { date: addYears(baseDate, 1), rate: treasuryCurve['1Y'] || 4.0 },
    ];
  }

  return relevantTenors.map(([tenor, years]) => ({
    date: addYears(baseDate, years),
    rate: treasuryCurve[tenor] || 4.0,
  }));
}

// Fetch Treasury rates from Treasury.gov XML feed
async function fetchTreasuryRates() {
  try {
    // Treasury.gov XML endpoint - use CORS proxy for browser access
    const year = new Date().getFullYear();
    const treasuryUrl = `https://home.treasury.gov/resource-center/data-chart-center/interest-rates/pages/xml?data=daily_treasury_yield_curve&field_tdr_date_value=${year}`;

    // Try direct fetch first (works if CORS is allowed)
    let response;
    try {
      response = await fetch(treasuryUrl);
    } catch (corsError) {
      // Fallback to CORS proxy
      const proxyUrl = `https://api.allorigins.win/raw?url=${encodeURIComponent(treasuryUrl)}`;
      response = await fetch(proxyUrl);
    }

    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }

    const xmlText = await response.text();

    // Parse XML to extract latest rates
    const parser = new DOMParser();
    const xmlDoc = parser.parseFromString(xmlText, 'text/xml');

    // Get all entries and find the most recent one
    const entries = xmlDoc.querySelectorAll('entry');
    if (entries.length === 0) {
      throw new Error('No data entries found');
    }

    // Get the last entry (most recent)
    const latestEntry = entries[entries.length - 1];
    const content = latestEntry.querySelector('content');
    if (!content) {
      throw new Error('No content in entry');
    }

    // Extract rates from the m:properties element
    const props = content.querySelector('properties');
    if (!props) {
      throw new Error('No properties found');
    }

    const rates = {};
    const mappings = {
      'BC_1MONTH': '1M',
      'BC_3MONTH': '3M',
      'BC_6MONTH': '6M',
      'BC_1YEAR': '1Y',
      'BC_2YEAR': '2Y',
      'BC_3YEAR': '3Y',
      'BC_5YEAR': '5Y',
      'BC_7YEAR': '7Y',
      'BC_10YEAR': '10Y',
      'BC_20YEAR': '20Y',
      'BC_30YEAR': '30Y',
    };

    for (const [xmlKey, tenorKey] of Object.entries(mappings)) {
      const elem = props.querySelector(xmlKey);
      if (elem && elem.textContent) {
        const rate = parseFloat(elem.textContent);
        if (!isNaN(rate)) {
          rates[tenorKey] = rate;
        }
      }
    }

    // Check if we got any rates
    if (Object.keys(rates).length === 0) {
      throw new Error('No rates parsed from XML');
    }

    return rates;
  } catch (error) {
    console.error('Failed to fetch Treasury rates:', error);
    throw error;
  }
}

function App({ wasmModule }) {
  // Bond parameters
  const [bond, setBond] = useState(getDefaultBond);
  const [price, setPrice] = useState(100.0);
  const [treasuryCurve, setTreasuryCurve] = useState(DEFAULT_TREASURY_CURVE);
  const [isFetchingRates, setIsFetchingRates] = useState(false);
  const [ratesLastUpdated, setRatesLastUpdated] = useState(null);

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

      // Generate curve points from Treasury curve
      const curvePoints = generateCurvePoints(treasuryCurve, bond.maturityDate);

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
  }, [wasmModule, bond, price, treasuryCurve]);

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

  // Handle Treasury curve rate change
  const handleCurveChange = useCallback((tenor, rate) => {
    setTreasuryCurve(prev => ({
      ...prev,
      [tenor]: rate,
    }));
  }, []);

  // Fetch live Treasury rates
  const handleFetchRates = useCallback(async () => {
    setIsFetchingRates(true);
    setError(null);
    try {
      const rates = await fetchTreasuryRates();
      setTreasuryCurve(prev => ({ ...prev, ...rates }));
      setRatesLastUpdated(new Date().toLocaleString());
    } catch (err) {
      setError('Failed to fetch Treasury rates. Using default values.');
      console.error(err);
    } finally {
      setIsFetchingRates(false);
    }
  }, []);

  // Reset to defaults
  const handleReset = useCallback(() => {
    setBond(getDefaultBond());
    setPrice(100.0);
    setTreasuryCurve(DEFAULT_TREASURY_CURVE);
    setRatesLastUpdated(null);
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
      treasuryCurve,
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
  }, [bond, price, treasuryCurve, analysis, cashFlows]);

  // Import from JSON
  const handleImport = useCallback((data) => {
    try {
      // Import bond parameters
      if (data.bond) {
        setBond(prev => ({
          ...prev,
          ...data.bond,
        }));
      }

      // Import price
      if (typeof data.price === 'number') {
        setPrice(data.price);
      }

      // Import Treasury curve
      if (data.treasuryCurve) {
        setTreasuryCurve(prev => ({
          ...prev,
          ...data.treasuryCurve,
        }));
      }

      // Clear previous analysis - user should recalculate
      setAnalysis(null);
      setCashFlows([]);
      setError(null);

      // Auto-calculate after import
      setTimeout(() => calculate(), 100);
    } catch (err) {
      setError('Failed to import data: ' + err.message);
    }
  }, [calculate]);

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
        onImport={handleImport}
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
            treasuryCurve={treasuryCurve}
            onCurveChange={handleCurveChange}
            onFetchRates={handleFetchRates}
            isFetchingRates={isFetchingRates}
            ratesLastUpdated={ratesLastUpdated}
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
