"""Actual schema admits full CDS recovery and issuer cash and rejects incomplete factor manifests."""
import argparse
import hashlib
import json
from copy import deepcopy
from pathlib import Path
import subprocess

from jsonschema import Draft202012Validator
from curve_replay_schema import formats


def validate(binary):
    raw = subprocess.check_output([binary, '--emit-schema'])
    schema = json.loads(raw)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=formats())
    folder = Path(__file__).resolve().parents[1] / 'fixtures/library'
    cases = [c for name in ['recovery-hedge-admitted.json', 'recovery-hedge-boundaries.json']
             for c in json.loads((folder / name).read_bytes())['cases']]
    positives = [c for c in cases if c['expected_error'] is None]
    for case in positives:
        validator.validate(case['request'])
    original = positives[0]['request']
    negatives = []
    paths = [['operation'], ['operation', 'input'],
             ['operation', 'input', 'hedge'], ['operation', 'input', 'hedge', 'identity'],
             ['operation', 'input', 'hedge', 'reference_position'],
             ['operation', 'input', 'hedge', 'factors', 0],
             ['operation', 'input', 'hedge', 'factors', 0, 'key'],
             ['operation', 'input', 'hedge', 'factors', 0, 'settings']]
    for path in paths:
        def node(value):
            for key in path:
                value = value[key]
            return value
        for key in node(original):
            # Reference-only cash may omit its explicitly optional legal mapping.
            if path == ['operation', 'input'] and key == 'credit':
                continue
            changed = deepcopy(original)
            del node(changed)[key]
            negatives.append(changed)
        changed = deepcopy(original)
        node(changed)['cached_column'] = True
        negatives.append(changed)
    for request in negatives:
        if validator.is_valid(request):
            raise ValueError('Schema admitted incomplete CDS recovery and issuer cash/factor input')
    cds = next(c['request'] for c in positives if c['id'] == 'recovery_hedge.book.cds')
    for path, keys in [
        (['operation'], ['graph', 'product', 'input']),
        (['operation', 'product'], ['terms', 'context']),
        (['operation', 'product', 'terms'], ['reference', 'notional', 'coupon_decimal',
            'premium_schedule', 'premium_calendar', 'settlement_calendar', 'survival_curve']),
        (['operation', 'product', 'terms', 'reference'], ['entity_id', 'seniority',
            'document', 'restructuring_clause', 'recovery_fraction']),
        (['operation', 'input'], ['identity', 'reference_position', 'snapshot_id',
            'factors', 'max_factor_evaluations']),
    ]:
        for key in keys:
            changed = deepcopy(cds)
            node = changed
            for component in path:
                node = node[component]
            del node[key]
            if validator.is_valid(changed):
                raise ValueError('Schema admitted incomplete full CDS input: ' + '.'.join(path + [key]))
            negatives.append(changed)
    return dict(status='passed', positive_cases=len(positives),
                rejected_mutations=len(negatives), schema_sha256=hashlib.sha256(raw).hexdigest(),
                certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
