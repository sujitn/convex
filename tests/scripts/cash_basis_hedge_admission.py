"""Separate original canonical tenor/legal-horizon admission; full cash answers stay.

Raw rebuild cases remain unchanged. The original factor ingress admits only
D/W/M/Y, parallel or spot labels. Comparison-CDS legal coverage must fit the
actual residual-basis horizon. These changes do not alter target cash pricing,
original credit/recovery assumptions, inverse domains or financial bounds.
"""
import argparse
from copy import deepcopy
import hashlib,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
SOURCES=['tests/fixtures/library/cash-basis-hedge-rebuild.json','tests/fixtures/library/cash-basis-hedge-rebuild-reference.json']
DEST=ROOT/'tests/fixtures/library/cash-basis-hedge-admitted.json'

def build():
    data=deepcopy(json.loads((ROOT/SOURCES[0]).read_bytes()))
    for c in data['cases']:
        op=c['request']['operation']
        for f in op['input']['hedge']['factors']:
            if f['key']['factor']['tenor']=='actual-full-cash':f['key']['factor']['tenor']='spot'
        if op['security']['helper']['quote_kind']['kind']=='ContextualIcmaYield':
            op['input']['credit']['cds']['premium_schedule']['end_date']='2025-06-20'
    data['source_reads']={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES}
    data['qualification']='Explicit original canonical spot tenor and separately supplied comparison-CDS maturity within the actual basis horizon. Original cash/quotes/parameters/five-PV/financial answers and bounds are byte-exact.'
    return data

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();b=(json.dumps(build(),indent=2)+'\n').encode()
    if a.write:
        if DEST.exists():raise ValueError('Refusing to overwrite original controls')
        DEST.write_bytes(b)
    elif DEST.read_bytes()!=b:raise ValueError('Declared admission differs')
    print(json.dumps(dict(status='passed',cases=7,certified=False)))
