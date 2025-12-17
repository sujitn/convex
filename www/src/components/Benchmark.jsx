import React, { useState, useEffect, useCallback, useRef, useMemo } from 'react';

// Standard benchmark tenors
const BENCHMARK_TENORS = ['2Y', '3Y', '5Y', '7Y', '10Y', '20Y', '30Y'];

// Typical on-the-run Treasury coupon rates (approximate, based on recent issuance)
// These represent realistic coupon rates for current benchmark securities
const BENCHMARK_COUPONS = {
  '2Y': 4.25,
  '3Y': 4.125,
  '5Y': 4.0,
  '7Y': 4.125,
  '10Y': 4.0,
  '20Y': 4.25,
  '30Y': 4.375,
};

// Calculate bond price from yield using standard bond pricing formula
function calculatePriceFromYield(couponRate, yieldPercent, yearsToMaturity, frequency = 2) {
  if (!yieldPercent || yearsToMaturity <= 0) return 100;

  const c = couponRate / 100 / frequency; // periodic coupon rate
  const y = yieldPercent / 100 / frequency; // periodic yield
  const n = Math.round(yearsToMaturity * frequency); // number of periods

  if (y === 0) {
    // Zero yield case
    return 100 + c * n * 100;
  }

  // PV of coupon payments + PV of principal
  const pvCoupons = c * 100 * (1 - Math.pow(1 + y, -n)) / y;
  const pvPrincipal = 100 / Math.pow(1 + y, n);

  return pvCoupons + pvPrincipal;
}

