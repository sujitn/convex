"""Check actual reverse-government-spread schema; native finance stays separate."""
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
    folder = Path(__file__).resolve().parents[1]/'fixtures/library'
    cases = json.loads((folder/'government-inverse-protocol.json').read_bytes())['cases']
    positives = [c for c in cases if c['expected_error'] is None]
    for case in positives:
        validator.validate(case['request'])
    original = positives[0]['request']; negative = []
    for field in ['security','terms','trade_date','settlement','on_payment']:
        candidate = deepcopy(original); del candidate['operation']['target']['target'][field]
        negative.append(candidate)
    for field in ['target','benchmarks','reference_date','settings','quote','position']:
        candidate = deepcopy(original); del candidate['operation'][field]; negative.append(candidate)
    for field in ['government','decimal_spread_tolerance']:
        candidate = deepcopy(original); del candidate['operation']['settings'][field]; negative.append(candidate)
    for owner, key, value in [
        (['target','target'], 'observed_price', 100),
        (['settings','government'], 'selection', 'Nearest'),
        (['quote'], 'quote_version', None),
        (['position'], 'quantity', 15000),
        (['target','target','terms'], 'face_value', 5000),
    ]:
        candidate = deepcopy(original); node = candidate['operation']
        for part in owner: node = node[part]
        node[key] = value; negative.append(candidate)
    candidate = deepcopy(original); candidate['operation']['benchmarks'][0]['quote']['cached_yield'] = 0.03
    negative.append(candidate)
    for candidate in negative:
        if validator.is_valid(candidate):
            raise ValueError('Schema admitted missing/lost/replaced reverse-spread input')
    return dict(status='passed', positive_cases=len(positives), rejected_mutations=len(negative),
                schema_sha256=hashlib.sha256(raw).hexdigest(), certified=False)


if __name__ == '__main__':
    parser=argparse.ArgumentParser(); parser.add_argument('--binary',required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
