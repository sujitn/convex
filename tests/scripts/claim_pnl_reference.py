"""Independent original cash-transfer conservation over complete financial books.

Reuse immutable full Decimal60 product/curve/calendar/model/history/basket rows.
Only explicit legal cash ownership and original actual receipt prefixes change.
Signed cash and original residual fractions are distinct from held security lots.
No native result, fitted financial expectation or relaxed bound is read.
"""
import argparse, copy, hashlib, json
from decimal import Decimal as D, localcontext
from pathlib import Path
import cohort_pnl_reference as r
import cohort_pnl_states as states
import cohort_pnl_bindings as binding
import residual_pnl_reference as residual
import residual_pnl_canonical as canonical
import lot_pnl_reference as lots
from cohort_pnl_wire import units

FOLDER = Path(__file__).resolve().parents[1] / 'fixtures/library'
TARGET = FOLDER / 'claim-pnl-reference.json'
SOURCE = FOLDER / 'lot-pnl-chronology.json'
clone = copy.deepcopy
ZERO = D(0)

def events(info, j, key):
    return [s['event'] for s in info['holdings'][j]['lifecycle']['events']
            if s['event']['action']['kind'] == 'payment'
            and s['event']['action']['terms']['obligation'] == key]

def proposal(info, j, key):
    return next(s['event'] for s in info['holdings'][j]['lifecycle']['events']
                if (s['event']['action']['kind'] == 'recovery' and key['kind'] in ['recovery', 'default_accrual'])
                or (s['event']['action']['kind'] == 'contractual_cash'
                    and s['event']['effective_date'] == key['effective_date']))

def allocations(case, book, owners, transfers):
    expected = case['expected']; points = r.sets(book)
    groups = {}
    for t in transfers: groups.setdefault((t['origin'], json.dumps(t['key'], sort_keys=True)), []).append(t)
    for i, info in enumerate(points):
        for trade in expected['trades']: trade['marks'][i]['transferred'] = []
        for (origin, _), records in groups.items():
            j = next(j for j, o in enumerate(owners) if o['trade_id'] == origin)
            source_mark = expected['trades'][j]['marks'][i]
            key = records[0]['key']; cash = next(c for c in source_mark['claims'] if c['key'] == key)
            ent = next(e for e in owners[j]['entitlements'] if e['key'] == key)
            active = [t for t in records if i in t['applied_points']]
            shares = {o['trade_id']: ZERO for o in owners}; shares[origin] = D(1)
            def move(t, values):
                f = D(t['fraction']); values[t['from']] -= f; values[t['to']] += f
                if min(values.values()) < 0: raise ValueError('Original ownership overdrawn')
            for t in active: move(t, shares)
            residual_claim = ent.get('residual'); f = D(residual_claim['fraction']) if residual_claim else D(1)
            excluded = residual_claim['cutoff']['receipt_ids'] if residual_claim else []
            receipts = [e for e in events(info, j, key) if e['action']['terms']['receipt_id'] not in excluded]
            original_recorded = D(cash['residual']['owned_recorded']) if cash.get('residual') else D(cash['recorded'])
            original_outstanding = D(cash['residual']['owned_outstanding']) if cash.get('residual') else D(cash['outstanding'])
            original_proposed = D(cash['residual']['owned_proposed']) if cash.get('residual') else D(cash['amount'])
            paid = {o['trade_id']: ZERO for o in owners}; receipt_allocations = {o['trade_id']: [] for o in owners}
            for event in receipts:
                at_receipt = {o['trade_id']: ZERO for o in owners}; at_receipt[origin] = D(1)
                for t in active:
                    if event['action']['terms']['receipt_id'] not in t['cutoff']['receipt_ids']: move(t, at_receipt)
                for ident, share in at_receipt.items():
                    if share:
                        amount = D(event['action']['terms']['amount']['value']) * f * share
                        paid[ident] += amount
                        receipt_allocations[ident].append(dict(event=clone(event), fraction=share, amount=amount))
            parties = {origin} | {t[k] for t in active for k in ['from', 'to']}
            ratio = D(cash['ratio'])
            for k, owner in enumerate(owners):
                ident = owner['trade_id']
                if ident not in parties: continue
                mark = expected['trades'][k]['marks'][i]
                out = original_outstanding * shares[ident]; recorded = paid[ident]; pv = out * ratio
                allocation = dict(origin=origin, key=clone(key), entitlement=clone(ent),
                    proposal=clone(proposal(info, j, key)), transfers=clone(active),
                    fraction=shares[ident], receipts=receipt_allocations[ident],
                    owned_proposed=recorded+out, owned_recorded=recorded,
                    owned_outstanding=out, ratio=ratio, pv=pv)
                delta_pv = pv; delta_recorded = recorded; delta_proposed = recorded + out
                if k == j:
                    delta_pv -= original_outstanding * ratio; delta_recorded -= original_recorded; delta_proposed -= original_proposed
                    cash['pv'] = pv; cash['transfer'] = allocation
                else: mark['transferred'].append(allocation)
                for kind in ['current', 'projected']:
                    mark['local'][kind] = D(mark['local'][kind]) + delta_pv
                    mark['reporting'][kind] = D(mark['reporting'][kind]) + delta_pv * D(mark['fx'])
                mark['recorded'] = D(mark['recorded']) + delta_recorded
                mark['proposed'] = D(mark['proposed']) + delta_proposed
            if sum(paid.values(), ZERO) != original_recorded:
                if abs(sum(paid.values(), ZERO)-original_recorded) > D('1e-55'): raise ValueError('Original received cash not conserved')
            if abs(sum((original_outstanding * v for v in shares.values()), ZERO)-original_outstanding) > D('1e-55'):
                raise ValueError('Original outstanding cash not conserved')
    binding.explain(expected, dict(cohorts=owners), book)

