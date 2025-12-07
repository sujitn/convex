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
  onImport,
  isCalculating,
}) {
  // Hidden file input for import
  const fileInputRef = React.useRef(null);

  const handleImportClick = () => {
    fileInputRef.current?.click();
  };

  const handleFileChange = (e) => {
    const file = e.target.files?.[0];
    if (file) {
      const reader = new FileReader();
      reader.onload = (event) => {
        try {
          const data = JSON.parse(event.target.result);
          onImport(data);
        } catch (err) {
          alert('Failed to parse JSON file: ' + err.message);
        }
      };
      reader.readAsText(file);
      // Reset input so same file can be imported again
      e.target.value = '';
    }
  };
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
        <button className="btn btn-secondary" onClick={handleImportClick}>
          IMPORT
        </button>
        <button className="btn btn-secondary" onClick={onExport}>
          EXPORT
        </button>
        <input
          type="file"
          ref={fileInputRef}
          onChange={handleFileChange}
          accept=".json"
          style={{ display: 'none' }}
        />
      </div>
    </header>
  );
}

export default Header;
