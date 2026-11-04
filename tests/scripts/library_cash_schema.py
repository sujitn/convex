#!/usr/bin/env python3
"""Validate actual generated full cash schemas and reject discarded terms."""
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
    corpus=json.loads((ROOT/'tests/fixtures/library/cash.json').read_bytes())['cases']
    requests={c['id']:c['request'] for c in corpus}
    for case in corpus:
        error=next(validator.iter_errors(case['request']),None)
        if error is not None:raise ValueError(f"{case['id']}: schema rejected typed request at {list(error.path)}")
    mutations=(
        ('cash.invoice.long',lambda r:r['operation']['action']['position'].update(quantity=15000.0)),
        ('cash.quote.USD.CashflowYield',lambda r:r['operation']['convention'].update(inferred_street=True)),
        ('cash.quote.USD.CashflowYield',lambda r:r['operation']['settings'].pop('parameter_tolerance')),
        ('government.linear',lambda r:r['operation']['benchmarks'][0]['quote'].update(cached_yield=0.03)),
        ('government.linear',lambda r:r['operation']['settings'].pop('selection')),
        ('government.linear',lambda r:r['operation']['benchmarks'][0]['quote']['security'].pop('version')),
    )
    for identity,mutate in mutations:
        candidate=copy.deepcopy(requests[identity]);mutate(candidate)
        if validator.is_valid(candidate):raise ValueError('Schema discarded an economic term: '+identity)
    return dict(status='passed',cases=len(corpus),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(content).hexdigest(),certified=False)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--binary',required=True)
    print(json.dumps(validate(parser.parse_args().binary),indent=2))
