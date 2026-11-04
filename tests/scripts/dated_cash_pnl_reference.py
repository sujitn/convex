"""Independent dated native coupon/principal ownership across physical custody.

Original complete Decimal60 schedules, profiles, bond/curve/basket/CTD/history
and unchanged financial bounds are reused. Legal fractions, prior paid cash,
actual receipts and disposals are independently supplied. No native executable
or candidate result is read. Custody never determines a cash recipient.
"""
import argparse, copy, hashlib, json
from decimal import Decimal as D, localcontext
import cohort_pnl_reference as r
import cohort_pnl_states as states
import cohort_pnl_bindings as binding
import cohort_pnl_cash_dates as dates
import custody_lineage_pnl_reference as custody
import residual_pnl_reference as residual
import residual_pnl_canonical as canonical
import claim_pnl_reference as claims
import lot_pnl_reference as lots
from cohort_pnl_wire import units
FOLDER=residual.FOLDER;SOURCE=FOLDER/'custody-lineage-pnl-admitted.json';TARGET=FOLDER/'dated-cash-pnl-reference.json'
clone=copy.deepcopy;ZERO=D(0)

def proposal(info,j,key):
    kind='contract_step' if key['kind']=='principal' else 'contractual_cash'
    return next(s['event'] for s in info['holdings'][j]['lifecycle']['events'] if s['event']['action']['kind']==kind and s['event']['effective_date']==key['effective_date'])

