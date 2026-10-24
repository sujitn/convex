#!/usr/bin/env python3
"""Actual full workout schema; native legal/economic admission remains separate."""
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
    cases=json.loads((ROOT/'tests/fixtures/library/workouts.json').read_bytes())['cases']
    for case in cases:
        error=next(validator.iter_errors(case['request']),None)
        if error is not None:raise ValueError(f"{case['id']}: full schema rejected {list(error.path)}")
    base=next(c['request'] for c in cases if c['id']=='workout.price.SemiAnnual.0.04125.exercise.0')
    mutations=[
        lambda q:q['operation']['context'].pop('notice'),
        lambda q:q['operation']['context'].pop('trade_date'),
        lambda q:q['operation']['input'].pop('quote_revision'),
        lambda q:q['operation']['input']['bond']['terms']['exercise'][0].pop('notice_offset'),
        lambda q:q['operation']['input']['bond']['terms']['profiles']['notional'][0].pop('value'),
        lambda q:q['operation']['input']['bond']['terms']['exercise'][0].update(price_per_100=101.25),
        lambda q:q['operation']['input'].update(cached_yield=0.04),
    ]
    for i,mutate in enumerate(mutations):
        q=copy.deepcopy(base);mutate(q)
        if validator.is_valid(q):raise ValueError(f'Incomplete workout mutation {i} admitted')
    return dict(status='passed',cases=len(cases),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(content).hexdigest(),certified=False)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',required=True)
    print(json.dumps(validate(p.parse_args().binary),indent=2))
