import React from 'react';
import { useWorkspace } from '../state/workspace';
import { useStore } from '../state/store';
import { formatLong } from '../lib/dates';

/** Screen title with function code, lede and the shared trade-date control. */
export default function ScreenHead({ fn, lede, children, dates = true }) {
  const { tradeDate, datasets } = useWorkspace();
  const { state, dispatch } = useStore();
  const asOf = datasets['ust-par'] && datasets['ust-par'].asOf;
  return (
    <div className="screen__head">
      <div>
        <h1 className="screen__title"><small>{fn.code}</small>{fn.title}</h1>
        {lede && <p className="screen__lede">{lede}</p>}
      </div>
      <div className="row">
        {children}
        {dates && (
          <label className="field" style={{ width: 168 }}>
            <span>Trade date{state.tradeDateOverride ? '' : ' (curve date)'}</span>
            <input type="date" value={tradeDate} onChange={(e) => e.target.value
              && dispatch({ type: 'tradeDate', date: e.target.value, override: e.target.value !== asOf })}
              title={asOf ? `Treasury curve as of ${formatLong(asOf)}` : undefined} />
          </label>
        )}
      </div>
    </div>
  );
}
