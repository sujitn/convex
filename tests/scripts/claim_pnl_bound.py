"""Complete original recovery lineage and simultaneous mixed claim ownership.

Retain the original failed recovery recipient replay. Its new legal cohort must
bind recovery to its actual native default event, not another cohort's event ID.
Only that exact original linkage is corrected; cash/profile/model values and
bounds are unchanged. Add simultaneous full OIS and CDS claim allocation plus
actual independent availability and original cash-owner failure controls.
"""
import argparse,copy,hashlib,json
from decimal import localcontext
import claim_pnl_reference as original
import claim_pnl_chronology as chronology
import claim_pnl_complete as complete
import cohort_pnl_reference as r
import cohort_pnl_states as states

SOURCE=chronology.TARGET;TARGET=SOURCE.with_name('claim-pnl-bound.json')
clone=copy.deepcopy

def generate():
    data=json.loads(SOURCE.read_bytes())
    for c in data['cases']:
        if c['expected_error'] is not None or not c['id'].startswith('claim.recovery.'):continue
        for h in c['request']['operation']['input']['book']['holdings']:
            if not h['id'].endswith('.recipient'):continue
            defaults=[s['event']['event_id'] for s in h['lifecycle']['events'] if s['event']['action']['kind']=='credit_default']
            for s in h['lifecycle']['events']:
                if s['event']['action']['kind']=='recovery':s['event']['action']['terms']['default_event']=defaults[0]
    mixed=clone(next(c for c in data['cases'] if c['id']=='claim.full.discounted_ois'))
    mixed.update(id='claim.full.simultaneous_ois_cds')
    spec=mixed['request']['operation']['input'];book=states.expand(mixed['request']);first=spec['claim_transfers'][0]['applied_points'][0]
    info=r.sets(book)[first];j=6;ent=spec['cohorts'][j]['entitlements'][0];origin=spec['cohorts'][j]['trade_id']
    spec['claim_transfers'].append(dict(id=mixed['id']+'.CDS.LEGAL',origin=origin,key=clone(ent['key']),proposal_id=original.proposal(info,j,ent['key'])['event_id'],
        entitlement_source=clone(ent['source']),residual_id=None,**{'from':origin,'to':spec['cohorts'][0]['trade_id']},fraction='0.5',
        cutoff=dict(date=info['point']['valuation_date'],known_at=info['point']['known_at'],receipt_ids=[]),source=r.c.pin(mixed['id']+'.DOCUMENT'),
        applied_points=clone(spec['claim_transfers'][0]['applied_points'])))
    with localcontext() as ctx:
        ctx.prec=60;mixed=complete.financial(mixed)
    data['cases'].append(mixed)
    def reject(name,fn,source):
        c=clone(next(c for c in data['cases'] if c['id']==source));c.update(id='claim.invalid.'+name,expected_error='invalid_input');c.pop('expected');fn(c['request']['operation']['input']);data['cases'].append(c)
    def unavailable(spec):
        phase=next(i for i,s in enumerate(spec['book']['steps']) if s['cause']=='position' and spec['book']['standalone'][i]['kind']=='available')
        point=len(spec['book']['steps'])+1+sum(s['kind']=='available' for s in spec['book']['standalone'][:phase])
        spec['claim_transfers'][0]['applied_points'].append(point)
    reject('unacquired_split_recipient',unavailable,'claim.full.residual_five_causes')
    def discount(spec):
        for binding in [spec['book']['before']]+[s['after'] for s in spec['book']['steps']]+[s['information'] for s in spec['book']['standalone'] if s['kind']=='available']:
            binding['point']['positions'][0]['cash_discount']['id']='FOREIGN.CASH'
    reject('foreign_cash_discount',discount,'claim.full.discounted_ois')
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
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete original bound ownership controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),max_nodes=max(c['input_nodes'] for c in data['cases']),certified=False)))
