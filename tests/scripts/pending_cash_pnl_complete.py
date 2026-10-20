"""Complete the explicitly empty future cash-transfer namespace.

Retain all earlier whole inputs and failed complete-checker execution. The
future owns no transferred claim; supply its original empty array explicitly.
Every request, financial value, full provenance and numerical bound is unchanged.
"""
import argparse, hashlib, json
import pending_cash_pnl_receipts as original
SOURCE=original.TARGET
TARGET=SOURCE.with_name('pending-cash-pnl-complete.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    for c in data['cases']:
        if c['expected_error'] is not None:continue
        for t in c['expected']['trades']:
            for m in t['marks']:m.setdefault('transferred',[])
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete explicit empty future namespace differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
