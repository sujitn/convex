"""Actual schema preserves direct contextual risk and full hedge identities."""
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
    cases = json.loads((Path(__file__).resolve().parents[1] / 'fixtures/library/icma-risk-owner-api.json').read_bytes())['cases']
    positives = [c for c in cases if c['expected_error'] is None]
    for c in positives: validator.validate(c['request'])
    negative = []
    for kind in ['yield_sensitivity', 'hedge_vector']:
        original = next(c['request'] for c in positives if c['request']['operation']['action']['kind'] == kind)
        locations = [['operation', 'input'], ['operation', 'input', 'reference_context'],
                     ['operation', 'input', 'security'], ['operation', 'input', 'position'],
                     ['operation', 'convention'], ['operation', 'cash'], ['operation', 'action'],
                     ['operation', 'action', 'input'], ['operation', 'action', 'input', 'observed_quote'],
                     ['operation', 'action', 'input', 'key'], ['operation', 'action', 'input', 'settings']]
        if kind == 'hedge_vector': locations.append(['operation', 'action', 'identity'])
        for path in locations:
            def node(r):
                for p in path: r = r[p]
                return r
            for key in node(original):
                r = deepcopy(original); del node(r)[key]; negative.append(r)
            r = deepcopy(original); node(r)['inferred_axis'] = True; negative.append(r)
    for r in negative:
        if validator.is_valid(r): raise ValueError('Schema admitted omitted full contextual risk inputs')
    return dict(status='passed', positive_cases=len(positives), rejected_mutations=len(negative),
                schema_sha256=hashlib.sha256(raw).hexdigest(), certified=False)


if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--binary', required=True)
    print(json.dumps(validate(p.parse_args().binary)))
