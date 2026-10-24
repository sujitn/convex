"""Separate full plain-Decimal receipts and complete ownership control.

The initial and earned trials remain immutable. Actual supplied test receipts
have explicitly declared milli-currency precision, within the original checked
Decimal wire; they are not sixty-digit model values paid as external cash.
Recompute all full cash/ownership/causal fields from those actual supplied events
and unchanged complete original independent product/model/curve/basket values.
Also exercise transfer of the entire original initial-CDS outstanding claim.
No native numerical answer, old financial file or wider tolerance is used.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
import claim_pnl_reference as original
import claim_pnl_earned as earned
import cohort_pnl_reference as r
import cohort_pnl_states as states
import residual_pnl_reference as residual
import residual_pnl_canonical as canonical
from cohort_pnl_wire import units

SOURCE=earned.TARGET;TARGET=SOURCE.with_name('claim-pnl-complete.json')
clone=copy.deepcopy

def financial(case):
    spec=case['request']['operation']['input'];book=states.expand(case['request'])
    owners=spec['cohorts'];splits=spec['lot_splits']
    old=r.acquired
    def acquired(info,owner,physical):
        if owner['acquisition']['kind']!='lot_split':return old(info,owner,physical)
        i=next(i for i,p in enumerate(r.sets(book)) if p['point']['snapshot_id']==info['point']['snapshot_id'])
        return any(s['id']==owner['acquisition']['split_id'] and i in s['applied_points'] for s in splits)
    r.acquired=acquired
    try:result=residual.derive(case,book,owners,[],list(range(len(r.sets(book)))))
    finally:r.acquired=old
    result.update(id=case['id']);result['request']['operation']['input'].update(lot_splits=splits,claim_transfers=spec['claim_transfers'])
    original.allocations(result,book,owners,spec['claim_transfers'])
    earned.complete_proposals(result)
    for trade in result['expected']['trades']:
        for mark in trade['marks']:
            for cash in mark['claims']:
                if cash.get('transfer') and D(cash['transfer']['owned_outstanding'])==0:
                    cash['ratio']=D(1);cash['transfer']['ratio']=D(1)
            for value in mark['transferred']:
                if D(value['owned_outstanding'])==0:value['ratio']=D(1)
    units(result);canonical.normalize(result)
    return result

def generate():
    data=json.loads(SOURCE.read_bytes())
    complete=clone(next(c for c in data['cases'] if c['id']=='claim.full.discounted_cds_initial'))
    complete.update(id='claim.full.entire_initial_cash')
    complete['request']['operation']['input']['claim_transfers'][0]['fraction']='1'
    data['cases'].append(complete)
    with localcontext() as ctx:
        ctx.prec=60
        for i,c in enumerate(data['cases']):
            for h in c['request']['operation']['input']['book']['holdings']:
                for s in h['lifecycle']['events']:
                    e=s['event'];a=e['action']
                    if a['kind']=='payment' and a['terms']['receipt_id'].startswith('claim.'):
                        amount=a['terms']['amount'];amount['value']=format(D(amount['value']).quantize(D('.001')),'f')
            if c['expected_error'] is None:data['cases'][i]=financial(c)
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    for c in data['cases']:
        c['input_nodes']=original.lots.nodes(c['request']);c['input_bytes']=len(json.dumps(c['request'],separators=(',',':')).encode())
    return data

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate()
    raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete independent cash-transfer controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),max_nodes=max(c['input_nodes'] for c in data['cases']),certified=False)))
