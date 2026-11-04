"""Separately available original paid prefixes for independent child claim transfers.

The initial trial borrowed later actual payments into an independent Position.
Retain it unchanged. This experiment supplies the complete original paid prefix
at the baseline, explicitly excludes its original recipients, and admits no new
payment event under independent Position. All later payments follow actual Event
causes and every original causal guard remains. Full earned cash on children:

Reuse complete Decimal60 contractual/model/curve/custody owners and unchanged
prior financial controls. Actual receipts, dated residual rights and separately
available transfer prerequisites are supplied explicitly. No candidate is read.
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

SOURCE = residual.FOLDER / 'pending-cash-pnl-bound.json'
TARGET = residual.FOLDER / 'child-cash-pnl-available.json'
clone = copy.deepcopy
ZERO = D(0)


def financial(book, owners, links, opening, parts, disposals, fact, transfer, origin):
    previous = r.native
    r.native = contracts.native
    try:
        expected = r.financial(book, owners, links, opening)
    finally:
        r.native = previous
    recipient = 3
    ent = next(e for e in owners[origin]['entitlements'] if e['key'] == fact['key'])
    for i, info in enumerate(r.sets(book)):
        balances, removed, acquired = custody.custody(info, owners, parts, disposals, i)
        root = clone(expected['trades'][1]['marks'][i]['physical'])
        events = custody.receipt_events(info)
        received = sum((D(e['action']['terms']['face']) for e in events), ZERO)
        root.update(security_receipt=events[-1] if events else None,
                    security_receipts=clone(events))
        for j, trade in enumerate(expected['trades']):
            m = trade['marks'][i]
            m['transferred'] = []
            if j == 0:
                continue
            m.update(recorded=ZERO, proposed=ZERO)
            b = balances.get(owners[j]['trade_id'])
            held = sum(b['receipts'].values(), ZERO) if b else ZERO
            pending = b['pending'] if b else ZERO
            pos = info['point']['positions'][j]
            pos['current']['quantity'] = str(held)
            phys = clone(root) if owners[j]['trade_id'] in acquired else None
            if phys:
                phys.update(proposed_face=held+pending, received_face=held,
                    outstanding_face=pending, outstanding_asset_present_value=
                    D(m['reference'])*pending/D(pos['current']['reference_quantity'])*D(m['remaining']))
                if b['facts']:
                    consumed = any(p['split']['parent'] == owners[j]['trade_id'] for p in b['lineage'])
                    phys['partition'] = dict(facts=clone(b['facts']),
                        original_proposed_face=D(owners[1]['quantity']),
                        original_received_face=received,
                        original_outstanding_face=D(owners[1]['quantity'])-received,
                        fraction=(held+pending)/D(owners[1]['quantity']),
                        receipts=[] if consumed else [dict(event=clone(e),
                            face=b['receipts'].get(e['action']['terms']['receipt_id'], ZERO)) for e in events])
                if len(b['lineage']) > 1:
                    phys['lineage'] = clone(b['lineage'])
                if removed[owners[j]['trade_id']]:
                    phys['disposals'] = clone(removed[owners[j]['trade_id']])
                if j != 1:
                    phys.update(invoice_excluded=True, outstanding_invoice=ZERO, invoice_present_value=ZERO)
            m['physical'] = phys
            if j == 1:
                m['recorded'] = D(root['invoice']['recorded'])
                m['proposed'] = D(root['invoice']['amount'])
            pv = D(phys['invoice_present_value']) if phys and j == 1 else ZERO
            effective = (held+pending)/D(pos['current']['reference_quantity'])*D(m['remaining'])
            model_share = (held+pending)/D(owners[j]['quantity'])*D(m['remaining'])
            product = info['holdings'][j]['product']['product']
            model_zero = r.c.curve_zero(info['inputs'], product['method']['curve']['id']) + D(str(product['method']['decimal_spread']))
            for cash in m['claims']:
                owned = j == origin and cash['key'] == fact['key'] and i in fact['applied_points']
                cash.update(entitled=owned, residual=None, transfer=None, pv=ZERO, ratio=D(1))
                days = D((r.c.day(cash['due'])-r.c.day(pos['current']['valuation_date'] if 'valuation_date' in pos['current'] else info['point']['valuation_date'])).days)
                cash['removed'] = D(cash['amount'])*model_share*(-model_zero*days/365).exp() if days > 0 and m['status'] == 'Active' else ZERO
                pv -= cash['removed']
                if owned:
                    prior = [e for e in claims.events(info, j, fact['key'])
                             if e['action']['terms']['receipt_id'] in fact['residual']['cutoff']['receipt_ids']]
                    paid = sum((D(e['action']['terms']['amount']['value']) for e in prior), ZERO)
                    fraction = D(fact['residual']['fraction'])
                    rv = dict(entitlement=clone(ent), proposal=clone(dated.proposal(info, j, fact['key'])),
                        dated_entitlement=clone(fact), prior_receipts=clone(prior), prior_recorded=paid,
                        owned_proposed=(D(cash['amount'])-paid)*fraction,
                        owned_recorded=(D(cash['recorded'])-paid)*fraction,
                        owned_outstanding=D(cash['outstanding'])*fraction)
                    ratio = (-r.c.curve_zero(info['inputs'], pos['cash_discount']['id'])*days/365).exp() if days > 0 else D(1)
                    cash.update(residual=rv, ratio=ratio, pv=rv['owned_outstanding']*ratio)
                    pv += cash['pv']; m['recorded'] += rv['owned_recorded']; m['proposed'] += rv['owned_proposed']
            value = D(m['reference'])*effective+pv
            m['local'] = dict(current=value, pending=ZERO, projected=value)
            m['reporting'] = {k:v*D(m['fx']) for k,v in m['local'].items()}
            fills = [f for f in info['point']['fills'] if f['trade_id'] == owners[j]['trade_id']]
            m['executed'] = sum((D(f['quantity']) for f in fills), ZERO)
            m['executed_marking'] = D(m['reference'])*m['executed']/D(pos['current']['reference_quantity'])*D(m['remaining'])
            m['consideration'] = sum((D(f['consideration']['value']) for f in fills), ZERO)
            m['fees'] = sum((D(f['fees']['value']) for f in fills), ZERO)
        if i not in fact['applied_points']:
            continue
        rootmark = expected['trades'][origin]['marks'][i]
        cash = next(c for c in rootmark['claims'] if c['key'] == fact['key'])
        rv = cash['residual']; active = [transfer] if i in transfer['applied_points'] else []
        shares = {origin:D('.75'), recipient:D('.25')} if active else {origin:D(1)}
        paid = {j:ZERO for j in shares}; allocations = {j:[] for j in shares}
        for event in claims.events(info, origin, fact['key']):
            rid = event['action']['terms']['receipt_id']
            if rid in fact['residual']['cutoff']['receipt_ids']:
                continue
            at = shares if active and rid not in transfer['cutoff']['receipt_ids'] else {origin:D(1)}
            for j, fraction in at.items():
                amount = D(event['action']['terms']['amount']['value'])*D('.6')*fraction
                paid[j] += amount
                allocations[j].append(dict(event=clone(event), fraction=fraction, amount=amount))
        for j, fraction in shares.items():
            m = expected['trades'][j]['marks'][i]; out = rv['owned_outstanding']*fraction; ratio = D(cash['ratio'])
            allocation = dict(origin=owners[origin]['trade_id'], key=clone(fact['key']), entitlement=clone(ent),
                proposal=clone(dated.proposal(info, origin, fact['key'])), transfers=clone(active), fraction=fraction,
                receipts=allocations[j], owned_proposed=out+paid[j], owned_recorded=paid[j], owned_outstanding=out,
                ratio=ratio, pv=out*ratio)
            if j == origin:
                delta = (out-rv['owned_outstanding'])*ratio
                m['recorded'] += paid[j]-rv['owned_recorded']; m['proposed'] += out+paid[j]-rv['owned_proposed']
                cash.update(pv=out*ratio, transfer=allocation)
            else:
                delta = out*ratio; m['recorded'] += paid[j]; m['proposed'] += out+paid[j]
                m['transferred'].append(allocation)
            for k in ['current','projected']:
                m['local'][k] += delta; m['reporting'][k] += delta*D(m['fx'])
    binding.explain(expected, dict(cohorts=owners), book)
    return expected


def make(sign, origin, delayed):
    old = json.loads(SOURCE.read_bytes())
    seed = clone(next(c for c in old['cases'] if c['id'] == f'pending.{sign}.coupon_partial'))
    spec = seed['request']['operation']['input']; original = states.expand(seed['request'])
    name = f'child.{sign}.{origin}.{"delayed" if delayed else "due"}'
    before = r.c.repoint(original['before'], name+'.before')
    on = '2027-05-08' if delayed else '2027-05-10'; r.c.set_contexts(before, on)
    owners = clone(spec['cohorts']); parts = clone(spec['physical_partitions'])
    key = clone(spec['dated_entitlements'][0]['key'])
    # Native proposals are supplied for every original lot. Old source cash is
    # explicitly excluded; the child has its own original contractual proposal.
    for h in before['holdings'][1:]:
        h['lifecycle']['events'] = [e for e in h['lifecycle']['events'] if e['event']['action']['kind'] != 'payment']
    for j, o in enumerate(owners):
        if j:
            for e in o['entitlements']:
                e['entitled'] = j == origin and e['key'] == key
    prior_id = name+'.PRIOR.PAID'
    if not delayed:
        residual.receipt(before, origin, dict(key=key), D(sign)*10, prior_id)
    opening = r.c.ClaimSet([r.rows(t['marks'][0]) for t in seed['expected']['trades']])
    opening[0] = []
    for rows in opening[1:]:
        for cash in rows:
            cash['recorded'] = ZERO
    if not delayed:
        next(c for c in opening[origin] if r.c.key(c['kind'],c['effective'],c['ordinal']) == key)['recorded'] = D(sign)*10
    opening._events = [0]+[len(h['lifecycle']['events']) for h in before['holdings'][1:]]
    book = {k:clone(v) for k,v in original.items() if k not in ['before','steps','standalone']}
    book.update(before=before, steps=[], standalone=[])
    def append(cause, label):
        info = r.c.repoint(book['steps'][-1]['after'] if book['steps'] else before, name+'.'+label)
        book['steps'].append(dict(cause=cause, after=info))
        book['standalone'].append(dict(kind='unavailable', reason='Original custody/earned proposal/paid-prefix prerequisite unavailable.'))
        return info, len(book['steps'])
    def split(k):
        x, at = append('position', 'custody.'+str(k))
        parts[k]['split'].update(effective_date=on, known_at=x['point']['known_at'], applied_points=[at])
        parts[k]['receipt_ids'] = [e['action']['terms']['receipt_id'] for e in custody.receipt_events(x)]
        return x, at
    split(0)
    if origin == 4:
        split(1)
    x, first = append('position', 'child.cash.acquisition')
    legal = next(e for e in owners[origin]['entitlements'] if e['key'] == key)
    fact = dict(origin=owners[origin]['trade_id'], key=clone(key),
        proposal_id=dated.proposal(x, origin, key)['event_id'], entitlement_source=clone(legal['source']),
        residual=dict(id=name+'.RESIDUAL', fraction='0.6', cutoff=dict(date=on,
            known_at=x['point']['known_at'], receipt_ids=[] if delayed else [prior_id])),
        source=r.c.pin(name+'.LEGAL.CASH'), applied_points=[])
    legal_independent = r.c.repoint(before, name+'.independent.cash')
    book['standalone'][first-1] = dict(kind='available', information=legal_independent)
    paid_id = name+'.PAID.BEFORE.TRANSFER'
    if origin == 2:
        split(1)
    sold, sold_at = append('position', 'dispose.received')
    for p in parts:
        p['split']['applied_points'] = list(range(min(p['split']['applied_points']), sold_at+1))
    balances, _, _ = custody.custody(sold, owners, parts, [], sold_at)
    stock = balances[owners[4]['trade_id']]['receipts']
    allocations = [dict(receipt_id=k, face=str(v/D(2))) for k,v in stock.items() if v]
    face = sum((D(a['face']) for a in allocations), ZERO); fill_id = name+'.DISPOSAL.FILL'
    r.execution(sold, 4, fill_id, -face, D(sign)*D('16811.25'))
    disposals = [dict(id=name+'.LEGAL.DISPOSAL', asset=owners[4]['trade_id'], future=owners[0]['trade_id'],
        delivery_id=parts[0]['delivery_id'], fill_id=fill_id, effective_date=sold['point']['valuation_date'],
        known_at=sold['point']['known_at'], source=r.c.pin(name+'.DISPOSAL.SOURCE'),
        receipt_ids=[e['action']['terms']['receipt_id'] for e in custody.receipt_events(sold)],
        receipts=allocations, applied_points=[sold_at])]
    independent_transfer = r.c.repoint(sold, name+'.independent.transfer')
    moved, move = append('position', 'child.claim.transfer')
    transfer = dict(id=name+'.TRANSFER', origin=owners[origin]['trade_id'], key=clone(key),
        proposal_id=fact['proposal_id'], entitlement_source=clone(legal['source']), residual_id=fact['residual']['id'],
        **{'from':owners[origin]['trade_id'], 'to':owners[3]['trade_id']}, fraction='0.25',
        cutoff=dict(date=moved['point']['valuation_date'], known_at=moved['point']['known_at'],
            receipt_ids=[] if delayed else [prior_id]), source=r.c.pin(name+'.LEGAL.TRANSFER'), applied_points=[])
    book['standalone'][move-1] = dict(kind='available', information=independent_transfer)
    if delayed:
        x, _ = append('time', 'cash.due'); r.c.set_contexts(x, '2027-05-10')
    x, _ = append('event', 'child.paid.after.transfer.first')
    residual.receipt(x, origin, dict(key=key), D(sign)*20, paid_id)
    future = original['steps'][-1]['after']['holdings'][0]
    initial_ids = {s['event']['event_id'] for s in before['holdings'][0]['lifecycle']['events']}
    ingress = [s for s in future['lifecycle']['events'] if s['event']['event_id'] not in initial_ids]
    for k, step in enumerate(ingress):
        x, _ = append('event', 'later.ingress.'+str(k))
        s = clone(step); e = s['event']; e.update(event_id=name+'.INGRESS.'+str(k),
            effective_date=x['point']['valuation_date'], known_at=x['point']['known_at'],
            expected_revision=len(x['holdings'][0]['lifecycle']['events']))
        e['action']['terms']['receipt_id'] = name+'.INGRESS.RECEIPT.'+str(k)
        if e['action']['kind'] == 'payment':
            e['action']['terms']['value_date'] = e['effective_date']
        x['holdings'][0]['lifecycle']['events'].append(s)
        if k == 1:
            x, _ = append('event', 'child.paid.after.transfer')
            residual.receipt(x, origin, dict(key=key), D(sign)*30, name+'.PAID.AFTER.TRANSFER')
    n = len(book['steps']); assert n <= 16
    cash_independent, transfer_independent = n+1, n+2
    fact['applied_points'] = list(range(first,n+1))+[cash_independent,transfer_independent]
    transfer['applied_points'] = list(range(move,n+1))+[transfer_independent]
    for k,p in enumerate(parts):
        active = list(range(min(p['split']['applied_points']),n+1))+[transfer_independent]
        if k == 0 or origin == 4:
            active.append(cash_independent)
        p['split']['applied_points'] = sorted(active)
    disposals[0]['applied_points'] = list(range(sold_at,n+1))+[transfer_independent]
    expected = financial(book, owners, spec['physical_deliveries'], opening, parts, disposals, fact, transfer, origin)
    request = residual.reencode(book, owners, spec['physical_deliveries'])
    request['operation']['input'].update(lot_splits=[], claim_transfers=[transfer],
        physical_partitions=parts, physical_disposals=disposals, dated_entitlements=[fact])
    case = dict(id=name, request=request, expected_error=None, expected=expected)
    expected['delivery_comparisons'] = dates.comparisons(case); units(case)
    for trade in expected['trades']:
        for mark in trade['marks']:
            for d in (mark.get('physical') or {}).get('disposals', []):
                fill = d['fill']; encoded = next(f for p in r.sets(book) for f in p['point']['fills'] if f['id'] == fill['id'])
                assert D(fill['quantity']) == D(encoded['quantity'])
                d['fill'] = clone(encoded)
    canonical.normalize(case)
    return case


def generate():
    old = json.loads(SOURCE.read_bytes())
    with localcontext() as context:
        context.prec = 60
        cases = [make(sign, origin, delayed) for sign in [1,-1]
                 for origin,delayed in [(2,False),(4,False),(4,True)]]
    # Complete prior pending principal/coupon and all original mixed/CTD books.
    for c in old['cases']:
        if c['expected_error'] is None:
            c = clone(c); c['id'] = 'child.regression.'+c['id']; cases.append(c)
    def reject(label, fn, code='invalid_input', index=0):
        c = clone(cases[index]); c.update(id='child.invalid.'+label, expected_error=code)
        c.pop('expected'); fn(c['request']['operation']['input']); cases.append(c)
    fact = lambda s:s['dated_entitlements'][0]
    transfer = lambda s:s['claim_transfers'][0]
    independent = lambda s:next(v['information'] for v in reversed(s['book']['standalone']) if v['kind']=='available')
    reject('missing_dated_child_owner', lambda s:s['dated_entitlements'].clear())
    reject('child_entitlement_excluded', lambda s:next(e for o in s['cohorts'] if o['trade_id']==fact(s)['origin'] for e in o['entitlements'] if e['key']==fact(s)['key']).update(entitled=False))
    reject('foreign_child_proposal', lambda s:fact(s).update(proposal_id='ABSENT'))
    reject('foreign_child_source', lambda s:fact(s)['entitlement_source'].update(version=99))
    reject('paid_prefix_reassignment', lambda s:fact(s)['residual']['cutoff']['receipt_ids'].clear())
    reject('cash_fraction_from_custody', lambda s:fact(s)['residual'].update(fraction='0'))
    reject('future_child_legal_clock', lambda s:fact(s)['residual']['cutoff'].update(known_at='2030-01-01T12:00:00Z'))
    reject('independent_transfer_unacquired', lambda s:s['physical_partitions'][0]['split']['applied_points'].remove(max(transfer(s)['applied_points'])))
    reject('independent_successor_unacquired', lambda s:s['physical_partitions'][1]['split']['applied_points'].remove(max(transfer(s)['applied_points'])), index=1)
    reject('independent_cash_unavailable', lambda s:fact(s)['applied_points'].remove(max(transfer(s)['applied_points'])))
    reject('transfer_changes_prior_recipient', lambda s:transfer(s)['cutoff']['receipt_ids'].pop())
    reject('independent_cash_party_quantity', lambda s:independent(s)['point']['positions'][2]['current'].update(quantity='80000'))
    reject('cash_overallocated', lambda s:transfer(s).update(fraction='1.1'))
    reject('future_transfer_clock', lambda s:transfer(s)['cutoff'].update(known_at='2030-01-01T12:00:00Z'))
    reject('custody_not_cash_source', lambda s:transfer(s).update(proposal_id=s['physical_partitions'][0]['split']['id']))
    reject('disposed_receivable', lambda s:s['physical_disposals'][0]['receipts'][0].update(face='199999'))
    reject('foreign_delivery_owner', lambda s:s['physical_deliveries'].clear(), code='missing_input')
    reject('cached_cash', lambda s:fact(s).update(owned_recorded='0'))
    reject('work_bound', lambda s:s['book'].update(max_product_evaluations=1))
    for c in cases:
        c['input_nodes'] = lots.nodes(c['request'])
        c['input_bytes'] = len(json.dumps(c['request'], separators=(',',':'), default=r.c.de.serial).encode())
    return dict(reference=__doc__, source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
                tolerances=old['tolerances'], cases=cases, certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--write', action='store_true'); args = parser.parse_args()
    data = generate(); raw = (json.dumps(data, indent=2, default=r.c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():
            raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes() != raw:
        raise ValueError('Full independent child cash controls differ')
    print(json.dumps(dict(status='passed', cases=len(data['cases']), financial_books=sum(c['expected_error'] is None for c in data['cases']), certified=False)))
