#!/usr/bin/env python3
"""Actual complete model/segment/delegation schema; financial guards are native."""
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
    cases=json.loads((ROOT/'tests/fixtures/library/curve-families.json').read_bytes())['cases']
    for case in cases:
        error=next(validator.iter_errors(case['request']),None)
        if error is not None:raise ValueError(f"{case['id']}: actual full schema rejected {list(error.path)}")
    sample=lambda id:next(c['request'] for c in cases if c['id']==id)
    model=sample('curve.model.observations.ns.positive')
    segment=sample('curve.segment.describe')
    delegate=sample('curve.delegate.Strict.3')
    fit=sample('curve.model.fit.sv.positive')
    mutations=[
        (model,lambda q:q['inputs']['curves'][0]['source']['Parametric'].pop('horizon')),
        (model,lambda q:q['inputs']['curves'][0]['source']['Parametric']['parameters'].pop('tau')),
        (model,lambda q:q['inputs']['curves'][0]['source']['Parametric']['observations'][0].pop('version')),
        (model,lambda q:q['inputs']['curves'][0]['source']['Parametric'].update(sampled_grid=[])),
        (segment,lambda q:q['inputs']['curves'][0]['source']['Segmented'].pop('join_tolerance')),
        (segment,lambda q:q['inputs']['curves'][0]['source']['Segmented']['segments'][0].pop('fallback')),
        (segment,lambda q:q['inputs']['curves'][0]['source']['Segmented']['segments'][0]['base'].pop('version')),
        (delegate,lambda q:q['inputs']['curves'][0]['source']['Delegated']['range'].pop('end')),
        (delegate,lambda q:q['inputs']['curves'][0]['source']['Delegated'].update(fallback='Default')),
        (fit,lambda q:q['operation']['recipe']['source']['Parametric']['parameters'].pop('tau2')),
        (fit,lambda q:q['operation']['settings'].pop('controls')),
        (fit,lambda q:q['operation']['settings']['controls'].pop('minimum_singular_ratio')),
    ]
    for i,(source,mutate) in enumerate(mutations):
        q=copy.deepcopy(source);mutate(q)
        if validator.is_valid(q):raise ValueError(f'Incomplete family mutation {i} admitted')
    return dict(status='passed',cases=len(cases),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(content).hexdigest(),certified=False)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',required=True)
    print(json.dumps(validate(p.parse_args().binary),indent=2))
