"""Complete independent overdue-policy facts in the new malformed maturity book.

The retained delayed trial changed actual overdue policy but omitted its earlier
available custody/disposal experiments, so the original causal guard rejected
that mismatch before overdue valuation. Supply the same Reject policy in every
complete independent input. Preserve the initial failure, every full financial
book/value/bound and the original due-date/causal guards. No candidate answer is
used as a financial reference or an accepted shortcut.
"""
import argparse, hashlib, json
import child_maturity_pnl_delayed as original
SOURCE=original.TARGET
TARGET=SOURCE.with_name('child-maturity-pnl-admitted.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    c=next(c for c in data['cases'] if c['id']=='maturity.invalid.no_overdue_policy')
    for s in c['request']['operation']['input']['book']['standalone']:
        if s['kind']=='available':
            for p in s['information']['point']['positions']:p['overdue']='reject'
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest())
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args()
    data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete independent overdue policy differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
