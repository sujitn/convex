import React, { useMemo } from 'react';

function CashFlowTable({ cashFlows, analysis }) {
  // Calculate present values for each cash flow
  const enrichedFlows = useMemo(() => {
    if (!cashFlows || cashFlows.length === 0) return [];

    const ytm = analysis?.ytm ? analysis.ytm / 100 : 0.05;
    const frequency = 2; // Assume semi-annual

    let cumulativePV = 0;

    return cashFlows.map((cf, index) => {
      // Simple PV calculation: PV = CF / (1 + y/n)^(n*t)
      const period = index + 1;
      const discountFactor = Math.pow(1 + ytm / frequency, period);
      const pv = cf.amount / discountFactor;
      cumulativePV += pv;

      // Separate coupon and principal
      let coupon = 0;
      let principal = 0;

      if (cf.cf_type === 'coupon') {
        coupon = cf.amount;
      } else if (cf.cf_type === 'principal') {
        principal = cf.amount;
      } else if (cf.cf_type === 'coupon_and_principal') {
        // Last cash flow includes both
        principal = 100;
        coupon = cf.amount - 100;
      }

      return {
        ...cf,
        coupon,
        principal,
        pv,
        cumulativePV,
      };
    });
  }, [cashFlows, analysis]);

  const formatDate = (dateStr) => {
    const date = new Date(dateStr);
    return date.toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: '2-digit',
    });
  };

  const formatNumber = (value, decimals = 2) => {
    return Number(value).toFixed(decimals);
  };

  const totalCoupon = enrichedFlows.reduce((sum, cf) => sum + cf.coupon, 0);
  const totalPrincipal = enrichedFlows.reduce((sum, cf) => sum + cf.principal, 0);
  const totalAmount = enrichedFlows.reduce((sum, cf) => sum + cf.amount, 0);
  const totalPV = enrichedFlows.reduce((sum, cf) => sum + cf.pv, 0);

  return (
    <div className="cashflow-panel">
      <div className="panel-header">
        <h3>Cash Flows</h3>
        <span className="flow-count">{enrichedFlows.length} payments</span>
      </div>

      <div className="cashflow-table-container">
        <table className="cashflow-table">
          <thead>
            <tr>
              <th>Date</th>
              <th className="number">Coupon</th>
              <th className="number">Principal</th>
              <th className="number">Total</th>
              <th className="number">PV</th>
              <th className="number">Cumul PV</th>
            </tr>
          </thead>
          <tbody>
            {enrichedFlows.map((cf, index) => (
              <tr key={index} className={cf.principal > 0 ? 'principal-row' : ''}>
                <td className="date-cell">{formatDate(cf.date)}</td>
                <td className="number">{cf.coupon > 0 ? formatNumber(cf.coupon) : '-'}</td>
                <td className="number principal">
                  {cf.principal > 0 ? formatNumber(cf.principal) : '-'}
                </td>
                <td className="number">{formatNumber(cf.amount)}</td>
                <td className="number pv">{formatNumber(cf.pv, 4)}</td>
                <td className="number cumulative">{formatNumber(cf.cumulativePV, 4)}</td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr className="totals-row">
              <td>Total</td>
              <td className="number">{formatNumber(totalCoupon)}</td>
              <td className="number">{formatNumber(totalPrincipal)}</td>
              <td className="number">{formatNumber(totalAmount)}</td>
              <td className="number">{formatNumber(totalPV, 4)}</td>
              <td className="number">-</td>
            </tr>
          </tfoot>
        </table>
      </div>

      {enrichedFlows.length === 0 && (
        <div className="no-data">
          No cash flows available. Click Calculate to generate.
        </div>
      )}
    </div>
  );
}

export default CashFlowTable;
