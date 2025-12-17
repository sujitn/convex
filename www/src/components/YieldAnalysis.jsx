import React, { useState, useEffect, useRef } from 'react';

function YieldAnalysis({
  analysis,
  price,
  onPriceChange,
  onYieldChange,
  settlementDate
}) {
  const [localPrice, setLocalPrice] = useState(price);
  const [localYtm, setLocalYtm] = useState(analysis?.ytm || '');
  const [editingPrice, setEditingPrice] = useState(false);
  const [editingYield, setEditingYield] = useState(false);
  // Track when user has explicitly set a value - don't overwrite until other input changes
  const userSetPriceRef = useRef(false);
  const userSetYtmRef = useRef(false);
  const lastPriceRef = useRef(price);

  useEffect(() => {
    // Only update localPrice from prop if user is not editing and hasn't just set it
    if (!editingPrice && !userSetPriceRef.current) {
      setLocalPrice(price);
    }
  }, [price, editingPrice]);

  useEffect(() => {
    // If price changed externally (not from YTM solve), allow YTM to update
    if (price !== lastPriceRef.current) {
      // Only clear userSetYtm if price changed significantly (not just from our solve)
      // Small changes might be from the solve, large changes are from user editing price
      const priceDiff = Math.abs(price - lastPriceRef.current);
      if (priceDiff > 0.01) {
        userSetYtmRef.current = false;
      }
      lastPriceRef.current = price;
    }
  }, [price]);

  useEffect(() => {
    // Only update localYtm from analysis if:
    // 1. User is not currently editing
    // 2. User hasn't just set a YTM value
    if (!editingYield && !userSetYtmRef.current && analysis?.ytm != null) {
      setLocalYtm(analysis.ytm);
    }
  }, [analysis?.ytm, editingYield]);

  const handlePriceBlur = () => {
    setEditingPrice(false);
    if (localPrice !== price) {
      // User changed price, so YTM should be recalculated
      userSetPriceRef.current = true;
      userSetYtmRef.current = false;
      onPriceChange(localPrice);
    }
  };

  const handlePriceKeyDown = (e) => {
    if (e.key === 'Enter') {
      setEditingPrice(false);
      // User changed price, so YTM should be recalculated
      userSetPriceRef.current = true;
      userSetYtmRef.current = false;
      onPriceChange(localPrice);
    } else if (e.key === 'Escape') {
      setLocalPrice(price);
      setEditingPrice(false);
      userSetPriceRef.current = false;
    }
  };

  const handleYieldBlur = () => {
    setEditingYield(false);
    const newYtm = parseFloat(localYtm) || 0;
    if (onYieldChange && newYtm !== analysis?.ytm) {
      // User set a YTM, don't overwrite it with calculated value
      // But price should be allowed to update from the yield solve
      userSetYtmRef.current = true;
      userSetPriceRef.current = false;
      onYieldChange(newYtm);
    }
  };

  const handleYieldKeyDown = (e) => {
    if (e.key === 'Enter') {
      setEditingYield(false);
      if (onYieldChange) {
        const newYtm = parseFloat(localYtm) || 0;
        // User set a YTM, don't overwrite it with calculated value
        // But price should be allowed to update from the yield solve
        userSetYtmRef.current = true;
        userSetPriceRef.current = false;
        onYieldChange(newYtm);
      }
    } else if (e.key === 'Escape') {
      setLocalYtm(analysis?.ytm ?? '');
      setEditingYield(false);
      userSetYtmRef.current = false;
    }
  };

  const formatValue = (value, decimals = 4) => {
    if (value === null || value === undefined) return '---';
    return Number(value).toFixed(decimals);
  };

  const formatPercent = (value) => {
    if (value === null || value === undefined) return '---';
    return `${Number(value).toFixed(4)}%`;
  };

  return (
    <div className="yield-analysis-panel">
      <div className="panel-header">
        <h3>Yield Analysis</h3>
      </div>

      <div className="yield-grid">
        {/* Price/Yield Inputs */}
        <div className="yield-section primary">
          <div className="metric-row editable">
            <div className="metric-label">Clean Price</div>
            <div className="metric-value">
              <input
                type="number"
                value={editingPrice || userSetPriceRef.current
                  ? localPrice
                  : (price != null ? Number(price).toFixed(4) : '')}
                onChange={(e) => {
                  setEditingPrice(true);
                  setLocalPrice(parseFloat(e.target.value) || 0);
                }}
                onBlur={handlePriceBlur}
                onKeyDown={handlePriceKeyDown}
                onFocus={() => setEditingPrice(true)}
                step="0.001"
                className="price-input"
              />
            </div>
          </div>

          <div className="metric-row">
            <div className="metric-label">Dirty Price</div>
            <div className="metric-value highlight">
              {formatValue(analysis?.dirty_price, 4)}
            </div>
          </div>

          <div className="metric-row">
            <div className="metric-label">Accrued Interest</div>
            <div className="metric-value">
              {formatValue(analysis?.accrued_interest, 6)}
            </div>
          </div>
        </div>

        {/* Yield Metrics */}
        <div className="yield-section">
          <div className="metric-row primary-metric editable">
            <div className="metric-label">Yield to Maturity</div>
            <div className="metric-value ytm">
              <input
                type="number"
                value={editingYield || userSetYtmRef.current
                  ? localYtm
                  : (analysis?.ytm != null ? Number(analysis.ytm).toFixed(4) : '')}
                onChange={(e) => {
                  setEditingYield(true);
                  setLocalYtm(e.target.value);
                }}
                onBlur={handleYieldBlur}
                onKeyDown={handleYieldKeyDown}
                onFocus={() => setEditingYield(true)}
                step="0.0001"
                className="yield-input"
                placeholder="---"
              />
              <span className="unit">%</span>
            </div>
          </div>

          {/* Callable Bond Yields - only show if bond is callable */}
          {analysis?.is_callable && (
            <>
              <div className="metric-row">
                <div className="metric-label">Yield to Call</div>
                <div className="metric-value ytc">
                  {analysis?.ytc != null ? formatPercent(analysis.ytc) : '---'}
                </div>
              </div>

              <div className="metric-row primary-metric">
                <div className="metric-label">Yield to Worst</div>
                <div className="metric-value ytw">
                  {analysis?.ytw != null ? formatPercent(analysis.ytw) : '---'}
                </div>
              </div>

              <div className="metric-row">
                <div className="metric-label">Workout Date</div>
                <div className="metric-value workout-date">
                  {analysis?.workout_date || '---'}
                </div>
              </div>

              <div className="metric-row">
                <div className="metric-label">Workout Price</div>
                <div className="metric-value">
                  {analysis?.workout_price != null ? formatValue(analysis.workout_price, 3) : '---'}
                </div>
              </div>
            </>
          )}

          <div className="metric-row">
            <div className="metric-label">Current Yield</div>
            <div className="metric-value">
              {formatPercent(analysis?.current_yield)}
            </div>
          </div>

          <div className="metric-row">
            <div className="metric-label">Simple Yield</div>
            <div className="metric-value">
              {formatPercent(analysis?.simple_yield)}
            </div>
          </div>

          <div className="metric-row">
            <div className="metric-label">Money Market Yield</div>
            <div className="metric-value">
              {analysis?.money_market_yield !== null
                ? formatPercent(analysis?.money_market_yield)
                : 'N/A'}
            </div>
          </div>
        </div>

        {/* Settlement & Maturity Info */}
        <div className="yield-section info">
          <div className="metric-row">
            <div className="metric-label">Settlement Date</div>
            <div className="metric-value settlement-date">
              {settlementDate || '---'}
            </div>
          </div>

          <div className="metric-row">
            <div className="metric-label">Days to Maturity</div>
            <div className="metric-value">
              {analysis?.days_to_maturity ?? '---'}
            </div>
          </div>

          <div className="metric-row">
            <div className="metric-label">Years to Maturity</div>
            <div className="metric-value">
              {formatValue(analysis?.years_to_maturity, 3)}
            </div>
          </div>
        </div>

        {/* Convention Info - show if available */}
        {(analysis?.yield_convention || analysis?.market) && (
          <div className="yield-section convention-info">
            {analysis?.market && (
              <div className="metric-row">
                <div className="metric-label">Market</div>
                <div className="metric-value">{analysis.market}</div>
              </div>
            )}
            {analysis?.yield_convention && (
              <div className="metric-row">
                <div className="metric-label">Convention</div>
                <div className="metric-value">{analysis.yield_convention}</div>
              </div>
            )}
            {analysis?.compounding_method && (
              <div className="metric-row">
                <div className="metric-label">Compounding</div>
                <div className="metric-value">{analysis.compounding_method}</div>
              </div>
            )}
            {analysis?.is_ex_dividend && (
              <div className="metric-row ex-dividend-warning">
                <div className="metric-label">Status</div>
                <div className="metric-value warning">Ex-Dividend</div>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

export default YieldAnalysis;
