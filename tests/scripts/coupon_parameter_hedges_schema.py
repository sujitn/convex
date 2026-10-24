"""Actual full native parameter and complete hedge request schema controls."""
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
    cases = json.loads((ROOT/'tests/fixtures/library/coupon-parameter-hedges.json').read_bytes())['cases']
    positives = [row for row in cases if row['expected_error'] is None]
    for row in positives:
        validator.validate(row['request'])
    mutations = [
        lambda r: r['operation']['native_settings'].pop('absolute_second_tolerance'),
        lambda r: r['operation']['native_settings'].update(currency_per_bp=True),
        lambda r: r['operation']['key']['coordinate'].pop('version'),
        lambda r: r['operation']['key']['coordinate'].update(cached_price=100),
        lambda r: r['operation']['input']['bond']['terms']['profiles']['notional'][0].update(value=5000.01),
        lambda r: r['operation'].update(first_derivative=0),
    ]
    for mutate in mutations:
        request = copy.deepcopy(positives[0]['request'])
        mutate(request)
        if validator.is_valid(request):
            raise ValueError('Parameter-risk schema admitted missing or ignored economics/policy')
    hedge = next(row for row in positives if row['request']['operation']['kind'] == 'term_coupon_complete_hedge_risk')
    for mutate in [
        lambda r: r['operation']['parameter_policies'][0]['settings'].pop('absolute_second_tolerance'),
        lambda r: r['operation']['parameter_policies'][0]['key']['coordinate'].update(cached_value=0),
    ]:
        request = copy.deepcopy(hedge['request'])
        mutate(request)
        if validator.is_valid(request):
            raise ValueError('Full hedge schema admitted missing policy or ignored parameter')
    return dict(status='passed', positive_requests=len(positives), rejected_mutations=len(mutations)+2,
                schema_sha256=hashlib.sha256(raw).hexdigest(), certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
