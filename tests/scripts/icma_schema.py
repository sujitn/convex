"""Validate the actual emitted contextual ICMA schema; no finance oracle."""
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
    cases=json.loads((Path(__file__).resolve().parents[1]/'fixtures/library/icma-qualified.json').read_bytes())['cases']
    positives=[c for c in cases if c['expected_error'] is None]
    for c in positives:validator.validate(c['request'])
    original=positives[0]['request'];negative=[]
    for location,fields in [(['operation'],['input','convention','cash','action']),
        (['operation','input'],['security','reference_context','quotation_policy','position']),
        (['operation','input','reference_context'],['revision','security','frequency','periods']),
        (['operation','input','reference_context','periods',0],['start','end']),
        (['operation','convention'],['schema_version','reference_context','quotation_policy','frequency','compounding','max_reference_evaluations']),
        (['operation','cash'],['settlement','on_payment']),
        (['operation','input','position'],['quantity','reference_quantity','quantity_unit','currency'])]:
        for field in fields:
            r=deepcopy(original);node=r
            for key in location:node=node[key]
            del node[field];negative.append(r)
    for location,field,value in [(['operation','convention'],'compounding','Street'),
        (['operation','input','reference_context'],'frequency','Weekly'),
        (['operation','action'],'kind','oas'), (['operation','cash'],'cached_cash',[]),
        (['operation','input','reference_context','periods',0],'adjusted_payment','2024-03-11')]:
        r=deepcopy(original);node=r
        for key in location:node=node[key]
        node[field]=value;negative.append(r)
    for r in negative:
        if validator.is_valid(r):raise ValueError('Schema admitted lost/replaced contextual ICMA economics')
    return dict(status='passed',positive_cases=len(positives),rejected_mutations=len(negative),schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True);print(json.dumps(validate(p.parse_args().binary)))
