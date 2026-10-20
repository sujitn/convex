import React from 'react';

function CashFlowTable({ cashFlows }) {
  const flows = Array.isArray(cashFlows) ? cashFlows : [];
  const formatNumber = (value, decimals = 2) =>
    value == null || !Number.isFinite(value) ? '--' : value.toFixed(decimals);
  const formatDate = (value) => new Date(`${value}T00:00:00Z`).toLocaleDateString('en-US', {
    year: 'numeric', month: 'short', day: '2-digit', timeZone: 'UTC',
  });
  const total = (field) => flows.reduce((sum, flow) => sum + flow[field], 0);

  return (
    <div className="cashflow-panel">
      <div className="panel-header">
        <h3>Projected Cash Flows</h3>
        <span className="flow-count">{flows.length} payments</span>
      </div>
      <div className="cashflow-table-container">
        <table className="cashflow-table">
          <thead>
            <tr>
              <th>Date</th>
              <th className="number">Coupon</th>
              <th className="number">Principal</th>
              <th className="number">Fee</th>
              <th className="number">Total</th>
            </tr>
          </thead>
          <tbody>
            {flows.map((flow, index) => (
              <tr key={`${flow.date}-${index}`} className={flow.principal_amount > 0 ? 'principal-row' : ''}>
                <td className="date-cell">{formatDate(flow.date)}</td>
                <td className="number">{formatNumber(flow.coupon_amount)}</td>
                <td className="number principal">{formatNumber(flow.principal_amount)}</td>
                <td className="number">{formatNumber(flow.fee_amount)}</td>
                <td className="number">{formatNumber(flow.amount)}</td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr className="totals-row">
              <td>Total</td>
              <td className="number">{formatNumber(total('coupon_amount'))}</td>
              <td className="number">{formatNumber(total('principal_amount'))}</td>
              <td className="number">{formatNumber(total('fee_amount'))}</td>
              <td className="number">{formatNumber(total('amount'))}</td>
            </tr>
          </tfoot>
        </table>
      </div>
      {flows.length === 0 && <div className="no-data">No projected cash flows available.</div>}
    </div>
  );
}

export default CashFlowTable;
