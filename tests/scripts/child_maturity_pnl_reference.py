"""Independent whole custody-child principal and final coupon through maturity.

Original complete Decimal60 contractual cash, pricing, custody, disposal,
invoice, legal share, receipt and causal P&L owners are reused. All positions
are acquired/disposed while the original security is active. Native final
contractual boundaries are supplied under actual Time, followed by real cash
and nominal delivery receipts. Matured legal Position experiments cannot borrow
those later Time/Event facts: their standalone outcome is explicitly Unavailable.
Earlier custody/disposal have complete independently available baseline facts.
No candidate executable, financial answer or tolerance is read or fitted.
"""
import argparse, copy, hashlib, json
from decimal import Decimal as D, localcontext
import child_cash_pnl_available as child
import cohort_pnl_reference as r
import cohort_pnl_states as states
import cohort_pnl_cash_dates as dates
import custody_lineage_pnl_reference as custody
import residual_pnl_reference as residual
import residual_pnl_canonical as canonical
import dated_cash_pnl_reference as dated
import lot_pnl_reference as lots
from cohort_pnl_wire import units

SOURCE = residual.FOLDER / 'child-cash-pnl-admitted.json'
CUSTODY = residual.FOLDER / 'custody-lineage-pnl-admitted.json'
TARGET = residual.FOLDER / 'child-maturity-pnl-reference.json'
clone = copy.deepcopy
ZERO = D(0)


