import React from 'react';

function CallSchedule({
  callSchedule,
  maturityDate,
  volatility,
  onChange,
  onVolatilityChange
}) {
  const handleCallAdd = () => {
    const newCall = {
      date: maturityDate,
      price: 100,
    };
    onChange([...callSchedule, newCall]);
  };

  const handleCallRemove = (index) => {
    const newSchedule = callSchedule.filter((_, i) => i !== index);
    onChange(newSchedule);
  };

  const handleCallChange = (index, field, value) => {
    const newSchedule = [...callSchedule];
    newSchedule[index] = { ...newSchedule[index], [field]: value };
    onChange(newSchedule);
  };

  return (
    <div className="call-schedule-panel">
      <div className="panel-header">
        <h3>Call Schedule</h3>
        <button className="btn btn-small" onClick={handleCallAdd}>
          + Add
        </button>
      </div>

      <div className="call-content">
        {callSchedule.length === 0 ? (
          <div className="no-calls">No call dates - bullet bond</div>
        ) : (
          <>
            <div className="call-list">
              <div className="call-header">
                <span>Date</span>
                <span>Price</span>
                <span></span>
              </div>
              {callSchedule.map((call, index) => (
                <div key={index} className="call-entry-row">
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
                    placeholder="100"
                  />
                  <button
                    className="btn-icon btn-remove"
                    onClick={() => handleCallRemove(index)}
                  >
                    ×
                  </button>
                </div>
              ))}
            </div>
            <div className="volatility-section">
              <div className="volatility-row">
                <label>Vol (OAS)</label>
                <input
                  type="number"
                  value={volatility || 1.0}
                  onChange={(e) => onVolatilityChange && onVolatilityChange(parseFloat(e.target.value) || 1.0)}
                  step="0.1"
                  min="0.1"
                  max="10"
                  placeholder="1.0"
                />
                <span className="unit">%</span>
              </div>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

export default CallSchedule;
