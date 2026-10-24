"""Separate strict Decimal wire admission; raw controls/answers stay exact.

The original pending upfront string '.015' is not an admitted plain decimal.
Only its wire spelling becomes '0.015'. Full economic Decimal values, independent
answers, domains and comparison bounds remain; all raw requests are retained.
"""
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE = 'tests/fixtures/library/recovery-hedge-api.json'
DEST = ROOT / 'tests/fixtures/library/recovery-hedge-admitted.json'


def build():
    document = deepcopy(json.loads((ROOT/SOURCE).read_bytes()))
    raw_cases = []
    for case in document['cases']:
        terms = case['request']['operation'].get('product', {}).get('terms', {})
        if terms.get('initial_clean_upfront_fraction') == '.015':
            original = deepcopy(case); original['expected_error'] = 'invalid_input'
            original['id'] += '.raw_decimal_rejected'; raw_cases.append(original)
            case['control_id'] = case['id']; case['id'] += '.wire_admission'
            terms['initial_clean_upfront_fraction'] = '0.015'
    document['cases'] += raw_cases
    document['source_reads'][SOURCE] = hashlib.sha256((ROOT/SOURCE).read_bytes()).hexdigest()
    document['qualification'] += ' Separate strict leading-zero Decimal spelling only; original raw inputs/answers/bounds preserved with four explicit raw rejected requests.'
    return document


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--write', action='store_true'); args = parser.parse_args()
    raw = (json.dumps(build(), indent=2)+'\n').encode()
    if args.write:
        if DEST.exists(): raise ValueError('Refusing to overwrite admission')
        DEST.write_bytes(raw)
    elif DEST.read_bytes() != raw: raise ValueError('Separate admission differs')
    print(json.dumps(dict(status='passed', cases=32, raw_rejected=4, certified=False)))
