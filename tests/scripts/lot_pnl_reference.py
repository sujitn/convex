"""Independent complete boundary-amendment and conserved-lot controls.

Reuse full original Decimal60 product, curve, history, model and basket owners.
Future profiles change at a processed original boundary; earned cash stays exact.
Named successor lots partition signed face and exclude historical cash. Original
parent residual ownership and receipts survive. No native answers or wider bounds.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
from pathlib import Path
import cohort_pnl_reference as r
import cohort_pnl_states as states
import cohort_pnl_contracts as fixed
import cohort_pnl_cash_dates as dates
import cohort_pnl_calendars as calendars
import residual_pnl_reference as residual
import residual_pnl_canonical as canonical
from cohort_pnl_wire import units
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
TARGET=FOLDER/'lot-pnl-reference.json'
SOURCES=['cohort-pnl-calendars.json','cohort-pnl-full-wire.json','residual-pnl-canonical.json']
clone=copy.deepcopy

def original(name):
    return clone(next(c for f in SOURCES for c in json.loads((FOLDER/f).read_bytes())['cases'] if c['id']==name))

def append(book,after,cause):
    book['steps'].append(dict(cause=cause,after=after))
    book['standalone'].append(dict(kind='unavailable',reason='Requires the exact original ordered legal amendment, security acquisition and receipt prefix.'))

def amend(info,label):
    info=r.c.repoint(info,label);h=info['holdings'][0];t=h['product']['product']['bond']['terms'];at='2026-10-07'
    for name,change in [('coupon',D('.008')),('spread',D('.001'))]:
        rows=[]
        for row in t['profiles'][name]:
            if row['start']<at:rows.append(dict(row,end=min(at,row['end'])))
            if row['end']>at:rows.append(dict(row,start=max(at,row['start']),value=str(D(row['value'])+change)))
        t['profiles'][name]=rows
    step=r.c.append_event(h,at,'bond_amendment',dict(prior_version=h['lifecycle']['version'],version=h['lifecycle']['version']+1,
        agreement=r.c.pin(label+'.LEGAL'),terms=clone(t)),known=info['point']['known_at'])
    step['event']['event_id']=h['id']+'.'+label+'.AMENDMENT'
    info['point']['positions'][0]['identity']['instrument']['version']+=1
    return info

def full_case(name,source,split=True,chain=False,sign=1,single=False):
    base=original(source);book=states.expand(base['request']);spec=base['request']['operation']['input']
    owners=clone(spec['cohorts']);opening=r.opening_from(base,book)
    if single:
        for info in r.sets(book):
            info['holdings']=info['holdings'][:1];info['point']['positions']=info['point']['positions'][:1]
            info['point']['fx']=[];info['point']['fills']=[]
        owners=owners[:1];opening=r.c.ClaimSet(opening[:1]);opening._events=[len(book['before']['holdings'][0]['lifecycle']['events'])]
        book['reporting_currency']='USD'
    if sign==-1:
        owners[0]['quantity']=str(-D(owners[0]['quantity']))
        for cash in opening[0]:cash['amount']=-D(cash['amount']);cash['recorded']=-D(cash['recorded'])
        for info in r.sets(book):
            p=info['point']['positions'][0];p['current']['quantity']=str(-D(p['current']['quantity']));p['pending_quantity']=str(-D(p['pending_quantity']))
            h=info['holdings'][0];h['lifecycle']['position']['value']=owners[0]['quantity']
            for s in h['lifecycle']['events']:
                if s['event']['action']['kind']=='payment':
                    a=s['event']['action']['terms']['amount'];a['value']=str(-D(a['value']))
    for info in r.sets(book):info['point']['positions'][0]['pending_quantity']='0'
    after=amend(book['steps'][-1]['after'],name+'.amended');append(book,after,'amendment');splits=[]
    if split:
        count=4 if chain else 2;fractions=[D('.4'),D('.6'),D('.1'),D('.3')][:count]
        parent=owners[0];quantity=D(parent['quantity']);child_ids=[]
        for k,fraction in enumerate(fractions):
            ident=name+'.child.'+str(k);child_ids.append(ident);child=clone(parent)
            child.update(trade_id=ident,quantity=str(quantity*fraction),acquisition=dict(kind='lot_split',split_id=name+('.SECOND' if k>=2 else '.FIRST')),disposal_fill=None)
            for e in child['entitlements']:e['entitled']=False;e.pop('residual',None)
            owners.append(child);opening.append([dict(x,amount=D(x['amount'])*fraction,recorded=D(0)) for x in opening[0]])
            for info in r.sets(book):
                h=clone(info['holdings'][0]);h['id']=ident;h['lifecycle']['trade_id']=ident;h['lifecycle']['position']['value']=child['quantity']
                h['lifecycle']['events']=[s for s in h['lifecycle']['events'] if s['event']['action']['kind']!='payment']
                for n,s in enumerate(h['lifecycle']['events']):s['event'].update(trade_id=ident,expected_revision=n,event_id=ident+'.ORIGINAL.'+str(n))
                p=clone(info['point']['positions'][0]);p['identity']['id']=ident
                p['identity']['economics']=r.c.pin(ident+'.ORIGINAL.'+info['point']['snapshot_id']);p['identity']['calculation']=r.c.pin(ident+'.CALC.'+info['point']['snapshot_id'])
                p['current']['quantity']='0';p['pending_quantity']='0';info['holdings'].append(h);info['point']['positions'].append(p)
            opening._events.append(len(book['before']['holdings'][-1]['lifecycle']['events']))
        first=r.c.repoint(book['steps'][-1]['after'],name+'.split');first['point']['positions'][0]['current']['quantity']='0';start=len(owners)-count
        for k in range(2):first['point']['positions'][start+k]['current']['quantity']=owners[start+k]['quantity']
        append(book,first,'position');first_index=len(book['steps'])
        splits.append(dict(id=name+'.FIRST',parent=parent['trade_id'],children=child_ids[:2],effective_date='2026-10-07',known_at=first['point']['known_at'],
            source=r.c.pin(name+'.CUSTODY.FIRST'),applied_points=[first_index]))
        if chain:
            second=r.c.repoint(first,name+'.second_split');second['point']['positions'][start]['current']['quantity']='0'
            for k in (2,3):second['point']['positions'][start+k]['current']['quantity']=owners[start+k]['quantity']
            append(book,second,'position');splits[0]['applied_points'].append(len(book['steps']))
            splits.append(dict(id=name+'.SECOND',parent=child_ids[0],children=child_ids[2:],effective_date='2026-10-07',known_at=second['point']['known_at'],
                source=r.c.pin(name+'.CUSTODY.SECOND'),applied_points=[len(book['steps'])]))
        paid=r.c.repoint(book['steps'][-1]['after'],name+'.parent_cash');claim=r.inferred(paid,0,opening,owners[0])[0][0]
        outstanding=D(claim['amount'])-D(claim['recorded'])
        if outstanding:
            residual.receipt(paid,0,dict(key=r.c.key(claim['kind'],claim['effective'],claim['ordinal'])),outstanding/2,name+'.PARENT.RECEIPT')
            append(book,paid,'event')
            for s in splits:s['applied_points'].append(len(book['steps']))
    acquired_original=r.acquired
    def acquired(info,owner,physical):
        if owner['acquisition']['kind']!='lot_split':return acquired_original(info,owner,physical)
        i=next(i for i,s in enumerate(r.sets(book)) if s['point']['snapshot_id']==info['point']['snapshot_id'])
        return any(s['id']==owner['acquisition']['split_id'] and i in s['applied_points'] for s in splits)
    previous=(r.native,r.inferred,r.c.de.cash,r.acquired)
    r.native=fixed.native;r.inferred=calendars.inferred;r.c.de.cash=dates.cash;r.acquired=acquired
    try:
        expected=r.financial(book,owners,[],opening)
        template=dict(request=residual.reencode(book,owners,[]),expected=expected)
        result=residual.derive(template,book,owners,[],list(range(len(r.sets(book)))))
    finally:r.native,r.inferred,r.c.de.cash,r.acquired=previous
    result['id']=name;result['request']['operation']['input']['lot_splits']=splits
    for j,trade in enumerate(result['expected']['trades']):
        for i,mark in enumerate(trade['marks']):
            for cash in mark['claims']:
                if cash.get('residual'):
                    cash['residual']['proposal']=next(s['event'] for s in r.sets(book)[i]['holdings'][j]['lifecycle']['events']
                        if s['event']['action']['kind']=='contractual_cash' and s['event']['effective_date']==cash['key']['effective_date'])
    units(result);canonical.normalize(result)
    return result

def nodes(v):
    return 1+sum(nodes(x) for x in v.values()) if isinstance(v,dict) else 1+sum(nodes(x) for x in v) if isinstance(v,list) else 1

def generate():
    with localcontext() as context:
        context.prec=60
        cases=[full_case('lot.full.ctd_amend_split','cohort.full.ctd_ties'),
            full_case('lot.single.short.successive','cohort.full.ctd_ties',chain=True,single=True),
            full_case('lot.single.long.successive','cohort.full.ctd_ties',chain=True,single=True,sign=-1),
            full_case('lot.full.five_causes_residual','residual.full.five_cause.0.4'),
            full_case('lot.full.five_causes_amendment','cohort.full.all_five_causes_full_irs_disposal',split=False)]
    def reject(name,change,code='invalid_input',source=0):
        c=clone(cases[source]);c.update(id='lot.invalid.'+name,expected_error=code);c.pop('expected');change(c['request']['operation']['input']);cases.append(c)
    reject('foreign_parent',lambda x:x['lot_splits'][0].update(parent='ABSENT'))
    reject('missing_child',lambda x:x['lot_splits'][0]['children'].pop())
    reject('repeated_child',lambda x:x['lot_splits'][0]['children'].__setitem__(1,x['lot_splits'][0]['children'][0]))
    reject('face_loss',lambda x:x['cohorts'][-1].update(quantity='-600'))
    reject('wrong_direction',lambda x:x['cohorts'][-1].update(quantity='720'))
    reject('missing_link',lambda x:x.update(lot_splits=[]))
    reject('unknown_cutoff',lambda x:x['lot_splits'][0].update(known_at='2027-10-07T12:00:00Z'))
    reject('unprocessed_date',lambda x:x['lot_splits'][0].update(effective_date='2026-10-08'),code='missing_input')
    reject('disappearing_fact',lambda x:x['lot_splits'][0]['applied_points'].pop())
    reject('parent_pending',lambda x:x['book']['steps'][-1]['after']['point']['positions'][0].update(pending_quantity='1'))
    reject('earned_claim_twice',lambda x:x['cohorts'][-1]['entitlements'][0].update(entitled=True))
    reject('wrong_cause',lambda x:x['book']['steps'][-2].update(cause='event'))
    def amendment(x,field,value):
        for h in x['book']['holdings']:
            for s in h['lifecycle']['events']:
                if s['event']['action']['kind']=='bond_amendment':s['event']['action']['terms'][field]=value
    reject('stale_amendment_revision',lambda x:amendment(x,'prior_version',99))
    reject('reused_amendment_revision',lambda x:amendment(x,'version',1))
    def history(x):
        for h in x['book']['holdings']:
            for s in h['lifecycle']['events']:
                if s['event']['action']['kind']=='bond_amendment':s['event']['action']['terms']['terms']['profiles']['coupon'][0]['value']='0.09'
    reject('amended_earned_coupon',history)
    reject('successor_before_original',lambda x:x['lot_splits'][1].update(applied_points=clone(x['lot_splits'][0]['applied_points'])),source=1)
    def orphan(x):
        for h in x['book']['holdings']:h['lifecycle']['events']=[s for s in h['lifecycle']['events'] if s['event']['action']['kind']!='bond_amendment']
    reject('terms_without_actual_amendment',orphan)
    result=dict(certified=False,reference=__doc__,sources={f:hashlib.sha256((FOLDER/f).read_bytes()).hexdigest() for f in SOURCES},
        tolerances=json.loads((FOLDER/'residual-pnl-canonical.json').read_bytes())['tolerances'],cases=cases)
    for c in cases:c['input_nodes']=nodes(c['request']);c['input_bytes']=len(json.dumps(c['request'],separators=(',',':')).encode())
    return result

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate()
    raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete independent financial controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),max_nodes=max(c['input_nodes'] for c in data['cases']),certified=False)))
