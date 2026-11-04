"""Explicit required original solve-policy revision for the immutable full books."""
import argparse
import hashlib
import json
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
SOURCE = 'tests/fixtures/library/optional-graph-hedge-bound-book.json'
TARGET = 'tests/fixtures/library/optional-graph-hedge-policy-book.json'


def generate():
    value = json.loads((ROOT / SOURCE).read_bytes())
    for case in value['cases']: case['problem']['settings']['policy_version'] = 1
    value['source_reads'] = {SOURCE: hashlib.sha256((ROOT / SOURCE).read_bytes()).hexdigest()}
    value['qualification'] = 'Explicit supplied original solve-policy revision. All original complete numerical settings, limits, financial answers, quantities, lots, cost/eligibility and FX pins retained.'
    return value


if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--write', action='store_true'); a = p.parse_args()
    raw = (json.dumps(generate(), indent=2) + '\n').encode(); out = ROOT / TARGET
    if a.write:
        if out.exists(): raise ValueError('Refusing to overwrite separate solve-policy admission')
        out.write_bytes(raw)
    elif out.read_bytes() != raw: raise ValueError('Separate solve-policy control differs')
    print(json.dumps(dict(status='passed', books=2, certified=False)))
