"""Bind the original singular latest receipt alongside the complete ordered list.

Retain the available declaration and failed singular-provenance check unchanged.
All requests, cash/custody calculations, complete receipts and bounds remain.
"""
import argparse, hashlib, json
import pending_cash_pnl_available as original
SOURCE=original.TARGET
TARGET=SOURCE.with_name('pending-cash-pnl-receipts.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    for c in data['cases']:
        if c['expected_error'] is not None or 'regression' in c['id']:continue
        for t in c['expected']['trades']:
            for m in t['marks']:
                p=m['physical']
                if p:
                    events=p['security_receipts']
                    p['security_receipt']=events[-1] if events else None
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete original receipt provenance differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
