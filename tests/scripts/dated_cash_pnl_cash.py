"""Preserve the original bullet-delivery guard and qualify actual amortizing cash.

The retained physical amortization trials hit the original complete invoice
convention guard. Keep those identical requests Unsupported; do not widen the
invoice/pricer. Add full native amortizing cash-bond books, actual full disposal,
independent dated principal residuals and real paid prefixes. Original Decimal60
contract/schedule/calendar/profile/spread owners supply every financial value.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
import dated_cash_pnl_wire as original
import dated_cash_pnl_reference as dated
import cohort_pnl_reference as r
import cohort_pnl_contracts as contracts
import cohort_pnl_states as states
import cohort_pnl_bindings as binding
import residual_pnl_reference as residual
import residual_pnl_canonical as canonical
import claim_pnl_reference as claims
import lot_pnl_reference as lots
from cohort_pnl_wire import units
SOURCE=original.TARGET;TARGET=SOURCE.with_name('dated-cash-pnl-cash.json');clone=copy.deepcopy;ZERO=D(0)
def cash_book(sign):
    template=clone(next(c for c in json.loads(SOURCE.read_bytes())['cases'] if c['id']==f'dated.{"long" if sign==1 else "short"}.amortization'))
    oldbook=states.expand(template['request']);spec=template['request']['operation']['input'];name=f'dated.cash.amortization.{sign}';info=clone(oldbook['before']);root=clone(info['holdings'][1]);p=clone(info['point']['positions'][1]);owner=clone(spec['cohorts'][1]);owner.update(acquisition=dict(kind='opening'),disposal_fill=name+'.ACTUAL.DISPOSAL');p['current']['quantity']=owner['quantity']
    target=clone(root);target['id']=name+'.recipient';life=target['lifecycle'];life['trade_id']=target['id'];life['position']['value']=str(D(sign)*1000)
    for s in life['events']:s['event']['trade_id']=target['id'];s['event']['event_id']=target['id']+'.event.'+str(s['event']['expected_revision'])
    tp=clone(p);tp['identity'].update(id=target['id'],economics=r.c.pin(name+'.RECIPIENT.ECONOMICS'),calculation=r.c.pin(name+'.RECIPIENT.CALCULATION'));tp['current']['quantity']=life['position']['value']
    to=clone(owner);to.update(trade_id=target['id'],quantity=life['position']['value'],disposal_fill=None)
    for e in to['entitlements']:e.update(entitled=False,source=r.c.pin(name+'.RECIPIENT.CASH'))
    info['point'].update(positions=[p,tp],fills=[]);info['holdings']=[root,target];owners=[owner,to]
    book={k:clone(v) for k,v in oldbook.items() if k not in ['before','steps','standalone']};book.update(before=r.c.repoint(info,name+'.before'),steps=[],standalone=[])
    def append(cause,label):
        prior=book['steps'][-1]['after'] if book['steps'] else book['before'];x=r.c.repoint(prior,name+'.'+label);book['steps'].append(dict(cause=cause,after=x));book['standalone'].append(dict(kind='unavailable',reason='Requires original actual disposal, earned principal and complete legal paid prefix.'));return x,len(book['steps'])
    sold,_=append('position','sold');r.execution(sold,0,owner['disposal_fill'],-D(owner['quantity']),D(sign)*201000)
    earned,_=append('time','earned');r.c.set_contexts(earned,'2027-05-10')
    for h in earned['holdings']:
        r.c.append_event(h,'2027-05-08','contract_step',known=earned['point']['known_at']);r.c.append_event(h,'2027-05-08','contractual_cash',known=earned['point']['known_at'])
    key=r.c.key('principal','2027-05-08',0);ent=next(e for e in owner['entitlements'] if e['key']==key)
    paid,_=append('event','prior');residual.receipt(paid,0,dict(key=key),D(sign)*10,name+'.PRIOR')
    legal,first=append('position','cash');fact=clone(spec['dated_entitlements'][0]);fact.update(origin=owner['trade_id'],proposal_id=dated.proposal(legal,0,key)['event_id'],entitlement_source=clone(ent['source']),source=r.c.pin(name+'.LEGAL.CASH'))
    fact['residual'].update(id=name+'.RESIDUAL',cutoff=dict(date='2027-05-10',known_at=legal['point']['known_at'],receipt_ids=[name+'.PRIOR']))
    paid,_=append('event','received');residual.receipt(paid,0,dict(key=key),D(sign)*20,name+'.RECEIVED')
    moved,move=append('position','transfer');transfer=clone(spec['claim_transfers'][0]);transfer.update(id=name+'.TRANSFER',origin=owner['trade_id'],proposal_id=fact['proposal_id'],entitlement_source=clone(ent['source']),residual_id=fact['residual']['id'],**{'from':owner['trade_id'],'to':to['trade_id']},cutoff=dict(date='2027-05-10',known_at=moved['point']['known_at'],receipt_ids=[name+'.PRIOR',name+'.RECEIVED']),source=r.c.pin(name+'.LEGAL.TRANSFER'))
    final,_=append('event','later');residual.receipt(final,0,dict(key=key),D(sign)*30,name+'.LATER')
    n=len(book['steps']);fact['applied_points']=list(range(first,n+1));transfer['applied_points']=list(range(move,n+1))
    opening=r.c.ClaimSet([r.rows(template['expected']['trades'][1]['marks'][0]),[{**c,'amount':D(c['amount'])*D(to['quantity'])/D(owner['quantity']),'recorded':ZERO} for c in r.rows(template['expected']['trades'][1]['marks'][0])]]);opening._events=[len(h['lifecycle']['events']) for h in book['before']['holdings']]
    old=r.native;r.native=contracts.native
    try:expected=r.financial(book,owners,[],opening)
    finally:r.native=old
    for i,x in enumerate(r.sets(book)):
        for j,t in enumerate(expected['trades']):
            m=t['marks'][i];m.update(transferred=[],recorded=ZERO,proposed=ZERO)
            value=D(m['reference'])*D(x['point']['positions'][j]['current']['quantity'])/D(x['point']['positions'][j]['current']['reference_quantity'])
            for c in m['claims']:
                c.update(residual=None,transfer=None,entitled=j==0 and c['key']==key and i in fact['applied_points'],pv=ZERO,ratio=D(1))
                if c['entitled']:
                    prior=claims.events(x,0,key);excluded=[e for e in prior if e['action']['terms']['receipt_id'] in fact['residual']['cutoff']['receipt_ids']];before=sum((D(e['action']['terms']['amount']['value']) for e in excluded),ZERO);f=D(fact['residual']['fraction']);rvalue=dict(entitlement=clone(ent),proposal=clone(dated.proposal(x,0,key)),dated_entitlement=clone(fact),prior_receipts=clone(excluded),prior_recorded=before,owned_proposed=(D(c['amount'])-before)*f,owned_recorded=(D(c['recorded'])-before)*f,owned_outstanding=D(c['outstanding'])*f);c['residual']=rvalue;c['pv']=rvalue['owned_outstanding'];value+=c['pv'];m['recorded']+=rvalue['owned_recorded'];m['proposed']+=rvalue['owned_proposed']
            m['local']=dict(current=value,pending=ZERO,projected=value);m['reporting']={k:v*D(m['fx']) for k,v in m['local'].items()}
        if i not in fact['applied_points']:continue
        root=expected['trades'][0]['marks'][i];c=next(c for c in root['claims'] if c['key']==key);rv=c['residual'];active=[transfer] if i in transfer['applied_points'] else [];shares=[D('.75'),D('.25')] if active else [D(1),ZERO];paid=[ZERO,ZERO];receipts=[[],[]]
        for e in claims.events(x,0,key):
            rid=e['action']['terms']['receipt_id']
            if rid in fact['residual']['cutoff']['receipt_ids']:continue
            at=shares if active and rid not in transfer['cutoff']['receipt_ids'] else [D(1),ZERO]
            for j,f in enumerate(at):
                if f:
                    amount=D(e['action']['terms']['amount']['value'])*D('.6')*f;paid[j]+=amount;receipts[j].append(dict(event=clone(e),fraction=f,amount=amount))
        for j in range(2 if active else 1):
            m=expected['trades'][j]['marks'][i];out=rv['owned_outstanding']*shares[j];a=dict(origin=owner['trade_id'],key=clone(key),entitlement=clone(ent),proposal=clone(dated.proposal(x,0,key)),transfers=clone(active),fraction=shares[j],receipts=receipts[j],owned_proposed=out+paid[j],owned_recorded=paid[j],owned_outstanding=out,ratio=D(1),pv=out)
            if j==0:delta=out-rv['owned_outstanding'];m['recorded']+=paid[j]-rv['owned_recorded'];m['proposed']+=out+paid[j]-rv['owned_proposed'];c['pv']=out;c['transfer']=a
            else:delta=out;m['recorded']+=paid[j];m['proposed']+=out+paid[j];m['transferred'].append(a)
            for k in ['current','projected']:m['local'][k]+=delta;m['reporting'][k]+=delta*D(m['fx'])
    binding.explain(expected,dict(cohorts=owners),book);request=residual.reencode(book,owners,[]);request['operation']['input'].update(lot_splits=[],claim_transfers=[transfer],physical_partitions=[],physical_disposals=[],dated_entitlements=[fact]);case=dict(id=name,request=request,expected_error=None,expected=expected);expected['delivery_comparisons']=[];units(case);canonical.normalize(case);case['input_nodes']=lots.nodes(request);case['input_bytes']=len(json.dumps(request,separators=(',',':')).encode());return case

def generate():
    data=json.loads(SOURCE.read_bytes())
    for c in data['cases']:
        if c['expected_error'] is None and c['id'].endswith('.amortization'):
            c['expected_error']='unsupported';c.pop('expected')
    with localcontext() as ctx:ctx.prec=60;data['cases'] += [cash_book(1),cash_book(-1)]
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete unchanged delivery guard and native amortizing cash differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
