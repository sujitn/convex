import React from 'react';

function SpreadAnalysis({ analysis }) {
  const formatBps = (value) => {
    if (value === null || value === undefined) return '---';
    return `${Number(value).toFixed(1)} bps`;
  };

  const getSpreadClass = (value) => {
    if (value === null || value === undefined) return '';
    if (value > 200) return 'spread-wide';
    if (value > 100) return 'spread-medium';
    return 'spread-tight';
  };

  return (
    <div className="spread-analysis-panel">
      <div className="panel-header">
        <h3>Spread Analysis</h3>
      </div>

      <div className="spread-grid">
        <div className={`spread-item ${getSpreadClass(analysis?.g_spread)}`}>
          <div className="spread-label">G-Spread</div>
          <div className="spread-value">{formatBps(analysis?.g_spread)}</div>
          <div className="spread-desc">vs Treasury</div>
        </div>

        <div className={`spread-item ${getSpreadClass(analysis?.z_spread)}`}>
          <div className="spread-label">Z-Spread</div>
          <div className="spread-value">{formatBps(analysis?.z_spread)}</div>
          <div className="spread-desc">Zero Vol</div>
        </div>

        <div className={`spread-item ${getSpreadClass(analysis?.asw_spread)}`}>
          <div className="spread-label">ASW Spread</div>
          <div className="spread-value">
            {analysis?.asw_spread !== null
              ? formatBps(analysis?.asw_spread)
              : 'N/A'}
          </div>
          <div className="spread-desc">Asset Swap</div>
        </div>

        <div className="spread-item">
          <div className="spread-label">OAS</div>
          <div className="spread-value">---</div>
          <div className="spread-desc">Option Adj</div>
        </div>
      </div>

      <div className="spread-notes">
        <div className="note">
          <span className="note-label">Benchmark:</span>
          <span className="note-value">US Treasury Curve</span>
        </div>
      </div>
    </div>
  );
}

export default SpreadAnalysis;
