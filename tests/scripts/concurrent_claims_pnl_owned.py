"""Complete concurrent cash with the original independent invoice entitlement.

The initial trial excluded the future's invoice while selecting child cash.
Retain that full declaration and failed native receipt. Restore the exact original
future entitlement before independently deriving every financial amount. Keep
each original child cash proposal, residual, paid prefix and complete transfer.
Transport allocation order follows the declared cohort order and original cash
kind order; no financial input, value or tolerance is fitted to a native answer.
"""
import argparse, copy, hashlib, json
import concurrent_claims_pnl_reference as original
SOURCE=original.TARGET
TARGET=SOURCE.with_name('concurrent-claims-pnl-owned.json')
financial_original=original.financial

def financial(book,owners,links,opening,parts,disposals,facts,transfers):
    prior=json.loads(original.SOURCE.read_bytes())
    seed=next(c for c in prior['cases'] if c['expected_error'] is None
              and c['id'].startswith('maturity.') and 'regression' not in c['id']
              and c['request']['operation']['input']['cohorts'][0]['quantity']==owners[0]['quantity'])
    owners[0]['entitlements']=copy.deepcopy(seed['request']['operation']['input']['cohorts'][0]['entitlements'])
    expected=financial_original(book,owners,links,opening,parts,disposals,facts,transfers)
    order={o['trade_id']:j for j,o in enumerate(owners)}
    for trade in expected['trades']:
        for mark in trade['marks']:
            mark['transferred'].sort(key=lambda a:(order[a['origin']],a['key']['kind'],a['key']['effective_date'],a['key']['ordinal']))
    return expected

def generate():
    previous=original.financial;original.financial=financial
    try:data=original.generate()
    finally:original.financial=previous
    data.update(reference=__doc__,initial_trial_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest())
    return data

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args()
    data=generate();raw=(json.dumps(data,indent=2,default=original.r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete original invoice/concurrent cash declaration differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
