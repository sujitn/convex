"""Validate actual full-contract scenario schemas and reject lost coordinates."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parents[2]


def validate(binary):
    raw = subprocess.check_output([binary, '--emit-schema'])
    schema = json.loads(raw)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=FormatChecker())
    cases = json.loads((ROOT / 'tests/fixtures/library/coupon-model-scenarios.json').read_bytes())['cases']
    positives = [c for c in cases if c['expected_error'] is None]
    for case in positives:
        validator.validate(case['request'])
    mutations = [
        lambda r: r['operation']['coordinate'].pop('source_version'),
        lambda r: r['operation']['coordinate'].update(cached_parameter=0.01),
        lambda r: r['operation']['coordinate']['parameter'].update(factor=0),
        lambda r: r['operation']['coordinate'].update(parameter={'kind': 'hw1f_volatility'}),
        lambda r: r['operation']['input']['bond']['terms']['profiles']['notional'][0].update(value=5000.01),
        lambda r: r['operation'].pop('delta'),
    ]
    for mutate in mutations:
        request = copy.deepcopy(positives[0]['request'])
        mutate(request)
        if validator.is_valid(request):
            raise ValueError('Scenario schema admitted an incomplete or ignored economic coordinate')
    return dict(status='passed', positive_requests=len(positives), rejected_mutations=len(mutations),
                schema_sha256=hashlib.sha256(raw).hexdigest(), certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
