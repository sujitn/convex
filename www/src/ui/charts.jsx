import React, { useEffect, useMemo, useRef, useState } from 'react';

// Small SVG chart kit: one y-axis, recessive grid, 2px lines, crosshair
// tooltip on hover, direct end labels for up to four series.

function useWidth() {
  const ref = useRef(null);
  const [width, setWidth] = useState(600);
  useEffect(() => {
    if (!ref.current || typeof ResizeObserver === 'undefined') return undefined;
    const ro = new ResizeObserver(([e]) => setWidth(Math.max(240, e.contentRect.width)));
    ro.observe(ref.current);
    return () => ro.disconnect();
  }, []);
  return [ref, width];
}

function niceTicks(min, max, count = 5) {
  if (!Number.isFinite(min) || !Number.isFinite(max)) return [];
  if (min === max) {
    const pad = Math.abs(min) * 0.01 || 1;
    min -= pad;
    max += pad;
  }
  const span = max - min;
  const step0 = span / count;
  const mag = 10 ** Math.floor(Math.log10(step0));
  const norm = step0 / mag;
  const step = (norm >= 5 ? 10 : norm >= 2 ? 5 : norm >= 1 ? 2 : 1) * mag;
  const start = Math.ceil(min / step - 1e-9) * step;
  const ticks = [];
  for (let v = start; v <= max + step * 1e-9; v += step) ticks.push(+v.toFixed(12));
  return ticks;
}

function extent(values, pad = 0.06, includeZero = false) {
  let lo = Infinity;
  let hi = -Infinity;
  for (const v of values) {
    if (!Number.isFinite(v)) continue;
    if (v < lo) lo = v;
    if (v > hi) hi = v;
  }
  if (includeZero) {
    lo = Math.min(lo, 0);
    hi = Math.max(hi, 0);
  }
  if (!Number.isFinite(lo)) return [0, 1];
  const span = hi - lo || Math.abs(hi) * 0.02 || 1;
  return [lo - span * pad, hi + span * pad];
}

const color = (c) => (c && c.startsWith('--') ? `var(${c})` : c);

export function Legend({ series }) {
  if (series.length < 2) return null;
  return (
    <div className="legend">
      {series.map((s) => (
        <span key={s.id}>
          <i className={s.dash ? 'dash' : ''} style={s.dash ? { color: color(s.color) } : { background: color(s.color) }} />
          {s.label}
        </span>
      ))}
    </div>
  );
}

/**
 * series: [{ id, label, color: '--s1', points: [{x, y}], dash, markers }]
 * refs: vertical reference lines [{ x, label }]
 */
