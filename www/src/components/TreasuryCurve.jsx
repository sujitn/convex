import React, { useMemo } from 'react';

const CURVE_TENORS = [
  { key: '1M', label: '1M', years: 1/12 },
  { key: '3M', label: '3M', years: 0.25 },
  { key: '6M', label: '6M', years: 0.5 },
  { key: '1Y', label: '1Y', years: 1 },
  { key: '2Y', label: '2Y', years: 2 },
  { key: '3Y', label: '3Y', years: 3 },
  { key: '5Y', label: '5Y', years: 5 },
  { key: '7Y', label: '7Y', years: 7 },
  { key: '10Y', label: '10Y', years: 10 },
  { key: '20Y', label: '20Y', years: 20 },
  { key: '30Y', label: '30Y', years: 30 },
];

const CURRENCY_NAMES = {
  'USD': 'US Treasury',
  'EUR': 'EUR Govt',
  'GBP': 'UK Gilt',
  'JPY': 'JGB',
  'CHF': 'Swiss Govt',
  'AUD': 'Australian Govt',
  'CAD': 'Canadian Govt',
  'NZD': 'NZ Govt',
};

const CURVE_TYPE_SHORT_NAMES = {
  UST: 'UST',
  DBR: 'DBR',
  OAT: 'OAT',
  BTP: 'BTP',
  SPGB: 'SPGB',
  DSL: 'DSL',
  OLO: 'OLO',
  RAGB: 'RAGB',
  PGB: 'PGB',
  GGB: 'GGB',
  GILT: 'GILT',
  JGB: 'JGB',
  SWISS: 'SWISS',
  ACGB: 'ACGB',
  CAN: 'CAN',
  NZGB: 'NZGB',
};

// Linear interpolation for curve
function interpolateCurve(treasuryCurve) {
  const points = CURVE_TENORS
    .filter(t => treasuryCurve[t.key] && treasuryCurve[t.key] > 0)
    .map(t => ({ years: t.years, rate: treasuryCurve[t.key] }))
    .sort((a, b) => a.years - b.years);

  if (points.length < 2) return [];

  const interpolated = [];
  const maxYears = Math.min(points[points.length - 1].years, 30);

  for (let y = 0; y <= maxYears; y += 0.5) {
    // Find surrounding points
    let lower = points[0];
    let upper = points[points.length - 1];

    for (let i = 0; i < points.length - 1; i++) {
      if (points[i].years <= y && points[i + 1].years >= y) {
        lower = points[i];
        upper = points[i + 1];
        break;
      }
    }

    // Linear interpolation
    let rate;
    if (y <= lower.years) {
      rate = lower.rate;
    } else if (y >= upper.years) {
      rate = upper.rate;
    } else {
      const t = (y - lower.years) / (upper.years - lower.years);
      rate = lower.rate + t * (upper.rate - lower.rate);
    }

    interpolated.push({ years: y, rate });
  }

  return interpolated;
}

