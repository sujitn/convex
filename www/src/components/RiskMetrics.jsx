import React from 'react';

function RiskMetrics({ analysis, faceAmount }) {
  const formatValue = (value, decimals = 3) =>
    value == null || !Number.isFinite(value) ? '--' : value.toFixed(decimals);

  const formatMoney = (value) => {
    if (value == null || !Number.isFinite(value)) return '--';
    if (Math.abs(value) >= 1000000) return (value / 1000000).toFixed(3) + 'M';
    if (Math.abs(value) >= 1000) return (value / 1000).toFixed(3) + 'K';
    return value.toFixed(2);
  };

  const face = Number(faceAmount);
  const dv01 = analysis?.dv01;
  const positionDv01 = Number.isFinite(face) && face > 0
    && dv01 != null && Number.isFinite(dv01) ? dv01 * face / 100 : null;

  return (
    <div className="risk-metrics-panel">
      <div className="panel-header"><h3>Yield Risk</h3></div>
      <div className="risk-table">
        <div className="risk-row">
          <span className="risk-label">Macaulay Duration</span>
          <span className="risk-value yield">{formatValue(analysis?.macaulay_duration)}</span>
        </div>
        <div className="risk-row">
          <span className="risk-label">Modified Duration</span>
          <span className="risk-value yield highlight">{formatValue(analysis?.modified_duration)}</span>
        </div>
        <div className="risk-row">
          <span className="risk-label">Convexity</span>
          <span className="risk-value yield">{formatValue(analysis?.convexity)}</span>
        </div>
        <div className="risk-row dv01-row">
          <span className="risk-label">DV01 loss on invoice face</span>
          <span className="risk-value yield dv01">{formatMoney(positionDv01)}</span>
        </div>
      </div>
    </div>
  );
}

export default RiskMetrics;
