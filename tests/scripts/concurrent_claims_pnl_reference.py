"""Independent concurrent principal/coupon cash over full native custody books.

Each original child/key owns an independently declared residual and paid prefix.
Sequential and returned shares preserve every paid recipient. Complete Decimal60
pricing, cash, curves, models, history, custody and invoice owners are reused.
No candidate executable or result supplies a financial answer or tolerance.
"""
import argparse, copy, hashlib, json
from decimal import Decimal as D, localcontext
import child_cash_pnl_reference as child
import cohort_pnl_reference as r
import cohort_pnl_states as states
import cohort_pnl_bindings as binding
import cohort_pnl_cash_dates as dates
import residual_pnl_reference as residual
import residual_pnl_canonical as canonical
import claim_pnl_reference as claims
import dated_cash_pnl_reference as dated
import lot_pnl_reference as lots
from cohort_pnl_wire import units

SOURCE = residual.FOLDER / 'child-maturity-pnl-admitted.json'
TARGET = residual.FOLDER / 'concurrent-claims-pnl-reference.json'
clone = copy.deepcopy
ZERO = D(0)


def financial(book, owners, links, opening, parts, disposals, facts, transfers):
    # Reuse the complete original custody/model/curve/receipt/invoice calculation
    # with no selected cash, then add each independently sourced legal claim.
    absent = clone(facts[0]); absent['applied_points'] = []
    origin = next(j for j, o in enumerate(owners) if o['trade_id'] == absent['origin'])
    expected = child.financial(book, owners, links, opening, parts, disposals,
                               absent, {'applied_points': []}, origin)
    for i, info in enumerate(r.sets(book)):
        for fact in facts:
            if i not in fact['applied_points']:
                continue
            origin = next(j for j, o in enumerate(owners) if o['trade_id'] == fact['origin'])
            mark = expected['trades'][origin]['marks'][i]
            cash = next(c for c in mark['claims'] if c['key'] == fact['key'])
            ent = next(e for e in owners[origin]['entitlements'] if e['key'] == fact['key'])
            events = claims.events(info, origin, fact['key'])
            excluded = fact['residual']['cutoff']['receipt_ids']
            prior = [e for e in events if e['action']['terms']['receipt_id'] in excluded]
            previous = sum((D(e['action']['terms']['amount']['value']) for e in prior), ZERO)
            fraction = D(fact['residual']['fraction'])
            rv = dict(entitlement=clone(ent), proposal=clone(dated.proposal(info, origin, fact['key'])),
                dated_entitlement=clone(fact), prior_receipts=clone(prior), prior_recorded=previous,
                owned_proposed=(D(cash['amount'])-previous)*fraction,
                owned_recorded=(D(cash['recorded'])-previous)*fraction,
                owned_outstanding=D(cash['outstanding'])*fraction)
            days = D((r.c.day(cash['due'])-r.c.day(info['point']['valuation_date'])).days)
            discount = info['point']['positions'][origin]['cash_discount']['id']
            ratio = (-r.c.curve_zero(info['inputs'], discount)*days/365).exp() if days > 0 else D(1)
            cash.update(entitled=True, residual=rv, ratio=ratio)
            active = [t for t in transfers if t['origin'] == fact['origin'] and t['key'] == fact['key'] and i in t['applied_points']]
            shares = [ZERO for _ in owners]; shares[origin] = D(1)
            def move(t, values):
                sender = next(j for j, o in enumerate(owners) if o['trade_id'] == t['from'])
                recipient = next(j for j, o in enumerate(owners) if o['trade_id'] == t['to'])
                values[sender] -= D(t['fraction']); values[recipient] += D(t['fraction'])
                if min(values) < 0 or sum(values, ZERO) != 1:
                    raise ValueError('Independent legal claim shares are not conserved')
            for t in active:
                move(t, shares)
            paid = [ZERO for _ in owners]; receipts = [[] for _ in owners]
            for event in events:
                rid = event['action']['terms']['receipt_id']
                if rid in excluded:
                    continue
                at = [ZERO for _ in owners]; at[origin] = D(1)
                for t in active:
                    if rid not in t['cutoff']['receipt_ids']:
                        move(t, at)
                for j, share in enumerate(at):
                    if share:
                        amount = D(event['action']['terms']['amount']['value'])*fraction*share
                        paid[j] += amount
                        receipts[j].append(dict(event=clone(event), fraction=share, amount=amount))
            if abs(sum(paid, ZERO)-rv['owned_recorded']) > D('1e-55'):
                raise ValueError('Original paid claim cash is not conserved')
            parties = {origin} | {next(j for j, o in enumerate(owners) if o['trade_id'] == t[k]) for t in active for k in ['from', 'to']}
            for j in sorted(parties):
                m = expected['trades'][j]['marks'][i]; outstanding = rv['owned_outstanding']*shares[j]
                allocation = dict(origin=fact['origin'], key=clone(fact['key']), entitlement=clone(ent),
                    proposal=clone(dated.proposal(info, origin, fact['key'])), transfers=clone(active),
                    fraction=shares[j], receipts=receipts[j], owned_proposed=outstanding+paid[j],
                    owned_recorded=paid[j], owned_outstanding=outstanding, ratio=ratio, pv=outstanding*ratio)
                if j == origin:
                    cash.update(pv=allocation['pv'], transfer=allocation)
                else:
                    m['transferred'].append(allocation)
                m['recorded'] += paid[j]; m['proposed'] += outstanding+paid[j]
                for k in ['current', 'projected']:
                    m['local'][k] += allocation['pv']; m['reporting'][k] += allocation['pv']*D(m['fx'])
        # Native ownership groups use original origin/key order. No financial
        # value changes when transport rows are placed in that declared order.
        for trade in expected['trades']:
            trade['marks'][i]['transferred'].sort(key=lambda a:(a['origin'],a['key']['kind'],a['key']['effective_date'],a['key']['ordinal']))
    binding.explain(expected, dict(cohorts=owners), book)
    return expected


