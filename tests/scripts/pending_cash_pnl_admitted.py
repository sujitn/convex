"""Separate actual ingress availability after a Time roll.

Retain the earlier entire control declarations and custody-cutoff failure. Later
security/cash ingress carries its actual supplied point date and knowledge clock,
with original native invoice/security terms and no candidate financial answer.
Latest singular receipts and full empty transfer namespaces are explicit.

Separate valid receipt dates, baseline contractual status and dormant custody.

The initial full controls/failed native receipt trial remain immutable. Original
contract due-date guards are retained. Declare available long/short legal cash and
delayed coupon controls, including a Time roll before real payments.

Full independent pending-delivery cash and available legal Position controls.

Reuse original complete Decimal60 product/schedule/profile/calendar/discount/FX
and cash/custody algebra. Cash rights and paid prefixes are explicitly supplied,
never inferred from held/receivable face. No candidate executable/output is read.
"""
import argparse, copy, hashlib, json
from decimal import Decimal as D, localcontext
import cohort_pnl_reference as r
import cohort_pnl_states as states
import cohort_pnl_contracts as contracts
import cohort_pnl_bindings as binding
import cohort_pnl_cash_dates as dates
import custody_lineage_pnl_reference as custody
import residual_pnl_reference as residual
import residual_pnl_canonical as canonical
import claim_pnl_reference as claims
import dated_cash_pnl_reference as dated
import lot_pnl_reference as lots
from cohort_pnl_wire import units
SOURCE = residual.FOLDER / 'custody-lineage-pnl-admitted.json'
TARGET = residual.FOLDER / 'pending-cash-pnl-admitted.json'
clone = copy.deepcopy
ZERO = D(0)