def make(sign,mode):
    old=json.loads(SOURCE.read_bytes());base=clone(next(c for c in old['cases'] if c['id']==f'custody.{"long" if sign==1 else "short"}.independent_full.available'))
    spec=base['request']['operation']['input'];book=states.expand(base['request']);owners=clone(spec['cohorts']);parts=clone(spec['physical_partitions']);disposals=clone(spec['physical_disposals']);name=f'dated.{"long" if sign==1 else "short"}.{mode}'
    # All original partial delivery receipts remain full native provenance.
    if mode=='amortization':
        for info in r.sets(book):
            for j,h in enumerate(info['holdings']):
                bonds=[]
                if j:bonds=[h['product']['product']['bond'],h['lifecycle']['instrument']['bond']]
                for step in h['lifecycle']['events']:
                    if step.get('delivery'):bonds.append(step['delivery']['bond'])
                for bond in bonds:
                    t=bond['terms'];t['profiles']['notional']=[dict(start='2025-05-08',end='2027-05-08',value='350'),dict(start='2027-05-08',end='2029-05-08',value='280')]
    selected='2029-05-08' if mode=='redemption' else '2027-05-08';on=selected if mode=='redemption' else '2027-05-10'
    key=r.c.key('bond_coupon' if mode=='coupon' else 'principal',selected,3 if mode=='coupon' else 0)
    def append(cause,label):
        info=r.c.repoint(book['steps'][-1]['after'],name+'.'+label);book['steps'].append(dict(cause=cause,after=info));book['standalone'].append(dict(kind='unavailable',reason='Requires the original earned proposal and exact custody/cash/receipt prerequisites.'));return info,len(book['steps'])
    future,first=append('time','earned');r.c.set_contexts(future,on)
    for j,h in enumerate(future['holdings'][1:],1):
        t=h['lifecycle']['instrument']['bond']['terms'];flows=[f for f in r.c.bond_flows(t,[],on) if f['effective']<=on]
        prior={s['event']['effective_date'] for s in h['lifecycle']['events'] if s['event']['action']['kind']=='contractual_cash'}
        boundaries=sorted({f['effective'] for f in flows if f['effective'] not in prior}|{p['end'] for p in t['profiles']['notional'] if p['end']<=on and p['end']>'2026-12-02'})
        for at in boundaries:
            if any(f['effective']==at and f['kind']=='principal' for f in flows):r.c.append_event(h,at,'contract_step',known=future['point']['known_at'])
            if any(f['effective']==at and f['kind']=='bond_coupon' for f in flows):r.c.append_event(h,at,'contractual_cash',known=future['point']['known_at'])
        for f in flows:
            k=r.c.key(f['kind'],f['effective'],f['ordinal'])
            if not any(e['key']==k for e in owners[j]['entitlements']):owners[j]['entitlements'].append(dict(key=k,entitled=j==1 and k==key,source=r.c.pin(name+'.ENTITLEMENT.'+str(j)+'.'+f['kind']+'.'+str(f['ordinal']))))
    prior,paid_at=append('event','prior.paid');residual.receipt(prior,1,dict(key=key),D(sign)*10,name+'.PRIOR.PAID')
    legal,legal_at=append('position','dated.entitlement');ent=next(e for e in owners[1]['entitlements'] if e['key']==key)
    dated=dict(origin=owners[1]['trade_id'],key=clone(key),proposal_id=proposal(legal,1,key)['event_id'],entitlement_source=clone(ent['source']),residual=dict(id=name+'.RESIDUAL',fraction='0.6',cutoff=dict(date=on,known_at=legal['point']['known_at'],receipt_ids=[name+'.PRIOR.PAID'])),source=r.c.pin(name+'.LEGAL.CASH'),applied_points=[])
    paid,_=append('event','received');residual.receipt(paid,1,dict(key=key),D(sign)*20,name+'.AFTER.PAID')
    moved,move_at=append('position','cash.transfer')
    transfer=dict(id=name+'.TRANSFER',origin=owners[1]['trade_id'],key=clone(key),proposal_id=dated['proposal_id'],entitlement_source=clone(ent['source']),residual_id=dated['residual']['id'],**{'from':owners[1]['trade_id'],'to':owners[3]['trade_id']},fraction='0.25',cutoff=dict(date=on,known_at=moved['point']['known_at'],receipt_ids=[name+'.PRIOR.PAID',name+'.AFTER.PAID']),source=r.c.pin(name+'.LEGAL.TRANSFER'),applied_points=[])
    if mode!='redemption':
        sold,sold_at=append('position','post.entitlement.disposal');target=4;balances,_,_=custody.custody(sold,owners,parts,disposals,3)
        stock=balances[owners[target]['trade_id']]['receipts'];alloc=[dict(receipt_id=k,face=str(v/D(4))) for k,v in stock.items() if v];face=sum((D(a['face']) for a in alloc),ZERO);fill_id=name+'.ACTUAL.DISPOSAL'
        r.execution(sold,target,fill_id,-face,D(sign)*D('1000'))
        disposals.append(dict(id=name+'.DISPOSAL',asset=owners[target]['trade_id'],future=owners[0]['trade_id'],delivery_id=parts[0]['delivery_id'],fill_id=fill_id,effective_date=on,known_at=sold['point']['known_at'],source=r.c.pin(name+'.LEGAL.DISPOSAL'),receipt_ids=[e['action']['terms']['receipt_id'] for e in custody.receipt_events(sold)],receipts=alloc,applied_points=[sold_at]))
    final,_=append('event','later.paid');residual.receipt(final,1,dict(key=key),D(sign)*30,name+'.LATER.PAID')
    count=len(book['steps']);dated['applied_points']=list(range(legal_at,count+1));transfer['applied_points']=list(range(move_at,count+1))
    for p in parts:p['split']['applied_points']=[i for i in p['split']['applied_points'] if i<=3]+list(range(4,count+1))+[count+1+i for i in range(3) if 4+i in p['split']['applied_points']]
    for d in disposals:
        begin=min(d['applied_points']);d['applied_points']=list(range(begin,count+1))+([count+3] if begin==3 else [])
    opening=r.opening_from(base,book);old_inferred=r.inferred
    def inferred(info,j,opening,owner):
        cash,status,remaining,notices=old_inferred(info,j,opening,owner)
        if j==0:status=base['expected']['trades'][0]['marks'][0]['status'];remaining=ZERO
        return cash,status,remaining,notices
    r.inferred=inferred
    try:expected=r.financial(book,owners,spec['physical_deliveries'],opening)
    finally:r.inferred=old_inferred
    for i,info in enumerate(r.sets(book)):
        balances,removed,acquired=custody.custody(info,owners,parts,disposals,i)
        # The original invoice already has its separate immutable paid/outstanding owner.
        root=clone(base['expected']['trades'][1]['marks'][0]['physical'])
        for j,t in enumerate(expected['trades']):
            m=t['marks'][i];m['transferred']=[]
            if j==0:continue
            b=balances[owners[j]['trade_id']];held=sum(b['receipts'].values(),ZERO);pending=b['pending'];info['point']['positions'][j]['current']['quantity']=str(held)
            phys=clone(root) if owners[j]['trade_id'] in acquired else None
            if phys:
                phys.update(proposed_face=held+pending,received_face=held,outstanding_face=pending,outstanding_asset_present_value=D(m['reference'])*pending/D(info['point']['positions'][j]['current']['reference_quantity']))
                if b['facts']:phys['partition']=dict(facts=clone(b['facts']),original_proposed_face=D(owners[1]['quantity']),original_received_face=D(owners[1]['quantity']),original_outstanding_face=ZERO,fraction=(held+pending)/D(owners[1]['quantity']),receipts=[] if any(p['split']['parent']==owners[j]['trade_id'] for p in b['lineage']) else [dict(event=clone(e),face=b['receipts'].get(e['action']['terms']['receipt_id'],ZERO)) for e in custody.receipt_events(info)])
                if len(b['lineage'])>1:phys['lineage']=clone(b['lineage'])
                if removed[owners[j]['trade_id']]:phys['disposals']=clone(removed[owners[j]['trade_id']])
                if j!=1:phys.update(invoice_excluded=True,outstanding_invoice=ZERO,invoice_present_value=ZERO)
            m['physical']=phys;m['recorded']=D(root['invoice']['recorded']) if j==1 else ZERO;m['proposed']=D(root['invoice']['amount']) if j==1 else ZERO
            active=i in dated['applied_points'];cashpv=ZERO
            for cash in m['claims']:
                cash['residual']=None;cash['transfer']=None;cash['entitled']=j==1 and cash['key']==key and active;cash['pv']=ZERO;cash['ratio']=D(1) if cash['ratio'] is not None else None
                # These complete original coupons have paid dates <= valuation date.
                if cash['due']>info['point']['valuation_date']:raise ValueError('Undeclared future earned cash decomposition')
                cash['removed']=ZERO
                if cash['entitled']:
                    past=claims.events(info,j,key);excluded=[e for e in past if e['action']['terms']['receipt_id'] in dated['residual']['cutoff']['receipt_ids']];before=sum((D(e['action']['terms']['amount']['value']) for e in excluded),ZERO);f=D(dated['residual']['fraction'])
                    res=dict(entitlement=clone(ent),proposal=clone(proposal(info,j,key)),dated_entitlement=clone(dated),prior_receipts=clone(excluded),prior_recorded=before,owned_proposed=(D(cash['amount'])-before)*f,owned_recorded=(D(cash['recorded'])-before)*f,owned_outstanding=D(cash['outstanding'])*f);cash['residual']=res;cash['pv']=res['owned_outstanding'];cashpv+=cash['pv'];m['recorded']+=res['owned_recorded'];m['proposed']+=res['owned_proposed']
            cashpv+=D(phys['invoice_present_value']) if phys and not phys['invoice_excluded'] else ZERO
            value=D(m['reference'])*(held+pending)/D(info['point']['positions'][j]['current']['reference_quantity'])*D(m['remaining'])+cashpv
            m['local']=dict(current=value,pending=ZERO,projected=value);m['reporting']={k:v*D(m['fx']) for k,v in m['local'].items()}
            fills=[f for f in info['point']['fills'] if f['trade_id']==owners[j]['trade_id']];m['executed']=sum((D(f['quantity']) for f in fills),ZERO);m['executed_marking']=D(m['reference'])*m['executed']/D(info['point']['positions'][j]['current']['reference_quantity'])*D(m['remaining']);m['consideration']=sum((D(f['consideration']['value']) for f in fills),ZERO);m['fees']=sum((D(f['fees']['value']) for f in fills),ZERO)
        if i not in dated['applied_points']:continue
        source=expected['trades'][1]['marks'][i];cash=next(c for c in source['claims'] if c['key']==key);res=cash['residual'];active=[transfer] if i in transfer['applied_points'] else [];shares=[ZERO,D(1),ZERO,ZERO,ZERO,ZERO]
        for a in active:shares[1]-=D(a['fraction']);shares[3]+=D(a['fraction'])
        paid=[ZERO]*6;allocated=[[] for _ in owners]
        for event in claims.events(info,1,key):
            rid=event['action']['terms']['receipt_id']
            if rid in dated['residual']['cutoff']['receipt_ids']:continue
            at=[ZERO,D(1),ZERO,ZERO,ZERO,ZERO]
            for a in active:
                if rid not in a['cutoff']['receipt_ids']:at[1]-=D(a['fraction']);at[3]+=D(a['fraction'])
            for j,share in enumerate(at):
                if share:
                    amount=D(event['action']['terms']['amount']['value'])*D('.6')*share;paid[j]+=amount;allocated[j].append(dict(event=clone(event),fraction=share,amount=amount))
        for j in {1}|({3} if active else set()):
            m=expected['trades'][j]['marks'][i];out=D(res['owned_outstanding'])*shares[j];allocation=dict(origin=owners[1]['trade_id'],key=clone(key),entitlement=clone(ent),proposal=clone(proposal(info,1,key)),transfers=clone(active),fraction=shares[j],receipts=allocated[j],owned_proposed=out+paid[j],owned_recorded=paid[j],owned_outstanding=out,ratio=D(1),pv=out)
            if j==1:delta=out-D(res['owned_outstanding']);m['recorded']+=paid[j]-res['owned_recorded'];m['proposed']+=out+paid[j]-res['owned_proposed'];cash['pv']=out;cash['transfer']=allocation
            else:delta=out;m['recorded']+=paid[j];m['proposed']+=out+paid[j];m['transferred'].append(allocation)
            for k in ['current','projected']:m['local'][k]+=delta;m['reporting'][k]+=delta*D(m['fx'])
    binding.explain(expected,dict(cohorts=owners),book)
    request=residual.reencode(book,owners,spec['physical_deliveries']);request['operation']['input'].update(lot_splits=[],claim_transfers=[transfer],physical_partitions=parts,physical_disposals=disposals,dated_entitlements=[dated]);units(request)
    case=dict(id=name,request=request,expected_error=None,expected=expected);expected['delivery_comparisons']=[];units(case);canonical.normalize(case);return case

