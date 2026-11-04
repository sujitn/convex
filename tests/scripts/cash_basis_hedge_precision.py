"""Separately declared stricter new helper inputs; no financial answer/bound changes.

Raw original rebuild cases retain their original native helper tolerance1e-8.
That permits a per100 residual whose original-face PV can exceed the independent
PV bound1e-8. The new precision declaration requires native quote1e-12 and, for
contextual helpers, price1e-10/parameter1e-14. Original domains/iterations and
all raw files/independent answers/bounds/defaults remain unchanged.
"""
import argparse
from copy import deepcopy
import hashlib,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
SOURCES=['tests/fixtures/library/cash-basis-hedge-admitted.json','tests/fixtures/library/cash-basis-hedge-rebuild-reference.json']
DEST=ROOT/'tests/fixtures/library/cash-basis-hedge-precise.json'
def build():
    data=deepcopy(json.loads((ROOT/SOURCES[0]).read_bytes()))
    for c in data['cases']:
        op=c['request']['operation']
        if not op['graph']['cash']:continue
        for h in [op['security']['helper'],op['graph']['cash'][0]['recipe']['helpers'][0]['input']]:
            h['tolerance']=1e-12
            if h['quote_kind']['kind']=='ContextualIcmaYield':h['quote_kind']['input']['inverse'].update(price_tolerance=1e-10,parameter_tolerance=1e-14)
    data['source_reads']={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES}
    data['qualification']='Separate stricter declared new calibration inputs qualify the same original PV/risk/quote/cash answers at unchanged independent bounds. Raw cases/failures/tolerances stay unqualified. No original financial source/default or existing numeric control changes.'
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();b=(json.dumps(build(),indent=2)+'\n').encode()
    if a.write:
        if DEST.exists():raise ValueError('Refusing to overwrite controls')
        DEST.write_bytes(b)
    elif DEST.read_bytes()!=b:raise ValueError('Declared precision differs')
    print(json.dumps(dict(status='passed',cases=7,certified=False)))
