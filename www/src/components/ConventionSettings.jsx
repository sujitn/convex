import React, { useEffect, useState } from 'react';

const marketOptions = [
  { value: 'US', label: 'United States' },
  { value: 'UK', label: 'United Kingdom' },
  { value: 'Germany', label: 'Germany' },
  { value: 'France', label: 'France' },
  { value: 'Italy', label: 'Italy' },
  { value: 'Spain', label: 'Spain' },
  { value: 'Japan', label: 'Japan' },
  { value: 'Switzerland', label: 'Switzerland' },
  { value: 'Australia', label: 'Australia' },
  { value: 'Canada', label: 'Canada' },
  { value: 'Netherlands', label: 'Netherlands' },
  { value: 'Eurozone', label: 'Eurozone' },
];

const instrumentTypeOptions = [
  { value: 'GovernmentBond', label: 'Government Bond' },
  { value: 'TreasuryBill', label: 'Treasury Bill' },
  { value: 'CorporateIG', label: 'Corporate IG' },
  { value: 'CorporateHY', label: 'Corporate HY' },
  { value: 'Municipal', label: 'Municipal' },
  { value: 'Agency', label: 'Agency' },
  { value: 'InflationLinked', label: 'Inflation Linked' },
  { value: 'Supranational', label: 'Supranational' },
  { value: 'CoveredBond', label: 'Covered Bond' },
];

const yieldConventionOptions = [
  { value: 'Street', label: 'Street Convention' },
  { value: 'True', label: 'True Yield' },
  { value: 'ISMA', label: 'ISMA/ICMA' },
  { value: 'Simple', label: 'Simple Yield' },
  { value: 'Municipal', label: 'Municipal (Tax-Equiv)' },
  { value: 'Discount', label: 'Discount Yield' },
  { value: 'BondEquivalent', label: 'Bond Equivalent' },
  { value: 'Annual', label: 'Annual' },
];

const compoundingOptions = [
  { value: 'SemiAnnual', label: 'Semi-Annual' },
  { value: 'Annual', label: 'Annual' },
  { value: 'Quarterly', label: 'Quarterly' },
  { value: 'Monthly', label: 'Monthly' },
  { value: 'Continuous', label: 'Continuous' },
  { value: 'Simple', label: 'Simple' },
];

// Maps for converting WASM display names to internal values
const yieldMap = {
  'Street Convention': 'Street',
  'True Yield': 'True',
  'ISMA/ICMA': 'ISMA',
  'Simple Yield': 'Simple',
  'Municipal (Tax-Equiv)': 'Municipal',
  'Discount Yield': 'Discount',
  'Bond Equivalent': 'BondEquivalent',
  'Annual': 'Annual',
};

const compMap = {
  'Semi-Annual': 'SemiAnnual',
  'Annual': 'Annual',
  'Quarterly': 'Quarterly',
  'Monthly': 'Monthly',
  'Continuous': 'Continuous',
  'Simple': 'Simple',
};

