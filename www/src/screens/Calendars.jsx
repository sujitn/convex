import React, { useEffect, useState } from 'react';
import { Panel, Seg, Field, KV } from '../ui/primitives';
import ScreenHead from '../ui/ScreenHead';
import { CALENDAR_RULES, CALENDAR_YEARS, calendarInput, holidaysInYear, addBusinessDays, isBusinessDay } from '../lib/calendars';
import { formatLong, isIso, weekday } from '../lib/dates';
import { useWorkspace } from '../state/workspace';

const DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

export default function Calendars({ fn }) {
  const { tradeDate } = useWorkspace();
  const [code, setCode] = useState('US.SIFMA');
  const [year, setYear] = useState(Number(tradeDate.slice(0, 4)));
  const [from, setFrom] = useState(tradeDate);
  const [edited, setEdited] = useState(false);
  // Follow the shared trade date until the field is edited here.
  useEffect(() => {
    if (!edited) setFrom(tradeDate);
  }, [tradeDate, edited]);
  const [lag, setLag] = useState('1');
  const input = calendarInput(code);
  const list = holidaysInYear(code, year);
  const n = Number(lag);
  const settle = isIso(from) && Number.isInteger(n) && n >= 0 && n < 30 ? addBusinessDays(code, from, n) : null;
  return (
    <div className="screen">
      <ScreenHead fn={fn} dates={false}
        lede="The engine never looks a calendar up by name. Every request carries the full holiday table, generated here from published rules." />
      <div className="row" style={{ marginBottom: 14 }}>
        <Seg value={code} onChange={setCode} options={Object.keys(CALENDAR_RULES).map((k) => [k, k])} label="Calendar" />
      </div>
      <div className="cols cols--wide">
        <Panel title={CALENDAR_RULES[code].label}
          actions={(
            <div className="row">
              <button type="button" className="btn btn--sm btn--ghost" onClick={() => setYear((y) => Math.max(CALENDAR_YEARS[0], y - 1))} aria-label="Previous year">‹</button>
              <b className="tabular">{year}</b>
              <button type="button" className="btn btn--sm btn--ghost" onClick={() => setYear((y) => Math.min(CALENDAR_YEARS[1], y + 1))} aria-label="Next year">›</button>
            </div>
          )}>
          <div className="calendar-grid">
            {list.map((h) => (
              <div key={h.date + h.name}><span>{h.name}</span><time dateTime={h.date}>{DAYS[weekday(h.date)]} {formatLong(h.date)}</time></div>
            ))}
          </div>
        </Panel>
        <div className="stack">
          <Panel title="Settlement date">
            <div className="form form--2" style={{ marginBottom: 10 }}>
              <Field label="Trade date"><input type="date" value={from} onChange={(e) => {
                setEdited(true);
                setFrom(e.target.value);
              }} /></Field>
              <Field label="Business days (T+n)"><input className="input--num" value={lag} onChange={(e) => setLag(e.target.value)} /></Field>
            </div>
            <KV rows={[
              ['Settlement', settle ? `${DAYS[weekday(settle)]} ${formatLong(settle)}` : '—'],
              ['Trade date is a business day', isIso(from) ? (isBusinessDay(code, from) ? 'Yes' : 'No') : '—'],
            ]} />
          </Panel>
          <Panel title="What the engine receives">
            <KV rows={[
              ['Pin', `${input.pin.id} v${input.pin.version}`],
              ['Holidays supplied', input.holidays.length.toLocaleString(), `${CALENDAR_YEARS[0]}–${CALENDAR_YEARS[1]}, weekdays only`],
              ['Weekend days', input.weekend_days.map((d) => DAYS[d]).join(', '), 'Monday = 0'],
            ]} />
          </Panel>
        </div>
      </div>
    </div>
  );
}
