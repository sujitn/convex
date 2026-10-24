"""Bind complete original actual fill Money units without numerical changes.

The retained bindings trial omitted required Money.unit in the independent
expected full fill provenance. The actual request already carries the explicit
currency units. Bind that identical original actual request fill after checking
all economic values; keep every financial answer, recipient and bound unchanged.
"""
import argparse,hashlib,json
from decimal import Decimal
import custody_lineage_pnl_bindings as original
SOURCE=original.TARGET;TARGET=SOURCE.with_name('custody-lineage-pnl-units.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    for c in data['cases']:
        if c['expected_error'] is not None:continue
        book=c['request']['operation']['input']['book']
        points=[book['before']]+[s['after'] for s in book['steps']]+[s['information'] for s in book['standalone'] if s['kind']=='available']
        for trade in c['expected']['trades']:
            for mark,point in zip(trade['marks'],points):
                for disposal in (mark.get('physical') or {}).get('disposals',[]):
                    actual=next(f for f in point['point']['fills'] if f['id']==disposal['fill']['id']);old=disposal['fill']
                    if Decimal(actual['quantity'])!=Decimal(old['quantity']):raise ValueError('Actual face changed')
                    for name in ['consideration','fees']:
                        if Decimal(actual[name]['value'])!=Decimal(old[name]['value']) or actual[name]['currency']!=old[name]['currency']:raise ValueError('Actual money changed')
                    disposal['fill']=actual
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete actual fill units differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
