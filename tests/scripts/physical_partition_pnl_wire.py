"""Separate exact original Decimal wire declaration; no financial change.

The original predeclared trial used .75/.25 spellings, outside ExactDecimal's
plain wire grammar. Declare numerically identical 0.75/0.25 facts and canonical
event identities independently before replay; retain the failed original corpus.
Zero-custody parent allocations have no owned receipts; full original receipt
events remain in security_receipts and every positive child's custody allocation.
"""
import argparse,hashlib,json
from decimal import Decimal
import physical_partition_pnl_reference as original
import residual_pnl_canonical as canonical
SOURCE=original.TARGET;TARGET=SOURCE.with_name('physical-partition-pnl-wire.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    def normalize(v):
        if isinstance(v,dict):
            if 'fraction' in v and isinstance(v['fraction'],str):v['fraction']=canonical.canonical(v['fraction'])
            if 'partition' in v and v['partition'] and Decimal(v['partition']['fraction'])==0:v['partition']['receipts']=[]
            for x in v.values():normalize(x)
        elif isinstance(v,list):
            for x in v:normalize(x)
    normalize(data)
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest())
    for c in data['cases']:
        c['input_nodes']=original.lots.nodes(c['request']);c['input_bytes']=len(json.dumps(c['request'],separators=(',',':')).encode())
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Exact original wire differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
