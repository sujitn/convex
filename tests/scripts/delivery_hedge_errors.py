"""Exact category declaration and independently missing exact curve control."""
import argparse,copy,hashlib,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];HERE=ROOT/'tests/fixtures/library';TARGET=HERE/'delivery-hedge-errors.json'

def generate():
    source=HERE/'delivery-hedge-final.json';value=json.loads(source.read_bytes())
    for case in value['cases']:
        if case['id']=='delivery.invalid.missing_discount': case['expected_error']='invalid_input'
    case=copy.deepcopy(value['cases'][0]);case['id']='delivery.invalid.actual_missing_discount'
    case['request']['operation']['action']['product']['discount']['curve']['version']='missing-exact-revision'
    case['expected_error']='missing_input';case.pop('expected',None);value['cases'].append(case)
    value['source_reads']['tests/fixtures/library/delivery-hedge-final.json']=hashlib.sha256(source.read_bytes()).hexdigest()
    value['qualification']+=' Raw preparation shared a curve-key object: its missing-discount trial actually changed the supplied recipe revision too and correctly fails source consistency. A separate exact missing dependency trial preserves the original supplied curve. Financial answers remain exact.'
    return value

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();v=generate();raw=(json.dumps(v,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists(): raise ValueError('Refusing to overwrite original error category declaration')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw: raise ValueError('Full original declaration differs')
    print(json.dumps(dict(status='passed',cases=len(v['cases']),certified=False)))
