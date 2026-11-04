"""Separately declared finite application limits/scales for the five-factor book.

Original broad inputs and numerical_failure receipt remain. Only explicit factor
normalization/financial bands change; targets, full five financial answers,
quantities/lots/costs/FX and every original solver/comparison bound stay exact.
"""
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE = 'tests/fixtures/library/recovery-hedge-problems.json'
DEST = ROOT/'tests/fixtures/library/recovery-hedge-limits.json'


def build():
    value = deepcopy(json.loads((ROOT/SOURCE).read_bytes()))
    for case in value['cases']:
        for factor, scale, limit in zip(case['factors'], [1, 1000000, 1, 100, 100000], [10, 1000000, 10, 100, 50000]):
            factor.update(scale=float(scale), lower=float(-limit), upper=float(limit))
    value['source_reads'][SOURCE] = hashlib.sha256((ROOT/SOURCE).read_bytes()).hexdigest()
    value['qualification'] += ' Separate finite factor bands and dimensioned risk normalization; original broad-limit numerical_failure remains open. Same full financial targets/answers/quantities/current/pending/costs/FX/settings and comparison bounds. No solver-default or oracle relaxation.'
    return value


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--write', action='store_true'); args = parser.parse_args()
    data = (json.dumps(build(), indent=2)+'\n').encode()
    if args.write:
        if DEST.exists(): raise ValueError('Refusing to overwrite declared limits')
        DEST.write_bytes(data)
    elif DEST.read_bytes() != data: raise ValueError('Declared limits differ')
    print(json.dumps(dict(status='passed', books=2, original_failure_retained=True, certified=False)))
