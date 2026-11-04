"""Declare structural exact-decimal controls independently of native decoders."""
import argparse
from copy import deepcopy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / 'tests/fixtures/library/term-decimals-reference.json'


def generate():
    original = json.loads((ROOT / 'tests/fixtures/library/cases.json').read_bytes())['cases']
    bond = deepcopy(next(c['request']['operation']['bond']['terms'] for c in original
                         if c['id'] == 'bond.construct'))
    principal = dict(effective_date='2027-04-07', multiplier='1.000', amount='100.000')
    exercise = dict(right='call', style=dict(style='european', date='2027-04-07'),
                    price_per_100='101.2500', notice_offset=dict(CalendarDays=-7),
                    settlement_lag=dict(CalendarDays=2), settlement_adjustment='Following',
                    accrued='pay_accrued', protection_end=None)
    fixing = dict(index_id='USD.TERM.3M', observation_date='2026-04-07', rate='0.0312500',
                  published_at='2026-04-07T12:00:00Z', known_at='2026-04-07T12:01:00Z',
                  revision=1, status='Final', source_id='REFERENCE.SOURCE.v1')
    profile = dict(start='2026-04-07', end='2027-10-07', value='0.05000')
    targets = [('bond', 'face_value', bond), ('bond', 'redemption_multiplier', bond),
               ('principal', 'multiplier', principal), ('principal', 'amount', principal),
               ('exercise', 'price_per_100', exercise), ('fixing', 'rate', fixing),
               ('profile', 'value', profile)]
    exact = ['0', '0.00000', '-0.031250000', '0.1234567890123456789012345678',
             '0.0000000000000000000000000001', '-0.0000000000000000000000000001',
             '1.0000000000000000000000000000', '97.1234567890123456789',
             '101.750000', '1000000000000000000.12500000',
             '79228162514264337593543950335', '-79228162514264337593543950335',
             '7.9228162514264337593543950335']
    rejected = [0, 1, 0.03125, 97.12345678901235, True, [], {}, '1e-4', '1E4',
                '+1', '1.', ' 1', '1 ', '1,000', 'NaN', 'Infinity', '',
                '0.12345678901234567890123456789', '79228162514264337593543950336',
                '7.9228162514264337593543950336', '0.' + '0' * 100 + '1']
    positive, negative, duplicates, aliases = [], [], [], []
    for kind, field, prototype in targets:
        for value in exact:
            terms = deepcopy(prototype)
            terms[field] = value
            positive.append(dict(id=f'{kind}.{field}.exact.{len(positive)}', kind=kind, terms=terms))
        if field == 'amount':
            terms = deepcopy(prototype)
            terms[field] = None
            positive.append(dict(id='principal.amount.null', kind=kind, terms=terms))
        for alias in ['.5', '-.5', '.0500', '-.0500']:
            terms = deepcopy(prototype)
            terms[field] = alias
            canonical = deepcopy(terms)
            canonical[field] = ('-0' + alias[1:]) if alias.startswith('-') else ('0' + alias)
            aliases.append(dict(id=f'{kind}.{field}.alias.{len(aliases)}', kind=kind,
                                terms=terms, canonical_terms=canonical))
        for value in rejected + ([] if field == 'amount' else [None]):
            terms = deepcopy(prototype)
            terms[field] = value
            negative.append(dict(id=f'{kind}.{field}.reject.{len(negative)}', kind=kind, terms=terms))
        terms = deepcopy(prototype)
        terms['ignored_economics'] = 'must be rejected'
        negative.append(dict(id=f'{kind}.{field}.unknown', kind=kind, terms=terms))
        duplicates.append(dict(kind=kind, json=json.dumps(prototype)[:-1] + f', "{field}": "1.25"}}'))
    for currency in ['USD', 'EUR', 'GBP']:
        complete = deepcopy(bond)
        complete['currency'] = currency
        complete['principal_prices'] = [deepcopy(principal)]
        complete['exercise'] = [deepcopy(exercise)]
        complete['face_value'] = '5000.00000000000000000001'
        complete['redemption_multiplier'] = '1.0123456789012345678901234567'
        for name in ['coupon', 'spread', 'gearing', 'notional']:
            complete['profiles'][name][0]['value'] = '0.1234567890123456789012345678'
        positive.append(dict(id='bond.complete.' + currency, kind='bond', terms=complete))
    return dict(schema_version=1, certified=False,
                reference='Original supplied input metadata plus independently declared exact strings/rejections; no native output',
                positive_cases=positive, negative_cases=negative, duplicate_cases=duplicates, alias_cases=aliases)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['write', 'check'])
    args = parser.parse_args()
    raw = (json.dumps(generate(), indent=2) + '\n').encode()
    if args.mode == 'write':
        if TARGET.exists():
            raise ValueError('Refuse declared reference replacement')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes() != raw:
        raise ValueError('Independent declaration changed')
    data = json.loads(raw)
    print(json.dumps(dict(status='declared' if args.mode == 'write' else 'passed',
                          roundtrips=len(data['positive_cases']), rejections=len(data['negative_cases']),
                          duplicate_rejections=len(data['duplicate_cases']), certified=False)))


if __name__ == '__main__':
    main()
