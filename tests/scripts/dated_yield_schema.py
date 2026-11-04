"""Validate actual generated dated-cash schemas and reject ignored context."""
import argparse, copy, hashlib, json, subprocess
from pathlib import Path
from jsonschema import Draft202012Validator, FormatChecker
ROOT=Path(__file__).resolve().parents[2]
def validate(binary):
    raw=subprocess.check_output([binary,'--emit-schema']);schema=json.loads(raw)
    Draft202012Validator.check_schema(schema)
    validator=Draft202012Validator(schema,format_checker=FormatChecker())
    cases=json.loads((ROOT/'tests/fixtures/library/dated-yield.json').read_bytes())['cases']
    positives=[c for c in cases if c['expected_error'] is None]
    for c in positives:validator.validate(c['request'])
    mutations=[]
    for kind in ['dated_price','dated_solve','dated_risk']:
        original=next(c['request'] for c in positives if c['request']['operation']['action']['kind']==kind)
        for field in original['operation']['action']:
            if field=='kind':continue
            r=copy.deepcopy(original);r['operation']['action'].pop(field);mutations.append(r)
    for kind in ['calendar','icma','act365_l','thirty360_e_isda']:
        original=next(c['request'] for c in positives if c['request']['operation']['action']['basis']['kind']==kind)
        for field in original['operation']['action']['basis']:
            if field=='kind':continue
            r=copy.deepcopy(original);r['operation']['action']['basis'].pop(field);mutations.append(r)
        r=copy.deepcopy(original);r['operation']['action']['basis']['market']='inferred';mutations.append(r)
    original=next(c['request'] for c in positives if c['request']['operation']['action']['basis']['kind']=='icma')
    for field in original['operation']['action']['basis']['schedule']:
        if field in ['first_regular_date','penultimate_date']:continue
        r=copy.deepcopy(original);r['operation']['action']['basis']['schedule'].pop(field);mutations.append(r)
    for field in ['first_regular_date','penultimate_date']:
        r=copy.deepcopy(original);r['operation']['action']['basis']['schedule'].pop(field)
        validator.validate(r) # Optional original anchors remain genuine None.
    for change in [dict(amount=5),dict(amount='5e0'),dict(paid=True)]:
        r=copy.deepcopy(original);r['operation']['action']['cash'][0].update(change);mutations.append(r)
    for r in mutations:
        if validator.is_valid(r):raise ValueError('Schema admitted missing/ignored supplied context')
    return dict(status='passed',positive_requests=len(positives),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True)
    print(json.dumps(validate(p.parse_args().binary)))
