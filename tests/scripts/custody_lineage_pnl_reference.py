"""Independent complete custody lineage, actual fills and original cash controls.

Reuse the original full Decimal60 product/model/history/basket/CTD references and
unchanged financial bounds. Legal partitions conserve remaining custody; actual
disposals bind explicitly supplied receipt allocations and real fill facts. No
candidate financial answer is used. Security and original invoice ownership are
derived separately, including prior paid recipients.
"""
import argparse, copy, hashlib, json
from decimal import Decimal as D, localcontext
import cohort_pnl_reference as r
import cohort_pnl_states as states
import cohort_pnl_bindings as binding
import cohort_pnl_cash_dates as dates
import residual_pnl_reference as residual
import residual_pnl_canonical as canonical
import lot_pnl_reference as lots

FOLDER = residual.FOLDER
SOURCE = FOLDER / 'physical-partition-pnl-final.json'
TARGET = FOLDER / 'custody-lineage-pnl-reference.json'
clone = copy.deepcopy
ZERO = D(0)

def receipt_events(info):
    return [clone(s['event']) for s in info['holdings'][0]['lifecycle']['events']
            if s['event']['action']['kind'] == 'security_receipt']

def legal_points(book, legal, first):
    legal['applied_points'] = list(range(first, len(book['steps']) + 1))

def custody(info, owners, partitions, disposals, point):
    """Independent exact security balances, keyed by actual receipt identity."""
    balances = {o['trade_id']: dict(pending=ZERO, receipts={}, lineage=[], facts=None)
                for o in owners[1:]}
    root = owners[1]['trade_id']; total = D(owners[1]['quantity'])
    balances[root]['pending'] = total
    events = receipt_events(info); cursor = 0; disposed = {k: [] for k in balances}
    acquired = {root}; operations = []
    for p in partitions:
        if point in p['split']['applied_points']:
            operations.append((min(p['split']['applied_points']), 'partition', p))
    for d in disposals:
        if point in d['applied_points']:
            operations.append((min(d['applied_points']), 'disposal', d))
    def receive(limit):
        nonlocal cursor
        if limit < cursor or limit > len(events): raise ValueError('Independent receipt chronology')
        while cursor < limit:
            event = events[cursor]; rid = event['action']['terms']['receipt_id']
            amount = D(event['action']['terms']['face'])
            outstanding = sum((b['pending'] for b in balances.values()), ZERO)
            if not outstanding or abs(amount) > abs(outstanding): raise ValueError('Independent ingress')
            for b in balances.values():
                owned = amount * b['pending'] / outstanding
                b['pending'] -= owned; b['receipts'][rid] = owned
            cursor += 1
    for _, kind, facts in sorted(operations, key=lambda row: row[0]):
        receive(len(facts['receipt_ids']))
        if kind == 'partition':
            p = facts['split']; parent = balances[p['parent']]
            held = sum(parent['receipts'].values(), ZERO); face = held + parent['pending']
            if p['parent'] not in acquired or not face: raise ValueError('Independent acquisition')
            quantities = [D(next(o['quantity'] for o in owners if o['trade_id'] == c)) for c in p['children']]
            if sum(quantities, ZERO) != face: raise ValueError('Independent partition conservation')
            ancestry = parent['lineage'] + [clone(facts)]
            for child, amount in zip(p['children'], quantities):
                share = amount / face; b = balances[child]
                b['pending'] = parent['pending'] * share
                b['receipts'] = {k: v * share for k, v in parent['receipts'].items()}
                b['lineage'] = clone(ancestry); b['facts'] = clone(facts); acquired.add(child)
            parent['pending'] = ZERO; parent['receipts'] = {k: ZERO for k in parent['receipts']}
            parent['lineage'] = ancestry; parent['facts'] = clone(facts)
        else:
            b = balances[facts['asset']]
            fill = next(f for f in info['point']['fills'] if f['id'] == facts['fill_id'])
            allocated = []; face = ZERO
            for a in facts['receipts']:
                rid = a['receipt_id']; amount = D(a['face'])
                if abs(amount) > abs(b['receipts'][rid]): raise ValueError('Independent disposal overdraw')
                b['receipts'][rid] -= amount; face += amount
                allocated.append(dict(event=clone(next(e for e in events if e['action']['terms']['receipt_id']==rid)), face=amount))
            if face != -D(fill['quantity']): raise ValueError('Independent actual fill conservation')
            disposed[facts['asset']].append(dict(facts=clone(facts), fill=clone(fill), face=face, receipts=allocated))
    receive(len(events))
    removed = sum((d['face'] for ds in disposed.values() for d in ds), ZERO)
    received = sum((sum(b['receipts'].values(), ZERO) for b in balances.values()), ZERO)
    pending = sum((b['pending'] for b in balances.values()), ZERO)
    if received + removed != sum((D(e['action']['terms']['face']) for e in events), ZERO) or received + pending + removed != total:
        raise ValueError('Independent full security conservation')
    return balances, disposed, acquired

