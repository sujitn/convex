"""Independent delivery custody and invoice cash, before native execution.

Original complete Decimal60 bond/basket/CTD/curve/history/FX and financial bounds
are reused unchanged. Legal custody partitions divide exact original security
face; invoice cash is allocated independently from immutable original receipts.
No native answer or scalar pricing substitute is used.
"""
import argparse, copy, hashlib, json
from decimal import Decimal as D, localcontext
import cohort_pnl_reference as r
import cohort_pnl_states as states
import cohort_pnl_bindings as binding
import residual_pnl_reference as residual
import residual_pnl_canonical as canonical
import claim_pnl_reference as claims
import lot_pnl_reference as lots

FOLDER = residual.FOLDER
SOURCE = FOLDER / 'residual-pnl-canonical.json'
TARGET = FOLDER / 'physical-partition-pnl-reference.json'
clone = copy.deepcopy
ZERO = D(0)

def make(sign, at, transfers=True):
    name = f'physical.partition.{"long" if sign == 1 else "short"}.{at}.{transfers}'
    template = clone(next(c for c in json.loads(SOURCE.read_bytes())['cases']
        if c['id'] == f'residual.physical.{"long" if sign == 1 else "short"}.interleaved'))
    book = states.expand(template['request']); oldpoints = r.sets(book)
    owners = clone(template['request']['operation']['input']['cohorts'])
    links = clone(template['request']['operation']['input']['physical_deliveries'])
    face = D(owners[1]['quantity']); child_ids = []
    for k, share in enumerate([D('.4'), D('.6')]):
        ident = name + '.child.' + str(k); child_ids.append(ident)
        owner = clone(owners[1]); owner.update(trade_id=ident, quantity=str(face*share),
            acquisition=dict(kind='lot_split', split_id=name+'.CUSTODY'))
        for e in owner['entitlements']:e['source']=r.c.pin(ident+'.ENTITLEMENT.'+str(e['key']['ordinal']))
        owners.append(owner)
        for info in oldpoints:
            h = clone(info['holdings'][1]); p = clone(info['point']['positions'][1])
            h['id'] = ident; life = h['lifecycle']; life['trade_id'] = ident
            life['position']['value'] = owner['quantity']
            for s in life['events']:
                s['event']['trade_id']=ident;s['event']['event_id']=ident+'.EVENT.'+str(s['event']['expected_revision'])
            p['identity']['id']=ident;p['identity']['economics']=r.c.pin(ident+'.ECONOMICS')
            p['identity']['calculation']=r.c.pin(ident+'.CALCULATION');p['current']['quantity']='0'
            info['holdings'].append(h);info['point']['positions'].append(p)
    # Insert a legal custody Position after actual delivery / partial ingress / full ingress.
    mapping = list(range(len(oldpoints)));cut = at
    after = r.c.repoint(book['steps'][cut-1]['after'], name+'.partition')
    book['steps'].insert(cut,dict(cause='position',after=after))
    book['standalone'].insert(cut,dict(kind='unavailable',reason='Custody requires the original checked delivery and actual receipt prefix.'))
    mapping.insert(cut+1,cut)
    first = cut+1; ordered = len(book['steps'])
    partition = dict(split=dict(id=name+'.CUSTODY',parent=owners[1]['trade_id'],children=child_ids,
        effective_date=after['point']['valuation_date'],known_at=after['point']['known_at'],
        source=r.c.pin(name+'.LEGAL.CUSTODY'),applied_points=list(range(first,ordered+1))),
        future=owners[0]['trade_id'],delivery_id=next(s['event']['event_id'] for s in after['holdings'][0]['lifecycle']['events'] if s['event']['action']['kind']=='bond_delivery'),
        receipt_ids=[s['event']['action']['terms']['receipt_id'] for s in after['holdings'][0]['lifecycle']['events'] if s['event']['action']['kind']=='security_receipt'])
    legal=[];key=next(e['key'] for e in owners[0]['entitlements'] if e['key']['kind']=='delivery_invoice')
    ent=next(e for e in owners[0]['entitlements'] if e['key']==key)
    def move(atpoint,sender,target,fraction,label):
        info=r.sets(book)[atpoint];proposal=next(s['event'] for s in info['holdings'][0]['lifecycle']['events'] if s['event']['action']['kind']=='bond_delivery')
        legal.append(dict(id=name+'.'+label,origin=owners[0]['trade_id'],key=clone(key),
            proposal_id=proposal['event_id'],entitlement_source=clone(ent['source']),residual_id=None,
            **{'from':owners[sender]['trade_id'],'to':owners[target]['trade_id']},fraction=fraction,
            cutoff=dict(date=info['point']['valuation_date'],known_at=info['point']['known_at'],
                receipt_ids=[e['action']['terms']['receipt_id'] for e in claims.events(info,0,key)]),
            source=r.c.pin(name+'.LEGAL.'+label),applied_points=list(range(atpoint,len(book['steps'])+1))))
    if transfers and at < 8:
        move(first,1,2,'.75','FIRST.CASH')
        # A second independently supplied Position follows the actual first invoice receipt.
        paid = next(i for i in range(first+1,len(book['steps'])+1) if claims.events(r.sets(book)[i],0,key))
        again=r.c.repoint(r.sets(book)[paid],name+'.return')
        book['steps'].insert(paid,dict(cause='position',after=again))
        book['standalone'].insert(paid,dict(kind='unavailable',reason='Cash return retains every original paid recipient and prior legal share.'))
        mapping.insert(paid+1,mapping[paid]);partition['split']['applied_points']=list(range(first,len(book['steps'])+1))
        legal[0]['applied_points']=list(range(first,len(book['steps'])+1))
        move(paid+1,2,3,'.25','SECOND.CASH')
        # Same cutoff ordered return proves outstanding ownership differs from paid recipients.
        move(paid+1,2,1,'.25','RETURN.CASH')
    points=r.sets(book);expected=clone(template['expected'])
    for trade in expected['trades']:trade['marks']=[clone(trade['marks'][i]) for i in mapping]
    for k,share in enumerate([D('.4'),D('.6')]):
        t=clone(expected['trades'][1]);t['id']=child_ids[k]
        for m in t['marks']:
            for cash in m['claims']:
                for keyname in ['amount','recorded','outstanding','removed']:
                    cash[keyname]=D(cash[keyname])*share
            m.update(physical=None,recorded=ZERO,proposed=ZERO)
        expected['trades'].append(t)
    for i,info in enumerate(points):
        original=expected['trades'][1]['marks'][i]['physical']
        received=D(original['received_face']) if original else ZERO
        active=i in partition['split']['applied_points']
        if active:
            info['point']['positions'][1]['current']['quantity']='0'
            for j,share in [(2,D('.4')),(3,D('.6'))]:info['point']['positions'][j]['current']['quantity']=str(received*share)
        for j,t in enumerate(expected['trades']):
            m=t['marks'][i];m['transferred']=[]
            if j<1:continue
            ownshare=D(1) if j==1 and not active else ZERO if j==1 or not active else [D('.4'),D('.6')][j-2]
            phys=clone(original) if original and (j==1 or active) else None
            if phys:
                for k in ['proposed_face','received_face','outstanding_face']:phys[k]=D(original[k])*ownshare
                phys['outstanding_asset_present_value']=D(m['reference'])*D(phys['outstanding_face'])/D(info['point']['positions'][j]['current']['reference_quantity'])
                if active:
                    phys['partition']=dict(facts=clone(partition),original_proposed_face=face,
                        original_received_face=received,original_outstanding_face=face-received,
                        fraction=ownshare,receipts=[dict(event=clone(e),face=D(e['action']['terms']['face'])*ownshare) for e in original['security_receipts']])
                if j!=1:phys.update(outstanding_invoice=ZERO,invoice_present_value=ZERO,invoice_excluded=True)
            m['physical']=phys
            q=D(info['point']['positions'][j]['current']['quantity'])+(D(phys['outstanding_face']) if phys else ZERO)
            ref=D(info['point']['positions'][j]['current']['reference_quantity']);v=D(m['reference'])*q/ref
            if j==1 and original:v+=D(original['invoice_present_value'])
            m['local']=dict(current=v,pending=ZERO,projected=v);m['reporting']={k:v*D(m['fx']) for k,v in m['local'].items()}
            m['recorded']=D(original['invoice']['recorded']) if j==1 and original else ZERO
            m['proposed']=D(original['invoice']['amount']) if j==1 and original else ZERO
    # Invoice allocation keeps original receipts, shares at receipt, and custody distinct.
    for i,info in enumerate(points):
        physical=expected['trades'][1]['marks'][i]['physical']
        if not physical or not legal:continue
        active=[t for t in legal if i in t['applied_points']]
        shares=[ZERO,D(1),ZERO,ZERO];paid=[ZERO]*4;receipts=[[] for _ in owners]
        def transfer(t,values):
            sender=next(j for j,o in enumerate(owners) if o['trade_id']==t['from']);to=next(j for j,o in enumerate(owners) if o['trade_id']==t['to']);f=D(t['fraction']);values[sender]-=f;values[to]+=f
            if min(values)<0:raise ValueError('Overdrawn independent claim')
        for t in active:transfer(t,shares)
        for event in claims.events(info,0,key):
            atreceipt=[ZERO,D(1),ZERO,ZERO]
            for t in active:
                if event['action']['terms']['receipt_id'] not in t['cutoff']['receipt_ids']:transfer(t,atreceipt)
            for j,share in enumerate(atreceipt):
                if share:
                    amount=D(event['action']['terms']['amount']['value'])*share;paid[j]+=amount
                    receipts[j].append(dict(event=clone(event),fraction=share,amount=amount))
        cash=physical['invoice'];out=D(cash['amount'])-D(cash['recorded'])
        parties={1}|{j for t in active for j,o in enumerate(owners) if o['trade_id'] in [t['from'],t['to']]}
        for j in parties:
            m=expected['trades'][j]['marks'][i];value=out*shares[j]
            allocation=dict(origin=owners[0]['trade_id'],key=clone(key),entitlement=clone(ent),
                proposal=clone(next(s['event'] for s in info['holdings'][0]['lifecycle']['events'] if s['event']['action']['kind']=='bond_delivery')),
                transfers=clone(active),fraction=shares[j],receipts=clone(receipts[j]),owned_recorded=paid[j],
                owned_outstanding=value,owned_proposed=paid[j]+value,ratio=D(1),pv=value)
            m['transferred'].append(allocation)
            delta=value-(out if j==1 else ZERO)
            for k in ['current','projected']:m['local'][k]+=delta;m['reporting'][k]+=delta*D(m['fx'])
            m['recorded']=paid[j];m['proposed']=paid[j]+value
            if m['physical']:m['physical'].update(outstanding_invoice=ZERO,invoice_present_value=ZERO,invoice_excluded=True)
        # Parties without current receipt/share remain zero as declared above.
        if sum(paid,ZERO)!=D(cash['recorded']) or sum(shares,ZERO)!=1:raise ValueError('Independent cash conservation')
    binding.explain(expected,dict(cohorts=owners),book)
    request=residual.reencode(book,owners,links)
    request['operation']['input'].update(lot_splits=[],claim_transfers=legal,physical_partitions=[partition])
    case=dict(id=name,request=request,expected_error=None,expected=expected)
    # Actual/model comparison uses the same original future; custody creates no exercise.
    expected['delivery_comparisons']=template['expected']['delivery_comparisons']
    canonical.normalize(case)
    return case

