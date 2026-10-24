"""Validate actual full-context all-path schema and strict disjoint controls."""
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats


def validate(binary):
    raw = subprocess.check_output([binary, '--emit-schema']); schema = json.loads(raw)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=formats())
    cases = json.loads((Path(__file__).resolve().parents[1]/'fixtures/library/icma-workout-qualified.json').read_bytes())['cases']
    positives = [c for c in cases if c['expected_error'] is None]
    for case in positives: validator.validate(case['request'])
    negatives = []
    for action in ['price_paths', 'worst']:
        original = next(c['request'] for c in positives if c['request']['operation']['action']['kind'] == action)
        bounds = ['operation', 'action', 'settings']+(['paths'] if action == 'worst' else [])
        locations = [
            (['operation'], ['input', 'convention', 'context', 'action']),
            (['operation', 'input'], ['security', 'reference_context', 'quotation_policy', 'position']),
            (['operation', 'input', 'reference_context'], ['revision', 'security', 'frequency', 'periods']),
            (['operation', 'convention'], ['schema_version', 'reference_context', 'quotation_policy', 'frequency', 'compounding', 'max_reference_evaluations']),
            (['operation', 'context'], ['reference_date', 'trade_date', 'settlement', 'on_payment', 'notice']),
            (bounds, ['max_dates', 'max_cash_evaluations', 'max_reference_evaluations']),
            (['operation', 'action'], ['settings', 'target' if action == 'worst' else 'decimal_yield']),
        ]
        if action == 'worst':
            locations += [(['operation', 'action', 'settings'], ['inverse', 'paths']),
                          (['operation', 'action', 'settings', 'inverse'], ['lower', 'upper', 'price_tolerance', 'parameter_tolerance', 'max_iterations'])]
        for path, fields in locations:
            for field in fields:
                request = deepcopy(original); node = request
                for key in path: node = node[key]
                del node[field]; negatives.append(request)
        for path, key, value in [(['operation'], 'cash', []), (['operation'], 'cached_yields', []),
                                  (['operation', 'action'], 'selection', {'kind': 'maturity'}),
                                  (bounds, 'default_budget', 100),
                                  (['operation', 'convention'], 'compounding', 'Street')]:
            request = deepcopy(original); node = request
            for element in path: node = node[element]
            node[key] = value; negatives.append(request)
        request = deepcopy(original)
        request['operation']['action']['target' if action == 'price_paths' else 'decimal_yield'] = {'kind': 'dirty', 'value': 100} if action == 'price_paths' else 0.03
        negatives.append(request)
    for request in negatives:
        if validator.is_valid(request): raise ValueError('Schema admitted omitted context or ambiguous all-path controls')
    return dict(status='passed', positive_cases=len(positives), rejected_mutations=len(negatives),
                schema_sha256=hashlib.sha256(raw).hexdigest(), certified=False)


if __name__ == '__main__':
    p=argparse.ArgumentParser();p.add_argument('--binary',required=True)
    print(json.dumps(validate(p.parse_args().binary)))
