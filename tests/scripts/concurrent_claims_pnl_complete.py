"""Original delivered-future replay at the independent concurrent-cash baseline.

The invoice-owned trial initialized the future from already earned cash rows but
used an active opening status. Replay its complete original settlement/delivery
and initial receipt history from the original opening mark, with no pre-inferred
cash rows. This preserves the original baseline native lifecycle owner, ended
model exposure and separate invoice/nominal rights. Complete independent Decimal60
child cash, prices, prefixes, shares and bounds remain; no candidate value is read.
Retain both prior complete declarations and native failures unchanged.
"""
import argparse, hashlib, json
import concurrent_claims_pnl_owned as original
SOURCE=original.TARGET
TARGET=SOURCE.with_name('concurrent-claims-pnl-complete.json')
financial_original=original.financial_original

def financial(book,owners,links,opening,parts,disposals,facts,transfers):
    opening[0]=[]
    opening._events[0]=0
    return financial_original(book,owners,links,opening,parts,disposals,facts,transfers)

def generate():
    previous=original.financial_original;original.financial_original=financial
    try:data=original.generate()
    finally:original.financial_original=previous
    data.update(reference=__doc__,invoice_trial_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest())
    return data

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args()
    data=generate();raw=(json.dumps(data,indent=2,default=original.original.r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete original future replay/concurrent claim declaration differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
