"""Separate complete hedge wire admission and explicit same-snapshot FX controls.

Original independent economic answers/limits remain immutable. Drop the template
label and supply a declared position-state pin required by the existing API.
No native result is read and no financial tolerance is changed.
"""
import argparse
from copy import deepcopy
from decimal import Decimal
import hashlib
import json
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
SOURCE='tests/fixtures/library/optional-graph-hedge-book.json'
TARGET='tests/fixtures/library/optional-graph-hedge-admitted-book.json'

def generate():
    original=json.loads((ROOT/SOURCE).read_bytes());cases=[]
    for currency,fx in [('USD','1'),('EUR','0.9')]:
        p=deepcopy(original['problem']);del p['id'];p['position_state']=dict(id='EXPLICIT.SYNTHETIC.POSITIONS',version=1)
        p['reporting_currency']=currency
        expected=deepcopy(original['expected'])
        for name in ['current','pending','before','after']:
            expected[name]=[str(Decimal(str(x))*Decimal(fx)) for x in expected[name]]
        expected['cost']=str(Decimal(str(expected['cost']))*Decimal(fx))
        cases.append(dict(id='optional_graph.book.'+currency,problem=p,expected=expected,fx=None if currency=='USD' else dict(from_='USD',to='EUR',rate=fx,observation=dict(id='EXPLICIT.SYNTHETIC.FX',version=1),snapshot_id=p['snapshot_id'])))
        if cases[-1]['fx']:cases[-1]['fx']['from']=cases[-1]['fx'].pop('from_')
    return dict(certified=False,source_reads={SOURCE:hashlib.sha256((ROOT/SOURCE).read_bytes()).hexdigest()},tolerances=original['tolerances'],cases=cases,raw_original_problem=original['problem'])

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args();raw=(json.dumps(generate(),indent=2)+'\n').encode();p=ROOT/TARGET
    if args.write:
        if p.exists():raise ValueError('Refusing to overwrite separately declared wire/FX controls')
        p.write_bytes(raw)
    elif p.read_bytes()!=raw:raise ValueError('Independent wire/FX admission differs')
    print(json.dumps(dict(status='passed',cases=2,certified=False)))