function Benchmark({
  analysis,
  treasuryCurve,
  maturityDate,
  settlementDate,
  onBenchmarkSpreadChange
}) {
  const [selectedTenor, setSelectedTenor] = useState('10Y');
  const [benchmarkYield, setBenchmarkYield] = useState(null);
  const [localSpread, setLocalSpread] = useState('');
  const [editingSpread, setEditingSpread] = useState(false);
  // Track when user has explicitly set benchmark spread - don't overwrite until other input changes
  const userSetSpreadRef = useRef(false);
  const spreadTimer = useRef(null);

  // Get benchmark coupon for selected tenor
  const benchmarkCoupon = BENCHMARK_COUPONS[selectedTenor] || 4.0;

  // Calculate benchmark price from yield
  const benchmarkPrice = useMemo(() => {
    if (!benchmarkYield) return null;
    const tenorYears = { '2Y': 2, '3Y': 3, '5Y': 5, '7Y': 7, '10Y': 10, '20Y': 20, '30Y': 30 };
    const years = tenorYears[selectedTenor] || 10;
    return calculatePriceFromYield(benchmarkCoupon, benchmarkYield, years, 2);
  }, [benchmarkYield, selectedTenor, benchmarkCoupon]);

  // Calculate years to maturity and auto-select appropriate benchmark
  useEffect(() => {
    if (!maturityDate || !settlementDate) return;

    const settlement = new Date(settlementDate);
    const maturity = new Date(maturityDate);
    const yearsToMaturity = (maturity - settlement) / (365.25 * 24 * 60 * 60 * 1000);

    // Auto-select nearest benchmark tenor
    const tenorYears = {
      '2Y': 2, '3Y': 3, '5Y': 5, '7Y': 7, '10Y': 10, '20Y': 20, '30Y': 30
    };

    let closestTenor = '10Y';
    let minDiff = Infinity;

    for (const [tenor, years] of Object.entries(tenorYears)) {
      const diff = Math.abs(years - yearsToMaturity);
      if (diff < minDiff) {
        minDiff = diff;
        closestTenor = tenor;
      }
    }

    setSelectedTenor(closestTenor);
  }, [maturityDate, settlementDate]);

  // Update benchmark yield when tenor or curve changes
  useEffect(() => {
    if (treasuryCurve && selectedTenor && treasuryCurve[selectedTenor]) {
      setBenchmarkYield(treasuryCurve[selectedTenor]);
    }
  }, [treasuryCurve, selectedTenor]);

  // Calculate spread based on bond YTM vs selected benchmark yield
  const calculatedSpread = useMemo(() => {
    if (analysis?.ytm != null && benchmarkYield != null) {
      // Spread = Bond YTM - Benchmark Yield (in bps)
      return (analysis.ytm - benchmarkYield) * 100;
    }
    return null;
  }, [analysis?.ytm, benchmarkYield]);

  // Update local spread when calculated spread changes (and not editing and not user-set)
  useEffect(() => {
    if (!editingSpread && !userSetSpreadRef.current && calculatedSpread != null) {
      setLocalSpread(calculatedSpread);
    }
  }, [calculatedSpread, editingSpread]);

  // Cleanup timer
  useEffect(() => {
    return () => {
      if (spreadTimer.current) clearTimeout(spreadTimer.current);
    };
  }, []);

  // Debounced spread change
  const debouncedSpreadChange = useCallback((value) => {
    if (spreadTimer.current) clearTimeout(spreadTimer.current);
    spreadTimer.current = setTimeout(() => {
      if (onBenchmarkSpreadChange) {
        const numValue = parseFloat(value);
        if (!isNaN(numValue)) {
          userSetSpreadRef.current = true;
          onBenchmarkSpreadChange(numValue, selectedTenor);
        }
      }
    }, 300);
  }, [onBenchmarkSpreadChange, selectedTenor]);

  // Generate benchmark ticker display
  const getBenchmarkTicker = () => {
    const tenorYears = {
      '2Y': 2, '3Y': 3, '5Y': 5, '7Y': 7, '10Y': 10, '20Y': 20, '30Y': 30
    };
    const years = tenorYears[selectedTenor] || 10;
    const maturity = new Date();
    maturity.setFullYear(maturity.getFullYear() + years);
    const month = maturity.toLocaleString('en-US', { month: '2-digit' });
    const day = maturity.toLocaleString('en-US', { day: '2-digit' });
    const year = maturity.getFullYear().toString().slice(-2);

    return `${selectedTenor} T 0% ${month}/${day}/${year}`;
  };

  const handleTenorChange = (e) => {
    setSelectedTenor(e.target.value);
    // When tenor changes, allow spread to recalculate
    userSetSpreadRef.current = false;
  };

  const handleSpreadChange = (e) => {
    setEditingSpread(true);
    setLocalSpread(e.target.value);
    debouncedSpreadChange(e.target.value);
  };

  const handleSpreadBlur = () => {
    if (spreadTimer.current) clearTimeout(spreadTimer.current);
    setEditingSpread(false);
    if (onBenchmarkSpreadChange) {
      const numValue = parseFloat(localSpread);
      if (!isNaN(numValue)) {
        userSetSpreadRef.current = true;
        onBenchmarkSpreadChange(numValue, selectedTenor);
      }
    }
  };

  const handleSpreadKeyDown = (e) => {
    if (e.key === 'Enter') {
      e.target.blur();
    } else if (e.key === 'Escape') {
      setLocalSpread(calculatedSpread ?? '');
      setEditingSpread(false);
      userSetSpreadRef.current = false;
    }
  };

  const formatNumber = (num, decimals = 3) => {
    if (num === null || num === undefined || isNaN(num)) return '--';
    return num.toFixed(decimals);
  };

  return (
    <div className="benchmark-panel compact">
      <div className="panel-header">
        <h3>Benchmark</h3>
        <select
          className="tenor-select-compact"
          value={selectedTenor}
          onChange={handleTenorChange}
        >
          {BENCHMARK_TENORS.map(tenor => (
            <option key={tenor} value={tenor}>{tenor}</option>
          ))}
        </select>
      </div>

      <div className="benchmark-content-compact">
        <div className="benchmark-grid">
          <div className="benchmark-cell">
            <span className="cell-label">Cpn</span>
            <span className="cell-value">{benchmarkCoupon.toFixed(3)}%</span>
          </div>
          <div className="benchmark-cell">
            <span className="cell-label">Price</span>
            <span className="cell-value">{benchmarkPrice !== null ? formatNumber(benchmarkPrice, 3) : '--'}</span>
          </div>
          <div className="benchmark-cell">
            <span className="cell-label">Yield</span>
            <span className="cell-value yield">{benchmarkYield !== null ? formatNumber(benchmarkYield, 3) : '--'}%</span>
          </div>
          <div className="benchmark-cell spread-cell">
            <span className="cell-label">Sprd</span>
            <div className="spread-input-compact">
              <input
                type="number"
                value={editingSpread || userSetSpreadRef.current
                  ? localSpread
                  : (localSpread !== '' ? Number(localSpread).toFixed(1) : '')}
                onChange={handleSpreadChange}
                onBlur={handleSpreadBlur}
                onKeyDown={handleSpreadKeyDown}
                onFocus={() => setEditingSpread(true)}
                step="0.1"
                placeholder="--"
              />
              <span className="unit">bp</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

export default Benchmark;
