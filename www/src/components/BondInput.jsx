import React, { useState } from 'react';

const dayCountOptions = [
  { value: '30/360', label: '30/360' },
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

function BondInput({ bond, onChange, benchmarkYield, onBenchmarkChange }) {
  const [showCallSchedule, setShowCallSchedule] = useState(false);

  const handleCallAdd = () => {
    const newCall = {
      date: bond.maturityDate,
      price: 100,
    };
    onChange('callSchedule', [...bond.callSchedule, newCall]);
  };

  const handleCallRemove = (index) => {
    const newSchedule = bond.callSchedule.filter((_, i) => i !== index);
    onChange('callSchedule', newSchedule);
  };

  const handleCallChange = (index, field, value) => {
    const newSchedule = [...bond.callSchedule];
    newSchedule[index] = { ...newSchedule[index], [field]: value };
    onChange('callSchedule', newSchedule);
  };

  return (
    <div className="bond-input-panel">
      <div className="panel-header">
        <h3>Bond Details</h3>
      </div>

      <div className="input-section">
        <h4>Coupon Information</h4>

        <div className="input-row">
          <label>Coupon Rate (%)</label>
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
        <h4>Dates</h4>

        <div className="input-row">
          <label>Maturity Date</label>
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
      </div>

      <div className="input-section">
        <h4>Face Value</h4>

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

      {/* Call Schedule Section */}
      <div className="input-section collapsible">
        <div
          className="section-header"
          onClick={() => setShowCallSchedule(!showCallSchedule)}
        >
          <h4>Call Schedule</h4>
          <span className="toggle-icon">{showCallSchedule ? '−' : '+'}</span>
        </div>

        {showCallSchedule && (
          <div className="call-schedule">
            {bond.callSchedule.map((call, index) => (
              <div key={index} className="call-entry">
                <input
                  type="date"
                  value={call.date}
                  onChange={(e) => handleCallChange(index, 'date', e.target.value)}
                />
                <input
                  type="number"
                  value={call.price}
                  onChange={(e) => handleCallChange(index, 'price', parseFloat(e.target.value) || 100)}
                  step="0.01"
                  placeholder="Price"
                />
                <button
                  className="btn-icon btn-remove"
                  onClick={() => handleCallRemove(index)}
                >
                  ×
                </button>
              </div>
            ))}
            <button className="btn btn-small" onClick={handleCallAdd}>
              + Add Call Date
            </button>
          </div>
        )}
      </div>

      {/* Benchmark Yield Section */}
      <div className="input-section">
        <h4>Benchmark</h4>

        <div className="input-row">
          <label>Treasury Yield (%)</label>
          <input
            type="number"
            value={benchmarkYield}
            onChange={(e) => onBenchmarkChange(parseFloat(e.target.value) || 0)}
            step="0.01"
            min="0"
            max="20"
            placeholder="Comparable Treasury yield"
          />
        </div>
        <div className="input-hint">
          Enter the yield of a comparable maturity Treasury bond
        </div>
      </div>
    </div>
  );
}

export default BondInput;
