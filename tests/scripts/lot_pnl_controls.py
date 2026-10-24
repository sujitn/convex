"""Preserve all full financial controls; correct one typed support diagnostic.

Replacing replayed complete legal terms without the original boundary amendment
event is Unsupported by the shared admission contract. No financial expectation,
request, numerical tolerance, earlier fixture or trial is changed.
"""
import argparse, copy, hashlib, json
from pathlib import Path

SOURCE = Path(__file__).resolve().parents[1] / 'fixtures/library/lot-pnl-extended.json'
TARGET = SOURCE.with_name('lot-pnl-controls.json')

def generate():
    data = copy.deepcopy(json.loads(SOURCE.read_bytes()))
    case = next(c for c in data['cases'] if c['id'] == 'lot.invalid.terms_without_actual_amendment')
    case['expected_error'] = 'unsupported'
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(), reference=__doc__)
    return data

if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--write', action='store_true'); a = p.parse_args()
    data = generate(); raw = (json.dumps(data, indent=2) + '\n').encode()
    if a.write:
        if TARGET.exists(): raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes() != raw: raise ValueError('Complete financial and diagnostic controls differ')
    print(json.dumps(dict(status='passed', cases=len(data['cases']), financial_books=sum(c['expected_error'] is None for c in data['cases']), certified=False)))