def make(sign, mode):
    source = json.loads(SOURCE.read_bytes())
    seed = clone(next(c for c in source['cases'] if c['id'] == f'maturity.{sign}.4.principal'))
    spec = seed['request']['operation']['input']; book = states.expand(seed['request'])
    book['steps'] = book['steps'][:4]; book['standalone'] = book['standalone'][:4]
    owners = clone(spec['cohorts']); parts = clone(spec['physical_partitions']); disposals = clone(spec['physical_disposals'])
    name = f'concurrent.{sign}.{mode}'; keys = [(j,r.c.key(kind,'2029-05-08',0 if kind=='principal' else 7)) for j in [2,4] for kind in ['principal','bond_coupon']]
    for j, owner in enumerate(owners):
        for e in owner['entitlements']:
            e['entitled'] = any(j == origin and e['key'] == key for origin,key in keys)
    opening = r.opening_from(seed, book)
    facts = []; transfers = []
    def append(cause, label):
        x = r.c.repoint(book['steps'][-1]['after'], name+'.'+label)
        book['steps'].append(dict(cause=cause, after=x))
        book['standalone'].append(dict(kind='unavailable',reason='Original mature cash, ordered paid-prefix and legal share prerequisites do not independently exist at the active baseline.'))
        return x,len(book['steps'])
    def pay(info, selected, label, amounts):
        for k,(j,key) in enumerate(keys):
            if j in selected:
                residual.receipt(info,j,dict(key=key),D(sign)*D(amounts[k]),name+f'.{label}.{k}')
    def acquire(info, at, selected):
        for k,(j,key) in enumerate(keys):
            if j not in selected:
                continue
            ent = next(e for e in owners[j]['entitlements'] if e['key'] == key)
            facts.append(dict(origin=owners[j]['trade_id'],key=clone(key),proposal_id=dated.proposal(info,j,key)['event_id'],
                entitlement_source=clone(ent['source']),residual=dict(id=name+f'.RESIDUAL.{k}',fraction=['0.6','0.7','0.5','0.8'][k],
                    cutoff=dict(date=info['point']['valuation_date'],known_at=info['point']['known_at'],
                        receipt_ids=[e['action']['terms']['receipt_id'] for e in claims.events(info,j,key)])),
                source=r.c.pin(name+f'.CASH.SOURCE.{k}'),applied_points=[at]))
    if mode in ['delayed','staggered']:
        x,at = append('position','mature.cash.acquisition'); acquire(x,at,[2,4] if mode=='delayed' else [4])
    if mode == 'delayed':
        x,at = append('position','delayed.first.transfer')
        first_transfer = at
    x,_ = append('time','original.cash.due'); r.c.set_contexts(x,'2029-05-10')
    if mode in ['prefix','staggered']:
        x,_ = append('event','excluded.original.cash')
        pay(x,[2,4] if mode=='prefix' else [2],'EXCLUDED.A',['10','11','12','13'])
        # Distinct full prefixes on different keys; same-date order is explicit.
        residual.receipt(x,2,dict(key=keys[1][1]),D(sign)*7,name+'.EXCLUDED.B.1')
        x,at = append('position','due.cash.acquisition'); acquire(x,at,[2,4] if mode=='prefix' else [2])
        x,_ = append('event','original.recipient.paid'); pay(x,[2,4],'PAID.0',['20','21','22','23'])
        x,at = append('position','first.transfer'); first_transfer=at
    def transfer_at(at, sender, recipient, fraction, label):
        info=book['steps'][at-1]['after']
        for k,(j,key) in enumerate(keys):
            fact=next(f for f in facts if f['origin']==owners[j]['trade_id'] and f['key']==key)
            transfers.append(dict(id=name+f'.TRANSFER.{label}.{k}',origin=fact['origin'],key=clone(key),proposal_id=fact['proposal_id'],
                entitlement_source=clone(fact['entitlement_source']),residual_id=fact['residual']['id'],
                **{'from':owners[j if sender=='origin' else sender]['trade_id'],'to':owners[j if recipient=='origin' else recipient]['trade_id']},
                fraction=fraction,cutoff=dict(date=info['point']['valuation_date'],known_at=info['point']['known_at'],
                    receipt_ids=[e['action']['terms']['receipt_id'] for e in claims.events(info,j,key)]),
                source=r.c.pin(name+f'.TRANSFER.SOURCE.{label}.{k}'),applied_points=[at]))
    transfer_at(first_transfer,'origin',3,'0.6','FIRST')
    x,_=append('event','first.recipient.paid');pay(x,[2,4],'PAID.1',['30','31','32','33'])
    x,second=append('position','onward.transfer');transfer_at(second,3,5,'0.25','SECOND')
    x,_=append('event','second.recipient.paid');pay(x,[2,4],'PAID.2',['40','41','42','43'])
    x,returned=append('position','returned.transfer');transfer_at(returned,5,'origin','0.25' if mode=='prefix' else '0.1','RETURN')
    x,_=append('event','returned.recipient.paid');pay(x,[2,4],'PAID.3',['50','51','52','53'])
    # All remaining original nominal-security and invoice ingress events retain
    # their complete checked native owners; multiple actual events may share a point.
    initial={s['event']['event_id'] for s in book['before']['holdings'][0]['lifecycle']['events']}
    tail=states.expand(seed['request'])['steps'][-1]['after']['holdings'][0]['lifecycle']['events']
    ingress=[clone(s) for s in tail if s['event']['event_id'] not in initial]
    x,_=append('event','actual.nominal.and.invoice.ingress')
    for k,s in enumerate(ingress):
        e=s['event'];e.update(event_id=name+f'.INGRESS.{k}',effective_date=x['point']['valuation_date'],known_at=x['point']['known_at'],expected_revision=len(x['holdings'][0]['lifecycle']['events']))
        e['action']['terms']['receipt_id']=name+f'.INGRESS.RECEIPT.{k}'
        if e['action']['kind']=='payment':e['action']['terms']['value_date']=e['effective_date']
        x['holdings'][0]['lifecycle']['events'].append(s)
    n=len(book['steps']);assert n<=16
    for f in facts:f['applied_points']=list(range(f['applied_points'][0],n+1))
    for t in transfers:t['applied_points']=list(range(t['applied_points'][0],n+1))
    for k,p in enumerate(parts):p['split']['applied_points']=list(range(k+1,n+1))+([n+1] if k==0 else [])+[n+2,n+3]
    disposals[0]['applied_points']=list(range(3,n+1))+[n+3]
    expected=financial(book,owners,spec['physical_deliveries'],opening,parts,disposals,facts,transfers)
    request=residual.reencode(book,owners,spec['physical_deliveries'])
    request['operation']['input'].update(lot_splits=[],claim_transfers=transfers,physical_partitions=parts,physical_disposals=disposals,dated_entitlements=facts)
    case=dict(id=name,request=request,expected_error=None,expected=expected,maturity_point=4,transfer_points=[first_transfer,second,returned])
    expected['delivery_comparisons']=dates.comparisons(case);units(case)
    wire=request['operation']['input']['book']
    fills={f['id']:f for p in [wire['before']]+[s['after'] for s in wire['steps']]+[s['information'] for s in wire['standalone'] if s['kind']=='available'] for f in p['point']['fills']}
    for trade in expected['trades']:
        for mark in trade['marks']:
            for d in (mark.get('physical') or {}).get('disposals',[]):
                actual=fills[d['fill']['id']]
                assert D(d['fill']['quantity'])==D(actual['quantity'])
                for role in ['consideration','fees']:assert D(d['fill'][role]['value'])==D(actual[role]['value'])
                d['fill']=clone(actual)
    canonical.normalize(case)
    return case


