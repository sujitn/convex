#!/usr/bin/env python3
"""Validate actual full CDS-helper schema; financial admission remains native."""
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
    schema = json.loads(content)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=FormatChecker())
    cases = json.loads((ROOT / 'tests/fixtures/library/credit-helpers.json').read_bytes())['cases']
    for case in cases:
        error = next(validator.iter_errors(case['request']), None)
        if error is not None:
            raise ValueError(f"{case['id']}: schema rejected original CDS terms at {list(error.path)}")
    mutations = [
        lambda r: r['operation']['input']['recipe']['helpers'][0]['terms'].update(notional=10000000),
        lambda r: r['operation']['input']['recipe']['helpers'][0].pop('quote_version'),
        lambda r: r['operation']['input']['recipe']['helpers'][0]['terms']['reference'].pop('document'),
        lambda r: r['operation']['input']['recipe']['helpers'][0]['context'].pop('initial_payment_state'),
        lambda r: r['operation']['input']['recipe']['helpers'][0].pop('quote_kind'),
        lambda r: r['operation']['input'].update(cached_residual=0.0),
    ]
    for i, mutate in enumerate(mutations):
        request = copy.deepcopy(cases[0]['request'])
        mutate(request)
        if validator.is_valid(request):
            raise ValueError(f'Schema discarded original CDS economics: mutation {i}')
    return dict(status='passed', cases=len(cases), rejected_mutations=len(mutations),
                schema_sha256=hashlib.sha256(content).hexdigest(), certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    print(json.dumps(validate(parser.parse_args().binary), indent=2))
