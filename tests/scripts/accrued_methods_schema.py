"""Actual generated schema preserves the complete supplied accrual/calendar inputs."""
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
    cases = json.loads((ROOT/'tests/fixtures/library/accrued-methods.json').read_bytes())['cases']
    positives = [c for c in cases if c['expected_error'] is None]
    for case in positives:
        validator.validate(case['request'])
    mutations = []
    for kind, fields in {
        'standard':['settlement','period_start','period_end','annual_rate','original_face','day_count','frequency'],
        'ex_dividend':['calendar','ex_dividend_days'],
        'irregular_period':['reference_start','reference_end','period_end'],
        'year_fraction':['annual_rate'],
    }.items():
        original=next(c['request'] for c in positives if c['request']['operation']['action']['kind']==kind)
        for field in fields:
            request=copy.deepcopy(original);request['operation']['action'].pop(field);mutations.append(request)
    for change in [{'unit':'per100'}, {'kind':'market_default'}, {'annual_rate':0.05}, {'original_face':100}]:
        request=copy.deepcopy(positives[0]['request']);request['operation']['action'].update(change);mutations.append(request)
    for request in mutations:
        if validator.is_valid(request):raise ValueError('Schema admitted ignored/missing accrual input')
    return dict(status='passed',positive_requests=len(positives),rejected_mutations=len(mutations),
        schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--binary',required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
