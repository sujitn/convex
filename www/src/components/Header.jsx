import React from 'react';

const currencies = ['USD', 'EUR', 'GBP', 'JPY', 'CHF', 'AUD', 'CAD', 'NZD'];

function Header({
  bondName,
  onBondNameChange,
  settlementDate,
  onSettlementChange,
  currency,
  onCurrencyChange,
  onCalculate,
  onReset,
  onExport,
  isCalculating,
}) {
  return (
    <header className="header">
      <div className="header-left">
        <div className="logo">
          <span className="logo-icon">C</span>
          <span className="logo-text">CONVEX YAS</span>
        </div>
        <div className="header-subtitle">Fixed Income Analytics</div>
      </div>

      <div className="header-center">
        <div className="header-input-group">
          <label>Bond Description</label>
          <input
            type="text"
            value={bondName}
            onChange={(e) => onBondNameChange(e.target.value)}
            placeholder="Enter bond name/description"
            className="bond-name-input"
          />
        </div>

        <div className="header-input-group">
          <label>Settlement</label>
          <input
            type="date"
            value={settlementDate}
            onChange={(e) => onSettlementChange(e.target.value)}
            className="date-input"
          />
        </div>

        <div className="header-input-group">
          <label>Currency</label>
          <select
            value={currency}
            onChange={(e) => onCurrencyChange(e.target.value)}
            className="currency-select"
          >
            {currencies.map(c => (
              <option key={c} value={c}>{c}</option>
            ))}
          </select>
        </div>
      </div>

      <div className="header-right">
        <button
          className="btn btn-primary"
          onClick={onCalculate}
          disabled={isCalculating}
        >
          {isCalculating ? 'CALC...' : 'CALCULATE'}
        </button>
        <button className="btn btn-secondary" onClick={onReset}>
          RESET
        </button>
        <button className="btn btn-secondary" onClick={onExport}>
          EXPORT
        </button>
      </div>
    </header>
  );
}

export default Header;
