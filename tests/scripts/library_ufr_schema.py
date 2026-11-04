#!/usr/bin/env python3
"""Validate actual generated full UFR input schema and rejected omissions."""
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
    cases=json.loads((ROOT/'tests/fixtures/library/ufr.json').read_bytes())['cases']
    for case in cases:
        error=next(validator.iter_errors(case['request']),None)
        if error is not None:raise ValueError(f"{case['id']}: actual schema rejected {list(error.path)}")
    source=next(c['request'] for c in cases if c['id']=='ufr.describe.positive')
    mutations=[
        lambda q:q['inputs']['curves'][0]['source']['UfrConvergence'].pop('base'),
        lambda q:q['inputs']['curves'][0]['source']['UfrConvergence'].pop('last_liquid_point'),
        lambda q:q['inputs']['curves'][0]['source']['UfrConvergence']['base'].pop('version'),
        lambda q:q['inputs']['curves'][0]['source']['UfrConvergence'].update(boundary_forward=0.03),
        lambda q:q['inputs']['curves'][0]['source']['UfrConvergence'].update(smith_wilson=True),
        lambda q:q['inputs']['curves'][0]['extrapolation']['UfrConvergence'].pop('ufr'),
        lambda q:q['inputs']['curves'][0]['extrapolation']['UfrConvergence'].pop('alpha'),
        lambda q:q['inputs']['curves'][0].pop('time_basis'),
        lambda q:q['inputs']['curves'][1]['source']['FlatForward'].pop('nodes'),
    ]
    for i,mutate in enumerate(mutations):
        q=copy.deepcopy(source);mutate(q)
        if validator.is_valid(q):raise ValueError(f'Incomplete UFR mutation {i} admitted')
    q=copy.deepcopy(next(c['request'] for c in cases if c['id']=='ufr.UfrForward.positive.20'))
    q['operation']['query'].pop('point')
    if validator.is_valid(q):raise ValueError('UFR forward without actual point admitted')
    return dict(status='passed',cases=len(cases),rejected_mutations=len(mutations)+1,
                schema_sha256=hashlib.sha256(content).hexdigest(),certified=False)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',required=True)
    print(json.dumps(validate(p.parse_args().binary),indent=2))
