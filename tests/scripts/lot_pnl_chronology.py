"""Retain failed forward input; replay every full original profile boundary.

The supplied amended coupon has an actual January profile boundary before the
April earned coupon and principal. Replay those original profile dates in order;
recompute the complete forward book with the same independent Decimal60 owners.
No native financial value, relaxed tolerance or skipped cash boundary is used.
"""
import argparse, copy, hashlib, json
from decimal import localcontext
import lot_pnl_extended as forward
from lot_pnl_controls import TARGET as SOURCE

TARGET = SOURCE.with_name('lot-pnl-chronology.json')

def generate():
    data = json.loads(SOURCE.read_bytes())
    previous = forward.r.c.append_event
    def append(h, on, kind, *args, **kw):
        if kind == 'contract_step':
            after = h['lifecycle']['events'][-1]['event']['effective_date']
            terms = h['product']['product']['bond']['terms']
            dates = sorted({row['start'] for rows in terms['profiles'].values() for row in rows[1:] if after < row['start'] < on})
            for date in dates: previous(h, date, 'contract_step', **kw)
        return previous(h, on, kind, *args, **kw)
    forward.r.c.append_event = append
    try:
        with localcontext() as ctx:
            ctx.prec = 60; case = forward.future_case()
    finally: forward.r.c.append_event = previous
    for i, old in enumerate(data['cases']):
        if old['id'] == case['id']: data['cases'][i] = case
    bad = copy.deepcopy(case); bad.update(id='lot.invalid.parent_future_cash', expected_error='invalid_input'); bad.pop('expected')
    next(e for e in bad['request']['operation']['input']['cohorts'][0]['entitlements'] if e['key']['effective_date'] == '2027-04-07')['entitled'] = True
    for i, old in enumerate(data['cases']):
        if old['id'] == bad['id']: data['cases'][i] = bad
    for c in data['cases']:
        c['input_nodes'] = forward.original.nodes(c['request'])
        c['input_bytes'] = len(json.dumps(c['request'], separators=(',', ':')).encode())
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(), reference=__doc__)
    return data

if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--write', action='store_true'); a = p.parse_args()
    data = generate(); raw = (json.dumps(data, indent=2, default=forward.r.c.de.serial) + '\n').encode()
    if a.write:
        if TARGET.exists(): raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes() != raw: raise ValueError('Original ordered complete financial controls differ')
    print(json.dumps(dict(status='passed', cases=len(data['cases']), financial_books=sum(c['expected_error'] is None for c in data['cases']), certified=False)))
