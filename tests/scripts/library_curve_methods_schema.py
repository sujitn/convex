#!/usr/bin/env python3
"""Validate actual generated method/one-sided forward schemas; no certificate."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator, FormatChecker

ROOT=Path(__file__).resolve().parents[2]


def validate(binary):
    content=subprocess.check_output([str(Path(binary).resolve()),'--emit-schema'])
    schema=json.loads(content);Draft202012Validator.check_schema(schema)
    validator=Draft202012Validator(schema,format_checker=FormatChecker())
    cases=json.loads((ROOT/'tests/fixtures/library/curve-methods.json').read_bytes())['cases']
    for case in cases:
        error=next(validator.iter_errors(case['request']),None)
        if error is not None:raise ValueError(f"{case['id']}: actual full schema rejected {list(error.path)}")
    source=next(c['request'] for c in cases if c['id']=='method.forward.Left.FlatForward.positive.2')
    mutations=[
        lambda q:q['operation']['query'].pop('side'),
        lambda q:q['operation']['query'].update(side='Average'),
        lambda q:q['inputs']['curves'][0]['source']['FlatForward'].pop('nodes'),
        lambda q:q['inputs']['curves'][0]['source']['FlatForward'].update(linear_proxy=True),
        lambda q:q['inputs']['curves'][0]['source']['FlatForward']['nodes'][0].pop('pillar'),
        lambda q:q['inputs']['curves'][0]['source']['FlatForward']['nodes'][0]['quote'].pop('unit'),
        lambda q:q['inputs']['curves'][0].pop('extrapolation'),
        lambda q:q['inputs']['curves'][0]['key'].pop('version'),
    ]
    for i,mutate in enumerate(mutations):
        q=copy.deepcopy(source);mutate(q)
        if validator.is_valid(q):raise ValueError(f'Incomplete method mutation {i} admitted')
    return dict(status='passed',cases=len(cases),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(content).hexdigest(),certified=False)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',required=True)
    print(json.dumps(validate(p.parse_args().binary),indent=2))
