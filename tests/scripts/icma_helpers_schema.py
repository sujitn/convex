"""Actual request schema: complete helper sources and disjoint original controls."""
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats


def node(value, path):
    for key in path:
        value = value[key]
    return value


def validate(binary):
    raw = subprocess.check_output([binary, '--emit-schema'])
    schema = json.loads(raw)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=formats())
    cases = json.loads((Path(__file__).resolve().parents[1] / 'fixtures/library/icma-helpers-final-api.json').read_bytes())['cases']
    positives = [c for c in cases if c['expected_error'] is None]
    for case in positives:
        validator.validate(case['request'])
    negatives = []
    for kind in ['cash_quote', 'cash_reprice', 'build', 'scenario', 'product_risk']:
        def selected(c):
            op = c['request']['operation']
            return (op.get('input', {}).get('kind') == kind if op['kind'] == 'helper' else op['action']['kind'] == kind)
        original = next(c['request'] for c in positives if selected(c))
        if kind == 'cash_quote':
            path = ['operation', 'input', 'helper']
        elif kind == 'cash_reprice':
            path = ['operation', 'input', 'input', 'recipe', 'helpers', 0, 'input']
        else:
            path = ['operation', 'input', 'cash', 0, 'recipe', 'helpers', 0, 'input']
        contextual = path + ['quote_kind', 'input']
        locations = [contextual, contextual + ['sources'], contextual + ['reference_context'], contextual + ['convention'], contextual + ['inverse'], contextual + ['position']]
        locations += [contextual + ['sources', name] for name in ['security', 'quote_revision', 'quotation_policy', 'calendars']]
        locations += [contextual + ['sources', 'calendars', name] for name in ['payment', 'reset', 'observation', 'exercise']]
        for location in locations:
            for field in node(original, location):
                changed = deepcopy(original)
                del node(changed, location)[field]
                negatives.append(changed)
            changed = deepcopy(original)
            node(changed, location)['inferred_context'] = True
            negatives.append(changed)
        for name, value in [('compounding', 'Street'), ('frequency', 'Zero')]:
            changed = deepcopy(original)
            node(changed, contextual + ['convention'])[name] = value
            # Zero frequency is admitted as a type but rejected by the contextual
            # native owner. Schema validation only tests undeclared enum values.
            if name == 'compounding':
                negatives.append(changed)
        changed = deepcopy(original)
        node(changed, path)['quote_kind'] = {'kind': 'CashflowYield', 'input': node(original, contextual)}
        negatives.append(changed)
    for request in negatives:
        if validator.is_valid(request):
            raise ValueError('Schema admitted omitted context or ambiguous helper controls')
    return dict(status='passed', positive_cases=len(positives), rejected_mutations=len(negatives), schema_sha256=hashlib.sha256(raw).hexdigest(), certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
