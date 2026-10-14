import React, { useState, useEffect, useRef } from 'react';

function SpreadAnalysis({ analysis, onSpreadChange }) {
  const [localZSpread, setLocalZSpread] = useState(analysis?.z_spread ?? '');
  const [editing, setEditing] = useState(false);
  const timer = useRef(null);
  const cancelled = useRef(false);

  useEffect(() => () => clearTimeout(timer.current), []);

  useEffect(() => {
    if (!editing) setLocalZSpread(analysis?.z_spread ?? '');
  }, [analysis?.z_spread, editing]);

  const submit = (value) => {
    const number = parseFloat(value);
    if (Number.isFinite(number) && onSpreadChange) onSpreadChange(number);
  };

  const formatBps = (value) => value == null ? '---' : Number(value).toFixed(1);
  const spreadClass = (value) => {
    if (value == null) return '';
    if (value > 200) return 'spread-wide';
    if (value > 100) return 'spread-medium';
    return 'spread-tight';
  };

  return (
    <div className="spread-analysis-panel">
      <div className="panel-header"><h3>Spread Analysis</h3></div>
      <div className="spread-grid">
        <div className={`spread-item editable ${spreadClass(analysis?.z_spread)}`}>
          <div className="spread-label">Z-Spread</div>
          <div className="spread-value">
            <input
              type="number"
              value={localZSpread}
              onFocus={() => { cancelled.current = false; setEditing(true); }}
              onChange={(event) => {
                const value = event.target.value;
                setLocalZSpread(value);
                clearTimeout(timer.current);
                timer.current = setTimeout(() => submit(value), 300);
              }}
              onBlur={() => {
                clearTimeout(timer.current);
                if (!cancelled.current) submit(localZSpread);
                setEditing(false);
              }}
              onKeyDown={(event) => {
                if (event.key === 'Escape') {
                  cancelled.current = true;
                  event.target.blur();
                } else if (event.key === 'Enter') {
                  event.target.blur();
                }
              }}
              step="0.1"
              className="spread-input"
              placeholder="---"
            />
            <span className="unit">bps</span>
          </div>
          <div className="spread-desc">Zero Vol</div>
        </div>
        <div className={`spread-item ${spreadClass(analysis?.asw_spread)}`}>
          <div className="spread-label">ASW Spread</div>
          <div className="spread-value">
            {formatBps(analysis?.asw_spread)}<span className="unit">bps</span>
          </div>
          <div className="spread-desc">Asset Swap</div>
        </div>

      </div>
    </div>
  );
}

export default SpreadAnalysis;
