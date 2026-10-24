"""Declare canonical original Decimal event wire without numerical changes.

ExactDecimal's original serializer removes insignificant decimal scale. Preserve
every earlier full input/reference and failed trial. Canonicalize only actual
receipt/proposal event face and Money spellings, checking Decimal equality.
No error envelope, bound, computed financial value or candidate answer is changed.
"""
import argparse,hashlib,json
from decimal import Decimal
from pathlib import Path
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
SOURCE=FOLDER/'residual-pnl-admitted.json';TARGET=SOURCE.with_name('residual-pnl-canonical.json')
def canonical(v):
    value=format(Decimal(v),'f')
    if '.' in value:value=value.rstrip('0').rstrip('.')
    if Decimal(value)!=Decimal(v):raise ValueError('Original exact value changed')
    return value
def normalize(v):
    if isinstance(v,dict):
        if 'action' in v and 'event_id' in v:
            action=v['action'];terms=action.get('terms',{})
            if action['kind']=='security_receipt':terms['face']=canonical(terms['face'])
            if action['kind']=='payment':terms['amount']['value']=canonical(terms['amount']['value'])
        for x in v.values():normalize(x)
    elif isinstance(v,list):
        for x in v:normalize(x)
def generate():
    data=json.loads(SOURCE.read_bytes());normalize(data)
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args()
    raw=(json.dumps(generate(),indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original exact canonical wire changed')
    print(json.dumps(dict(status='passed',cases=len(json.loads(raw)['cases']),certified=False)))
