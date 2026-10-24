"""Lossless ExactDecimal FX spelling; no financial answer or bound changes."""
import argparse,hashlib,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];HERE=ROOT/'tests/fixtures/library';TARGET=HERE/'delivery-hedge-decimal.json'
def generate():
    source=HERE/'delivery-hedge-book-admitted.json';value=json.loads(source.read_bytes())
    for book in value['books']:
        if book['fx']: book['fx']['rate']='0.9'
    value['source_reads']['tests/fixtures/library/delivery-hedge-book-admitted.json']=hashlib.sha256(source.read_bytes()).hexdigest()
    value['qualification']+=' ExactDecimal canonical FX spelling0.9 for the unchanged supplied nine-tenths monetary rate; all original financial answers and limits unchanged.'
    return value
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();v=generate();raw=(json.dumps(v,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists(): raise ValueError('Refusing to overwrite decimal wire admission')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw: raise ValueError('Declared full decimal admission differs')
    print(json.dumps(dict(status='passed',cases=len(v['cases']),books=len(v['books']),certified=False)))
