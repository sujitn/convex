"""Bind the complete independent overdue-policy rejection experiment.

All ten positive whole books and sixteen other typed inputs stay unchanged.
Retain the old malformed experiment: changing policy only in the ordered sets
caused a prior causal-policy mismatch. Apply the same supplied Reject policy to
its entire independent set so the original overdue owner is actually exercised.
No pricer, numerical answer, tolerance or admission guard changes.
"""
import argparse, hashlib, json
import pending_cash_pnl_discounted as original
SOURCE=original.TARGET
TARGET=SOURCE.with_name('pending-cash-pnl-final.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    c=next(c for c in data['cases'] if c['id']=='pending.invalid.no_overdue_policy')
    for s in c['request']['operation']['input']['book']['standalone']:
        if s['kind']=='available':
            for p in s['information']['point']['positions']:p['overdue']='reject'
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete independent policy control differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
