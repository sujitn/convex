"""Bind the preserved nonnegative-clean quote guard to actual error controls.

The raw 0.01 dirty-price benchmark has negative clean price before inversion:
its native error is invalid_input. Preserve both earlier corpora unchanged,
and add a separate positive-clean benchmark price outside the inverse bracket.
No financial expected answer or tolerance changes.
"""
import argparse
import copy
import json
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]


def build():
    p=ROOT/'tests/fixtures/library/government-inverse-admitted.json'
    value=json.loads(p.read_bytes())
    row=next(c for c in value['cases'] if c['id']=='gov_inverse.failure.unselected_no_root')
    row['expected_error']='invalid_input'
    new=copy.deepcopy(row);new['id']='gov_inverse.failure.unselected_valid_price_no_root';new['expected_error']='no_root'
    new['request']['operation']['benchmarks'][0]['quote']['helper']['quote']['value']=5.0
    value['cases'].append(new)
    value['qualification']+=' Original nonnegative-clean guard rejects raw 0.01 dirty benchmark before solving; a separate positive-clean 5.0 dirty quote exercises no root. All independent financial answers and accuracy bounds unchanged.'
    return value


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    path=ROOT/'tests/fixtures/library/government-inverse-protocol.json'
    value=build();raw=(json.dumps(value,sort_keys=True,separators=(',',':'))+'\n').encode()
    if args.write:
        if path.exists():raise ValueError('Refusing overwrite')
        path.write_bytes(raw)
    elif path.read_bytes()!=raw:raise ValueError('Protocol control changed')
    print(json.dumps(dict(status='passed',requests=len(value['cases']),financial_answers_changed=False,certified=False)))
