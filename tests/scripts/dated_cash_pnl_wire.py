"""Bind identical actual fill wire and canonical exact legal residual fractions.

The original full native checker compares complete actual fills. The earlier
new expected fill retained insignificant quantity scale before lossless wire
encoding. Bind the very same complete supplied fill after exact Decimal/Money
comparisons; no quantity, actual payment, financial answer or bound is changed.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D
import dated_cash_pnl_contracts as original
import cohort_pnl_states as states
import cohort_pnl_reference as r
SOURCE=original.TARGET;TARGET=SOURCE.with_name('dated-cash-pnl-wire.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    for c in data['cases']:
        if c['expected_error'] is not None:continue
        book=states.expand(c['request'])
        for j,t in enumerate(c['expected']['trades']):
            for i,m in enumerate(t['marks']):
                if not m['physical']:continue
                for d in m['physical'].get('disposals',[]):
                    fill=next(f for f in r.sets(book)[i]['point']['fills'] if f['id']==d['facts']['fill_id'])
                    if D(fill['quantity'])!=D(d['fill']['quantity']):raise ValueError('Actual original quantity changed')
                    for k in ['consideration','fees']:
                        if D(fill[k]['value'])!=D(d['fill'][k]['value']):raise ValueError('Actual original Money changed')
                    d['fill']=copy.deepcopy(fill)
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Exact actual fill wire differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
