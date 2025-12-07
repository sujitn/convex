import React, { useState } from 'react';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  ReferenceLine,
  ReferenceDot,
} from 'recharts';

function PriceYieldChart({ data, currentPrice, currentYield }) {
  const [chartMode, setChartMode] = useState('priceYield');

  const CustomTooltip = ({ active, payload, label }) => {
    if (active && payload && payload.length) {
      return (
        <div className="chart-tooltip">
          <p className="tooltip-price">Price: {Number(label).toFixed(3)}</p>
          <p className="tooltip-yield">Yield: {payload[0].value?.toFixed(4)}%</p>
          {payload[1] && (
            <p className="tooltip-duration">Duration: {payload[1].value?.toFixed(3)}</p>
          )}
        </div>
      );
    }
    return null;
  };

  const renderPriceYieldChart = () => (
    <ResponsiveContainer width="100%" height={250}>
      <LineChart
        data={data}
        margin={{ top: 20, right: 30, left: 20, bottom: 20 }}
      >
        <CartesianGrid strokeDasharray="3 3" stroke="#3d3d5c" />
        <XAxis
          dataKey="price"
          stroke="#888"
          tick={{ fill: '#00ff00', fontSize: 11 }}
          label={{
            value: 'Price',
            position: 'bottom',
            fill: '#888',
            offset: 0,
          }}
          domain={['auto', 'auto']}
        />
        <YAxis
          stroke="#888"
          tick={{ fill: '#00ff00', fontSize: 11 }}
          label={{
            value: 'Yield (%)',
            angle: -90,
            position: 'insideLeft',
            fill: '#888',
          }}
          domain={['auto', 'auto']}
        />
        <Tooltip content={<CustomTooltip />} />
        <Line
          type="monotone"
          dataKey="yield"
          stroke="#00ff00"
          strokeWidth={2}
          dot={false}
          activeDot={{ r: 6, fill: '#ff6600' }}
        />
        {currentPrice && currentYield && (
          <ReferenceDot
            x={currentPrice}
            y={currentYield}
            r={8}
            fill="#ff6600"
            stroke="#fff"
            strokeWidth={2}
          />
        )}
        {currentPrice && (
          <ReferenceLine
            x={currentPrice}
            stroke="#ff6600"
            strokeDasharray="5 5"
            label={{
              value: `${currentPrice.toFixed(2)}`,
              fill: '#ff6600',
              position: 'top',
            }}
          />
        )}
      </LineChart>
    </ResponsiveContainer>
  );

  const renderDurationChart = () => (
    <ResponsiveContainer width="100%" height={250}>
      <LineChart
        data={data}
        margin={{ top: 20, right: 30, left: 20, bottom: 20 }}
      >
        <CartesianGrid strokeDasharray="3 3" stroke="#3d3d5c" />
        <XAxis
          dataKey="price"
          stroke="#888"
          tick={{ fill: '#00ff00', fontSize: 11 }}
          label={{
            value: 'Price',
            position: 'bottom',
            fill: '#888',
            offset: 0,
          }}
        />
        <YAxis
          stroke="#888"
          tick={{ fill: '#00ff00', fontSize: 11 }}
          label={{
            value: 'Duration',
            angle: -90,
            position: 'insideLeft',
            fill: '#888',
          }}
        />
        <Tooltip content={<CustomTooltip />} />
        <Line
          type="monotone"
          dataKey="duration"
          stroke="#00ccff"
          strokeWidth={2}
          dot={false}
          activeDot={{ r: 6, fill: '#ff6600' }}
        />
        {currentPrice && (
          <ReferenceLine
            x={currentPrice}
            stroke="#ff6600"
            strokeDasharray="5 5"
          />
        )}
      </LineChart>
    </ResponsiveContainer>
  );

  return (
    <div className="chart-panel">
      <div className="panel-header">
        <h3>Price/Yield Analysis</h3>
        <div className="chart-controls">
          <button
            className={`chart-toggle ${chartMode === 'priceYield' ? 'active' : ''}`}
            onClick={() => setChartMode('priceYield')}
          >
            Price/Yield
          </button>
          <button
            className={`chart-toggle ${chartMode === 'duration' ? 'active' : ''}`}
            onClick={() => setChartMode('duration')}
          >
            Duration
          </button>
        </div>
      </div>

      <div className="chart-container">
        {data && data.length > 0 ? (
          chartMode === 'priceYield' ? renderPriceYieldChart() : renderDurationChart()
        ) : (
          <div className="chart-placeholder">
            <p>Chart data will appear after calculation</p>
          </div>
        )}
      </div>

      <div className="chart-legend">
        <div className="legend-item">
          <span className="legend-color" style={{ backgroundColor: '#00ff00' }}></span>
          <span>{chartMode === 'priceYield' ? 'Yield Curve' : 'Duration Profile'}</span>
        </div>
        <div className="legend-item">
          <span className="legend-color" style={{ backgroundColor: '#ff6600' }}></span>
          <span>Current Position</span>
        </div>
      </div>
    </div>
  );
}

export default PriceYieldChart;
