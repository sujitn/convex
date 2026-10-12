"""Full original claim exhaustion and settlement, independently declared.

Keep every original term, model, history, paid prefix and cash owner. Calculate
fixed cash with Decimal60 and the original 96-bit/28-place monetary boundary at
each contractual arithmetic step. This is independent of native execution and
never reads transport answers. Selected cash exhaustion precedes full nominal
delivery/invoice ingress; native closure requires all excluded original cash too.
"""
import argparse
import copy
import hashlib
import json
from decimal import Decimal as D, ROUND_HALF_EVEN, localcontext

import concurrent_claims_pnl_reference as concurrent
import cohort_pnl_bindings as binding
import cohort_pnl_cash_dates as dates
import cohort_pnl_reference as r
import cohort_pnl_states as states
import residual_pnl_canonical as canonical
import residual_pnl_reference as residual
import lot_pnl_reference as lots
from cohort_pnl_wire import units

SOURCE = residual.FOLDER / 'concurrent-claims-pnl-complete.json'
TARGET = residual.FOLDER / 'settled-claims-pnl-reference.json'
clone = copy.deepcopy
ZERO = D(0)
MAX_COEFFICIENT = (1 << 96) - 1
original_inferred = r.inferred


def bounded(value):
    """Round an exact Decimal to the original largest representable scale."""
    for scale in range(28, -1, -1):
        answer = value.quantize(D(1).scaleb(-scale), rounding=ROUND_HALF_EVEN)
        if abs(answer.scaleb(scale)) <= MAX_COEFFICIENT:
            return answer
    raise ValueError('Original Decimal cash arithmetic overflows')


def original_bond_cash(terms, quantity):
    if (terms['coupon']['kind'] != 'fixed' or terms['day_count'] != 'Act365Fixed'
            or terms['rounding'] != 'Unrounded' or terms['exercise']
            or terms['coupon_principal'] != 'AccrualProfile'):
        raise ValueError('No invented convention in the independent cash control')
    grid = r.c.op.grid(terms)
    result = {}
    for ordinal, (start, end) in enumerate(zip(grid, grid[1:])):
        cuts = {str(start), str(end)}
        for profile in terms['profiles'].values():
            for entry in profile:
                if str(start) < entry['start'] < str(end):
                    cuts.add(entry['start'])
        total = ZERO
        cuts = sorted(cuts)
        for first, last in zip(cuts, cuts[1:]):
            rate = bounded(r.c.profile(terms, 'coupon', first)
                           + r.c.profile(terms, 'spread', first))
            fraction = bounded(D((r.c.day(last) - r.c.day(first)).days) / 365)
            amount = bounded(bounded(r.c.profile(terms, 'notional', first) * rate)
                             * fraction)
            total = bounded(total + amount)
        total = bounded(bounded(total * quantity) / D(terms['face_value']))
        result[('bond_coupon', str(end), ordinal)] = total
    # These original full controls have a nominal bullet principal profile.
    notionals = terms['profiles']['notional']
    if len(notionals) != 1:
        raise ValueError('Amortizing invoices are not silently qualified')
    principal = bounded(D(notionals[0]['value']) * D(terms['redemption_multiplier']))
    result[('principal', terms['schedule']['end_date'], 0)] = bounded(
        bounded(principal * quantity) / D(terms['face_value']))
    return result


def inferred(info, j, opening, owner):
    cash, status, remaining, notices = original_inferred(info, j, opening, owner)
    product = info['holdings'][j]['product']['product']
    if product['kind'] == 'bond':
        exact = original_bond_cash(product['bond']['terms'], D(owner['quantity']))
        for claim in cash:
            key = (claim['kind'], claim['effective'], claim['ordinal'])
            claim['amount'] = exact[key]
            if claim['amount'] != 0 and abs(claim['recorded']) > abs(claim['amount']):
                raise ValueError('Independent receipt overdraws original cash')
        if remaining == 0:
            status = 'Closed' if all(c['amount'] == c['recorded'] for c in cash) else 'Matured'
    return cash, status, remaining, notices


def financial(book, owners, links, opening, parts, disposals, facts, transfers):
    previous = r.inferred
    r.inferred = inferred
    opening[0] = []
    opening._events[0] = 0
    try:
        expected = concurrent.financial(book, owners, links, opening, parts,
                                        disposals, facts, transfers)
    finally:
        r.inferred = previous
    order = {owner['trade_id']: i for i, owner in enumerate(owners)}
    for trade in expected['trades']:
        for mark in trade['marks']:
            mark['transferred'].sort(key=lambda allocation: (
                order[allocation['origin']], allocation['key']['kind'],
                allocation['key']['effective_date'], allocation['key']['ordinal']))
    binding.explain(expected, dict(cohorts=owners), book)
    return expected


