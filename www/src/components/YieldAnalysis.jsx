import React, { useState, useEffect } from 'react';

function YieldAnalysis({ analysis, price, onPriceChange, onCalculate, settlementDate }) {
  const [localPrice, setLocalPrice] = useState(price);

  useEffect(() => {
    setLocalPrice(price);
  }, [price]);

  const handlePriceBlur = () => {
    if (localPrice !== price) {
      onPriceChange(localPrice);
      onCalculate();
    }
  };

  const handlePriceKeyDown = (e) => {
    if (e.key === 'Enter') {
      onPriceChange(localPrice);
      onCalculate();
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
                value={localPrice}
                onChange={(e) => setLocalPrice(parseFloat(e.target.value) || 0)}
                onBlur={handlePriceBlur}
                onKeyDown={handlePriceKeyDown}
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
          <div className="metric-row primary-metric">
            <div className="metric-label">Yield to Maturity</div>
            <div className="metric-value ytm">
              {formatPercent(analysis?.ytm)}
            </div>
          </div>

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
      </div>
    </div>
  );
}

export default YieldAnalysis;