def make(sign, mode):
    old = json.loads(SOURCE.read_bytes())
    base = clone(next(c for c in old['cases'] if c['id'] ==
                     f'custody.{"long" if sign == 1 else "short"}.successor_partial'))
    spec = base['request']['operation']['input']; original = states.expand(base['request'])
    name = f'pending.{sign}.{mode}'; principal = mode == 'principal'
    delayed = mode == 'coupon_delayed'
    earned = '2029-05-08' if principal else '2027-05-08'
    on = earned if delayed else ('2029-05-10' if principal else '2027-05-10')
    key = r.c.key('principal' if principal else 'bond_coupon', earned, 0 if principal else 3)
    before = r.c.repoint(original['steps'][3]['after'], name + '.before')
    r.c.set_contexts(before, on)
    owners = clone(spec['cohorts']); parts = clone(spec['physical_partitions'])
    future_events = clone(original['steps'][-1]['after']['holdings'][0]['lifecycle']['events'])
    ingress = [s for s in future_events if s['event']['action']['kind'] in ['security_receipt', 'payment']]
    # Independently supplied delayed ingress; the original invoice/security terms
    # and source roles remain. New actual event identities are not old receipts.
    for k, step in enumerate(ingress):
        event = step['event']; event.update(event_id=name+'.INGRESS.'+str(k),
            effective_date=on, known_at=before['point']['known_at'])
        event['action']['terms']['receipt_id'] = name+'.RECEIPT.'+str(k)
        if event['action']['kind'] == 'payment':
            event['action']['terms']['value_date'] = on
    if principal:
        before['holdings'] = before['holdings'][:2]
        before['point']['positions'] = before['point']['positions'][:2]
        owners = owners[:2]; parts = []
        h = clone(before['holdings'][1]); p = clone(before['point']['positions'][1]); o = clone(owners[1])
        h['id'] = name+'.CASH.RECIPIENT'; h['lifecycle']['trade_id'] = h['id']
        h['lifecycle']['position']['value'] = str(D(sign)*1000)
        for s in h['lifecycle']['events']:
            s['event']['trade_id'] = h['id']; s['event']['event_id'] = h['id']+'.EVENT.'+str(s['event']['expected_revision'])
        p['identity'].update(id=h['id'], economics=r.c.pin(h['id']+'.ECONOMICS'), calculation=r.c.pin(h['id']+'.CALCULATION'))
        p['current']['quantity'] = h['lifecycle']['position']['value']
        o.update(trade_id=h['id'], quantity=p['current']['quantity'], acquisition=dict(kind='opening'))
        before['holdings'].append(h); before['point']['positions'].append(p); owners.append(o)
    # Complete earned native proposals before the independent legal experiment.
    opening = r.c.ClaimSet([r.rows(t['marks'][4]) for t in base['expected']['trades'][:len(owners)]])
    for j, h in enumerate(before['holdings'][1:], 1):
        t = h['lifecycle']['instrument']['bond']['terms']
        flows = [f for f in r.c.bond_flows(t, [], on) if f['effective'] <= on]
        previous = {s['event']['effective_date'] for s in h['lifecycle']['events'] if s['event']['action']['kind'] == 'contractual_cash'}
        boundaries = sorted({f['effective'] for f in flows if f['effective'] not in previous})
        pin = h['product']['product']['bond']['calendars']['payment']
        holidays = next(c['holidays'] for c in before['inputs']['calendars'] if c['pin'] == pin)
        for at in boundaries:
            if any(f['effective'] == at and f['kind'] == 'principal' for f in flows):
                r.c.append_event(h, at, 'contract_step', known=before['point']['known_at'])
            if any(f['effective'] == at and f['kind'] == 'bond_coupon' for f in flows):
                r.c.append_event(h, at, 'contractual_cash', known=before['point']['known_at'])
        q = D(owners[j]['quantity']) / D(t['face_value'])
        opening[j] = [dict(kind=f['kind'], effective=f['effective'], ordinal=f['ordinal'],
            due=str(r.c.op.cash_date(t, r.c.day(f['effective']), holidays)), amount=D(f['amount'])*q, recorded=ZERO) for f in flows]
        owners[j]['entitlements'] = [dict(key=r.c.key(f['kind'], f['effective'], f['ordinal']),
            entitled=j == 1 and r.c.key(f['kind'], f['effective'], f['ordinal']) == key,
            source=r.c.pin(name+'.ENTITLEMENT.'+str(j)+'.'+str(k))) for k, f in enumerate(flows)]
    opening[0] = []
    if not delayed:
        residual.receipt(before, 1, dict(key=key), D(sign)*10, name+'.PRIOR.PAID')
        next(c for c in opening[1] if r.c.key(c['kind'], c['effective'], c['ordinal']) == key)['recorded'] = D(sign)*10
    def add_ingress(info, step):
        h = info['holdings'][0]; s = clone(step)
        s['event'].update(effective_date=info['point']['valuation_date'],known_at=info['point']['known_at'])
        if s['event']['action']['kind']=='payment':s['event']['action']['terms']['value_date']=info['point']['valuation_date']
        s['event']['expected_revision'] = len(h['lifecycle']['events'])
        h['lifecycle']['events'].append(s)
    start = 1 if mode == 'coupon_partial' else 0
    for step in ingress[:start]: add_ingress(before, step)
    opening._events = [0] + [len(h['lifecycle']['events']) for h in before['holdings'][1:]]
    book = {k:clone(v) for k,v in original.items() if k not in ['before','steps','standalone']}
    book.update(before=before, steps=[], standalone=[])
    def append(cause, label):
        x = r.c.repoint(book['steps'][-1]['after'] if book['steps'] else before, name+'.'+label)
        book['steps'].append(dict(cause=cause, after=x))
        book['standalone'].append(dict(kind='unavailable', reason='Requires original actual custody/receipt/fill/cash prerequisites.'))
        return x, len(book['steps'])
    ent = next(e for e in owners[1]['entitlements'] if e['key'] == key)
    legal, first = append('position', 'cash.acquisition')
    fact = dict(origin=owners[1]['trade_id'], key=clone(key),
        proposal_id=dated.proposal(legal, 1, key)['event_id'], entitlement_source=clone(ent['source']),
        residual=dict(id=name+'.RESIDUAL', fraction='0.6', cutoff=dict(date=on,
            known_at=before['point']['known_at'], receipt_ids=[] if delayed else [name+'.PRIOR.PAID'])),
        source=r.c.pin(name+'.LEGAL.CASH'), applied_points=[])
    book['standalone'][0] = dict(kind='available', information=r.c.repoint(before, name+'.independent.cash'))
    if not start:
        x,_ = append('event', 'partial.security'); add_ingress(x, ingress[0])
    if not principal:
        x,p1 = append('position', 'custody')
        parts[0]['split'].update(effective_date=on, known_at=x['point']['known_at'], applied_points=[p1])
        parts[0]['receipt_ids'] = [e['action']['terms']['receipt_id'] for e in custody.receipt_events(x)]
        parts[1]['split'].update(effective_date=on, known_at=x['point']['known_at'], applied_points=[])
    if delayed:
        x,_ = append('time', 'payment.date');r.c.set_contexts(x,'2027-05-10')
    paid,_ = append('event', 'paid.before.transfer'); residual.receipt(paid,1,dict(key=key),D(sign)*20,name+'.PAID.BEFORE.TRANSFER')
    disposals = []
    if not principal:
        x,p2 = append('position', 'successors')
        parts[0]['split']['applied_points'] += [p2-1,p2]
        parts[1]['split']['applied_points'] = [p2]
        parts[1]['receipt_ids'] = [e['action']['terms']['receipt_id'] for e in custody.receipt_events(x)]
        sold,sold_at = append('position', 'dispose.received')
        for p in parts:p['split']['applied_points'] += [sold_at]
        balances,_,_ = custody.custody(sold,owners,parts,[],sold_at)
        stock = balances[owners[4]['trade_id']]['receipts']
        allocations = [dict(receipt_id=k,face=str(v/D(2))) for k,v in stock.items() if v]
        face = sum((D(a['face']) for a in allocations),ZERO); fill_id=name+'.ACTUAL.DISPOSAL'
        r.execution(sold,4,fill_id,-face,D(sign)*D('16811.25'))
        disposals.append(dict(id=name+'.LEGAL.DISPOSAL',asset=owners[4]['trade_id'],future=owners[0]['trade_id'],
            delivery_id=parts[0]['delivery_id'],fill_id=fill_id,effective_date=sold['point']['valuation_date'],known_at=sold['point']['known_at'],
            source=r.c.pin(name+'.DISPOSAL.SOURCE'),receipt_ids=[e['action']['terms']['receipt_id'] for e in custody.receipt_events(sold)],
            receipts=allocations,applied_points=[sold_at]))
    moved,move = append('position','cash.transfer'); recipient = 2 if principal else 3
    transfer = dict(id=name+'.TRANSFER',origin=owners[1]['trade_id'],key=clone(key),proposal_id=fact['proposal_id'],
        entitlement_source=clone(ent['source']),residual_id=fact['residual']['id'],
        **{'from':owners[1]['trade_id'],'to':owners[recipient]['trade_id']},fraction='0.25',
        cutoff=dict(date=moved['point']['valuation_date'],known_at=moved['point']['known_at'],receipt_ids=([name+'.PRIOR.PAID'] if not delayed else [])+[name+'.PAID.BEFORE.TRANSFER']),
        source=r.c.pin(name+'.LEGAL.TRANSFER'),applied_points=[])
    for k, step in enumerate(ingress[1:]):
        x,_ = append('event','ingress.'+str(k+1)); add_ingress(x,step)
        if k == 1:
            x,_ = append('event','cash.after.ingress');residual.receipt(x,1,dict(key=key),D(sign)*30,name+'.PAID.AFTER.TRANSFER')
    n = len(book['steps']); independent = n+1
    fact['applied_points'] = list(range(first,n+1))+[independent]
    transfer['applied_points'] = list(range(move,n+1))
    for p in parts:p['split']['applied_points'] = list(range(min(p['split']['applied_points']),n+1))
    for d in disposals:d['applied_points'] = list(range(min(d['applied_points']),n+1))
    previous = (r.native,r.inferred)
    def inferred(info,j,opening,owner):
        cash,status,remaining,notices=previous[1](info,j,opening,owner)
        if j and principal:
            h=info['holdings'][j];end=h['lifecycle']['instrument']['bond']['terms']['schedule']['end_date']
            assert any(s['event']['action']['kind']=='contract_step' and s['event']['effective_date']==end for s in h['lifecycle']['events'])
            status='Matured';remaining=ZERO
        return cash,status,remaining,notices
    r.native=contracts.native;r.inferred=inferred
    try:expected = r.financial(book,owners,spec['physical_deliveries'],opening)
    finally:r.native,r.inferred = previous
    for i, info in enumerate(r.sets(book)):
        balances, removed, acquired = custody.custody(info,owners,parts,disposals,i) if not principal else (
            {owners[1]['trade_id']:dict(receipts={e['action']['terms']['receipt_id']:D(e['action']['terms']['face']) for e in custody.receipt_events(info)},pending=D(owners[1]['quantity'])-sum((D(e['action']['terms']['face']) for e in custody.receipt_events(info)),ZERO),facts=None,lineage=[])}, {owners[1]['trade_id']:[]}, {owners[1]['trade_id']})
        root = clone(expected['trades'][1]['marks'][i]['physical']); events = custody.receipt_events(info)
        received = sum((D(e['action']['terms']['face']) for e in events),ZERO)
        root.update(security_receipt=events[-1] if events else None, security_receipts=clone(events))
        for j,t in enumerate(expected['trades']):
            if j == 0:
                t['marks'][i]['transferred']=[]
                continue
            m = t['marks'][i];m.update(transferred=[],recorded=ZERO,proposed=ZERO)
            b = balances.get(owners[j]['trade_id']);held = sum(b['receipts'].values(),ZERO) if b else (D(owners[j]['quantity']) if principal else ZERO);pending=b['pending'] if b else ZERO
            info['point']['positions'][j]['current']['quantity'] = str(held)
            phys = clone(root) if owners[j]['trade_id'] in acquired else None
            if phys:
                phys.update(proposed_face=held+pending,received_face=held,outstanding_face=pending,
                    outstanding_asset_present_value=D(m['reference'])*pending/D(info['point']['positions'][j]['current']['reference_quantity'])*D(m['remaining']))
                if b['facts']:
                    consumed=any(p['split']['parent']==owners[j]['trade_id'] for p in b['lineage'])
                    phys['partition']=dict(facts=clone(b['facts']),original_proposed_face=D(owners[1]['quantity']),
                        original_received_face=received,original_outstanding_face=D(owners[1]['quantity'])-received,
                        fraction=(held+pending)/D(owners[1]['quantity']),receipts=[] if consumed else [dict(event=clone(e),face=b['receipts'].get(e['action']['terms']['receipt_id'],ZERO)) for e in events])
                if len(b['lineage'])>1:phys['lineage']=clone(b['lineage'])
                if removed[owners[j]['trade_id']]:phys['disposals']=clone(removed[owners[j]['trade_id']])
                if j != 1:phys.update(invoice_excluded=True,outstanding_invoice=ZERO,invoice_present_value=ZERO)
            m['physical']=phys
            if j==1:m['recorded']=D(root['invoice']['recorded']);m['proposed']=D(root['invoice']['amount'])
            pv = D(phys['invoice_present_value']) if phys and j==1 else ZERO
            effective=(held+pending)/D(info['point']['positions'][j]['current']['reference_quantity'])*D(m['remaining'])
            for cash in m['claims']:
                cash.update(entitled=j==1 and cash['key']==key and i in fact['applied_points'],residual=None,transfer=None,pv=ZERO)
                ratio=(-r.c.curve_zero(info['inputs'],info['point']['positions'][j]['cash_discount']['id'])*D((r.c.day(cash['due'])-r.c.day(info['point']['valuation_date'])).days)/365).exp() if cash['due']>info['point']['valuation_date'] else D(1)
                cash['ratio']=ratio
                cash['removed']=D(cash['amount'])*effective*ratio if cash['due']>info['point']['valuation_date'] and m['status']=='Active' else ZERO
                pv-=cash['removed']
                if cash['entitled']:
                    receipts=claims.events(info,j,key);prior=[e for e in receipts if e['action']['terms']['receipt_id'] in fact['residual']['cutoff']['receipt_ids']]
                    before_paid=sum((D(e['action']['terms']['amount']['value']) for e in prior),ZERO);f=D(fact['residual']['fraction'])
                    rv=dict(entitlement=clone(ent),proposal=clone(dated.proposal(info,j,key)),dated_entitlement=clone(fact),prior_receipts=clone(prior),prior_recorded=before_paid,
                        owned_proposed=(D(cash['amount'])-before_paid)*f,owned_recorded=(D(cash['recorded'])-before_paid)*f,owned_outstanding=D(cash['outstanding'])*f)
                    cash['residual']=rv;cash['pv']=rv['owned_outstanding']*ratio;pv+=cash['pv'];m['recorded']+=rv['owned_recorded'];m['proposed']+=rv['owned_proposed']
            value=D(m['reference'])*effective+pv;m['local']=dict(current=value,pending=ZERO,projected=value);m['reporting']={k:v*D(m['fx']) for k,v in m['local'].items()}
            fills=[f for f in info['point']['fills'] if f['trade_id']==owners[j]['trade_id']]
            m['executed']=sum((D(f['quantity']) for f in fills),ZERO);m['executed_marking']=D(m['reference'])*m['executed']/D(info['point']['positions'][j]['current']['reference_quantity'])*D(m['remaining'])
            m['consideration']=sum((D(f['consideration']['value']) for f in fills),ZERO);m['fees']=sum((D(f['fees']['value']) for f in fills),ZERO)
        if i not in fact['applied_points']:continue
        rootmark=expected['trades'][1]['marks'][i];cash=next(c for c in rootmark['claims'] if c['key']==key);rv=cash['residual']
        active=[transfer] if i in transfer['applied_points'] else [];shares={1:D('.75'),recipient:D('.25')} if active else {1:D(1)}
        paid={j:ZERO for j in shares};alloc={j:[] for j in shares}
        for e in claims.events(info,1,key):
            rid=e['action']['terms']['receipt_id']
            if rid in fact['residual']['cutoff']['receipt_ids']:continue
            at=shares if active and rid not in transfer['cutoff']['receipt_ids'] else {1:D(1)}
            for j,f in at.items():
                amount=D(e['action']['terms']['amount']['value'])*D('.6')*f;paid[j]+=amount;alloc[j].append(dict(event=clone(e),fraction=f,amount=amount))
        for j,f in shares.items():
            m=expected['trades'][j]['marks'][i];out=rv['owned_outstanding']*f;ratio=D(cash['ratio'])
            a=dict(origin=owners[1]['trade_id'],key=clone(key),entitlement=clone(ent),proposal=clone(dated.proposal(info,1,key)),transfers=clone(active),fraction=f,receipts=alloc[j],owned_proposed=out+paid[j],owned_recorded=paid[j],owned_outstanding=out,ratio=ratio,pv=out*ratio)
            if j==1:delta=(out-rv['owned_outstanding'])*ratio;m['recorded']+=paid[j]-rv['owned_recorded'];m['proposed']+=out+paid[j]-rv['owned_proposed'];cash['pv']=out*ratio;cash['transfer']=a
            else:delta=out*ratio;m['recorded']+=paid[j];m['proposed']+=out+paid[j];m['transferred'].append(a)
            for k in ['current','projected']:m['local'][k]+=delta;m['reporting'][k]+=delta*D(m['fx'])
    binding.explain(expected,dict(cohorts=owners),book)
    request=residual.reencode(book,owners,spec['physical_deliveries']);request['operation']['input'].update(
        lot_splits=[],claim_transfers=[transfer],physical_partitions=parts,physical_disposals=disposals,dated_entitlements=[fact])
    case=dict(id=name,request=request,expected_error=None,expected=expected)
    expected['delivery_comparisons']=dates.comparisons(case);units(case)
    # Bind exact supplied fill representation after proving Decimal/Money identity.
    for trade in expected['trades']:
        for mark in trade['marks']:
            for d in (mark.get('physical') or {}).get('disposals',[]):
                original_fill=d['fill'];encoded=next(f for p in [request['operation']['input']['book']['before']]+[s['after'] for s in request['operation']['input']['book']['steps']] for f in p['point']['fills'] if f['id']==original_fill['id'])
                assert D(original_fill['quantity'])==D(encoded['quantity'])
                assert original_fill['consideration']==encoded['consideration'] and original_fill['fees']==encoded['fees']
                d['fill']=clone(encoded)
    canonical.normalize(case);return case