def make(seed):
    book = states.expand(seed['request'])
    spec = seed['request']['operation']['input']
    owners = clone(spec['cohorts'])
    facts = clone(spec['dated_entitlements'])
    transfers = clone(spec['claim_transfers'])
    parts = clone(spec['physical_partitions'])
    disposals = clone(spec['physical_disposals'])
    name = seed['id'].replace('concurrent.', 'settled.', 1)
    # Move the complete previously declared ingress after selected cash payment.
    ingress = book['steps'].pop()
    book['standalone'].pop()
    initial = len(book['steps'])
    opening = r.opening_from(seed, book)
    opening[0] = []
    opening._events[0] = 0

    def append(label):
        info = r.c.repoint(book['steps'][-1]['after'], name + '.' + label)
        book['steps'].append(dict(cause='event', after=info))
        book['standalone'].append(dict(kind='unavailable', reason=
            'Independent Position cannot borrow actual maturity, legal prefixes or final Event receipts.'))
        return info, len(book['steps'])

    def finish(info, j, selected=None):
        cash = inferred(info, j, opening, owners[j])[0]
        for claim in cash:
            key = r.c.key(claim['kind'], claim['effective'], claim['ordinal'])
            if selected is not None and key not in selected:
                continue
            amount = bounded(claim['amount'] - claim['recorded'])
            if amount:
                residual.receipt(info, j, dict(key=key), amount,
                    name + '.FINAL.' + str(j) + '.' + claim['kind'] + '.'
                    + str(claim['ordinal']) + '.' + str(len(book['steps'])))

    # Final payments follow the original returned-recipient payments in their
    # actual ordered Event, preserving the immutable sixteen-cause admission.
    info = book['steps'][-1]['after']
    selected_at = len(book['steps'])
    for j, owner in enumerate(owners):
        selected = [f['key'] for f in facts if f['origin'] == owner['trade_id']]
        if selected:
            finish(info, j, selected)
    info, ingress_at = append('original.nominal.and.invoice.ingress')
    old_events = ingress['after']['holdings'][0]['lifecycle']['events']
    before_count = len(book['steps'][initial - 1]['after']['holdings'][0]['lifecycle']['events'])
    for event in clone(old_events[before_count:]):
        event['event']['expected_revision'] = len(info['holdings'][0]['lifecycle']['events'])
        info['holdings'][0]['lifecycle']['events'].append(event)
    # Original ordered native events remain distinct within one causal Event.
    # Keep the selected-cash point separate from complete ingress and closure.
    closed_at = ingress_at
    for j in range(len(owners)):
        finish(info, j)
    n = len(book['steps'])
    if n > 16:
        raise ValueError('Original cause bound must remain')
    for fact in facts:
        fact['applied_points'] = list(range(fact['applied_points'][0], n + 1))
    for transfer in transfers:
        transfer['applied_points'] = list(range(transfer['applied_points'][0], n + 1))
    for k, part in enumerate(parts):
        part['split']['applied_points'] = list(range(k + 1, n + 1)) + ([n + 1] if k == 0 else []) + [n + 2, n + 3]
    disposals[0]['applied_points'] = list(range(3, n + 1)) + [n + 3]
    expected = financial(book, owners, spec['physical_deliveries'], opening,
                         parts, disposals, facts, transfers)
    request = residual.reencode(book, owners, spec['physical_deliveries'])
    request['operation']['input'].update(lot_splits=[], claim_transfers=transfers,
        physical_partitions=parts, physical_disposals=disposals, dated_entitlements=facts)
    # Serde's Option inputs admit absent fields as None. Omit only absent event
    # attachments and prove reconstruction equals the complete original step.
    # Actual credit rounding, futures and delivery inputs remain unchanged.
    for holding in request['operation']['input']['book']['holdings']:
        for step in holding['lifecycle']['events']:
            original = clone(step)
            for field in ['credit_rounding', 'futures', 'delivery']:
                if step.get(field) is None:
                    step.pop(field, None)
            restored = dict(step)
            for field in ['credit_rounding', 'futures', 'delivery']:
                restored.setdefault(field, None)
            if restored != original:
                raise ValueError('Omitted absent attachment changed full native input')
    case = dict(id=name, request=request, expected_error=None, expected=expected,
        maturity_point=4, selected_settlement_point=selected_at,
        ingress_point=ingress_at, closure_point=closed_at,
        transfer_points=clone(seed['transfer_points']))
    expected['delivery_comparisons'] = dates.comparisons(case)
    units(case)
    wire = request['operation']['input']['book']
    fills = {f['id']: f for point in [wire['before']] + [s['after'] for s in wire['steps']]
             + [s['information'] for s in wire['standalone'] if s['kind'] == 'available']
             for f in point['point']['fills']}
    for trade in expected['trades']:
        for mark in trade['marks']:
            for disposal in (mark.get('physical') or {}).get('disposals', []):
                disposal['fill'] = clone(fills[disposal['fill']['id']])
    canonical.normalize(case)
    return case


