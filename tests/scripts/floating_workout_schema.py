"""Check actual emitted complete floating-workout input schema; no finance oracle."""
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats

def validate(binary):
    raw=subprocess.check_output([binary,'--emit-schema']);schema=json.loads(raw)
    Draft202012Validator.check_schema(schema)
    validator=Draft202012Validator(schema,format_checker=formats())
    cases=json.loads((Path(__file__).resolve().parents[1]/'fixtures/library/floating-workout-qualified.json').read_bytes())['cases']
    positives=[c for c in cases if c['expected_error'] is None]
    for c in positives:validator.validate(c['request'])
    original=next(c['request'] for c in positives if c['id']=='floating.shifted.price');negative=[]
    for location,fields in [(['operation'],['context','input','action']),
        (['operation','input'],['security','projection_context','projection','discount','position','quotation_policy']),
        (['operation','input','security'],['bond','reference','quote_revision']),
        (['operation','input','projection_context'],['fixing','settlement','on_payment','overnight_accrual_cutoffs']),
        (['operation','action','selection'],['path','weight_end']),
        (['operation','input','position'],['quantity','reference_quantity','quantity_unit','currency'])]:
        for field in fields:
            r=deepcopy(original);node=r
            for key in location:node=node[key]
            del node[field];negative.append(r)
    for field in ['cap','floor','make_whole','trigger']:
        r=deepcopy(original);r['operation']['input']['security']['bond']['terms']['coupon'][field]=0.05;negative.append(r)
    for field,value in [('decimal_dm','6.25bp'),('kind','worst'),('cached_cash',[]),('convention',{'automatic':True})]:
        r=deepcopy(original);r['operation']['action'][field]=value;negative.append(r)
    for r in negative:
        if validator.is_valid(r):raise ValueError('Schema admitted missing/lost/replaced floating workout economics')
    return dict(status='passed',positive_cases=len(positives),rejected_mutations=len(negative),schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True);print(json.dumps(validate(p.parse_args().binary)))
