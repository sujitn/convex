"""Exercise actual generated supplied-cash schemas without inferring market terms."""
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
    cases = json.loads((ROOT/'tests/fixtures/library/periodic-growth.json').read_bytes())['cases']
    positives = [c for c in cases if c['expected_error'] is None]
    for case in positives:
        validator.validate(case['request'])
    mutations = []
    def action(request):
        return request['operation']['action']
    for kind, fields in {
        'project': ['terms', 'cash'],
        'growth': ['annual_rate', 'frequency', 'start', 'end'],
    }.items():
        original = next(c['request'] for c in positives if action(c['request'])['kind'] == kind)
        for field in fields:
            request = copy.deepcopy(original)
            action(request).pop(field)
            mutations.append(request)
    project = next(c['request'] for c in positives if action(c['request'])['kind'] == 'project')
    for field in action(project)['terms']:
        request = copy.deepcopy(project)
        action(request)['terms'].pop(field)
        mutations.append(request)
    for field in ['date', 'amount']:
        request = copy.deepcopy(project)
        action(request)['cash'][0].pop(field)
        mutations.append(request)
    for target, change in [
        ('action', {'market': 'default'}),
        ('terms', {'provider': 'inferred'}),
        ('terms', {'valuation_rate': 0.05}),
        ('terms', {'initial_cash_value': 100}),
        ('cash', {'amount': 2.5}),
        ('cash', {'paid': True}),
    ]:
        request = copy.deepcopy(project)
        owner = action(request)
        if target == 'terms': owner = owner['terms']
        if target == 'cash': owner = owner['cash'][0]
        owner.update(change)
        mutations.append(request)
    for request in mutations:
        if validator.is_valid(request):
            raise ValueError('Schema admitted omitted/ignored native projection inputs')
    return dict(status='passed', positive_requests=len(positives), rejected_mutations=len(mutations),
                schema_sha256=hashlib.sha256(raw).hexdigest(), certified=False)

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