def generate():
    with localcontext() as ctx:
        ctx.prec=60;cases=[make(sign,mode) for sign in [1,-1] for mode in ['coupon_unreceived','coupon_partial','principal','coupon_delayed']]
    old=json.loads((residual.FOLDER/'dated-cash-pnl-cash.json').read_bytes())
    for c in old['cases']:
        if c['expected_error'] is None and 'regression' in c['id']:
            c=clone(c);c['id']='pending.regression.'+c['id'];cases.append(c)
    def reject(label, fn, source=0, error='invalid_input'):
        c=clone(cases[source]);c.update(id='pending.invalid.'+label,expected_error=error);c.pop('expected');fn(c['request']['operation']['input']);cases.append(c)
    fact=lambda s:s['dated_entitlements'][0]
    independent=lambda s:s['book']['standalone'][0]['information']
    reject('missing_independent_cash',lambda s:fact(s)['applied_points'].pop())
    reject('future_legal_clock',lambda s:fact(s)['residual']['cutoff'].update(known_at='2030-01-01T12:00:00Z'))
    reject('missing_paid_prefix',lambda s:fact(s)['residual']['cutoff']['receipt_ids'].clear())
    reject('unavailable_original_party',lambda s:s['physical_deliveries'].clear(),error='missing_input')
    reject('foreign_proposal',lambda s:fact(s).update(proposal_id='ABSENT'))
    reject('wrong_revision',lambda s:fact(s)['entitlement_source'].update(version=99))
    reject('custody_as_cash_fraction',lambda s:fact(s)['residual'].update(fraction='0'))
    reject('independent_receipt_rewrite',lambda s:next(e for e in s['book']['holdings'][independent(s)['holdings'][1]]['lifecycle']['events'] if e['event']['action']['kind']=='payment')['event']['action']['terms']['amount'].update(value='11'))
    reject('independent_missing_proposal',lambda s:s['book']['holdings'][independent(s)['holdings'][1]]['lifecycle']['events'].pop())
    reject('invented_security_quantity',lambda s:independent(s)['point']['positions'][1]['current'].update(quantity='200000'))
    reject('missing_ingress_prefix',lambda s:s['physical_partitions'][1]['receipt_ids'].clear())
    reject('dispose_receivable',lambda s:s['physical_disposals'][0]['receipts'][0].update(face='199999'))
    reject('transfer_paid_recipient',lambda s:s['claim_transfers'][0]['cutoff']['receipt_ids'].pop())
    reject('foreign_invoice_owner',lambda s:s['cohorts'][0]['entitlements'][1].update(entitled=False))
    reject('no_overdue_policy',lambda s:[p.update(overdue='reject') for b in [s['book']['before']]+[t['after'] for t in s['book']['steps']] for p in b['point']['positions']],error='missing_input')
    reject('cached_cash',lambda s:fact(s).update(owned_outstanding='0'))
    reject('work_bound',lambda s:s['book'].update(max_product_evaluations=1),error='resource_limit')
    for c in cases:c['input_nodes']=lots.nodes(c['request']);c['input_bytes']=len(json.dumps(c['request'],separators=(',',':'),default=r.c.de.serial).encode())
    return dict(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),tolerances=old['tolerances'],cases=cases,certified=False)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Full independent pending cash controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
