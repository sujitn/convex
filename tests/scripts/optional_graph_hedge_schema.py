"""Actual request schema for complete guarded optional/model/hedge composition."""
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess

from jsonschema import Draft202012Validator
from curve_replay_schema import formats


def validate(binary):
    raw = subprocess.check_output([binary, '--emit-schema'])
    schema = json.loads(raw); Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=formats())
    folder = Path(__file__).resolve().parents[1] / 'fixtures/library'
    cases = [c for name in ['optional-graph-hedge-resolved.json',
        'optional-graph-hedge-direct.json', 'optional-graph-hedge-qualification.json', 'optional-graph-hedge-calibrated.json']
        for c in json.loads((folder / name).read_bytes())['cases']]
    cases = [c for c in cases if c['id'] != 'optional_graph.qualification.calibrated']
    positives = [c for c in cases if c['expected_error'] is None]
    for c in positives: validator.validate(c['request'])
    negatives = []

    def reject(original, path, keys):
        def node(r):
            for k in path: r = r[k]
            return r
        for k in keys:
            r = deepcopy(original); del node(r)[k]; negatives.append(r)
        r = deepcopy(original); node(r)['cached_risk'] = 0; negatives.append(r)

    original = next(c['request'] for c in positives if c['id'] == 'optional_graph.frn.call')
    reject(original, ['operation'], ['graph', 'product', 'input'])
    reject(original, ['operation', 'product'], ['input', 'instrument', 'oas_decimal', 'policies'])
    reject(original, ['operation', 'product', 'policies'], ['rates', 'model'])
    for owner, keys in [('rates', ['absolute_duration_tolerance', 'relative_duration_tolerance',
        'absolute_convexity_tolerance', 'relative_convexity_tolerance']),
        ('model', ['absolute_first_tolerance', 'relative_first_tolerance',
        'absolute_second_tolerance', 'relative_second_tolerance'])]:
        reject(original, ['operation', 'product', 'policies', owner], keys)
    reject(original, ['operation', 'product', 'input'], ['bond', 'context', 'discount', 'model',
        'projection_context', 'settings'])
    reject(original, ['operation', 'input'], ['identity', 'reference_position',
        'snapshot_id', 'factors', 'max_factor_evaluations'])
    for i in [0, 2, 4, 5]:
        reject(original, ['operation', 'input', 'factors', i], ['key', 'settings'])
    reject(original, ['operation', 'input', 'factors', 2, 'key', 'factor', 'target'],
        ['discount_curve_id', 'discount_curve_version', 'projection_curve_id', 'projection_curve_version'])
    reject(original, ['operation', 'input', 'factors', 4, 'key', 'factor', 'target'],
        ['model_id', 'model_version'])
    for action, keys in [('model_scenario', ['oas_decimal', 'parameter', 'delta']),
        ('model_risk', ['oas_decimal', 'parameter', 'bump', 'settings'])]:
        r = next(c['request'] for c in positives if c['request']['operation'].get('action', {}).get('kind') == action)
        reject(r, ['operation', 'action'], keys)
    for r in negatives:
        if validator.is_valid(r): raise ValueError('Schema admitted incomplete or ignored optional/model input')
    return dict(status='passed', positive_cases=len(positives), rejected_mutations=len(negatives),
                schema_sha256=hashlib.sha256(raw).hexdigest(), certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--binary', required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
