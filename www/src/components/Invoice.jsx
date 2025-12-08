import React, { useState, useEffect } from 'react';

function Invoice({ analysis, price, faceAmount, onFaceAmountChange }) {
  const [localFace, setLocalFace] = useState(faceAmount || 1000000);

  useEffect(() => {
    if (faceAmount !== undefined) {
      setLocalFace(faceAmount);
    }
  }, [faceAmount]);

  const handleFaceChange = (e) => {
    const value = parseFloat(e.target.value.replace(/,/g, '')) || 0;
    setLocalFace(value);
    if (onFaceAmountChange) {
      onFaceAmountChange(value);
    }
  };

  // Calculate invoice values
  const cleanPrice = analysis?.clean_price ?? price ?? 100;
  const accruedInterest = analysis?.accrued_interest ?? 0;
  const accruedDays = analysis?.days_accrued ?? 0;

  const principal = (localFace * cleanPrice) / 100;
  const accrued = (localFace * accruedInterest) / 100;
  const total = principal + accrued;

  const formatNumber = (num, decimals = 2) => {
    if (num === null || num === undefined || isNaN(num)) return '--';
    return num.toLocaleString('en-US', {
      minimumFractionDigits: decimals,
      maximumFractionDigits: decimals,
    });
  };

  const formatFace = (num) => {
    if (num >= 1000000) {
      return (num / 1000000).toFixed(3) + ' M';
    } else if (num >= 1000) {
      return (num / 1000).toFixed(0) + ' K';
    }
    return num.toLocaleString();
  };

  return (
    <div className="invoice-panel">
      <div className="panel-header">
        <h3>Invoice</h3>
      </div>

      <div className="invoice-content">
        <div className="invoice-row face-input-row">
          <span className="invoice-label">Face</span>
          <div className="invoice-value-input">
            <input
              type="text"
              className="face-input"
              value={localFace.toLocaleString()}
              onChange={handleFaceChange}
            />
            <span className="face-display">{formatFace(localFace)}</span>
          </div>
        </div>

        <div className="invoice-row">
          <span className="invoice-label">Principal</span>
          <span className="invoice-value">{formatNumber(principal)}</span>
        </div>

        <div className="invoice-row">
          <span className="invoice-label">
            Accrued ({accruedDays || Math.round((accruedInterest / (analysis?.ytm || 5) * 100) * 365 / 2) || '--'} Days)
          </span>
          <span className="invoice-value accrued">{formatNumber(accrued)}</span>
        </div>

        <div className="invoice-row total-row">
          <span className="invoice-label">Total (USD)</span>
          <span className="invoice-value total">{formatNumber(total)}</span>
        </div>
      </div>
    </div>
  );
}

export default Invoice;
