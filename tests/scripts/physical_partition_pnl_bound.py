"""Complete original security lineage and whole-invoice financial controls.

Retain all earlier full trials. Add transfer of the entire independently supplied
invoice, and full malformed native ownership/source/work controls. Reallocate
only original exact invoice receipts; every original product and bound survives.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
import physical_partition_pnl_reference as original
import physical_partition_pnl_chronology as chronology
import cohort_pnl_reference as r
import cohort_pnl_states as states
import cohort_pnl_bindings as binding
import claim_pnl_reference as claims
import residual_pnl_canonical as canonical
SOURCE=chronology.TARGET;TARGET=SOURCE.with_name('physical-partition-pnl-bound.json')
clone=copy.deepcopy;ZERO=D(0)
def entire(case):
    case=clone(case);case['id']='physical.partition.entire_invoice';spec=case['request']['operation']['input'];spec['claim_transfers'][0]['fraction']='1'
    owners=spec['cohorts'];book=states.expand(case['request']);key=spec['claim_transfers'][0]['key'];ent=next(e for e in owners[0]['entitlements'] if e['key']==key)
    for i,info in enumerate(r.sets(book)):
        parent=case['expected']['trades'][1]['marks'][i];physical=parent['physical']
        if not physical:continue
        # Remove all earlier independent invoice allocations, leaving original full security PV.
        for t in case['expected']['trades'][1:]:
            m=t['marks'][i]
            for a in m['transferred']:
                for k in ['current','projected']:m['local'][k]=D(m['local'][k])-D(a['pv']);m['reporting'][k]=D(m['reporting'][k])-D(a['pv'])*D(m['fx'])
            m.update(transferred=[],recorded=ZERO,proposed=ZERO)
        active=[t for t in spec['claim_transfers'] if i in t['applied_points']];shares=[ZERO,D(1),ZERO,ZERO];paid=[ZERO]*4;received=[[] for _ in owners]
        def move(t,values):
            a=next(j for j,o in enumerate(owners) if o['trade_id']==t['from']);b=next(j for j,o in enumerate(owners) if o['trade_id']==t['to']);values[a]-=D(t['fraction']);values[b]+=D(t['fraction'])
            if min(values)<0:raise ValueError('Original invoice overdraft')
        for t in active:move(t,shares)
        for e in claims.events(info,0,key):
            at=[ZERO,D(1),ZERO,ZERO]
            for t in active:
                if e['action']['terms']['receipt_id'] not in t['cutoff']['receipt_ids']:move(t,at)
            for j,f in enumerate(at):
                if f:
                    amount=D(e['action']['terms']['amount']['value'])*f;paid[j]+=amount;received[j].append(dict(event=clone(e),fraction=f,amount=amount))
        cash=physical['invoice'];out=D(cash['amount'])-D(cash['recorded']);parties={1}|{j for t in active for j,o in enumerate(owners) if o['trade_id'] in [t['from'],t['to']]}
        proposal=next(s['event'] for s in info['holdings'][0]['lifecycle']['events'] if s['event']['action']['kind']=='bond_delivery')
        for j in parties:
            m=case['expected']['trades'][j]['marks'][i];pv=out*shares[j]
            m['transferred'].append(dict(origin=owners[0]['trade_id'],key=clone(key),entitlement=clone(ent),proposal=clone(proposal),transfers=clone(active),fraction=shares[j],receipts=received[j],owned_proposed=paid[j]+pv,owned_recorded=paid[j],owned_outstanding=pv,ratio=D(1),pv=pv))
            for k in ['current','projected']:m['local'][k]+=pv;m['reporting'][k]+=pv*D(m['fx'])
            m['recorded']=paid[j];m['proposed']=paid[j]+pv
        if sum(paid,ZERO)!=D(cash['recorded']):raise ValueError('Paid invoice conservation')
    binding.explain(case['expected'],dict(cohorts=owners),book);canonical.normalize(case);return case
def generate():
    data=json.loads(SOURCE.read_bytes())
    with localcontext() as ctx:ctx.prec=60;data['cases'].append(entire(data['cases'][0]))
    def reject(name,fn,code='invalid_input',source=0):
        c=clone(data['cases'][source]);c.update(id='physical.invalid.'+name,expected_error=code);c.pop('expected');fn(c['request']['operation']['input']);data['cases'].append(c)
    reject('future_child',lambda s:s['physical_partitions'][0]['split']['children'].__setitem__(0,s['cohorts'][0]['trade_id']))
    reject('foreign_child',lambda s:s['physical_partitions'][0]['split']['children'].__setitem__(0,'ABSENT'))
    reject('duplicate_cash_id',lambda s:s['claim_transfers'][1].update(id=s['claim_transfers'][0]['id']))
    reject('zero_cash_fraction',lambda s:s['claim_transfers'][0].update(fraction='0'))
    reject('self_cash_transfer',lambda s:s['claim_transfers'][0].update(to=s['claim_transfers'][0]['from']))
    reject('future_cash_recipient',lambda s:s['claim_transfers'][0].update(to=s['cohorts'][0]['trade_id']))
    reject('missing_cash_entitlement',lambda s:s['cohorts'][0]['entitlements'].pop(),'missing_input')
    reject('foreign_entitlement_revision',lambda s:s['claim_transfers'][0]['entitlement_source'].update(version=99))
    reject('cash_before_acquired_child',lambda s:s['claim_transfers'][0]['applied_points'].insert(0,4))
    reject('duplicate_cash_cutoff_receipt',lambda s:s['claim_transfers'][1]['cutoff']['receipt_ids'].append(s['claim_transfers'][1]['cutoff']['receipt_ids'][0]))
    reject('duplicate_security_cutoff_receipt',lambda s:s['physical_partitions'][0]['receipt_ids'].append(s['physical_partitions'][0]['receipt_ids'][0]),source=1)
    reject('reversed_security_prefix',lambda s:s['physical_partitions'][0]['receipt_ids'].reverse(),source=2)
    reject('missing_ordered_custody_point',lambda s:s['physical_partitions'][0]['split']['applied_points'].remove(6))
    reject('allocation_work_bound',lambda s:s['book'].update(max_product_evaluations=1))
    reject('dormant_pending',lambda s:s['book']['before']['point']['positions'][2].update(pending_quantity='1'))
    reject('wrong_wire_face',lambda s:s['book']['steps'][5]['after']['point']['positions'][2]['current'].update(quantity='1'))
    def revision(s):
        for h in s['book']['holdings']:
            if h['id']==s['cohorts'][2]['trade_id']:h['product']['product']['bond']['terms']['profiles']['coupon'][0]['value']='0.026'
    reject('foreign_full_security_terms',revision)
    for c in data['cases']:
        c['input_nodes']=original.lots.nodes(c['request']);c['input_bytes']=len(json.dumps(c['request'],separators=(',',':')).encode())
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original complete delivery ownership controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
