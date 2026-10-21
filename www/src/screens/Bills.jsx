import React, { useEffect, useMemo, useState } from 'react';
import { useWorkspace, useComputation } from '../state/workspace';
import { useStore } from '../state/store';
import { billFromDiscountRate, billFromInvestmentRate, billFromPrice, billResult } from '../lib/bills';
import { settlementFor } from '../lib/terms';
import { Panel, Seg, Field, Figure, KV, EngineError, Empty, Spinner, Prov } from '../ui/primitives';
import SecurityBand from '../ui/SecurityBand';
import ScreenHead from '../ui/ScreenHead';
import { LineChart } from '../ui/charts';
import { fmt, money } from '../lib/format';
import { formatLong, daysBetween } from '../lib/dates';
import { isDecimal } from '../lib/decimal';

const KINDS = [['discount', 'Discount rate %'], ['investment', 'Investment rate %'], ['price', 'Price']];

function quoteRequest(bill, settlement, kind, text) {
  if (kind === 'discount') return billFromDiscountRate(bill, settlement, text);
  if (kind === 'investment') return billFromInvestmentRate(bill, settlement, text);
  return billFromPrice(bill, settlement, text);
}

/** Today's published discount rate for a bill, from the daily bill-rates feed when the CUSIP matches. */
function dailyRate(billRates, cusip) {
  if (!billRates || !billRates.extra) return null;
  const tenor = Object.entries(billRates.extra.cusip || {}).find(([, c]) => c === cusip);
  return tenor ? { tenor: tenor[0], discount: billRates.extra.discount[tenor[0]] } : null;
}

