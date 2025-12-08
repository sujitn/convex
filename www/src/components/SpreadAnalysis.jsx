import React, { useState, useEffect } from 'react';

function SpreadAnalysis({
  analysis,
  onSpreadChange,
  onGSpreadChange,
  onBenchmarkSpreadChange
}) {
  // Z-Spread state
  const [localZSpread, setLocalZSpread] = useState(analysis?.z_spread || '');
  const [editingZSpread, setEditingZSpread] = useState(false);

  // G-Spread state
  const [localGSpread, setLocalGSpread] = useState(analysis?.g_spread || '');
  const [editingGSpread, setEditingGSpread] = useState(false);

  // Benchmark Spread state
  const [localBenchmarkSpread, setLocalBenchmarkSpread] = useState(analysis?.benchmark_spread || '');
  const [editingBenchmarkSpread, setEditingBenchmarkSpread] = useState(false);

  useEffect(() => {
    if (!editingZSpread && analysis?.z_spread != null) {
      setLocalZSpread(analysis.z_spread);
    }
  }, [analysis?.z_spread, editingZSpread]);

  useEffect(() => {
    if (!editingGSpread && analysis?.g_spread != null) {
      setLocalGSpread(analysis.g_spread);
    }
  }, [analysis?.g_spread, editingGSpread]);

  useEffect(() => {
    if (!editingBenchmarkSpread && analysis?.benchmark_spread != null) {
      setLocalBenchmarkSpread(analysis.benchmark_spread);
    }
  }, [analysis?.benchmark_spread, editingBenchmarkSpread]);

  const formatBps = (value) => {
    if (value === null || value === undefined) return '---';
    return `${Number(value).toFixed(1)}`;
  };

  const getSpreadClass = (value) => {
    if (value === null || value === undefined) return '';
    if (value > 200) return 'spread-wide';
    if (value > 100) return 'spread-medium';
    return 'spread-tight';
  };

  // Z-Spread handlers
  const handleZSpreadBlur = () => {
    setEditingZSpread(false);
    if (onSpreadChange && localZSpread !== analysis?.z_spread) {
      onSpreadChange(parseFloat(localZSpread) || 0);
    }
  };

  const handleZSpreadKeyDown = (e) => {
    if (e.key === 'Enter') {
      setEditingZSpread(false);
      if (onSpreadChange) {
        onSpreadChange(parseFloat(localZSpread) || 0);
      }
    }
  };

  // G-Spread handlers
  const handleGSpreadBlur = () => {
    setEditingGSpread(false);
    if (onGSpreadChange && localGSpread !== analysis?.g_spread) {
      onGSpreadChange(parseFloat(localGSpread) || 0);
    }
  };

  const handleGSpreadKeyDown = (e) => {
    if (e.key === 'Enter') {
      setEditingGSpread(false);
      if (onGSpreadChange) {
        onGSpreadChange(parseFloat(localGSpread) || 0);
      }
    }
  };

  // Benchmark Spread handlers
  const handleBenchmarkSpreadBlur = () => {
    setEditingBenchmarkSpread(false);
    if (onBenchmarkSpreadChange && localBenchmarkSpread !== analysis?.benchmark_spread) {
      onBenchmarkSpreadChange(parseFloat(localBenchmarkSpread) || 0, analysis?.benchmark_tenor);
    }
  };

  const handleBenchmarkSpreadKeyDown = (e) => {
    if (e.key === 'Enter') {
      setEditingBenchmarkSpread(false);
      if (onBenchmarkSpreadChange) {
        onBenchmarkSpreadChange(parseFloat(localBenchmarkSpread) || 0, analysis?.benchmark_tenor);
      }
    }
  };

  return (
    <div className="spread-analysis-panel">
      <div className="panel-header">
        <h3>Spread Analysis</h3>
      </div>

      <div className="spread-grid">
        <div className={`spread-item editable ${getSpreadClass(analysis?.g_spread)}`}>
          <div className="spread-label">G-Spread</div>
          <div className="spread-value">
            <input
              type="number"
              value={editingGSpread ? localGSpread : (analysis?.g_spread != null ? Number(analysis.g_spread).toFixed(1) : '')}
              onChange={(e) => {
                setEditingGSpread(true);
                setLocalGSpread(e.target.value);
              }}
              onBlur={handleGSpreadBlur}
              onKeyDown={handleGSpreadKeyDown}
              onFocus={() => setEditingGSpread(true)}
              step="0.1"
              className="spread-input"
              placeholder="---"
            />
            <span className="unit">bps</span>
          </div>
          <div className="spread-desc">vs Interpolated</div>
        </div>

        <div className={`spread-item editable ${getSpreadClass(analysis?.benchmark_spread)}`}>
          <div className="spread-label">Benchmark</div>
          <div className="spread-value">
            <input
              type="number"
              value={editingBenchmarkSpread ? localBenchmarkSpread : (analysis?.benchmark_spread != null ? Number(analysis.benchmark_spread).toFixed(1) : '')}
              onChange={(e) => {
                setEditingBenchmarkSpread(true);
                setLocalBenchmarkSpread(e.target.value);
              }}
              onBlur={handleBenchmarkSpreadBlur}
              onKeyDown={handleBenchmarkSpreadKeyDown}
              onFocus={() => setEditingBenchmarkSpread(true)}
              step="0.1"
              className="spread-input"
              placeholder="---"
            />
            <span className="unit">bps</span>
          </div>
          <div className="spread-desc">vs {analysis?.benchmark_tenor || '---'}</div>
        </div>

        <div className={`spread-item editable ${getSpreadClass(analysis?.z_spread)}`}>
          <div className="spread-label">Z-Spread</div>
          <div className="spread-value">
            <input
              type="number"
              value={editingZSpread ? localZSpread : (analysis?.z_spread != null ? Number(analysis.z_spread).toFixed(1) : '')}
              onChange={(e) => {
                setEditingZSpread(true);
                setLocalZSpread(e.target.value);
              }}
              onBlur={handleZSpreadBlur}
              onKeyDown={handleZSpreadKeyDown}
              onFocus={() => setEditingZSpread(true)}
              step="0.1"
              className="spread-input"
              placeholder="---"
            />
            <span className="unit">bps</span>
          </div>
          <div className="spread-desc">Zero Vol</div>
        </div>

        <div className={`spread-item ${getSpreadClass(analysis?.asw_spread)}`}>
          <div className="spread-label">ASW Spread</div>
          <div className="spread-value">
            {analysis?.asw_spread !== null
              ? formatBps(analysis?.asw_spread)
              : 'N/A'}
            <span className="unit">bps</span>
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