def make(name, source, root=0, recipients=None, residual_only=False, standalone=False):
    template = clone(next(c for c in json.loads(SOURCE.read_bytes())['cases'] if c['id'] == source))
    book = states.expand(template['request']); spec = template['request']['operation']['input']
    owners = clone(spec['cohorts']); splits = clone(spec['lot_splits']); oldf = len(book['steps'])
    mapping = list(range(len(r.sets(book)))); transfers = []
    recipients = recipients or [len(owners)-2, len(owners)-1]
    before_transfer = book['steps'][-1]['after']; key = clone(template['expected']['trades'][root]['marks'][oldf]['claims'][0]['key'])
    ent = next(e for e in owners[root]['entitlements'] if e['key'] == key)
    ids = [o['trade_id'] for o in owners]
    def append(cause, label, payment=None):
        after = r.c.repoint(book['steps'][-1]['after'], name+'.'+label)
        if payment: residual.receipt(after, root, dict(key=key), payment, name+'.'+label+'.RECEIPT')
        book['steps'].append(dict(cause=cause, after=after))
        book['standalone'].append(dict(kind='unavailable', reason='Requires the exact original legal acquisition, earned proposal and prior transfer/receipt prefix.'))
        mapping.insert(len(book['steps']), oldf)
        return after
    def transfer(info, sender, target, fraction, label):
        transfers.append(dict(id=name+'.'+label, origin=ids[root], key=clone(key),
            proposal_id=proposal(info, root, key)['event_id'], entitlement_source=clone(ent['source']),
            residual_id=ent.get('residual', {}).get('id'), **{'from':ids[sender], 'to':ids[target]},
            fraction=str(fraction), cutoff=dict(date=info['point']['valuation_date'], known_at=info['point']['known_at'],
                receipt_ids=[e['action']['terms']['receipt_id'] for e in events(info, root, key)]),
            source=r.c.pin(name+'.LEGAL.'+label), applied_points=[len(book['steps'])]))
    first = append('position', 'first'); transfer(first, root, recipients[0], D('.6'), 'FIRST')
    cash = template['expected']['trades'][root]['marks'][oldf]['claims'][0]
    left = D(cash['outstanding'])
    paid = append('event', 'paid', left/D(4))
    second = append('position', 'second'); transfer(second, recipients[0], recipients[1], D('.25'), 'SECOND')
    append('event', 'final_paid', left/D(8))
    newf = len(book['steps']); shift = newf-oldf
    for split in splits:
        split['applied_points'] = [p if p <= oldf else p+shift for p in split['applied_points']]
        if oldf in split['applied_points']: split['applied_points'].extend(range(oldf+1,newf+1))
    for t in transfers: t['applied_points'] = list(range(t['applied_points'][0],newf+1))
    if standalone:
        separate = r.c.repoint(book['before'],name+'.single')
        book['standalone'][oldf] = dict(kind='available',information=separate)
        mapping.insert(newf+1+sum(s['kind']=='available' for s in book['standalone'][:oldf]),0)
        point = newf+1+sum(s['kind']=='available' for s in book['standalone'][:oldf])
        # Existing standalone indices after this new cause, if any, are retained.
        transfers[0]['applied_points'].append(point)
    original_acquired = r.acquired
    def acquired(info, owner, physical):
        if owner['acquisition']['kind'] != 'lot_split': return original_acquired(info,owner,physical)
        i=next(i for i,x in enumerate(r.sets(book)) if x['point']['snapshot_id']==info['point']['snapshot_id'])
        return any(s['id']==owner['acquisition']['split_id'] and i in s['applied_points'] for s in splits)
    r.acquired = acquired
    try: result = residual.derive(template,book,owners,[],mapping)
    finally: r.acquired = original_acquired
    result.update(id=name); result['request']['operation']['input'].update(lot_splits=splits,claim_transfers=transfers)
    allocations(result, book, owners, transfers); units(result); canonical.normalize(result)
    return result