def generate():
    with localcontext() as context:
        context.prec=60
        cases=[make(sign,mode) for sign in [1,-1] for mode in ['delayed','prefix','staggered']]
    old=json.loads(SOURCE.read_bytes())
    for c in old['cases']:
        if c['expected_error'] is None:
            c=clone(c);c['id']='concurrent.regression.'+c['id'];cases.append(c)
    def reject(label,fn,code='invalid_input',index=1):
        c=clone(cases[index]);c.update(id='concurrent.invalid.'+label,expected_error=code);c.pop('expected');fn(c['request']['operation']['input']);cases.append(c)
    fact=lambda s:s['dated_entitlements'][0]
    tr=lambda s:s['claim_transfers']
    reject('duplicate_claim',lambda s:s['dated_entitlements'].append(clone(fact(s))))
    reject('cross_key_paid_prefix',lambda s:fact(s)['residual']['cutoff'].update(receipt_ids=s['dated_entitlements'][1]['residual']['cutoff']['receipt_ids']))
    reject('cross_origin_paid_prefix',lambda s:fact(s)['residual']['cutoff'].update(receipt_ids=s['dated_entitlements'][2]['residual']['cutoff']['receipt_ids']))
    reject('cross_key_proposal',lambda s:fact(s).update(proposal_id=s['dated_entitlements'][1]['proposal_id']))
    reject('cross_origin_proposal',lambda s:fact(s).update(proposal_id=s['dated_entitlements'][2]['proposal_id']))
    reject('duplicate_residual_identity',lambda s:s['dated_entitlements'][1]['residual'].update(id=fact(s)['residual']['id']))
    reject('cross_key_residual',lambda s:tr(s)[0].update(residual_id=s['dated_entitlements'][1]['residual']['id']))
    reject('cross_origin_transfer',lambda s:tr(s)[0].update(origin=s['cohorts'][4]['trade_id']))
    reject('foreign_transfer_prefix',lambda s:tr(s)[4]['cutoff'].update(receipt_ids=tr(s)[5]['cutoff']['receipt_ids']))
    reject('omitted_prior_recipient',lambda s:tr(s)[4]['cutoff']['receipt_ids'].pop())
    reject('reversed_paid_prefix',lambda s:tr(s)[4]['cutoff']['receipt_ids'].reverse())
    reject('duplicate_paid_prefix',lambda s:tr(s)[4]['cutoff']['receipt_ids'].append(tr(s)[4]['cutoff']['receipt_ids'][0]))
    reject('overdrawn_onward',lambda s:tr(s)[4].update(fraction='0.7'))
    reject('overdrawn_return',lambda s:tr(s)[8].update(fraction='0.3'))
    reject('returned_wrong_sender',lambda s:tr(s)[8].update(**{'from':s['cohorts'][1]['trade_id']}))
    reject('returned_self',lambda s:tr(s)[8].update(to=tr(s)[8]['from']))
    reject('missing_first_share',lambda s:tr(s).pop(0))
    reject('missing_second_share',lambda s:tr(s).pop(4))
    reject('reversed_transfer_order',lambda s:tr(s).reverse())
    reject('disappearing_transfer',lambda s:tr(s)[0]['applied_points'].pop())
    reject('disappearing_claim',lambda s:fact(s)['applied_points'].pop())
    reject('unavailable_custody',lambda s:s['physical_partitions'][0]['split']['applied_points'].remove(min(fact(s)['applied_points'])))
    reject('foreign_entitlement',lambda s:fact(s)['entitlement_source'].update(version=99))
    reject('future_return_clock',lambda s:tr(s)[8]['cutoff'].update(known_at='2030-01-01T12:00:00Z'))
    reject('residual_security_face',lambda s:fact(s)['residual'].update(fraction='80000'))
    reject('cached_cash',lambda s:fact(s).update(owned_outstanding='0'))
    def borrowed(s):
        at=min(fact(s)['applied_points'])-1
        s['book']['standalone'][at]=dict(kind='available',information=clone(s['book']['steps'][at]['after']))
    reject('independent_borrows_actual_history',borrowed)
    reject('work_bound',lambda s:s['book'].update(max_product_evaluations=1))
    for c in cases:
        c['input_nodes']=lots.nodes(c['request']);c['input_bytes']=len(json.dumps(c['request'],separators=(',',':'),default=r.c.de.serial).encode())
    return dict(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),tolerances=old['tolerances'],cases=cases,certified=False)


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args()
    data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite of independent financial declaration')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete independent concurrent claim declaration differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