def make(sign, origin, kind):
    old = json.loads(CUSTODY.read_bytes())
    seed = clone(next(c for c in old['cases'] if c['id'] ==
                     f'custody.{"long" if sign == 1 else "short"}.successor_partial'))
    spec = seed['request']['operation']['input']; original = states.expand(seed['request'])
    name = f'maturity.{sign}.{origin}.{kind}'
    delayed = origin != 2
    before = r.c.repoint(original['steps'][3]['after'], name+'.before')
    on, end, due = '2029-05-07', '2029-05-08', '2029-05-10'
    r.c.set_contexts(before, on)
    owners = clone(spec['cohorts']); parts = clone(spec['physical_partitions'])
    key = r.c.key(kind, end, 0 if kind == 'principal' else 7)
    opening = r.c.ClaimSet([[] for _ in owners])
    # Complete original earned cash before maturity, and exact complete legal
    # coverage for the future principal/final coupon. Custody owns no cash.
    for j, h in enumerate(before['holdings'][1:], 1):
        h['lifecycle']['events'] = [e for e in h['lifecycle']['events']
                                    if e['event']['action']['kind'] != 'payment']
        t = h['lifecycle']['instrument']['bond']['terms']
        flows = r.c.bond_flows(t, [], end)
        available = [f for f in flows if f['effective'] <= on]
        existing = {(s['event']['action']['kind'], s['event']['effective_date'])
                    for s in h['lifecycle']['events']}
        for at in sorted({f['effective'] for f in available}):
            for action, cash_kind in [('contract_step', 'principal'), ('contractual_cash', 'bond_coupon')]:
                if any(f['kind'] == cash_kind and f['effective'] == at for f in available) and (action, at) not in existing:
                    r.c.append_event(h, at, action, known=before['point']['known_at'])
        pin = h['product']['product']['bond']['calendars']['payment']
        holidays = next(c['holidays'] for c in before['inputs']['calendars'] if c['pin'] == pin)
        q = D(owners[j]['quantity']) / D(t['face_value'])
        opening[j] = [dict(kind=f['kind'], effective=f['effective'], ordinal=f['ordinal'],
            due=str(r.c.op.cash_date(t, r.c.day(f['effective']), holidays)),
            amount=D(f['amount'])*q, recorded=ZERO) for f in available]
        owners[j]['entitlements'] = [dict(key=r.c.key(f['kind'], f['effective'], f['ordinal']),
            entitled=j == origin and r.c.key(f['kind'], f['effective'], f['ordinal']) == key,
            source=r.c.pin(name+'.ENTITLEMENT.'+str(j)+'.'+str(k))) for k, f in enumerate(flows)]
    # The actual future retains its original checked invoice, notice, security,
    # CF/accrual/rounding and initial partial ingress. New ingress is supplied later.
    initial = {s['event']['event_id'] for s in before['holdings'][0]['lifecycle']['events']}
    ingress = [clone(s) for s in original['steps'][-1]['after']['holdings'][0]['lifecycle']['events']
               if s['event']['event_id'] not in initial]
    # A separately declared initial partial receipt is available at the active
    # baseline; the initial trial incorrectly assumed it was already present.
    initial_receipt = ingress.pop(0)
    assert initial_receipt['event']['action']['kind'] == 'security_receipt'
    e = initial_receipt['event']; e.update(event_id=name+'.INITIAL.INGRESS',
        effective_date=on, known_at=before['point']['known_at'],
        expected_revision=len(before['holdings'][0]['lifecycle']['events']))
    e['action']['terms']['receipt_id'] = name+'.INITIAL.SECURITY.RECEIPT'
    before['holdings'][0]['lifecycle']['events'].append(initial_receipt)
    opening._events = [0]+[len(h['lifecycle']['events']) for h in before['holdings'][1:]]
    book = {k:clone(v) for k,v in original.items() if k not in ['before','steps','standalone']}
    book.update(before=before, steps=[], standalone=[])
    def append(cause, label):
        x = r.c.repoint(book['steps'][-1]['after'] if book['steps'] else before, name+'.'+label)
        book['steps'].append(dict(cause=cause, after=x))
        book['standalone'].append(dict(kind='unavailable', reason=
            'Original earned maturity proposal and later Time/Event/receipt prerequisites do not independently exist at the active baseline.'))
        return x, len(book['steps'])
    for k, p in enumerate(parts):
        x, at = append('position', 'custody.'+str(k))
        p['split'].update(effective_date=on, known_at=x['point']['known_at'], applied_points=[at])
        p['receipt_ids'] = [e['action']['terms']['receipt_id'] for e in custody.receipt_events(x)]
        book['standalone'][at-1] = dict(kind='available', information=r.c.repoint(before, name+'.independent.custody.'+str(k)))
    sold, sold_at = append('position', 'dispose.received')
    for p in parts:p['split']['applied_points'] = list(range(min(p['split']['applied_points']), sold_at+1))
    balances, _, _ = custody.custody(sold, owners, parts, [], sold_at)
    stock = balances[owners[4]['trade_id']]['receipts']
    allocations = [dict(receipt_id=k, face=str(v/D(2))) for k,v in stock.items() if v]
    face = sum((D(a['face']) for a in allocations), ZERO); fill_id = name+'.ACTUAL.DISPOSAL'
    r.execution(sold, 4, fill_id, -face, D(sign)*D('16811.25'))
    disposals = [dict(id=name+'.LEGAL.DISPOSAL', asset=owners[4]['trade_id'], future=owners[0]['trade_id'],
        delivery_id=parts[0]['delivery_id'], fill_id=fill_id, effective_date=on, known_at=sold['point']['known_at'],
        source=r.c.pin(name+'.DISPOSAL.SOURCE'), receipt_ids=[e['action']['terms']['receipt_id'] for e in custody.receipt_events(sold)],
        receipts=allocations, applied_points=[sold_at])]
    independent_sold = r.c.repoint(before, name+'.independent.disposal')
    r.execution(independent_sold, 4, fill_id, -face, D(sign)*D('16811.25'))
    book['standalone'][sold_at-1] = dict(kind='available', information=independent_sold)
    mature, mature_at = append('time', 'native.maturity'); r.c.set_contexts(mature, end)
    for h in mature['holdings'][1:]:
        r.c.append_event(h, end, 'contract_step', known=mature['point']['known_at'])
        r.c.append_event(h, end, 'contractual_cash', known=mature['point']['known_at'])
    # At maturity the final earned cash is unpaid and still discounted to its
    # original delayed May10 cash date. This is not a fresh active-security price.
    excluded = []
    if not delayed:
        x, _ = append('time', 'cash.due.before.acquisition'); r.c.set_contexts(x, due)
        x, _ = append('event', 'prior.excluded.recipient')
        excluded = [name+'.PRIOR.EXCLUDED']
        residual.receipt(x, origin, dict(key=key), D(sign)*10, excluded[0])
    legal, first = append('position', 'earned.child.cash')
    ent = next(e for e in owners[origin]['entitlements'] if e['key'] == key)
    fact = dict(origin=owners[origin]['trade_id'], key=clone(key),
        proposal_id=dated.proposal(legal, origin, key)['event_id'], entitlement_source=clone(ent['source']),
        residual=dict(id=name+'.RESIDUAL', fraction='0.6', cutoff=dict(date=legal['point']['valuation_date'],
            known_at=legal['point']['known_at'], receipt_ids=clone(excluded))),
        source=r.c.pin(name+'.LEGAL.CASH'), applied_points=[])
    if delayed:
        cashdate, _ = append('time', 'cash.due'); r.c.set_contexts(cashdate, due)
    prior, _ = append('event', 'original.paid.recipient')
    prior_id = name+'.PAID.BEFORE.TRANSFER'
    residual.receipt(prior, origin, dict(key=key), D(sign)*20, prior_id)
    moved, move = append('position', 'earned.claim.transfer')
    transfer = dict(id=name+'.TRANSFER', origin=owners[origin]['trade_id'], key=clone(key),
        proposal_id=fact['proposal_id'], entitlement_source=clone(ent['source']), residual_id=fact['residual']['id'],
        **{'from':owners[origin]['trade_id'], 'to':owners[3]['trade_id']}, fraction='0.25',
        cutoff=dict(date=due, known_at=moved['point']['known_at'], receipt_ids=excluded+[prior_id]),
        source=r.c.pin(name+'.LEGAL.TRANSFER'), applied_points=[])
    for k, step in enumerate(ingress):
        x, _ = append('event', 'later.ingress.'+str(k))
        s = clone(step); e = s['event']; e.update(event_id=name+'.INGRESS.'+str(k),
            effective_date=x['point']['valuation_date'], known_at=x['point']['known_at'],
            expected_revision=len(x['holdings'][0]['lifecycle']['events']))
        e['action']['terms']['receipt_id'] = name+'.INGRESS.RECEIPT.'+str(k)
        if e['action']['kind'] == 'payment':e['action']['terms']['value_date'] = e['effective_date']
        x['holdings'][0]['lifecycle']['events'].append(s)
        if k == 1:
            paid, _ = append('event', 'later.paid.recipient')
            residual.receipt(paid, origin, dict(key=key), D(sign)*30, name+'.PAID.AFTER.TRANSFER')
    n = len(book['steps']); assert n <= 16
    fact['applied_points'] = list(range(first, n+1))
    transfer['applied_points'] = list(range(move, n+1))
    for k,p in enumerate(parts):
        p['split']['applied_points'] = list(range(k+1,n+1))+[n+2,n+3]
        if k == 0:p['split']['applied_points'].append(n+1)
        p['split']['applied_points'].sort()
    disposals[0]['applied_points'] = list(range(sold_at,n+1))+[n+3]
    expected = child.financial(book, owners, spec['physical_deliveries'], opening, parts, disposals, fact, transfer, origin)
    request = residual.reencode(book, owners, spec['physical_deliveries'])
    request['operation']['input'].update(lot_splits=[], claim_transfers=[transfer], physical_partitions=parts,
        physical_disposals=disposals, dated_entitlements=[fact])
    case = dict(id=name, request=request, expected_error=None, expected=expected,
                maturity_point=mature_at, cash_point=first, transfer_point=move)
    expected['delivery_comparisons'] = dates.comparisons(case); units(case)
    # Full exact fill encoding is part of the predeclared financial facts.
    wirebook = request['operation']['input']['book']
    points = [wirebook['before']]+[s['after'] for s in wirebook['steps']]+[s['information'] for s in wirebook['standalone'] if s['kind']=='available']
    fills = {f['id']:f for p in points for f in p['point']['fills']}
    for trade in expected['trades']:
        for mark in trade['marks']:
            for d in (mark.get('physical') or {}).get('disposals', []):
                prior = d['fill']; actual = fills[prior['id']]
                assert D(prior['quantity']) == D(actual['quantity'])
                for role in ['consideration','fees']:
                    assert prior[role]['currency'] == actual[role]['currency']
                    assert D(prior[role]['value']) == D(actual[role]['value'])
                    assert actual[role]['unit'] == 'currency'
                d['fill'] = clone(actual)
    canonical.normalize(case)
    return case


