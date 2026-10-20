"""Complete predeclared transport/category and book protocol qualification."""
import argparse
import hashlib
import json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];HERE=ROOT/'tests/fixtures/library'
TARGET=HERE/'delivery-hedge-final.json'

def generate():
    source=HERE/'delivery-hedge-ready.json';value=json.loads(source.read_bytes())
    for case in value['cases']:
        if case['id']=='delivery.invalid.wrong_delivery_calendar': case['expected_error']='missing_input'
    value['source_reads']['tests/fixtures/library/delivery-hedge-ready.json']=hashlib.sha256(source.read_bytes()).hexdigest()
    value['qualification']+=' Wrong calendar revision is a missing resolved input, preserving the original native category; no numerical result changes.'
    value['book_protocol']=[dict(name=name,error=error,status=status) for name,error,status in [
        ('missing_holding','missing_input',None),('duplicate_factor','invalid_input',None),
        ('factor_revision','invalid_input',None),('wrong_reference','invalid_input',None),
        ('wrong_pending_snapshot','invalid_input',None),('missing_lot','invalid_input',None),
        ('ineligible','invalid_input',None),('liquidity',None,'infeasible'),('cost_budget',None,'infeasible'),
        ('missing_fx','missing_input',None),('stale_fx','invalid_input',None)]]
    return value

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();v=generate();raw=(json.dumps(v,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists(): raise ValueError('Refusing to overwrite protocol qualification')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw: raise ValueError('Original protocol qualification differs')
    print(json.dumps(dict(status='passed',cases=len(v['cases']),books=len(v['books']),negative_books=len(v['book_protocol']),certified=False)))