def generate():
    with localcontext() as ctx:
        ctx.prec=60;cases=[make(sign,mode) for sign in [1,-1] for mode in ['coupon','amortization','redemption']]
    # Entire prior causal model/curve/history/credit/optional/CTD bodies survive.
    old=json.loads(SOURCE.read_bytes())
    for c in old['cases']:
        if c['expected_error'] is None and 'regression' in c['id']:
            c=clone(c);c['id']='dated.regression.'+c['id'];c['request']['operation']['input']['dated_entitlements']=[];cases.append(c)
    def reject(label,fn,error='invalid_input'):
        c=clone(cases[0]);c.update(id='dated.invalid.'+label,expected_error=error);c.pop('expected');fn(c['request']['operation']['input']);cases.append(c)
    fact=lambda s:s['dated_entitlements'][0]
    reject('duplicate_claim',lambda s:s['dated_entitlements'].append(clone(fact(s))))
    reject('foreign_origin',lambda s:fact(s).update(origin='ABSENT'))
    reject('foreign_proposal',lambda s:fact(s).update(proposal_id='ABSENT'))
    reject('wrong_entitlement_revision',lambda s:fact(s)['entitlement_source'].update(version=99))
    reject('future_cutoff',lambda s:fact(s)['residual']['cutoff'].update(known_at='2030-01-01T12:00:00Z'))
    reject('negative_fraction',lambda s:fact(s)['residual'].update(fraction='-0.1'))
    reject('overdrawn_fraction',lambda s:fact(s)['residual'].update(fraction='1.1'))
    reject('missing_paid_prefix',lambda s:fact(s)['residual']['cutoff']['receipt_ids'].clear())
    reject('foreign_paid_prefix',lambda s:fact(s)['residual']['cutoff']['receipt_ids'].append('ABSENT'))
    reject('duplicate_paid_prefix',lambda s:fact(s)['residual']['cutoff']['receipt_ids'].append(fact(s)['residual']['cutoff']['receipt_ids'][0]))
    reject('disappearing_entitlement',lambda s:fact(s)['applied_points'].pop())
    reject('wrong_position_cause',lambda s:fact(s)['applied_points'].insert(0,4))
    reject('legacy_and_dated_residual',lambda s:next(e for e in s['cohorts'][1]['entitlements'] if e['key']==fact(s)['key']).update(residual=clone(fact(s)['residual'])),error='missing_input')
    reject('wrong_transfer_residual',lambda s:s['claim_transfers'][0].update(residual_id='ABSENT'))
    reject('transfer_prior_recipient',lambda s:s['claim_transfers'][0]['cutoff']['receipt_ids'].pop())
    reject('ignored_cached_cash',lambda s:fact(s).update(owned_outstanding='0'))
    reject('legal_identity_collision',lambda s:fact(s)['residual'].update(id=s['physical_partitions'][0]['split']['id']))
    for c in cases:c['input_nodes']=lots.nodes(c['request']);c['input_bytes']=len(json.dumps(c['request'],separators=(',',':'),default=r.c.de.serial).encode())
    return dict(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),tolerances=old['tolerances'],cases=cases,certified=False)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Full independent dated cash controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
