import React from 'react';
import Rosette from './Rosette';
import { formatLong, isIso } from '../lib/dates';
import { CALENDAR_RULES } from '../lib/calendars';

const DAY_COUNTS = {
  ActActIcma: 'Act/Act ICMA', ActActIsda: 'Act/Act ISDA', Thirty360US: '30/360 US', Thirty360E: '30E/360',
  Act360: 'Act/360', Act365Fixed: 'Act/365F',
};
const FREQ = { Annual: 'Annual', SemiAnnual: 'Semi-annual', Quarterly: 'Quarterly', Monthly: 'Monthly' };

export const dayCountLabel = (d) => DAY_COUNTS[d] || d;
// Overnight indices a floating-rate note can reference, as the band names them.
const FLOATING_INDEX = { 'USD.SOFR': 'Compounded SOFR' };
export const frequencyLabel = (f) => FREQ[f] || f;

function Fact({ label, children }) {
  return (
    <div className="fact">
      <span className="fact__label">{label}</span>
      <span className="fact__value">{children}</span>
    </div>
  );
}

/** The certificate band: the security's identity, set like an engraved title. */
export default function SecurityBand({ sec, settlement, extra }) {
  if (!sec) return null;
  const bill = sec.kind === 'bill';
  const floating = sec.kind === 'floating';
  return (
    <section className="cert" aria-label="Security">
      <Rosette className="cert__rosette" />
      <div className="cert__issuer">{sec.issuer}{sec.source === 'example' ? ' · editable example' : ''}</div>
      <h2 className="cert__name">{sec.name}</h2>
      <div className="cert__facts">
        {sec.cusip && <Fact label="CUSIP">{sec.cusip}</Fact>}
        {sec.isin && <Fact label="ISIN">{sec.isin}</Fact>}
        {floating && <Fact label="Index">{FLOATING_INDEX[sec.index] || sec.index} + {sec.spreadBp} bp</Fact>}
        {floating && (
          <Fact label="Observation">
            {sec.lookbackDays ?? 2} business day{(sec.lookbackDays ?? 2) === 1 ? '' : 's'}{sec.observationShift === false ? ' lookback' : ' observation shift'}{sec.lockoutDays ? `, ${sec.lockoutDays} business day${sec.lockoutDays === 1 ? '' : 's'} lockout` : ''}
          </Fact>
        )}
        {!bill && !floating && <Fact label="Coupon">{sec.coupon}%{sec.steps && sec.steps.length ? ' step-up' : ''}</Fact>}
        <Fact label="Maturity">{formatLong(sec.maturity)}</Fact>
        {!bill && <Fact label="Dated">{formatLong(sec.datedDate || sec.issueDate)}</Fact>}
        {!bill && <Fact label="Frequency">{frequencyLabel(sec.frequency)}</Fact>}
        {!bill && <Fact label="Day count">{dayCountLabel(sec.dayCount)}</Fact>}
        <Fact label="Settlement">T+{sec.settlementDays}{settlement ? ` · ${formatLong(settlement)}` : ''}</Fact>
        <Fact label="Calendar">{CALENDAR_RULES[sec.calendar] ? CALENDAR_RULES[sec.calendar].label : sec.calendar}</Fact>
        {sec.calls && sec.calls.length > 0 && (
          <Fact label="Calls">
            {sec.calls.length} date{sec.calls.length === 1 ? '' : 's'}{isIso(sec.calls[0].date) ? ` from ${formatLong(sec.calls[0].date)}` : ''}
          </Fact>
        )}
        {sec.sinking && sec.sinking.length > 0 && <Fact label="Sinking fund">{sec.sinking.length} payments</Fact>}
        {extra}
      </div>
    </section>
  );
}