def generate():
    with localcontext() as ctx:
        ctx.prec=60
        cases=[make(sign,at,transfer) for sign in [1,-1] for at,transfer in [(4,True),(6,True),(9,False)]]
    # Preserve whole full mixed cash-transfer books and CTD switches as regression controls.
    old=json.loads((FOLDER/'claim-pnl-bound.json').read_bytes())
    for name in ['claim.full.ctd_signed','claim.full.simultaneous_ois_cds']:
        c=clone(next(c for c in old['cases'] if c['id']==name));c['request']['operation']['input']['physical_partitions']=[];cases.append(c)
    def reject(name,fn,code='invalid_input',source=0):
        c=clone(cases[source]);c.update(id='physical.invalid.'+name,expected_error=code);c.pop('expected');fn(c['request']['operation']['input']);cases.append(c)
    part=lambda s:s['physical_partitions'][0]
    reject('missing_child',lambda s:part(s)['split']['children'].pop())
    reject('duplicate_child',lambda s:part(s)['split']['children'].append(part(s)['split']['children'][0]))
    reject('foreign_future',lambda s:part(s).update(future='ABSENT'))
    reject('wrong_delivery',lambda s:part(s).update(delivery_id='ABSENT'))
    reject('foreign_security_receipt',lambda s:part(s)['receipt_ids'].append('ABSENT'))
    reject('duplicate_partition',lambda s:s['physical_partitions'].append(clone(part(s))))
    reject('unavailable_custody',lambda s:part(s)['split'].update(known_at='2027-12-02T12:00:00Z'))
    reject('disappearing_custody',lambda s:part(s)['split']['applied_points'].pop())
    reject('wrong_child_face',lambda s:s['cohorts'][2].update(quantity='1'))
    reject('wrong_child_acquisition',lambda s:s['cohorts'][2]['acquisition'].update(split_id='ABSENT'))
    reject('cash_overdraw',lambda s:s['claim_transfers'][1].update(fraction='.9'))
    reject('cash_from_future',lambda s:s['claim_transfers'][0].update(**{'from':s['cohorts'][0]['trade_id']}))
    reject('foreign_invoice_proposal',lambda s:s['claim_transfers'][0].update(proposal_id='ABSENT'))
    reject('invoice_paid_prefix_omitted',lambda s:s['claim_transfers'][1]['cutoff']['receipt_ids'].clear())
    reject('invoice_prefix_foreign',lambda s:s['claim_transfers'][1]['cutoff']['receipt_ids'].append('ABSENT'))
    reject('omitted_security_prefix',lambda s:part(s)['receipt_ids'].clear(),source=1)
    reject('wrong_custody_cause',lambda s:s['book']['steps'][4].update(cause='event'))
    reject('unsupported_physical_resplit',lambda s:s['lot_splits'].append(dict(part(s)['split'],parent=s['cohorts'][2]['trade_id'])))
    for c in cases:
        c['input_nodes']=lots.nodes(c['request']);c['input_bytes']=len(json.dumps(c['request'],separators=(',',':')).encode())
    return dict(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
        tolerances=json.loads(SOURCE.read_bytes())['tolerances'],cases=cases,certified=False)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate()
    raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Full independent delivery custody controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),max_nodes=max(c['input_nodes'] for c in data['cases']),certified=False)))
