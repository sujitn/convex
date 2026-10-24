import React from 'react';
import { useWorkspace } from '../state/workspace';
import { Select } from './primitives';

/** Which curve this currency's spreads are quoted against: the government curve or one you supplied. */
export default function CurvePicker({ label = 'Spread curve', width = 220 }) {
  const { curveChoices, curveId, setSpreadCurve } = useWorkspace();
  if (curveChoices.length < 2) return null;
  return (
    <label className="field" style={{ width }}>
      <span>{label}</span>
      <Select value={curveId || ''} onChange={setSpreadCurve} aria-label={label}
        options={curveChoices.map((d) => [d.id, d.custom ? `${d.label} (yours)` : d.label])} />
    </label>
  );
}
