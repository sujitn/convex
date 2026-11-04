"""Separate full supplied cost/eligibility revision admission for original books.

Add mandatory, explicitly synthetic observation and universe decision pins.
Original quantities, financial answers, pending positions, limits, side costs,
lots, FX observations and solver settings are retained exactly.
"""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE = 'tests/fixtures/library/optional-graph-hedge-admitted-book.json'
TARGET = 'tests/fixtures/library/optional-graph-hedge-bound-book.json'


def generate():
    value = json.loads((ROOT / SOURCE).read_bytes())
    for case in value['cases']:
        for candidate in case['problem']['candidates']:
            candidate['costs']['observation'] = dict(id='EXPLICIT.SYNTHETIC.COSTS', version=1)
            candidate['eligibility']['decision'] = dict(id='EXPLICIT.SYNTHETIC.UNIVERSE', version=1)
    value['source_reads'] = {SOURCE: hashlib.sha256((ROOT / SOURCE).read_bytes()).hexdigest()}
    value['qualification'] = 'Original complete book economics and controls plus explicitly supplied synthetic cost observation and eligibility decision revisions. Proposal only.'
    return value


if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--write', action='store_true'); a = p.parse_args()
    raw = (json.dumps(generate(), indent=2) + '\n').encode(); out = ROOT / TARGET
    if a.write:
        if out.exists(): raise ValueError('Refusing to overwrite original book binding')
        out.write_bytes(raw)
    elif out.read_bytes() != raw: raise ValueError('Original book binding differs')
    print(json.dumps(dict(status='passed', books=2, certified=False)))
