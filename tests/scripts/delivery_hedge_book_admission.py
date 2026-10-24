"""Exact f64 cost-ceiling wire type; original numerical value/bounds unchanged."""
import argparse,hashlib,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];HERE=ROOT/'tests/fixtures/library';TARGET=HERE/'delivery-hedge-book-admitted.json'

def generate():
    source=HERE/'delivery-hedge-errors.json';value=json.loads(source.read_bytes())
    for book in value['books']: book['problem']['max_cost']=float(book['problem']['max_cost'])
    value['source_reads']['tests/fixtures/library/delivery-hedge-errors.json']=hashlib.sha256(source.read_bytes()).hexdigest()
    value['qualification']+=' Explicit exact f64 cost-ceiling wire value10.0; original ten-unit limit, financial answers, full solve policy and all scenarios unchanged.'
    return value

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();v=generate();raw=(json.dumps(v,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists(): raise ValueError('Refusing to overwrite declared cost-bound admission')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw: raise ValueError('Full original cost-bound declaration differs')
    print(json.dumps(dict(status='passed',cases=len(v['cases']),books=len(v['books']),certified=False)))
