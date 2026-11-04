"""Generated full native price-inverse schemas preserve target units and policies."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator, FormatChecker

ROOT=Path(__file__).resolve().parents[2]


def validate(binary):
    raw=subprocess.check_output([binary,'--emit-schema'])
    schema=json.loads(raw);Draft202012Validator.check_schema(schema)
    validator=Draft202012Validator(schema,format_checker=FormatChecker())
    cases=json.loads((ROOT/'tests/fixtures/library/coupon-parameter-inverse.json').read_bytes())['cases']
    positives=[case for case in cases if case['expected_error'] is None]
    for case in positives:validator.validate(case['request'])
    mutations=[
        lambda r:r['operation']['settings'].pop('max_price_evaluations'),
        lambda r:r['operation']['settings'].update(cached_parameter=0.01),
        lambda r:r['operation']['target'].update(unit='price_per_100'),
        lambda r:r['operation']['target'].pop('currency'),
        lambda r:r['operation']['coordinate'].pop('source_version'),
        lambda r:r['operation'].update(globally_unique=True),
        lambda r:r['operation']['input']['bond']['terms']['profiles']['notional'][0].update(value=5000.01),
    ]
    for mutate in mutations:
        request=copy.deepcopy(positives[0]['request']);mutate(request)
        if validator.is_valid(request):raise ValueError('Inverse schema admitted missing/ignored economics or target policy')
    return dict(status='passed',positive_requests=len(positives),rejected_mutations=len(mutations),schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--binary',required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