export default function Bills({ fn }) {
  const { engine, selected, datasets, tradeDate, treasuries } = useWorkspace();
  const { state, dispatch } = useStore();
  const bills = useMemo(() => treasuries.filter((s) => s.kind === 'bill' && s.maturity > tradeDate
    && s.issueDate <= settlementFor(s, tradeDate)), [treasuries, tradeDate]);
  const fallback = useMemo(() => {
    const thirteen = bills.filter((b) => /13-Week/.test(b.term)).sort((a, b) => (a.auction.date < b.auction.date ? 1 : -1));
    return thirteen[0] || bills[Math.floor(bills.length / 2)];
  }, [bills]);
  const bill = selected && selected.kind === 'bill' ? selected : fallback;
  const settlement = bill ? settlementFor(bill, tradeDate) : tradeDate;
  const billRates = datasets['ust-bills'];
  const published = bill ? dailyRate(billRates, bill.cusip) : null;
  const [kind, setKind] = useState('discount');
  const [text, setText] = useState('');
  useEffect(() => {
    setKind('discount');
    setText(published ? published.discount : (bill && bill.auction.discountRate) || '4.00');
  }, [bill && bill.id, published && published.discount]); // eslint-disable-line react-hooks/exhaustive-deps
  const face = Number(state.quote.face || '1000000');

  const valid = bill && isDecimal(text) && Number(text) > 0;
  const { data, busy } = useComputation(
    () => (valid ? engine.call(quoteRequest(bill, settlement, kind, text), { label: `${bill.ticker} ${kind}` }) : null),
    [bill && bill.id, settlement, kind, text],
  );

  // The daily bill curve: every published tenor through the same native convention.
  const curve = useComputation(async () => {
    if (!billRates || !billRates.extra) return null;
    // Only bills whose actual issue date is known (from auction results) are
    // converted; the issue date sets the 365/366-day basis.
    const rows = Object.keys(billRates.extra.discount).map((tenor) => {
      const cusip = billRates.extra.cusip[tenor];
      const sec = treasuries.find((s) => s.cusip === cusip) || null;
      return { tenor, cusip, sec, maturity: billRates.extra.maturity[tenor], discount: billRates.extra.discount[tenor],
        published: (billRates.points.find((p) => p.tenor === tenor) || {}).value };
    }).filter((r) => r.maturity > tradeDate && (!r.sec || r.sec.issueDate <= settlementFor(r.sec, tradeDate)));
    const known = rows.filter((r) => r.sec);
    const res = await engine.callMany(known.map((r) => billFromDiscountRate(r.sec, settlementFor(r.sec, tradeDate), r.discount)),
      { label: 'bill curve' });
    return rows.map((r) => {
      const i = known.indexOf(r);
      if (i < 0) return { ...r, result: null, error: { code: 'missing_input', message: 'Issue date not in the loaded auction results' } };
      return { ...r, result: res[i].ok ? billResult(res[i].data) : null, error: res[i].ok ? null : res[i].error };
    });
  }, [billRates && billRates.asOf, billRates && billRates.status, treasuries, tradeDate]);

  const r = data && data.ok ? billResult(data.data) : null;
  return (
    <div className="screen">
      <ScreenHead fn={fn} lede="Treasury bills on the official convention: bank discount, investment rate and price, each from the others." />
      <div className="cols cols--side">
        <Panel title="Bills" note="from auction results" flush>
          <div className="table-wrap" style={{ maxHeight: 640 }}>
            <table className="grid">
              <tbody>
                {bills.map((b) => (
                  <tr key={b.id} className={bill && b.id === bill.id ? 'is-active' : ''} style={{ cursor: 'pointer' }} tabIndex={0}
                    onClick={() => dispatch({ type: 'select', id: b.id })} onKeyDown={(e) => e.key === 'Enter' && dispatch({ type: 'select', id: b.id })}>
                    <td><div>{formatLong(b.maturity)}</div><div className="muted" style={{ fontSize: 11.5 }}>{b.cusip} · {b.term}</div></td>
                    <td className="num dim">{daysBetween(tradeDate, b.maturity)}d</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Panel>
        <div className="stack">
          {!bill ? <Empty>No bills loaded.</Empty> : (
            <>
              <SecurityBand sec={bill} settlement={settlement}
                extra={bill.auction && <div className="fact"><span className="fact__label">Auction</span><span className="fact__value">{formatLong(bill.auction.date)} · {bill.auction.discountRate}% disc.</span></div>} />
              <Panel>
                <div className="row" style={{ alignItems: 'flex-end', gap: 16 }}>
                  <Field label="Quote as" as="div"><Seg value={kind} onChange={(k) => {
                    if (r) setText(k === 'discount' ? fmt(Number(r.discountPercent), 6) : k === 'investment' ? fmt(Number(r.investmentPercent), 6) : fmt(Number(r.pricePer100), 6));
                    setKind(k);
                  }} options={KINDS} label="Quote type" /></Field>
                  <Field label={KINDS.find((k) => k[0] === kind)[1]}>
                    <input className="input input--quote" style={{ width: 200 }} value={text} onChange={(e) => setText(e.target.value)} aria-invalid={!valid} />
                  </Field>
                  <div className="grow" />
                  {published && <Prov status={billRates.status}>Daily {published.tenor} bill rate {formatLong(billRates.asOf)}: {published.discount}% discount</Prov>}
                  {busy && <Spinner />}
                </div>
              </Panel>
              {data && !data.ok && <EngineError error={data.error} context="Bill" />}
              {r && (
                <>
                  <div className="panel">
                    <div className="figures">
                      <Figure label="Price" value={fmt(Number(r.pricePer100), 6)} sub={`auction rounding ${r.auctionPricePer100}`} />
                      <Figure label="Discount rate" value={fmt(Number(r.discountPercent), 4)} unit="%" />
                      <Figure label="Investment rate" value={fmt(Number(r.investmentPercent), 4)} unit="%" sub="bond-equivalent" />
                      <Figure label="Days to maturity" value={r.remainingDays} sub={`${r.basisDays}-day year · ${r.method} formula`} />
                    </div>
                  </div>
                  <div className="cols cols--2">
                    <Panel title={`Cost for ${money(face, 0)} face`}>
                      <KV rows={[
                        ['Purchase price', money((Number(r.pricePer100) * face) / 100)],
                        ['Discount earned at maturity', money(face - (Number(r.pricePer100) * face) / 100)],
                        ['Price change, +1 bp discount', money((Number(r.pricePerUp1bpDiscount) * face) / 100), 'per bp'],
                        ['Price change, +1 bp investment', money((Number(r.pricePerUp1bpInvestment) * face) / 100), 'per bp'],
                      ]} />
                    </Panel>
                    <Panel title="Settlement">
                      <KV rows={[
                        ['Trade date', formatLong(tradeDate)],
                        ['Settlement', formatLong(settlement), 'T+1, SIFMA calendar'],
                        ['Maturity', formatLong(bill.maturity)],
                        ['Issue', formatLong(bill.issueDate)],
                      ]} />
                    </Panel>
                  </div>
                </>
              )}
            </>
          )}
          {curve.data && curve.data.length > 0 && (
            <Panel title="Daily bill curve" note={`published discount rates, ${formatLong(billRates.asOf)}, converted by the engine`} flush>
              <div>
                <table className="grid">
                  <thead><tr><th>Tenor</th><th>Maturity</th><th className="num">Discount %</th><th className="num">Investment %</th><th className="num">Published CE %</th><th className="num">Price</th></tr></thead>
                  <tbody>
                    {curve.data.map((c) => (
                      <tr key={c.tenor}>
                        <td>{c.tenor}</td><td>{formatLong(c.maturity)}</td><td className="num">{c.discount}</td>
                        <td className="num">{c.result ? fmt(Number(c.result.investmentPercent), 3) : '—'}</td>
                        <td className="num dim">{c.published}</td>
                        <td className="num">{c.result ? fmt(Number(c.result.pricePer100), 6) : <span className="neg" title={c.error && c.error.message}>error</span>}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                <div style={{ padding: 12, borderTop: '1px solid var(--rule)' }}>
                  <LineChart height={200}
                    series={[
                      { id: 'disc', label: 'Discount', color: '--s1', points: curve.data.map((c) => ({ x: daysBetween(tradeDate, c.maturity), y: Number(c.discount) })) },
                      { id: 'inv', label: 'Investment', color: '--s2', points: curve.data.filter((c) => c.result).map((c) => ({ x: daysBetween(tradeDate, c.maturity), y: Number(c.result.investmentPercent) })) },
                    ]}
                    xFormat={(v) => `${Math.round(v)}d`} yFormat={(v) => v.toFixed(2)} tipX={(v) => `${Math.round(v)} days`} tipY={(v) => `${v.toFixed(3)}%`} />
                </div>
              </div>
            </Panel>
          )}
        </div>
      </div>
    </div>
  );
}
