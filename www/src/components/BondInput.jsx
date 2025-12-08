import React from 'react';

const dayCountOptions = [
  { value: '30/360', label: '30/360 US (NASD)' },
  { value: '30E/360', label: '30E/360 EU (ISMA)' },
  { value: 'ACT/360', label: 'ACT/360' },
  { value: 'ACT/365', label: 'ACT/365' },
  { value: 'ACT/ACT', label: 'ACT/ACT ICMA' },
];

const frequencyOptions = [
  { value: 1, label: 'Annual' },
  { value: 2, label: 'Semi-Annual' },
  { value: 4, label: 'Quarterly' },
  { value: 12, label: 'Monthly' },
  { value: 0, label: 'Zero Coupon' },
];

function BondInput({ bond, onChange }) {
  return (
    <div className="bond-input-panel">
      <div className="panel-header">
        <h3>Bond Details</h3>
      </div>

      <div className="input-section">
        <div className="input-row">
          <label>Coupon (%)</label>
          <input
            type="number"
            value={bond.couponRate}
            onChange={(e) => onChange('couponRate', parseFloat(e.target.value) || 0)}
            step="0.125"
            min="0"
            max="20"
          />
        </div>

        <div className="input-row">
          <label>Frequency</label>
          <select
            value={bond.frequency}
            onChange={(e) => onChange('frequency', parseInt(e.target.value))}
          >
            {frequencyOptions.map(opt => (
              <option key={opt.value} value={opt.value}>{opt.label}</option>
            ))}
          </select>
        </div>

        <div className="input-row">
          <label>Day Count</label>
          <select
            value={bond.dayCount}
            onChange={(e) => onChange('dayCount', e.target.value)}
          >
            {dayCountOptions.map(opt => (
              <option key={opt.value} value={opt.value}>{opt.label}</option>
            ))}
          </select>
        </div>
      </div>

      <div className="input-section">
        <div className="input-row">
          <label>Maturity</label>
          <input
            type="date"
            value={bond.maturityDate}
            onChange={(e) => onChange('maturityDate', e.target.value)}
          />
        </div>

        <div className="input-row">
          <label>Issue Date</label>
          <input
            type="date"
            value={bond.issueDate}
            onChange={(e) => onChange('issueDate', e.target.value)}
          />
        </div>

        <div className="input-row">
          <label>First Coupon</label>
          <input
            type="date"
            value={bond.firstCouponDate}
            onChange={(e) => onChange('firstCouponDate', e.target.value)}
            placeholder="Optional"
          />
        </div>

        <div className="input-row">
          <label>Face Value</label>
          <input
            type="number"
            value={bond.faceValue}
            onChange={(e) => onChange('faceValue', parseFloat(e.target.value) || 100)}
            step="1"
            min="1"
          />
        </div>
      </div>
    </div>
  );
}

export default BondInput;