function ConventionSettings({
  conventions,
  onChange,
  onApplyDefaults,
  wasmModule,
  expanded = false,
  onToggle,
  compoundingLinked = true,
  onCompoundingLinkToggle
}) {
  const [defaults, setDefaults] = useState(null);
  const [lastAppliedKey, setLastAppliedKey] = useState(null);

  // Get default conventions when market/instrument changes
  useEffect(() => {
    if (wasmModule && conventions.market && conventions.instrumentType) {
      try {
        const defaultConv = wasmModule.get_default_conventions(
          conventions.market,
          conventions.instrumentType
        );
        if (defaultConv) {
          setDefaults(defaultConv);
        }
      } catch (e) {
        console.warn('Failed to get default conventions:', e);
      }
    }
  }, [wasmModule, conventions.market, conventions.instrumentType]);

  // Auto-apply defaults when market/instrument changes (separate effect to avoid stale closure)
  useEffect(() => {
    if (!defaults) return;

    const currentKey = `${conventions.market}-${conventions.instrumentType}`;
    if (currentKey === lastAppliedKey) return;

    // Apply yield convention
    if (defaults.yield_convention) {
      const yieldValue = yieldMap[defaults.yield_convention] || 'Street';
      onChange('yieldConvention', yieldValue);
    }
    // Apply settlement days
    if (defaults.settlement_days != null) {
      onChange('settlementDays', defaults.settlement_days);
    }
    // Apply ex-dividend days
    if (defaults.ex_dividend_days != null) {
      onChange('exDividendDays', defaults.ex_dividend_days);
    }
    // Notify parent
    if (onApplyDefaults) {
      onApplyDefaults(defaults);
    }

    setLastAppliedKey(currentKey);
  }, [defaults, conventions.market, conventions.instrumentType, onChange, onApplyDefaults, lastAppliedKey]);

  // Apply defaults manually (for button click)
  const applyDefaultsFromData = (defaultData) => {
    if (!defaultData) return;

    if (defaultData.yield_convention) {
      const yieldValue = yieldMap[defaultData.yield_convention] || 'Street';
      onChange('yieldConvention', yieldValue);
    }
    if (defaultData.settlement_days != null) {
      onChange('settlementDays', defaultData.settlement_days);
    }
    if (defaultData.ex_dividend_days != null) {
      onChange('exDividendDays', defaultData.ex_dividend_days);
    }
    if (onApplyDefaults) {
      onApplyDefaults(defaultData);
    }
  };

  const handleMarketChange = (value) => {
    onChange('market', value);
  };

  const handleInstrumentChange = (value) => {
    onChange('instrumentType', value);
  };

  const applyDefaults = () => {
    applyDefaultsFromData(defaults);
  };

  return (
    <div className="convention-settings-panel">
      <div className="panel-header clickable" onClick={onToggle}>
        <h3>Market Conventions</h3>
        <span className={`expand-icon ${expanded ? 'expanded' : ''}`}>
          {expanded ? '\u25BC' : '\u25B6'}
        </span>
      </div>

      {expanded && (
        <div className="convention-content">
          <div className="input-section">
            <div className="input-row">
              <label>Market</label>
              <select
                value={conventions.market || 'US'}
                onChange={(e) => handleMarketChange(e.target.value)}
              >
                {marketOptions.map(opt => (
                  <option key={opt.value} value={opt.value}>{opt.label}</option>
                ))}
              </select>
            </div>

            <div className="input-row">
              <label>Instrument</label>
              <select
                value={conventions.instrumentType || 'GovernmentBond'}
                onChange={(e) => handleInstrumentChange(e.target.value)}
              >
                {instrumentTypeOptions.map(opt => (
                  <option key={opt.value} value={opt.value}>{opt.label}</option>
                ))}
              </select>
            </div>

            {defaults && (
              <button
                className="apply-defaults-btn"
                onClick={applyDefaults}
                title="Apply standard conventions for this market/instrument"
              >
                Apply Defaults
              </button>
            )}
          </div>

          <div className="input-section">
            <div className="input-row">
              <label>Yield Convention</label>
              <select
                value={conventions.yieldConvention || 'Street'}
                onChange={(e) => onChange('yieldConvention', e.target.value)}
              >
                {yieldConventionOptions.map(opt => (
                  <option key={opt.value} value={opt.value}>{opt.label}</option>
                ))}
              </select>
            </div>

            <div className={`input-row ${compoundingLinked ? 'linked-field' : ''}`}>
              <label>
                Compounding
                <button
                  type="button"
                  className={`link-toggle ${compoundingLinked ? 'linked' : 'unlinked'}`}
                  onClick={() => onCompoundingLinkToggle && onCompoundingLinkToggle(!compoundingLinked)}
                  title={compoundingLinked ? 'Click to unlink from bond frequency' : 'Click to link to bond frequency'}
                >
                  {compoundingLinked ? '🔗' : '🔓'}
                </button>
              </label>
              <select
                value={conventions.compounding || 'SemiAnnual'}
                onChange={(e) => onChange('compounding', e.target.value)}
                disabled={compoundingLinked}
                title={compoundingLinked ? 'Linked to bond frequency - click 🔗 to override' : 'Manual override active'}
              >
                {compoundingOptions.map(opt => (
                  <option key={opt.value} value={opt.value}>{opt.label}</option>
                ))}
              </select>
            </div>
          </div>

          <div className="input-section">
            <div className="input-row">
              <label>Settlement (T+N)</label>
              <input
                type="number"
                value={conventions.settlementDays ?? 1}
                onChange={(e) => onChange('settlementDays', parseInt(e.target.value) || 1)}
                min="0"
                max="5"
              />
            </div>

            <div className="input-row">
              <label>Ex-Div Days</label>
              <input
                type="number"
                value={conventions.exDividendDays ?? ''}
                onChange={(e) => onChange('exDividendDays', e.target.value ? parseInt(e.target.value) : null)}
                min="0"
                max="30"
                placeholder="None"
              />
            </div>
          </div>

          {defaults && (
            <div className="defaults-info">
              <small>
                Market defaults: {defaults.yield_convention}, {defaults.compounding}
                {defaults.settlement_days != null && `, T+${defaults.settlement_days}`}
                {defaults.ex_dividend_days != null && `, Ex-div: ${defaults.ex_dividend_days}d`}
              </small>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export default ConventionSettings;