export function LineChart({ series, height = 260, xFormat = (v) => v, yFormat = (v) => v,
  tipX = xFormat, tipY = yFormat, xLabel, yLabel, refs = [], yIncludeZero = false, xDomain, legend = true }) {
  const [ref, width] = useWidth();
  const [hover, setHover] = useState(null);
  const m = { top: 12, right: series.length <= 4 ? 92 : 16, bottom: 28, left: 52 };
  const w = width - m.left - m.right;
  const h = height - m.top - m.bottom;
  const all = series.flatMap((s) => s.points);
  const [x0, x1] = xDomain || extent(all.map((p) => p.x), 0);
  const [y0, y1] = extent(all.map((p) => p.y), 0.08, yIncludeZero);
  const sx = (x) => m.left + ((x - x0) / (x1 - x0 || 1)) * w;
  const sy = (y) => m.top + h - ((y - y0) / (y1 - y0 || 1)) * h;
  const xt = niceTicks(x0, x1, Math.max(3, Math.round(w / 90)));
  const yt = niceTicks(y0, y1, Math.max(3, Math.round(h / 48)));

  const paths = useMemo(
    () =>
      series.map((s) =>
        s.points
          .filter((p) => Number.isFinite(p.y))
          .map((p, i) => `${i ? 'L' : 'M'}${sx(p.x).toFixed(1)} ${sy(p.y).toFixed(1)}`)
          .join(''),
      ),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [series, width, height, x0, x1, y0, y1],
  );

  const onMove = (e) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const px = e.clientX - rect.left;
    if (px < m.left || px > m.left + w) return setHover(null);
    const x = x0 + ((px - m.left) / w) * (x1 - x0);
    const rows = series
      .map((s) => {
        let best = null;
        for (const p of s.points) if (Number.isFinite(p.x) && Number.isFinite(p.y) && (!best || Math.abs(p.x - x) < Math.abs(best.x - x))) best = p;
        return best && { s, p: best };
      })
      .filter(Boolean);
    if (!rows.length) return setHover(null);
    setHover({ px: sx(rows[0].p.x), rows });
  };

  const labels = series.length <= 4
    ? series.map((s) => {
      const last = [...s.points].reverse().find((p) => Number.isFinite(p.y));
      return last && { s, y: sy(last.y) };
    }).filter(Boolean)
    : [];
  // Nudge end labels apart so they never collide.
  labels.sort((a, b) => a.y - b.y);
  for (let i = 1; i < labels.length; i++) if (labels[i].y - labels[i - 1].y < 14) labels[i].y = labels[i - 1].y + 14;

  return (
    <div className="chart" ref={ref}>
      {legend && <Legend series={series} />}
      <svg height={height} onMouseMove={onMove} onMouseLeave={() => setHover(null)} role="img"
        aria-label={series.map((s) => s.label).join(', ')}>
        {yt.map((t) => (
          <g key={`y${t}`}>
            <line className="grid-line" x1={m.left} x2={m.left + w} y1={sy(t)} y2={sy(t)} />
            <text className="tick" x={m.left - 8} y={sy(t) + 4} textAnchor="end">{yFormat(t)}</text>
          </g>
        ))}
        {xt.map((t) => (
          <text key={`x${t}`} className="tick" x={sx(t)} y={height - 8} textAnchor="middle">{xFormat(t)}</text>
        ))}
        <line className="axis-line" x1={m.left} x2={m.left + w} y1={m.top + h} y2={m.top + h} />
        {yIncludeZero && y0 < 0 && y1 > 0 && <line className="zero-line" x1={m.left} x2={m.left + w} y1={sy(0)} y2={sy(0)} />}
        {refs.map((r) => (
          <g key={`r${r.x}`}>
            <line className="crosshair" x1={sx(r.x)} x2={sx(r.x)} y1={m.top} y2={m.top + h} />
            {r.label && <text className="label" x={sx(r.x) + 4} y={m.top + 10}>{r.label}</text>}
          </g>
        ))}
        {series.map((s, i) => (
          <path key={s.id} d={paths[i]} className={`series${s.dash ? ' series--dash' : ''}`} stroke={color(s.color)} />
        ))}
        {series.map((s) =>
          (s.markers || []).map((p, j) => (
            <circle key={`${s.id}m${j}`} className="marker" cx={sx(p.x)} cy={sy(p.y)} r={4} fill={color(s.color)} />
          )),
        )}
        {labels.map(({ s, y }) => (
          <text key={`l${s.id}`} className="label" x={m.left + w + 8} y={y + 4}>{s.label}</text>
        ))}
        {xLabel && <text className="tick" x={m.left + w} y={m.top + h - 6} textAnchor="end">{xLabel}</text>}
        {yLabel && <text className="tick" x={m.left + 4} y={m.top + 10}>{yLabel}</text>}
        {hover && (
          <g>
            <line className="crosshair" x1={hover.px} x2={hover.px} y1={m.top} y2={m.top + h} />
            {hover.rows.map(({ s, p }) => (
              <circle key={s.id} className="marker" cx={sx(p.x)} cy={sy(p.y)} r={4.5} fill={color(s.color)} />
            ))}
          </g>
        )}
      </svg>
      {hover && (
        <div className="chart__tip" style={{ left: Math.min(hover.px + 12, width - 190), top: m.top }}>
          <div className="chart__tip-head">{tipX(hover.rows[0].p.x, hover.rows[0].p)}</div>
          {hover.rows.map(({ s, p }) => (
            <div className="chart__tip-row" key={s.id}>
              <i style={{ background: color(s.color) }} />
              <span>{s.label}</span>
              <b>{tipY(p.y, p)}</b>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * Stacked bars at x positions. bars: [{ x, label, parts: [{ key, value }] }];
 * keys: [{ key, label, color }].
 */
export function BarChart({ bars, keys, height = 220, xFormat = (v) => v, yFormat = (v) => v, tipX = xFormat, xTicks }) {
  const [ref, width] = useWidth();
  const [hover, setHover] = useState(null);
  const m = { top: 10, right: 16, bottom: 28, left: 58 };
  const w = width - m.left - m.right;
  const h = height - m.top - m.bottom;
  const tops = bars.map((b) => b.parts.reduce((a, p) => a + Math.max(0, p.value), 0));
  const bottoms = bars.map((b) => b.parts.reduce((a, p) => a + Math.min(0, p.value), 0));
  const [y0, y1] = extent([0, ...tops, ...bottoms], 0.05, true);
  const xs = bars.map((b) => b.x);
  const [x0, x1] = extent(xs, 0.03);
  const sx = (x) => m.left + ((x - x0) / (x1 - x0 || 1)) * w;
  const sy = (y) => m.top + h - ((y - y0) / (y1 - y0 || 1)) * h;
  const minGap = bars.length > 1 ? Math.min(...xs.slice(1).map((x, i) => sx(x) - sx(xs[i]))) : w / 4;
  const bw = Math.max(2, Math.min(26, minGap - 2));
  const yt = niceTicks(y0, y1, 4);
  const xt = xTicks || niceTicks(x0, x1, Math.max(3, Math.round(w / 90)));
  return (
    <div className="chart" ref={ref}>
      <Legend series={keys.map((k) => ({ id: k.key, label: k.label, color: k.color }))} />
      <svg height={height} role="img" aria-label="Cash flow bars" onMouseLeave={() => setHover(null)}>
        {yt.map((t) => (
          <g key={t}>
            <line className="grid-line" x1={m.left} x2={m.left + w} y1={sy(t)} y2={sy(t)} />
            <text className="tick" x={m.left - 8} y={sy(t) + 4} textAnchor="end">{yFormat(t)}</text>
          </g>
        ))}
        {xt.map((t) => (
          <text key={`x${t}`} className="tick" x={sx(t)} y={height - 8} textAnchor="middle">{xFormat(t)}</text>
        ))}
        {bars.map((b, i) => {
          let up = 0;
          let down = 0;
          return (
            <g key={i} onMouseEnter={() => setHover({ b, i })}>
              <rect x={sx(b.x) - bw / 2 - 3} y={m.top} width={bw + 6} height={h} fill="transparent" />
              {b.parts.map((p) => {
                const k = keys.find((kk) => kk.key === p.key);
                if (!p.value) return null;
                let yTop;
                let hh;
                if (p.value > 0) {
                  yTop = sy(up + p.value);
                  hh = Math.max(0, sy(up) - yTop - (up > 0 ? 2 : 0));
                  up += p.value;
                } else {
                  yTop = sy(down) + (down < 0 ? 2 : 0);
                  hh = Math.max(0, sy(down + p.value) - yTop);
                  down += p.value;
                }
                return (
                  <rect key={p.key} className="bar" x={sx(b.x) - bw / 2} y={yTop} width={bw} height={hh}
                    rx={Math.min(3, bw / 3)} fill={color(k.color)} opacity={hover && hover.i !== i ? 0.55 : 1} />
                );
              })}
            </g>
          );
        })}
        <line className="axis-line" x1={m.left} x2={m.left + w} y1={sy(0)} y2={sy(0)} />
      </svg>
      {hover && (
        <div className="chart__tip" style={{ left: Math.min(sx(hover.b.x) + 14, width - 200), top: m.top }}>
          <div className="chart__tip-head">{tipX(hover.b.x, hover.b)}</div>
          {hover.b.parts.map((p) => {
            const k = keys.find((kk) => kk.key === p.key);
            return (
              <div className="chart__tip-row" key={p.key}>
                <i style={{ background: color(k.color) }} />
                <span>{k.label}</span>
                <b>{yFormat(p.value, true)}</b>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
