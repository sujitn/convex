"""Separately declared strict identifier/FX wire admission. All financial controls
and numerical bounds are copied exactly; the original raw requests stay immutable.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
HERE=ROOT/'tests/fixtures/library'
TARGET=HERE/'delivery-hedge-ready.json'

def wire(value):
    if isinstance(value,dict):
        if value.get('source')=='independent engineering control': value['source']='independent-engineering-control'
        for v in value.values(): wire(v)
    elif isinstance(value,list):
        for v in value: wire(v)
    return value

def generate():
    sources=['delivery-hedge-reference.json','delivery-hedge-qualified.json','delivery-hedge-admission.json','delivery-hedge-book.json']
    controls=[];books=[]
    for name in sources:
        original=json.loads((HERE/name).read_bytes())
        if name.endswith('book.json'):
            books=wire(copy.deepcopy(original['cases']))
            for book in books:
                if book['fx']: book['fx']['observation']=book['fx'].pop('source')
        else: controls.extend(wire(copy.deepcopy(original['cases'])))
    return dict(certified=False,qualification='Strict ASCII SourceId and exact original FX observation field only. Every original numeric financial answer, formula, domain, quantity, convention and comparison bound is exact. Original failed wire inputs and requests remain immutable.',
        source_reads={'tests/fixtures/library/'+p:hashlib.sha256((HERE/p).read_bytes()).hexdigest() for p in sources},cases=controls,books=books)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();value=generate();raw=(json.dumps(value,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists(): raise ValueError('Refusing to overwrite declared wire admission')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw: raise ValueError('Original complete wire admission differs')
    print(json.dumps(dict(status='passed',cases=len(value['cases']),books=len(value['books']),certified=False)))
