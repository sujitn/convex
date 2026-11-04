"""Actual schema admits full common-factor products and rejects missing identities."""
import argparse, hashlib, json, subprocess
from copy import deepcopy
from pathlib import Path
from jsonschema import Draft202012Validator
from curve_replay_schema import formats

def validate(binary):
    raw=subprocess.check_output([binary,'--emit-schema']);schema=json.loads(raw)
    Draft202012Validator.check_schema(schema);validator=Draft202012Validator(schema,format_checker=formats())
    cases=json.loads((Path(__file__).resolve().parents[1]/'fixtures/library/graph-hedge-api.json').read_bytes())['cases']
    positives=[c for c in cases if c['expected_error'] is None]
    for c in positives:validator.validate(c['request'])
    original=positives[0]['request'];negative=[]
    for path in [['operation'],['operation','input'],['operation','input','identity'],['operation','input','reference_position'],['operation','input','factors',0],['operation','input','factors',0,'key'],['operation','input','factors',0,'settings']]:
        def node(value):
            for key in path:value=value[key]
            return value
        for key in node(original):
            r=deepcopy(original);del node(r)[key];negative.append(r)
        r=deepcopy(original);node(r)['cached_column']=True;negative.append(r)
    for r in negative:
        if validator.is_valid(r):raise ValueError('Schema admitted incomplete original graph hedge input')
    return dict(status='passed',positive_cases=len(positives),rejected_mutations=len(negative),schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--binary',required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
