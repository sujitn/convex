"""Actual generated term coupon option schemas, exact terms and rejected ignored fields."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator, FormatChecker

ROOT=Path(__file__).resolve().parents[2]


def validate(binary):
    raw=subprocess.check_output([binary,'--emit-schema']);schema=json.loads(raw)
    Draft202012Validator.check_schema(schema)
    validator=Draft202012Validator(schema,format_checker=FormatChecker())
    cases=json.loads((ROOT/'tests/fixtures/library/term-coupon-options.json').read_bytes())['cases']
    positives=[c for c in cases if c['expected_error'] is None]
    for c in positives:
        validator.validate(c['request'])
    original=positives[0]['request']
    mutations=[
        lambda r:r.update(ignored=True),
        lambda r:r['operation']['input'].update(cached_price=100),
        lambda r:r['operation']['input']['options'].update(whole_coupon=True),
        lambda r:r['operation']['input']['options']['models'][0]['model'].update(kind='assumed'),
        lambda r:r['operation']['input']['options']['models'][0]['model'].update(ignored=True),
        lambda r:r['operation']['input']['bond']['terms']['profiles']['notional'][0].update(value=5000.01),
    ]
    for m in mutations:
        r=copy.deepcopy(original);m(r)
        if validator.is_valid(r): raise ValueError('Schema admitted lost or numeric economics')
    return dict(status='passed',positive_requests=len(positives),rejected_mutations=len(mutations),
                schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True)
    print(json.dumps(validate(p.parse_args().binary)))
