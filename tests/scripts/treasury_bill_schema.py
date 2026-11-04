"""Actual generated Treasury schemas, exact quotes and rejected ignored fields."""
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
    cases=json.loads((ROOT/'tests/fixtures/library/treasury-bill-admitted.json').read_bytes())['cases']
    positives=[c for c in cases if c['expected_error'] is None]
    for c in positives:
        validator.validate(c['request'])
    original=positives[0]['request']
    mutations=[
        lambda r:r.update(ignored=True),
        lambda r:r['operation']['input'].update(cached_yield='0.04'),
        lambda r:r['operation']['action'].update(ignored=True),
        lambda r:r['operation']['action']['quote'].update(ignored=True),
        lambda r:r['operation']['action']['quote'].update(price_original_face=98.0975),
        lambda r:r['operation']['action']['quote'].update(kind='street'),
        lambda r:r['operation']['action'].pop('quote'),
    ]
    for m in mutations:
        r=copy.deepcopy(original);m(r)
        if validator.is_valid(r): raise ValueError('Schema admitted lost or numeric economics')
    return dict(status='passed',positive_requests=len(positives),rejected_mutations=len(mutations),
                schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True)
    print(json.dumps(validate(p.parse_args().binary)))