def generate():
    old = json.loads(SOURCE.read_bytes())
    with localcontext() as context:
        context.prec = 60
        cases = [make(sign, origin, kind) for sign in [1,-1]
                 for origin,kind in [(2,'principal'),(4,'principal'),(4,'bond_coupon')]]
    # Preserve all16 complete previous child/pending/mixed models and every CTD.
    for c in old['cases']:
        if c['expected_error'] is None:
            c = clone(c); c['id'] = 'maturity.regression.'+c['id']; cases.append(c)
    def reject(label, fn, code='invalid_input', index=0):
        c = clone(cases[index]); c.update(id='maturity.invalid.'+label, expected_error=code)
        c.pop('expected'); fn(c['request']['operation']['input']); cases.append(c)
    fact = lambda s:s['dated_entitlements'][0]
    transfer = lambda s:s['claim_transfers'][0]
    reject('missing_earned_owner',lambda s:s['dated_entitlements'].clear())
    reject('foreign_principal_proposal',lambda s:fact(s).update(proposal_id='ABSENT'))
    reject('coupon_as_principal',lambda s:fact(s)['key'].update(kind='bond_coupon'))
    reject('foreign_earned_revision',lambda s:fact(s)['entitlement_source'].update(version=99))
    reject('false_paid_prefix',lambda s:fact(s)['residual']['cutoff']['receipt_ids'].append('FOREIGN'))
    reject('unavailable_child_custody',lambda s:s['physical_partitions'][0]['split']['applied_points'].remove(min(fact(s)['applied_points'])))
    reject('unavailable_successor_custody',lambda s:s['physical_partitions'][1]['split']['applied_points'].remove(min(fact(s)['applied_points'])),index=1)
    reject('native_face_as_legal_cash',lambda s:fact(s)['residual'].update(fraction='0'))
    reject('future_legal_clock',lambda s:fact(s)['residual']['cutoff'].update(known_at='2030-01-01T12:00:00Z'))
    reject('paid_recipient_reassigned',lambda s:transfer(s)['cutoff']['receipt_ids'].clear())
    reject('foreign_transfer_proposal',lambda s:transfer(s).update(proposal_id='ABSENT'))
    reject('overallocated_transfer',lambda s:transfer(s).update(fraction='1.1'))
    reject('future_transfer_clock',lambda s:transfer(s)['cutoff'].update(known_at='2030-01-01T12:00:00Z'))
    reject('disposed_delivery_receivable',lambda s:s['physical_disposals'][0]['receipts'][0].update(face='199999'))
    reject('foreign_invoice_owner',lambda s:s['physical_deliveries'].clear(),code='missing_input')
    reject('cached_maturity_price',lambda s:fact(s).update(model_present_value='1'))
    reject('no_overdue_policy',lambda s:[p.update(overdue='reject') for b in [s['book']['before']]+[t['after'] for t in s['book']['steps']] for p in b['point']['positions']],code='missing_input')
    reject('work_bound',lambda s:s['book'].update(max_product_evaluations=1))
    def borrowed(s):
        pos=min(fact(s)['applied_points'])-1
        s['book']['standalone'][pos]=dict(kind='available',information=clone(s['book']['steps'][pos]['after']))
    reject('independent_borrows_actual_maturity',borrowed)
    reject('cash_before_due',lambda s:next(e['event']['action']['terms'] for h in s['book']['holdings'] for e in h['lifecycle']['events'] if e['event']['action']['kind']=='payment' and e['event']['action']['terms']['receipt_id'].endswith('.PAID.BEFORE.TRANSFER')).update(value_date='2029-05-08'))
    for c in cases:
        c['input_nodes']=lots.nodes(c['request'])
        c['input_bytes']=len(json.dumps(c['request'],separators=(',',':'),default=r.c.de.serial).encode())
    return dict(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
                custody_source_sha256=hashlib.sha256(CUSTODY.read_bytes()).hexdigest(),
                tolerances=old['tolerances'],cases=cases,certified=False)


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args()
    data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite of independent declaration')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original full custody-child maturity declaration differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
