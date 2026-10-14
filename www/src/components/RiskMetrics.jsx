import React from 'react';

function RiskMetrics({ analysis, faceAmount = 1000000, benchmarkRisk }) {
  const formatValue = (value, decimals = 3) => {
    if (value === null || value === undefined) return '--';
    return Number(value).toFixed(decimals);
  };

  const formatMoney = (value, suffix = '') => {
    if (value === null || value === undefined || isNaN(value)) return '--';
    const num = Number(value);
    if (Math.abs(num) >= 1000000) {
      return (num / 1000000).toFixed(3) + 'M' + suffix;
    } else if (Math.abs(num) >= 1000) {
      return (num / 1000).toFixed(0) + 'K' + suffix;
    }
    return num.toFixed(2) + suffix;
  };

  // Calculate derived risk metrics
  const cleanPrice = analysis?.clean_price ?? 100;
  const dirtyPrice = analysis?.dirty_price ?? cleanPrice;
  const modDuration = analysis?.modified_duration ?? 0;
  const macDuration = analysis?.macaulay_duration ?? 0;
  const convexity = analysis?.convexity ?? 0;
  const dv01 = analysis?.dv01 ?? 0;

  // Risk = Modified Duration * Dirty Price / 100 (DV01 in percentage terms)
  const risk = modDuration * dirtyPrice / 100;

  // Benchmark risk (use provided or estimate from benchmark duration)
  const bmkRisk = benchmarkRisk ?? (analysis?.benchmark_duration ?? modDuration) * 100 / 100;

  // Risk Hedge = Face * (Bond Risk / Benchmark Risk)
  const riskHedge = bmkRisk > 0 ? faceAmount * (risk / bmkRisk) : 0;

  // Proceeds Hedge = (Face * Dirty Price / 100) * (Bond Risk / Benchmark Risk)
  const proceeds = faceAmount * dirtyPrice / 100;
  const proceedsHedge = bmkRisk > 0 ? proceeds * (risk / bmkRisk) : 0;

  return (
    <div className="risk-metrics-panel">
      <div className="panel-header">
        <h3>Risk</h3>
        <div className="risk-header-labels">
          <span className="yield-label">Yield</span>
        </div>
      </div>

      <div className="risk-table">
        <div className="risk-row">
          <span className="risk-label">M.Dur</span>
          <span className="risk-value yield">{formatValue(macDuration, 3)}</span>
        </div>

        <div className="risk-row">
          <span className="risk-label">
            <span className="radio-indicator">&#9679;</span> Dur
          </span>
          <span className="risk-value yield highlight">{formatValue(modDuration, 3)}</span>
        </div>

        <div className="risk-row">
          <span className="risk-label">Risk</span>
          <span className="risk-value yield">{formatValue(risk, 3)}</span>
        </div>

        <div className="risk-row">
          <span className="risk-label">Convexity</span>
          <span className="risk-value yield">{formatValue(convexity, 3)}</span>
        </div>

        <div className="risk-row dv01-row">
          <span className="risk-label">
            <span className="dv-badge">DV</span>
            <select className="dv-select" defaultValue="01">
              <option value="01">01</option>
              <option value="10">10</option>
              <option value="25">25</option>
            </select>
            <span className="dv-unit">on 1MM</span>
          </span>
          <span className="risk-value yield dv01">{formatMoney(dv01 * 10000)}</span>
        </div>

        <div className="risk-row">
          <span className="risk-label">Benchmark Risk</span>
          <span className="risk-value yield">{formatValue(bmkRisk, 3)}</span>
        </div>

        <div className="risk-row">
          <span className="risk-label">Risk Hedge</span>
          <span className="risk-value yield">{formatMoney(riskHedge)}</span>
        </div>

        <div className="risk-row">
          <span className="risk-label">Proceeds Hedge</span>
          <span className="risk-value yield">{formatMoney(proceedsHedge)}</span>
        </div>
      </div>
    </div>
  );
}

export default RiskMetrics;
