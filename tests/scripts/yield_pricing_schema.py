"""Generated supplied-cash schema rejects omitted/ignored basis and settings."""
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
    cases = json.loads((ROOT/'tests/fixtures/library/yield-pricing.json').read_bytes())['cases']
    positives = [c for c in cases if c['expected_error'] is None]
    for case in positives:
        validator.validate(case['request'])
    mutations = [lambda a:a.pop('cash'), lambda a:a.pop('compounding'),
        lambda a:a.pop('annual_yield'), lambda a:a.update(unit='per100'),
        lambda a:a.update(kind='market_default'), lambda a:a.update(cash=[[1,100,5]]),
        lambda a:a['compounding'].update(unused_calendar='WEEKEND')]
    for mutation in mutations:
        request=copy.deepcopy(positives[0]['request']);mutation(request['operation']['action'])
        if validator.is_valid(request):raise ValueError('Schema admitted ignored/missing supplied-cash input')
    inverse=next(c for c in positives if c['request']['operation']['action']['kind']=='solve')
    for field in ['initial_guess','tolerance','max_iterations','target_dirty_price']:
        request=copy.deepcopy(inverse['request']);request['operation']['action'].pop(field)
        if validator.is_valid(request):raise ValueError('Schema omitted required inverse settings')
    return dict(status='passed',positive_requests=len(positives),rejected_mutations=len(mutations)+4,
        schema_sha256=hashlib.sha256(raw).hexdigest(),certified=False)

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--binary',required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
