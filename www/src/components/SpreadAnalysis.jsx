import React, { useState, useEffect, useRef, useCallback } from 'react';

function SpreadAnalysis({
  analysis,
  onSpreadChange,
  onGSpreadChange
}) {
  // Z-Spread state
  const [localZSpread, setLocalZSpread] = useState(analysis?.z_spread || '');
  const [editingZSpread, setEditingZSpread] = useState(false);
  // Track when user has explicitly set Z-spread - don't overwrite until other input changes
  const userSetZSpreadRef = useRef(false);

  // G-Spread state
  const [localGSpread, setLocalGSpread] = useState(analysis?.g_spread || '');
  const [editingGSpread, setEditingGSpread] = useState(false);
  // Track when user has explicitly set G-spread - don't overwrite until other input changes
  const userSetGSpreadRef = useRef(false);

  // Debounce refs
  const zSpreadTimer = useRef(null);
  const gSpreadTimer = useRef(null);

  // Debounced spread change handlers
  const debouncedZSpreadChange = useCallback((value) => {
    if (zSpreadTimer.current) clearTimeout(zSpreadTimer.current);
    zSpreadTimer.current = setTimeout(() => {
      if (onSpreadChange) {
        const numValue = parseFloat(value);
        if (!isNaN(numValue)) {
          userSetZSpreadRef.current = true;
          // When user sets Z-spread, G-spread should recalculate
          userSetGSpreadRef.current = false;
          onSpreadChange(numValue);
        }
      }
    }, 300);
  }, [onSpreadChange]);

  const debouncedGSpreadChange = useCallback((value) => {
    if (gSpreadTimer.current) clearTimeout(gSpreadTimer.current);
    gSpreadTimer.current = setTimeout(() => {
      if (onGSpreadChange) {
        const numValue = parseFloat(value);
        if (!isNaN(numValue)) {
          userSetGSpreadRef.current = true;
          // When user sets G-spread, Z-spread should recalculate
          userSetZSpreadRef.current = false;
          onGSpreadChange(numValue);
        }
      }
    }, 300);
  }, [onGSpreadChange]);

  // Cleanup timers
  useEffect(() => {
    return () => {
      if (zSpreadTimer.current) clearTimeout(zSpreadTimer.current);
      if (gSpreadTimer.current) clearTimeout(gSpreadTimer.current);
    };
  }, []);

  useEffect(() => {
    // Only update localZSpread from analysis if:
    // 1. User is not currently editing
    // 2. User hasn't just set a Z-spread value
    if (!editingZSpread && !userSetZSpreadRef.current && analysis?.z_spread != null) {
      setLocalZSpread(analysis.z_spread);
    }
  }, [analysis?.z_spread, editingZSpread]);

  useEffect(() => {
    // Only update localGSpread from analysis if:
    // 1. User is not currently editing
    // 2. User hasn't just set a G-spread value
    if (!editingGSpread && !userSetGSpreadRef.current && analysis?.g_spread != null) {
      setLocalGSpread(analysis.g_spread);
    }
  }, [analysis?.g_spread, editingGSpread]);

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
    // Clear any pending debounce and trigger immediately
    if (zSpreadTimer.current) clearTimeout(zSpreadTimer.current);
    setEditingZSpread(false);
    if (onSpreadChange) {
      const numValue = parseFloat(localZSpread);
      if (!isNaN(numValue)) {
        userSetZSpreadRef.current = true;
        // When user sets Z-spread, G-spread should recalculate
        userSetGSpreadRef.current = false;
        onSpreadChange(numValue);
      }
    }
  };

  const handleZSpreadKeyDown = (e) => {
    if (e.key === 'Enter') {
      e.target.blur();
    } else if (e.key === 'Escape') {
      setLocalZSpread(analysis?.z_spread || '');
      setEditingZSpread(false);
      userSetZSpreadRef.current = false;
    }
  };

  // G-Spread handlers
  const handleGSpreadBlur = () => {
    if (gSpreadTimer.current) clearTimeout(gSpreadTimer.current);
    setEditingGSpread(false);
    if (onGSpreadChange) {
      const numValue = parseFloat(localGSpread);
      if (!isNaN(numValue)) {
        userSetGSpreadRef.current = true;
        // When user sets G-spread, Z-spread should recalculate
        userSetZSpreadRef.current = false;
        onGSpreadChange(numValue);
      }
    }
  };

  const handleGSpreadKeyDown = (e) => {
    if (e.key === 'Enter') {
      e.target.blur();
    } else if (e.key === 'Escape') {
      setLocalGSpread(analysis?.g_spread || '');
      setEditingGSpread(false);
      userSetGSpreadRef.current = false;
    }
  };

  // Reset userSet flags when analysis changes significantly (e.g., from price/yield change)
  // This is detected by checking if both spreads changed at once
  const prevZSpread = useRef(analysis?.z_spread);
  const prevGSpread = useRef(analysis?.g_spread);

  useEffect(() => {
    const zChanged = analysis?.z_spread !== prevZSpread.current;
    const gChanged = analysis?.g_spread !== prevGSpread.current;

    // If both spreads changed, it's likely from an external source (price/yield change)
    // Clear the userSet flags so spreads can update
    if (zChanged && gChanged) {
      // But only if the user didn't just set one of them
      if (!userSetZSpreadRef.current && !userSetGSpreadRef.current) {
        // Both can update from analysis
      }
    }

    prevZSpread.current = analysis?.z_spread;
    prevGSpread.current = analysis?.g_spread;
  }, [analysis?.z_spread, analysis?.g_spread]);

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
              value={editingGSpread || userSetGSpreadRef.current
                ? localGSpread
                : (analysis?.g_spread != null ? Number(analysis.g_spread).toFixed(1) : '')}
              onChange={(e) => {
                setEditingGSpread(true);
                setLocalGSpread(e.target.value);
                debouncedGSpreadChange(e.target.value);
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

        <div className={`spread-item editable ${getSpreadClass(analysis?.z_spread)}`}>
          <div className="spread-label">Z-Spread</div>
          <div className="spread-value">
            <input
              type="number"
              value={editingZSpread || userSetZSpreadRef.current
                ? localZSpread
                : (analysis?.z_spread != null ? Number(analysis.z_spread).toFixed(1) : '')}
              onChange={(e) => {
                setEditingZSpread(true);
                setLocalZSpread(e.target.value);
                debouncedZSpreadChange(e.target.value);
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

        <div className={`spread-item ${getSpreadClass(analysis?.oas)}`}>
          <div className="spread-label">OAS</div>
          <div className="spread-value">
            {analysis?.oas != null && Math.abs(analysis.oas) < 999
              ? formatBps(analysis.oas)
              : (analysis?.is_callable
                  ? (analysis?.z_spread != null ? `~${formatBps(analysis.z_spread)}` : '---')
                  : 'N/A')}
            <span className="unit">bps</span>
          </div>
          <div className="spread-desc">
            {analysis?.is_callable
              ? (analysis?.oas != null && Math.abs(analysis.oas) < 999 ? 'Option Adj' : '≈ Z-Sprd')
              : 'No Call'}
          </div>
        </div>
      </div>
    </div>
  );
}

export default SpreadAnalysis;
