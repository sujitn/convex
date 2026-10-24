"""Validate actual shared schema, full contexts and required common representation."""
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
    cases = json.loads((Path(__file__).resolve().parents[1]/'fixtures/library/icma-government-qualified.json').read_bytes())['cases']
    positives = [c for c in cases if c['expected_error'] is None]
    for c in positives: validator.validate(c['request'])
    negative = []
    for kind in ['spread', 'price']:
        original = next(c['request'] for c in positives if c['request']['operation']['kind'] == 'icma_government_'+kind)
        common = ['operation', 'settings'] + (['government'] if kind == 'price' else [])
        paths = [(['operation'], ['target', 'benchmarks', 'reference_date', 'settings']),
                 (['operation', 'target', 'icma'], ['reference_context', 'convention', 'quotation_policy', 'calendars', 'position']),
                 (['operation', 'target', 'icma', 'reference_context'], ['revision', 'security', 'frequency', 'periods']),
                 (['operation', 'benchmarks', 0], ['quote', 'icma']),
                 (common, ['benchmark_id', 'quotation_policy', 'compounding', 'inverse', 'selection', 'max_reference_evaluations'])]
        if kind == 'price': paths += [(['operation'], ['quote']), (['operation', 'settings'], ['decimal_spread_tolerance'])]
        for location, fields in paths:
            for field in fields:
                r = deepcopy(original); node = r
                for k in location: node = node[k]
                del node[field]; negative.append(r)
        r = deepcopy(original); node = r
        for k in common: node = node[k]
        node['compounding'] = 'Street'; negative.append(r)
        r = deepcopy(original); r['operation']['target']['icma']['cached_yield'] = 0.03; negative.append(r)
    for r in negative:
        if validator.is_valid(r): raise ValueError('Schema accepted missing/replaced contextual government economics')
    return dict(status='passed', positive_cases=len(positives), rejected_mutations=len(negative),
                schema_sha256=hashlib.sha256(raw).hexdigest(), certified=False)


if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--binary', required=True); print(json.dumps(validate(p.parse_args().binary)))
