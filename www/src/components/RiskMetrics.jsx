import React from 'react';

function RiskMetrics({ analysis }) {
  const formatValue = (value, decimals = 4) => {
    if (value === null || value === undefined) return '---';
    return Number(value).toFixed(decimals);
  };

  const formatMoney = (value) => {
    if (value === null || value === undefined) return '---';
    // DV01 per 100 face value, display in dollars
    return `$${(Number(value) * 100).toFixed(2)}`;
  };

  return (
    <div className="risk-metrics-panel">
      <div className="panel-header">
        <h3>Risk Metrics</h3>
      </div>

      <div className="risk-grid">
        {/* Duration Section */}
        <div className="risk-section">
          <h4>Duration</h4>

          <div className="metric-row">
            <div className="metric-label">Macaulay Duration</div>
            <div className="metric-value">
              {formatValue(analysis?.macaulay_duration, 3)}
              <span className="metric-unit">years</span>
            </div>
          </div>

          <div className="metric-row primary-metric">
            <div className="metric-label">Modified Duration</div>
            <div className="metric-value highlight">
              {formatValue(analysis?.modified_duration, 3)}
            </div>
          </div>
        </div>

        {/* Convexity Section */}
        <div className="risk-section">
          <h4>Convexity</h4>

          <div className="metric-row">
            <div className="metric-label">Convexity</div>
            <div className="metric-value">
              {formatValue(analysis?.convexity, 2)}
            </div>
          </div>
        </div>

        {/* Dollar Risk Section */}
        <div className="risk-section">
          <h4>Dollar Risk</h4>

          <div className="metric-row">
            <div className="metric-label">DV01 (per 100)</div>
            <div className="metric-value">
              {formatValue(analysis?.dv01, 4)}
            </div>
          </div>

          <div className="metric-row">
            <div className="metric-label">DV01 (per 10,000)</div>
            <div className="metric-value">
              {formatMoney(analysis?.dv01)}
            </div>
          </div>
        </div>

        {/* Risk Summary */}
        <div className="risk-summary">
          <div className="summary-row">
            <span className="summary-label">1bp Move Impact:</span>
            <span className="summary-value">
              {analysis?.dv01
                ? `${(Number(analysis.dv01) * -1).toFixed(4)}`
                : '---'}
            </span>
          </div>
          <div className="summary-row">
            <span className="summary-label">Effective Duration:</span>
            <span className="summary-value">
              {formatValue(analysis?.modified_duration, 3)}
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}

export default RiskMetrics;
