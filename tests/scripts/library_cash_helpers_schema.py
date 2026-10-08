#!/usr/bin/env python3
"""Validate actual complete cash-helper schema; financial admission stays native."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parents[2]


def validate(binary):
    content = subprocess.check_output([str(Path(binary).resolve()), '--emit-schema'])
    schema = json.loads(content); Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=FormatChecker())
    cases = json.loads((ROOT / 'tests/fixtures/library/cash-helpers.json').read_bytes())['cases']
    for case in cases:
        error = next(validator.iter_errors(case['request']), None)
        if error is not None:
            raise ValueError(f"{case['id']}: original cash schema rejected at {list(error.path)}")
    mutations = [
        lambda r: r['operation']['input']['input']['recipe']['helpers'][0]['input'].pop('quote_version'),
        lambda r: r['operation']['input']['input']['recipe']['helpers'][0]['input'].pop('quote_kind'),
        lambda r: r['operation']['input']['input']['recipe']['helpers'][0]['input']['terms']['profiles']['notional'][0].pop('value'),
        lambda r: r['operation']['input']['input'].pop('calendars'),
        lambda r: r['operation']['input'].update(cached_residual=0.0),
        lambda r: r['operation']['input']['input']['recipe']['helpers'][0]['input']['terms']['profiles']['coupon'][0].update(value=0.05),
    ]
    base = next(c['request'] for c in cases if c['id'] == 'cash_helpers.government.positive')
    for i, mutate in enumerate(mutations):
        request = copy.deepcopy(base); mutate(request)
        if validator.is_valid(request): raise ValueError(f'Ignored original cash input mutation {i}')
    return dict(status='passed', cases=len(cases), rejected_mutations=len(mutations),
                schema_sha256=hashlib.sha256(content).hexdigest(), certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument('--binary', required=True)
    print(json.dumps(validate(parser.parse_args().binary), indent=2))
