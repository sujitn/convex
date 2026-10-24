"""Retained original option failure and separately predeclared full floating input."""
import argparse
from copy import deepcopy as clone
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'tests/fixtures/library'


def qualify():
    source = json.loads((DEST / 'icma-risk-admitted.json').read_bytes())
    cases = clone(source['cases'])
    next(c for c in cases if c['id'] == 'icma_risk.invalid.optional')['expected_error'] = 'unsupported'
    floating = clone(cases[0]); floating['id'] = 'icma_risk.invalid.full_term_floating'
    terms = floating['request']['operation']['input']['security']['bond']['terms']
    terms['coupon'] = dict(kind='term', index_id='ICMA.TERM', reset_frequency='Quarterly',
                           reset_adjustment='Following', fixing_offset={'CalendarDays': -2},
                           projection_day_count='Act360')
    for p in terms['profiles']['gearing']: p['value'] = '1'
    floating['expected_error'] = 'unsupported'; cases.append(floating)
    missing = clone(next(c for c in cases if c['id'] == 'icma_risk.hedge.DecimalRate'))
    missing['id'] = 'icma_risk.invalid.missing_hedge_calculation'
    del missing['request']['operation']['action']['identity']['calculation']
    missing['expected_error'] = 'invalid_input'; cases.append(missing)
    return dict(certified=False, qualification='Original raw option request and failure remain. '
                'Actual owning option guard is unsupported, not malformed input. '
                'Separate full term-floating and missing hedge-calculation inputs are declared '
                'before their execution; no financial answer/domain/bound is changed.', cases=cases)


if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--write', action='store_true'); a = p.parse_args()
    v = qualify(); data = (json.dumps(v, indent=2) + '\n').encode(); path = DEST / 'icma-risk-api.json'
    if a.write:
        if path.exists(): raise ValueError('Refusing to overwrite failure admission')
        path.write_bytes(data)
    elif path.read_bytes() != data: raise ValueError('Original failure admission changed')
    print(json.dumps(dict(status='passed', requests=len(v['cases']), certified=False)))
