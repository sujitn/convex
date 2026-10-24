#!/usr/bin/env python3
"""Check actual complete model/side schemas and rejected omissions; no certificate."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator, FormatChecker

ROOT=Path(__file__).resolve().parents[2]


def validate(binary):
    raw=subprocess.check_output([str(Path(binary).resolve()),'--emit-schema'])
    schema=json.loads(raw);Draft202012Validator.check_schema(schema)
    validator=Draft202012Validator(schema,format_checker=FormatChecker())
    cases=json.loads((ROOT/'tests/fixtures/library/monotone.json').read_bytes())['cases']
    for case in cases:
        error=next(validator.iter_errors(case['request']),None)
        if error is not None:raise ValueError(f"{case['id']}: original schema rejects {list(error.path)}")
    source=next(c['request'] for c in cases if c['request']['operation'].get('query',{}).get('kind')=='MonotoneConvexForward')
    mutations=[
        lambda q:q['operation']['query'].pop('side'),
        lambda q:q['operation']['query'].pop('point'),
        lambda q:q['operation']['query'].update(side='Average'),
        lambda q:q['inputs']['curves'][0]['source']['MonotoneConvex'].pop('nodes'),
        lambda q:q['inputs']['curves'][0]['source']['MonotoneConvex'].update(signed_forwards=True),
        lambda q:q['inputs']['curves'][0]['source']['MonotoneConvex'].update(sampled_derivative=True),
        lambda q:q['inputs']['curves'][0]['source']['MonotoneConvex']['nodes'][0].pop('pillar'),
        lambda q:q['inputs']['curves'][0]['source']['MonotoneConvex']['nodes'][0]['quote'].pop('unit'),
        lambda q:q['inputs']['curves'][0]['key'].pop('version'),
        lambda q:q['inputs']['curves'][0].pop('time_basis'),
    ]
    for i,mutate in enumerate(mutations):
        q=copy.deepcopy(source);mutate(q)
        if validator.is_valid(q):raise ValueError(f'Incomplete original model mutation {i} admitted')
    return dict(status='passed',cases=len(cases),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',required=True)
    print(json.dumps(validate(p.parse_args().binary),indent=2))
