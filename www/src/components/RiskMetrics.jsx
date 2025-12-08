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

  // OAS-adjusted values (placeholder - would need OAS calculator)
  const oasModDuration = analysis?.oas_duration ?? null;
  const oasRisk = oasModDuration ? oasModDuration * dirtyPrice / 100 : null;
  const oasConvexity = analysis?.oas_convexity ?? null;
  const oasBmkRisk = oasRisk ?? null;
  const oasRiskHedge = oasRisk && bmkRisk > 0 ? faceAmount * (oasRisk / bmkRisk) : null;
  const oasProceedsHedge = oasRisk && bmkRisk > 0 ? proceeds * (oasRisk / bmkRisk) : null;

  return (
    <div className="risk-metrics-panel">
      <div className="panel-header">
        <h3>Risk</h3>
        <div className="risk-header-labels">
          <span className="workout-label">Workout</span>
          <span className="oas-label">OAS</span>
        </div>
      </div>

      <div className="risk-table">
        <div className="risk-row">
          <span className="risk-label">M.Dur</span>
          <span className="risk-value workout">{formatValue(macDuration, 3)}</span>
          <span className="risk-value oas">{oasModDuration ? formatValue(oasModDuration, 3) : 'N.A.'}</span>
        </div>

        <div className="risk-row">
          <span className="risk-label">
            <span className="radio-indicator">&#9679;</span> Dur
          </span>
          <span className="risk-value workout highlight">{formatValue(modDuration, 3)}</span>
          <span className="risk-value oas">{oasModDuration ? formatValue(oasModDuration, 3) : 'N.A.'}</span>
        </div>

        <div className="risk-row">
          <span className="risk-label">Risk</span>
          <span className="risk-value workout">{formatValue(risk, 3)}</span>
          <span className="risk-value oas">{oasRisk ? formatValue(oasRisk, 3) : 'N.A.'}</span>
        </div>

        <div className="risk-row">
          <span className="risk-label">Convexity</span>
          <span className="risk-value workout">{formatValue(convexity, 3)}</span>
          <span className="risk-value oas">{oasConvexity ? formatValue(oasConvexity, 3) : 'N.A.'}</span>
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
          <span className="risk-value workout dv01">{formatMoney(dv01 * 10000)}</span>
          <span className="risk-value oas dv01">{formatMoney(dv01 * 10000)}</span>
        </div>

        <div className="risk-row">
          <span className="risk-label">Benchmark Risk</span>
          <span className="risk-value workout">{formatValue(bmkRisk, 3)}</span>
          <span className="risk-value oas">{oasBmkRisk ? formatValue(oasBmkRisk, 3) : 'N.A.'}</span>
        </div>

        <div className="risk-row">
          <span className="risk-label">Risk Hedge</span>
          <span className="risk-value workout">{formatMoney(riskHedge)}</span>
          <span className="risk-value oas">{oasRiskHedge ? formatMoney(oasRiskHedge) : 'N.A.'}</span>
        </div>

        <div className="risk-row">
          <span className="risk-label">Proceeds Hedge</span>
          <span className="risk-value workout">{formatMoney(proceedsHedge)}</span>
          <span className="risk-value oas">{oasProceedsHedge ? formatMoney(oasProceedsHedge) : 'N.A.'}</span>
        </div>
      </div>
    </div>
  );
}

export default RiskMetrics;