def generate():
    old = json.loads(SOURCE.read_bytes())
    with localcontext() as context:
        context.prec = 60
        cases = [make(c) for c in old['cases'] if c['expected_error'] is None
                 and 'regression' not in c['id']]
    for case in old['cases']:
        if case['expected_error'] is None:
            case = clone(case)
            case['id'] = 'settled.regression.' + case['id']
            cases.append(case)

    def reject(label, change, index=1):
        case = clone(cases[index])
        case.update(id='settled.invalid.' + label, expected_error='invalid_input')
        case.pop('expected')
        change(case['request']['operation']['input'], case)
        cases.append(case)

    def mutate_final(spec, case, key, value):
        wire = spec['book']
        holding = wire['holdings'][wire['steps'][case['selected_settlement_point'] - 1]['after']['holdings'][2]]
        event = next(s['event'] for s in reversed(holding['lifecycle']['events'])
                     if '.FINAL.' in s['event']['action'].get('terms', {}).get('receipt_id', ''))
        if key == 'amount':
            event['action']['terms']['amount']['value'] = value
        else:
            event['action']['terms'][key] = value

    reject('final_overpayment', lambda s, c: mutate_final(s, c, 'amount', '1000000'))
    reject('final_wrong_direction', lambda s, c: mutate_final(s, c, 'amount', '-1000000'))
    reject('final_foreign_obligation', lambda s, c: mutate_final(s, c, 'obligation',
        r.c.key('principal', '2030-05-08')))
    reject('final_before_due', lambda s, c: mutate_final(s, c, 'value_date', '2029-05-08'))
    reject('disappearing_paid_recipient', lambda s, c: s['claim_transfers'][0]['applied_points'].pop())
    reject('foreign_paid_prefix', lambda s, c: s['claim_transfers'][8]['cutoff']['receipt_ids'].append('FOREIGN.RECEIPT'))
    reject('cached_closure', lambda s, c: s['dated_entitlements'][0].update(settled=True))
    reject('borrowed_final_position', lambda s, c: s['book']['standalone'].__setitem__(
        c['selected_settlement_point'] - 1, dict(kind='available', information=clone(
            s['book']['steps'][c['selected_settlement_point'] - 1]['after']))))
    # These are complete actual Position acquisitions after all original cash.
    def exhausted(spec, case, transfer):
        at = case['closure_point']
        spec['book']['steps'][at - 1]['cause'] = 'position'
        fact = spec['dated_entitlements'][0]
        origin = next(j for j, owner in enumerate(spec['cohorts']) if owner['trade_id'] == fact['origin'])
        wire = spec['book']
        h = wire['holdings'][wire['steps'][at - 1]['after']['holdings'][origin]]
        receipts = [s['event']['action']['terms']['receipt_id'] for s in h['lifecycle']['events']
                    if s['event']['action']['kind'] == 'payment'
                    and s['event']['action']['terms']['obligation'] == fact['key']]
        if transfer:
            t = clone(spec['claim_transfers'][0])
            t.update(id='EXHAUSTED.TRANSFER', applied_points=[at], fraction='0.01')
            t['cutoff']['receipt_ids'] = receipts
            spec['claim_transfers'].append(t)
        else:
            spec['claim_transfers'] = [t for t in spec['claim_transfers']
                if not (t['origin'] == fact['origin'] and t['key'] == fact['key'])]
            fact.update(applied_points=[at])
            fact['residual']['cutoff']['receipt_ids'] = receipts
    reject('exhausted_acquisition', lambda s, c: exhausted(s, c, False))
    reject('exhausted_transfer', lambda s, c: exhausted(s, c, True))
    reject('work_bound', lambda s, c: s['book'].update(max_product_evaluations=1))
    for case in cases:
        case['input_nodes'] = lots.nodes(case['request'])
        case['input_bytes'] = len(json.dumps(case['request'], separators=(',', ':'),
                                          default=r.c.de.serial).encode())
        if case['input_nodes'] > 32768 or case['input_bytes'] > 2 * 1024 * 1024:
            raise ValueError('Full original request exceeds immutable wire bounds')
    return dict(reference=__doc__, source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
                tolerances=old['tolerances'], cases=cases, certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    data = generate()
    raw = (json.dumps(data, indent=2, default=r.c.de.serial) + '\n').encode()
    if args.write:
        if TARGET.exists():
            raise ValueError('Refusing overwrite of independent financial declaration')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes() != raw:
        raise ValueError('Full independent settlement declaration differs')
    print(json.dumps(dict(status='passed', cases=len(data['cases']),
        financial_books=sum(c['expected_error'] is None for c in data['cases']), certified=False)))
