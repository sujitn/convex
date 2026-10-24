"""Generated scalar schemas preserve explicit units, exact Decimals and fields."""
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
    cases = json.loads((ROOT/'tests/fixtures/library/yield-scalars.json').read_bytes())['cases']
    positives = [c for c in cases if c['expected_error'] is None]
    for case in positives:
        validator.validate(case['request'])
    mutations = [
        lambda a:a.pop('annual_coupon'),
        lambda a:a.pop('clean_price'),
        lambda a:a.update(unit='percent'),
        lambda a:a.update(kind='street_yield'),
        lambda a:a.update(annual_coupon='NaN'),
    ]
    for mutation in mutations:
        request = copy.deepcopy(positives[0]['request'])
        mutation(request['operation']['action'])
        if validator.is_valid(request):
            raise ValueError('Schema admitted ignored/missing scalar basis or unknown operation')
    exact = next(c for c in positives if c['request']['operation']['action']['kind'] == 'fixed_current_percent')
    for field in ['annual_rate','original_face','clean_price_per100']:
        request = copy.deepcopy(exact['request'])
        request['operation']['action'][field] = 100
        if validator.is_valid(request):
            raise ValueError('Schema admitted lossy numeric Decimal input')
    return dict(status='passed', positive_requests=len(positives), rejected_mutations=len(mutations)+3,
                schema_sha256=hashlib.sha256(raw).hexdigest(), certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