function TreasuryCurve({
  treasuryCurve,
  currency,
  curveType,
  availableCurveTypes,
  curveTypeNames,
  onCurveTypeChange,
  onCurveChange,
  onFetchRates,
  isFetchingRates,
  ratesLastUpdated
}) {
  const interpolatedCurve = useMemo(() => interpolateCurve(treasuryCurve), [treasuryCurve]);

  // Calculate chart dimensions
  const chartWidth = 380;
  const chartHeight = 140;
  const padding = { top: 15, right: 15, bottom: 25, left: 35 };
  const innerWidth = chartWidth - padding.left - padding.right;
  const innerHeight = chartHeight - padding.top - padding.bottom;

  // Calculate scales
  const rates = interpolatedCurve.map(p => p.rate);
  const minRate = rates.length > 0 ? Math.floor(Math.min(...rates) * 2) / 2 : 0;
  const maxRate = rates.length > 0 ? Math.ceil(Math.max(...rates) * 2) / 2 : 5;
  const maxYears = interpolatedCurve.length > 0 ? interpolatedCurve[interpolatedCurve.length - 1].years : 30;

  // Generate path
  const pathD = interpolatedCurve.length > 0
    ? interpolatedCurve.map((p, i) => {
        const x = padding.left + (p.years / maxYears) * innerWidth;
        const y = padding.top + innerHeight - ((p.rate - minRate) / (maxRate - minRate)) * innerHeight;
        return `${i === 0 ? 'M' : 'L'} ${x} ${y}`;
      }).join(' ')
    : '';

  // Generate data points for actual tenor rates
  const dataPoints = CURVE_TENORS
    .filter(t => treasuryCurve[t.key] && treasuryCurve[t.key] > 0)
    .map(t => ({
      x: padding.left + (t.years / maxYears) * innerWidth,
      y: padding.top + innerHeight - ((treasuryCurve[t.key] - minRate) / (maxRate - minRate)) * innerHeight,
      label: t.label,
      rate: treasuryCurve[t.key]
    }));

  const curveName = curveTypeNames?.[curveType] || CURRENCY_NAMES[currency] || 'Govt Curve';
  const hasMultipleCurves = availableCurveTypes && availableCurveTypes.length > 1;

  return (
    <div className="treasury-curve-panel">
      <div className="panel-header">
        <div className="curve-title-row">
          {hasMultipleCurves ? (
            <select
              className="curve-type-select"
              value={curveType}
              onChange={(e) => onCurveTypeChange(e.target.value)}
            >
              {availableCurveTypes.map(ct => (
                <option key={ct} value={ct}>
                  {CURVE_TYPE_SHORT_NAMES[ct] || ct} - {curveTypeNames?.[ct] || ct}
                </option>
              ))}
            </select>
          ) : (
            <h3>{curveName}</h3>
          )}
        </div>
        <div className="curve-actions-header">
          <button
            className="btn btn-small btn-fetch"
            onClick={onFetchRates}
            disabled={isFetchingRates}
          >
            {isFetchingRates ? '...' : 'Fetch'}
          </button>
        </div>
      </div>

      <div className="curve-content">
        <div className="curve-chart-row">
          <svg width={chartWidth} height={chartHeight} className="curve-chart">
            {/* Grid lines */}
            {[0, 0.25, 0.5, 0.75, 1].map(t => (
              <line
                key={`h-${t}`}
                x1={padding.left}
                y1={padding.top + innerHeight * (1 - t)}
                x2={padding.left + innerWidth}
                y2={padding.top + innerHeight * (1 - t)}
                stroke="var(--border-primary)"
                strokeWidth="0.5"
              />
            ))}
            {[0, 5, 10, 15, 20, 25, 30].filter(y => y <= maxYears).map(y => (
              <line
                key={`v-${y}`}
                x1={padding.left + (y / maxYears) * innerWidth}
                y1={padding.top}
                x2={padding.left + (y / maxYears) * innerWidth}
                y2={padding.top + innerHeight}
                stroke="var(--border-primary)"
                strokeWidth="0.5"
              />
            ))}

            {/* Y-axis labels */}
            <text x={padding.left - 4} y={padding.top + 4} fontSize="8" fill="var(--text-muted)" textAnchor="end">
              {maxRate.toFixed(1)}%
            </text>
            <text x={padding.left - 4} y={padding.top + innerHeight} fontSize="8" fill="var(--text-muted)" textAnchor="end">
              {minRate.toFixed(1)}%
            </text>

            {/* X-axis labels */}
            <text x={padding.left} y={chartHeight - 4} fontSize="8" fill="var(--text-muted)" textAnchor="middle">0</text>
            <text x={padding.left + innerWidth / 2} y={chartHeight - 4} fontSize="8" fill="var(--text-muted)" textAnchor="middle">
              {Math.round(maxYears / 2)}Y
            </text>
            <text x={padding.left + innerWidth} y={chartHeight - 4} fontSize="8" fill="var(--text-muted)" textAnchor="middle">
              {Math.round(maxYears)}Y
            </text>

            {/* Curve line */}
            {pathD && (
              <path
                d={pathD}
                fill="none"
                stroke="var(--accent-green)"
                strokeWidth="1.5"
              />
            )}

            {/* Data points */}
            {dataPoints.map((p, i) => (
              <circle
                key={i}
                cx={p.x}
                cy={p.y}
                r="2.5"
                fill="var(--accent-orange)"
              />
            ))}
          </svg>

          <div className="curve-grid-compact">
            {CURVE_TENORS.map(({ key, label }) => (
              <div key={key} className="curve-point-compact">
                <label>{label}</label>
                <input
                  type="number"
                  value={treasuryCurve[key] || ''}
                  onChange={(e) => onCurveChange(key, parseFloat(e.target.value) || 0)}
                  step="0.01"
                  min="0"
                  max="20"
                  placeholder="--"
                />
              </div>
            ))}
          </div>
        </div>

        {ratesLastUpdated && (
          <div className="rates-timestamp">
            Updated: {ratesLastUpdated}
          </div>
        )}
      </div>
    </div>
  );
}

export default TreasuryCurve;
