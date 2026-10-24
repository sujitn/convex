import React, { useMemo } from 'react';

// A guilloche rosette — the engraved interference pattern printed on bond
// certificates — drawn as overlapping wave-modulated rings.
function ringPath(cx, cy, radius, amp, lobes, phase, steps = 360) {
  let d = '';
  for (let i = 0; i <= steps; i++) {
    const t = (i / steps) * Math.PI * 2;
    const r = radius + amp * Math.sin(lobes * t + phase);
    const x = cx + r * Math.cos(t);
    const y = cy + r * Math.sin(t);
    d += `${i ? 'L' : 'M'}${x.toFixed(2)} ${y.toFixed(2)}`;
  }
  return `${d}Z`;
}

export default function Rosette({ className, rings = 18, lobes = 14, size = 300, strokeWidth = 0.6, ...rest }) {
  const paths = useMemo(() => {
    const c = size / 2;
    const out = [];
    for (let i = 0; i < rings; i++) {
      const radius = size * 0.16 + (i * size * 0.3) / rings;
      out.push(ringPath(c, c, radius, size * 0.035, lobes, (i * Math.PI) / rings));
    }
    return out;
  }, [rings, lobes, size]);
  return (
    <svg className={className} viewBox={`0 0 ${size} ${size}`} aria-hidden="true" {...rest}>
      {paths.map((d, i) => (
        <path key={i} d={d} fill="none" stroke="currentColor" strokeWidth={strokeWidth} />
      ))}
    </svg>
  );
}

export function Mark(props) {
  return <Rosette rings={6} lobes={9} size={60} strokeWidth={1.1} {...props} />;
}
