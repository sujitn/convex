"""Separate original proposal provenance and complete signed recovery transfers.

The first declaration and failed trials remain immutable. Original residual
checks require the complete actual proposal event as well as its cash values;
the first extension omitted that provenance in its expected residual object.
Restore only the original supplied event, then add full residual recovery books
and complete ownership transfers. No native numerical answer or wider bound.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
import claim_pnl_reference as original
import cohort_pnl_reference as r
import cohort_pnl_states as states
import residual_pnl_reference as residual
import residual_pnl_canonical as canonical
from cohort_pnl_wire import units

SOURCE=original.TARGET;TARGET=SOURCE.with_name('claim-pnl-earned.json')
RECOVERY=SOURCE.with_name('residual-pnl-canonical.json')
clone=copy.deepcopy

def recovery(sign):
    ident='residual.recovery.short.0.4.disposed' if sign=='short' else 'residual.recovery.long.0.6.disposed'
    template=clone(next(c for c in json.loads(RECOVERY.read_bytes())['cases'] if c['id']==ident))
    book=states.expand(template['request']);owner=clone(template['request']['operation']['input']['cohorts'][0])
    owner.update(acquisition=dict(kind='opening'),disposal_fill=None)
    name='claim.recovery.'+sign;target=clone(owner);target.update(trade_id=name+'.recipient')
    for e in target['entitlements']:e.update(entitled=False,source=r.c.pin(target['trade_id']+'.LEGAL'));e.pop('residual',None)
    for info in r.sets(book):
        h=clone(info['holdings'][0]);h['id']=target['trade_id'];life=h['lifecycle'];life['trade_id']=h['id']
        life['instrument']['terms']['trade_id']=h['id'];h['product']['product']['terms']['trade_id']=h['id']
        for k,s in enumerate(life['events']):
            e=s['event'];e.update(trade_id=h['id'],event_id=h['id']+'.EVENT.'+str(k))
            if e['action']['kind']=='payment':e['action']['terms']['receipt_id']=h['id']+'.RECEIPT.'+str(k)
        p=clone(info['point']['positions'][0]);p['identity'].update(id=h['id'],economics=r.c.pin(h['id']+'.ECONOMICS'),calculation=r.c.pin(h['id']+'.CALC'))
        p['current']['quantity']=owner['quantity'];p['pending_quantity']='0'
        info['holdings'].append(h);info['point']['positions'].append(p)
    other=clone(template['expected']['trades'][0]);other['id']=target['trade_id'];template['expected']['trades'].append(other)
    template['request']=residual.reencode(book,[owner,target],[])
    before=r.c.repoint(book['steps'][2]['after'],name+'.before')
    before['point']['fills']=[];before['point']['positions'][0]['current']['quantity']=owner['quantity'];before['point']['positions'][0]['pending_quantity']='0'
    first=r.c.repoint(before,name+'.first');paid=r.c.repoint(first,name+'.paid')
    key=next(e['key'] for e in owner['entitlements'] if e['key']['kind']=='recovery')
    cash=next(c for c in template['expected']['trades'][0]['marks'][3]['claims'] if c['key']==key)
    residual.receipt(paid,0,dict(key=key),D(cash['outstanding'])/4,name+'.PAID')
    second=r.c.repoint(paid,name+'.returned');single=r.c.repoint(first,name+'.single')
    newbook=r.c.request(before,[('position',first),('event',paid),('position',second)],[])['operation']['input']
    newbook['standalone']=[dict(kind='available',information=single),
        dict(kind='unavailable',reason='Actual original receipt requires the transferred legal ownership.'),
        dict(kind='unavailable',reason='Actual claim return requires its original prior transfer and paid cash prefix.')]
    ent=next(e for e in owner['entitlements'] if e['key']==key)
    def transfer(info,sender,to,fraction,label,points):
        return dict(id=name+'.'+label,origin=owner['trade_id'],key=clone(key),
            proposal_id=original.proposal(info,0,key)['event_id'],entitlement_source=clone(ent['source']),residual_id=ent['residual']['id'],
            **{'from':sender,'to':to},fraction=fraction,cutoff=dict(date=info['point']['valuation_date'],known_at=info['point']['known_at'],
            receipt_ids=[e['action']['terms']['receipt_id'] for e in original.events(info,0,key)]),source=r.c.pin(name+'.LEGAL.'+label),applied_points=points)
    transfers=[transfer(first,owner['trade_id'],target['trade_id'],'0.75','FIRST',[1,2,3,4]),
               transfer(second,target['trade_id'],owner['trade_id'],'0.25','RETURN',[3])]
    result=residual.derive(template,newbook,[owner,target],[],[3]*5)
    result.update(id=name);result['request']['operation']['input'].update(lot_splits=[],claim_transfers=transfers)
    original.allocations(result,newbook,[owner,target],transfers);units(result);canonical.normalize(result)
    return result

def complete_proposals(case):
    if case['expected_error'] is not None:return
    book=states.expand(case['request'])
    for j,trade in enumerate(case['expected']['trades']):
        for i,mark in enumerate(trade['marks']):
            for cash in mark['claims']:
                if cash.get('residual'):cash['residual']['proposal']=original.proposal(r.sets(book)[i],j,cash['key'])

def generate():
    data=json.loads(SOURCE.read_bytes())
    with localcontext() as ctx:
        ctx.prec=60;data['cases'].extend([recovery('short'),recovery('long')])
    for case in data['cases']:complete_proposals(case)
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),recovery_sha256=hashlib.sha256(RECOVERY.read_bytes()).hexdigest(),reference=__doc__)
    for c in data['cases']:
        c['input_nodes']=original.lots.nodes(c['request']);c['input_bytes']=len(json.dumps(c['request'],separators=(',',':')).encode())
    return data

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate()
    raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete original earned controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),max_nodes=max(c['input_nodes'] for c in data['cases']),certified=False)))
