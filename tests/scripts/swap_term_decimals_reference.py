"""Predeclare exact IRS/OIS profile decoder controls, without native answers."""
import argparse
from copy import deepcopy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / 'tests/fixtures/library/swap-term-decimals-reference.json'


def generate():
    cases = json.loads((ROOT / 'tests/fixtures/library/cases.json').read_bytes())['cases']
    base = deepcopy(next(c['request']['operation']['terms'] for c in cases
                         if c['id'] == 'swap.0.2.construct'))
    paths = [('fixed', 'leg', 'notionals'), ('fixed', 'rates'),
             ('floating', 'leg', 'notionals'), ('floating', 'profiles', 'spreads'),
             ('floating', 'profiles', 'gearings')]
    exact = ['0.000000', '-0.031250000', '0.1234567890123456789012345678',
             '0.0000000000000000000000000001', '-0.0000000000000000000000000001',
             '1.0000000000000000000000000000', '97.1234567890123456789',
             '1000000000000000000.12500000', '79228162514264337593543950335',
             '-79228162514264337593543950335', '7.9228162514264337593543950335']
    rejected = [0, 0.05, True, None, [], {}, '1e-4', '1E4', '+1', '1.',
                ' 1', '1 ', '1,000', 'NaN', 'Infinity', '',
                '0.12345678901234567890123456789', '79228162514264337593543950336',
                '7.9228162514264337593543950336', '0.' + '0' * 100 + '1']
    positive, negative, aliases = [], [], []
    for path in paths:
        def change(value):
            terms = deepcopy(base)
            cursor = terms
            for key in path:
                cursor = cursor[key]
            cursor[0]['value'] = value
            return terms
        for value in exact:
            positive.append(dict(id='swap.' + '.'.join(path) + f'.exact.{len(positive)}',
                                 kind='swap', terms=change(value)))
        for alias in ['.5', '-.5', '.0500', '-.0500']:
            canonical = ('-0' + alias[1:]) if alias.startswith('-') else ('0' + alias)
            aliases.append(dict(id='swap.' + '.'.join(path) + f'.alias.{len(aliases)}',
                                kind='swap', terms=change(alias), canonical_terms=change(canonical)))
        for value in rejected:
            negative.append(dict(id='swap.' + '.'.join(path) + f'.reject.{len(negative)}',
                                 kind='swap', terms=change(value)))
    return dict(schema_version=1, certified=False,
                reference='Full original supplied swap input metadata and independently declared exact strings; no native output',
                positive_cases=positive, negative_cases=negative, duplicate_cases=[], alias_cases=aliases)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['write', 'check'])
    args = parser.parse_args()
    raw = (json.dumps(generate(), indent=2) + '\n').encode()
    if args.mode == 'write':
        if TARGET.exists():
            raise ValueError('Refuse declared reference replacement')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes() != raw:
        raise ValueError('Independent swap metadata declaration changed')
    data = json.loads(raw)
    print(json.dumps(dict(status='declared' if args.mode == 'write' else 'passed',
                          roundtrips=len(data['positive_cases']), rejections=len(data['negative_cases']), certified=False)))
