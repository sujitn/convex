"""Bind full native fill units while retaining the initial complete financial trial.

The independent disposal reference copied its pre-wire fill after units encoding.
Supply the entire actual encoded fill, proving identical exact quantities and
Money. No request, financial answer, tolerance or native financial owner changes.
"""
import argparse, copy, hashlib, json
from decimal import Decimal
import child_cash_pnl_available as original
SOURCE = original.TARGET
TARGET = SOURCE.with_name('child-cash-pnl-wire.json')
def generate():
    data = json.loads(SOURCE.read_bytes())
    for case in data['cases']:
        if case['expected_error'] is not None or 'regression' in case['id']:
            continue
        book = case['request']['operation']['input']['book']
        points = [book['before']]+[s['after'] for s in book['steps']]+[s['information'] for s in book['standalone'] if s['kind']=='available']
        fills = {f['id']:f for p in points for f in p['point']['fills']}
        for trade in case['expected']['trades']:
            for mark in trade['marks']:
                for disposal in (mark.get('physical') or {}).get('disposals', []):
                    prior = disposal['fill']; actual = fills[prior['id']]
                    assert Decimal(prior['quantity']) == Decimal(actual['quantity'])
                    for role in ['consideration', 'fees']:
                        assert prior[role]['currency'] == actual[role]['currency']
                        assert Decimal(prior[role]['value']) == Decimal(actual[role]['value'])
                        assert actual[role]['unit'] == 'currency'
                    disposal['fill'] = copy.deepcopy(actual)
    data.update(reference=__doc__, source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest())
    return data
if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete original fill encoding differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
