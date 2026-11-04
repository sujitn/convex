"""Bind unchanged original cash ownership to every newly inserted legal point.

The retained bound trial used the original shorter applied-point arrays in the
expected full cash provenance. Bind each unchanged cash transfer to its complete
new ordered/independent point list. No financial value, recipient, receipt,
native model, malformed request or numerical bound changes.
"""
import argparse,hashlib,json
import custody_lineage_pnl_bound as original
SOURCE=original.TARGET;TARGET=SOURCE.with_name('custody-lineage-pnl-bindings.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    for case in data['cases']:
        if case['expected_error'] is not None:continue
        transfers={t['id']:t for t in case['request']['operation']['input']['claim_transfers']}
        for trade in case['expected']['trades']:
            for mark in trade['marks']:
                allocations=mark['transferred']+[c['transfer'] for c in mark['claims'] if c.get('transfer')]
                for allocation in allocations:
                    allocation['transfers']=[transfers[t['id']] for t in allocation['transfers']]
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete unchanged cash provenance differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