def make(sign, mode):
    old = json.loads(SOURCE.read_bytes())
    original = clone(next(c for c in old['cases'] if c['id'] == f'physical.partition.{"long" if sign==1 else "short"}.4.True'))
    name = f'custody.{"long" if sign==1 else "short"}.{mode}'
    spec = original['request']['operation']['input']; book = states.expand(original['request'])
    owners = clone(spec['cohorts']); parts = clone(spec['physical_partitions']); cash = clone(spec['claim_transfers'])
    expected = clone(original['expected']); disposals = []
    # First original child becomes a parent of complete same-security successors.
    parent = 2; first = None; new_ids = []
    def insert(at, label):
        after = r.c.repoint(r.sets(book)[at], name+'.'+label)
        book['steps'].insert(at, dict(cause='position', after=after))
        book['standalone'].insert(at, dict(kind='unavailable', reason='Complete original custody acquisition, receipt cutoff and actual execution prerequisites required.'))
        for t in expected['trades']:t['marks'].insert(at+1, clone(t['marks'][at]))
        def extend(facts):
            previous=facts['applied_points']
            facts['applied_points']=[i+1 if i>at else i for i in previous]
            if at in previous:facts['applied_points'].append(at+1);facts['applied_points'].sort()
        for p in parts:extend(p['split'])
        for t in cash:extend(t)
        for d in disposals:extend(d)
        return at+1
    # Use all original partial receipts; legal changes are independently supplied.
    ingress = next(i for i, s in enumerate(r.sets(book)[:len(book['steps'])+1]) if receipt_events(s))
    at = ingress
    if mode == 'independent_full':
        at = next(i for i,s in enumerate(r.sets(book)[:len(book['steps'])+1]) if sum((D(e['action']['terms']['face']) for e in receipt_events(s)), ZERO)==D(owners[1]['quantity']))
    def dispose(j, at, label, fraction):
        point = insert(at, label); info = r.sets(book)[point]
        balances, _, _ = custody(info, owners, parts, disposals, point)
        stock = balances[owners[j]['trade_id']]['receipts']
        allocations=[dict(receipt_id=k,face=str(v*fraction)) for k,v in stock.items() if v]
        face=sum((D(a['face']) for a in allocations), ZERO)
        fill_id=name+'.ACTUAL.'+label
        r.execution(info,j,fill_id,-face,D(sign)*D('16811.25'))
        fill=clone(info['point']['fills'][-1])
        for later in r.sets(book)[point+1:len(book['steps'])+1]:later['point']['fills'].append(clone(fill))
        facts=dict(id=name+'.LEGAL.'+label,asset=owners[j]['trade_id'],future=owners[0]['trade_id'],delivery_id=parts[0]['delivery_id'],fill_id=fill_id,
            effective_date=info['point']['valuation_date'],known_at=info['point']['known_at'],source=r.c.pin(name+'.SOURCE.'+label),
            receipt_ids=[e['action']['terms']['receipt_id'] for e in receipt_events(info)],receipts=allocations,applied_points=list(range(point,len(book['steps'])+1)))
        disposals.append(facts)
        return point
    if mode == 'dispose_then_split':at=dispose(parent,at,'DISPOSE.PARENT',D('0.5'))
    first=insert(at,'SUCCESSORS'); info=r.sets(book)[first]
    balances,_,_=custody(info,owners,parts,disposals,first)
    remaining=balances[owners[parent]['trade_id']]['pending']+sum(balances[owners[parent]['trade_id']]['receipts'].values(),ZERO)
    for k,share in enumerate([D('0.25'),D('0.75')]):
        ident=name+'.successor.'+str(k);new_ids.append(ident);owner=clone(owners[parent])
        owner.update(trade_id=ident,quantity=str(remaining*share),acquisition=dict(kind='lot_split',split_id=name+'.SUCCESSORS'))
        for e in owner['entitlements']:e['source']=r.c.pin(ident+'.CASH.'+str(e['key']['ordinal']))
        owners.append(owner)
        for info in r.sets(book):
            h=clone(info['holdings'][parent]);p=clone(info['point']['positions'][parent]);h['id']=ident;life=h['lifecycle'];life['trade_id']=ident;life['position']['value']=owner['quantity']
            for s in life['events']:s['event']['trade_id']=ident;s['event']['event_id']=ident+'.EVENT.'+str(s['event']['expected_revision'])
            p['identity']['id']=ident;p['identity']['economics']=r.c.pin(ident+'.ECONOMICS');p['identity']['calculation']=r.c.pin(ident+'.CALCULATION');p['current']['quantity']='0'
            info['holdings'].append(h);info['point']['positions'].append(p)
        t=clone(expected['trades'][parent]);t['id']=ident;ratio=D(owner['quantity'])/D(owners[parent]['quantity'])
        for m in t['marks']:
            m.update(transferred=[],recorded=ZERO,proposed=ZERO,consideration=ZERO,fees=ZERO,executed=ZERO,executed_marking=ZERO)
            for claim in m['claims']:
                for key in ['amount','recorded','outstanding','removed']:claim[key]=D(claim[key])*ratio
        expected['trades'].append(t)
    info=r.sets(book)[first]
    parts.append(dict(split=dict(id=name+'.SUCCESSORS',parent=owners[parent]['trade_id'],children=new_ids,effective_date=info['point']['valuation_date'],known_at=info['point']['known_at'],source=r.c.pin(name+'.SUCCESSOR.LEGAL'),applied_points=list(range(first,len(book['steps'])+1))),future=owners[0]['trade_id'],delivery_id=parts[0]['delivery_id'],receipt_ids=[e['action']['terms']['receipt_id'] for e in receipt_events(info)]))
    if mode != 'dispose_then_split':dispose(4,first,'DISPOSE.SUCCESSOR',D(1) if mode=='full_stock' else D('0.5'))
    # Every ordered legal fact persists. New independent sets below bind complete prerequisites.
    for p in parts:legal_points(book,p['split'],min(p['split']['applied_points']))
    for t in cash:legal_points(book,t,min(t['applied_points']))
    for d in disposals:legal_points(book,d,min(d['applied_points']))
    for i,info in enumerate(r.sets(book)):
        if not expected['trades'][1]['marks'][i]['physical']:continue
        balances, disposed, acquired = custody(info,owners,parts,disposals,i)
        root_physical=clone(expected['trades'][1]['marks'][i]['physical'])
        root_physical.pop('partition',None)
        full_received=sum((D(e['action']['terms']['face']) for e in receipt_events(info)),ZERO)
        for j in range(1,len(owners)):
            m=expected['trades'][j]['marks'][i];b=balances[owners[j]['trade_id']];held=sum(b['receipts'].values(),ZERO);face=held+b['pending']
            info['point']['positions'][j]['current']['quantity']=str(held)
            physical=clone(root_physical) if owners[j]['trade_id'] in acquired else None
            if physical:
                physical.update(proposed_face=face,received_face=held,outstanding_face=b['pending'],outstanding_asset_present_value=D(m['reference'])*b['pending']/D(info['point']['positions'][j]['current']['reference_quantity']))
                if b['facts']:
                    physical['partition']=dict(facts=clone(b['facts']),original_proposed_face=D(owners[1]['quantity']),original_received_face=full_received,original_outstanding_face=D(owners[1]['quantity'])-full_received,fraction=face/D(owners[1]['quantity']),receipts=[dict(event=clone(e),face=b['receipts'].get(e['action']['terms']['receipt_id'],ZERO)) for e in receipt_events(info)])
                if len(b['lineage'])>1:physical['lineage']=clone(b['lineage'])
                if disposed[owners[j]['trade_id']]:physical['disposals']=clone(disposed[owners[j]['trade_id']])
                if j!=1:physical.update(invoice_excluded=True,outstanding_invoice=ZERO,invoice_present_value=ZERO)
            m['physical']=physical
            fills=[f for f in info['point']['fills'] if f['trade_id']==owners[j]['trade_id']]
            m['executed']=sum((D(f['quantity']) for f in fills),ZERO);m['executed_marking']=D(m['reference'])*m['executed']/D(info['point']['positions'][j]['current']['reference_quantity'])
            m['consideration']=sum((D(f['consideration']['value']) for f in fills),ZERO);m['fees']=sum((D(f['fees']['value']) for f in fills),ZERO)
            claim_pv=sum((D(c['pv'] or 0)-D(c['removed']) for c in m['claims']),ZERO)
            cash_pv=sum((D(t['pv']) for t in m['transferred']),ZERO)
            if physical and not physical.get('invoice_excluded',False):cash_pv+=D(physical['invoice_present_value'])
            value=D(m['reference'])*face/D(info['point']['positions'][j]['current']['reference_quantity'])+claim_pv+cash_pv
            m['local']=dict(current=value,pending=ZERO,projected=value);m['reporting']={k:v*D(m['fx']) for k,v in m['local'].items()}
    binding.explain(expected,dict(cohorts=owners),book)
    request=residual.reencode(book,owners,spec['physical_deliveries'])
    request['operation']['input'].update(lot_splits=[],claim_transfers=cash,physical_partitions=parts,physical_disposals=disposals)
    case=dict(id=name,request=request,expected_error=None,expected=expected)
    expected['delivery_comparisons']=dates.comparisons(case);canonical.normalize(case)
    return case