def generate():
    with localcontext() as ctx:
        ctx.prec = 60
        cases = [make('claim.full.ctd_signed','lot.full.ctd_amend_split'),
            make('claim.short.successive','lot.single.short.successive'),
            make('claim.long.successive','lot.single.long.successive'),
            make('claim.full.residual_five_causes','lot.full.five_causes_residual'),
            make('claim.full.discounted_ois','lot.full.five_causes_amendment',root=2,recipients=[0,5],standalone=True),
            make('claim.full.discounted_cds_initial','lot.full.ctd_amend_split',root=6,recipients=[0,5],standalone=True)]
    def reject(name, change, code='invalid_input', source=0):
        c=clone(cases[source]); c.update(id='claim.invalid.'+name,expected_error=code);c.pop('expected')
        change(c['request']['operation']['input']);cases.append(c)
    reject('foreign_origin',lambda x:x['claim_transfers'][0].update(origin='ABSENT'))
    reject('foreign_sender',lambda x:x['claim_transfers'][0].update(**{'from':'ABSENT'}))
    reject('foreign_recipient',lambda x:x['claim_transfers'][0].update(to='ABSENT'))
    reject('self_transfer',lambda x:x['claim_transfers'][0].update(to=x['claim_transfers'][0]['from']))
    reject('duplicate_id',lambda x:x['claim_transfers'][1].update(id=x['claim_transfers'][0]['id']))
    reject('zero_fraction',lambda x:x['claim_transfers'][0].update(fraction='0'))
    reject('negative_fraction',lambda x:x['claim_transfers'][0].update(fraction='-0.1'))
    reject('overdraw',lambda x:x['claim_transfers'][1].update(fraction='0.7'))
    reject('unavailable_knowledge',lambda x:x['claim_transfers'][0]['cutoff'].update(known_at='2027-10-07T12:00:00Z'))
    reject('retroactive_date',lambda x:x['claim_transfers'][0]['cutoff'].update(date='2026-10-06'))
    reject('missing_proposal',lambda x:x['claim_transfers'][0].update(proposal_id='ABSENT'))
    reject('foreign_entitlement',lambda x:x['claim_transfers'][0]['entitlement_source'].update(version=99))
    reject('foreign_residual',lambda x:x['claim_transfers'][0].update(residual_id='ABSENT'),source=3)
    reject('omitted_paid_prefix',lambda x:x['claim_transfers'][0]['cutoff']['receipt_ids'].pop())
    reject('foreign_receipt',lambda x:x['claim_transfers'][0]['cutoff']['receipt_ids'].append('ABSENT'))
    reject('duplicate_receipt',lambda x:x['claim_transfers'][0]['cutoff']['receipt_ids'].append(x['claim_transfers'][0]['cutoff']['receipt_ids'][0]))
    reject('reversed_prefix',lambda x:x['claim_transfers'][1]['cutoff']['receipt_ids'].reverse())
    reject('disappearing_transfer',lambda x:x['claim_transfers'][0]['applied_points'].pop())
    reject('missing_dependency',lambda x:x['claim_transfers'][0]['applied_points'].remove(x['claim_transfers'][1]['applied_points'][0]))
    reject('opening_transfer',lambda x:x['claim_transfers'][0]['applied_points'].append(0))
    reject('unrelated_standalone',lambda x:x['claim_transfers'][0]['applied_points'].append(len(x['book']['steps'])+1))
    reject('wrong_cause',lambda x:x['book']['steps'][-4].update(cause='event'))
    reject('unowned_source',lambda x:x['cohorts'][0]['entitlements'][0].update(entitled=False))
    reject('budget',lambda x:x.update(claim_transfers=x['claim_transfers']*65))
    data=dict(certified=False,reference=__doc__,sources={SOURCE.name:hashlib.sha256(SOURCE.read_bytes()).hexdigest()},
              tolerances=json.loads(SOURCE.read_bytes())['tolerances'],cases=cases)
    for c in cases:
        c['input_nodes']=lots.nodes(c['request']);c['input_bytes']=len(json.dumps(c['request'],separators=(',',':')).encode())
    return data

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate()
    raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete independent claim controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),
        max_nodes=max(c['input_nodes'] for c in data['cases']),certified=False)))