def generate():
    with localcontext() as context:
        context.prec=60
        cases=[make(sign,mode) for sign in [1,-1] for mode in ['successor_partial','full_stock','dispose_then_split','independent_full']]
    old=json.loads(SOURCE.read_bytes())
    for name in ['physical.regression.claim.full.ctd_signed','physical.regression.claim.full.simultaneous_ois_cds']:
        c=clone(next(c for c in old['cases'] if c['id']==name));c['id']='custody.regression.'+name;c['request']['operation']['input']['physical_disposals']=[];cases.append(c)
    def reject(label, fn, source=0, error='invalid_input'):
        c=clone(cases[source]);c.update(id='custody.invalid.'+label,expected_error=error);c.pop('expected');fn(c['request']['operation']['input']);cases.append(c)
    part=lambda s:s['physical_partitions'][1]
    disposal=lambda s:s['physical_disposals'][0]
    reject('cycle',lambda s:part(s)['split']['children'].__setitem__(0,s['cohorts'][1]['trade_id']))
    reject('double_parent',lambda s:s['physical_partitions'].append(clone(part(s))))
    reject('unacquired_parent',lambda s:part(s)['split'].update(parent=s['cohorts'][3]['trade_id']))
    reject('foreign_delivery',lambda s:part(s).update(delivery_id='ABSENT'))
    reject('foreign_future',lambda s:part(s).update(future='ABSENT'))
    reject('missing_ancestor',lambda s:s['physical_partitions'].pop(0))
    reject('same_acquisition_point',lambda s:part(s)['split'].update(applied_points=clone(s['physical_partitions'][0]['split']['applied_points'])))
    reject('missing_successor',lambda s:part(s)['split']['children'].pop())
    reject('unavailable_lineage',lambda s:part(s)['split'].update(known_at='2028-12-02T12:00:00Z'))
    reject('receipt_prefix_omitted',lambda s:part(s)['receipt_ids'].clear())
    reject('foreign_disposal_receipt',lambda s:disposal(s)['receipts'][0].update(receipt_id='ABSENT'))
    reject('overdrawn_stock',lambda s:disposal(s)['receipts'][0].update(face='999999999'))
    reject('opposite_disposal',lambda s:disposal(s)['receipts'][0].update(face=str(-D(disposal(s)['receipts'][0]['face']))))
    reject('missing_fill',lambda s:disposal(s).update(fill_id='ABSENT'))
    reject('foreign_disposal_asset',lambda s:disposal(s).update(asset=s['cohorts'][3]['trade_id']))
    reject('wrong_disposal_delivery',lambda s:disposal(s).update(delivery_id='ABSENT'))
    reject('duplicate_disposal',lambda s:s['physical_disposals'].append(clone(disposal(s))))
    reject('duplicate_receipt_allocation',lambda s:disposal(s)['receipts'].append(clone(disposal(s)['receipts'][0])))
    reject('disposal_prefix_omitted',lambda s:disposal(s)['receipt_ids'].clear())
    reject('disappearing_disposal',lambda s:disposal(s)['applied_points'].pop())
    reject('future_disposal_knowledge',lambda s:disposal(s).update(known_at='2028-12-02T12:00:00Z'))
    reject('competing_full_disposal',lambda s:s['cohorts'][4].update(disposal_fill=disposal(s)['fill_id']))
    reject('unconsumed_fill',lambda s:s.update(physical_disposals=[]))
    reject('disposed_parent_overdrawn_split',lambda s:s['cohorts'][4].update(quantity='200000'),source=2)
    reject('duplicate_legal_identity',lambda s:disposal(s).update(id=part(s)['split']['id']))
    for c in cases:
        c['input_nodes']=lots.nodes(c['request']);c['input_bytes']=len(json.dumps(c['request'],separators=(',',':')).encode())
    return dict(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),tolerances=old['tolerances'],cases=cases,certified=False)

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing to overwrite declared financial controls')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Independent complete custody controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
