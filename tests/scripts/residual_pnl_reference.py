"""Independent residual-cash algebra over immutable complete financial owners.

Reuse full prior independently declared product/curve/history/model valuations,
never native candidate answers. Only legal ownership, actual dated cash receipt
prefixes and cumulative security face are changed. Every original native price,
all basket alternatives/CTD winners, cash dates and financial bounds survive.
Exact Decimal60 ownership/cash/face conservation and full causal P&L are derived
from original complete inputs and original independent financial rows.
"""
import argparse, copy, hashlib, json
from decimal import Decimal as D, localcontext
from pathlib import Path
import cohort_pnl_reference as r
import cohort_pnl_states as expanded
import cohort_pnl_bindings as binding
import cohort_pnl_cash_dates as dates
import causal_pnl_ready as indexed
import causal_pnl_wire as wire
from cohort_pnl_wire import units

FOLDER = Path(__file__).resolve().parents[1] / 'fixtures/library'
TARGET = FOLDER / 'residual-pnl-original-events.json'
SOURCES = ['cohort-pnl-calendars.json', 'cohort-pnl-full-wire.json']
clone = copy.deepcopy
ZERO = D(0)

def source_cases():
    return [c for name in SOURCES for c in json.loads((FOLDER/name).read_bytes())['cases']]

def original(name):
    return clone(next(c for c in source_cases() if c['id'] == name))

def receipt(info, j, claim, amount, label):
    h = info['holdings'][j]
    event = r.c.append_event(h, info['point']['valuation_date'], 'payment',
        dict(receipt_id=label, obligation=clone(claim['key']),
             amount=r.c.money(str(amount), info['point']['positions'][j]['current']['currency']),
             value_date=info['point']['valuation_date'], source='residual.synthetic.cash-record'),
        known=info['point']['known_at'])
    return event['event']

def reencode(book, owners, links):
    request=r.c.request(book['before'],[],[])
    request['operation']['input']=book
    result=wire.normalize(indexed.encode(request))
    result['operation']=dict(kind='cohort_book_pnl',input=dict(
        book=result['operation']['input'],cohorts=clone(owners),physical_deliveries=clone(links)))
    units(result)
    return result

def derive(template, book, owners, links, mapping, signs=None):
    """Full unchanged native valuations plus exact original receipt ownership."""
    signs=signs or {}; expected=clone(template['expected']); points=r.sets(book)
    for j, trade in enumerate(expected['trades']):
        owner=owners[j]; oldmarks=trade['marks']; marks=[]
        for i, info in enumerate(points):
            m=clone(oldmarks[mapping[i]]); p=info['point']['positions'][j]
            events=[x['event'] for x in info['holdings'][j]['lifecycle']['events']]
            payments=[e for e in events if e['action']['kind']=='payment']
            acquired=r.acquired(info,owner,m['physical'])
            physical=m['physical']
            oldface=D(physical['outstanding_face']) if physical else ZERO
            if physical:
                future=next(k for k,o in enumerate(owners) if o['trade_id']==physical['future'])
                history=[x['event'] for x in info['holdings'][future]['lifecycle']['events']]
                security=[e for e in history if e['action']['kind']=='security_receipt']
                face=D(owner['quantity']); received=sum((D(e['action']['terms']['face']) for e in security),ZERO)
                physical.update(security_receipt=clone(security[-1]) if security else None,
                    security_receipts=clone(security),proposed_face=face,received_face=received,outstanding_face=face-received)
                invoice=physical['invoice']; key=r.c.key(invoice['kind'],invoice['effective'],invoice['ordinal'])
                invoice['recorded']=sum((D(e['action']['terms']['amount']['value']) for e in history
                    if e['action']['kind']=='payment' and e['action']['terms']['obligation']==key),ZERO)
                physical['outstanding_invoice']=D(invoice['amount'])-invoice['recorded']
                physical['invoice_present_value']=physical['outstanding_invoice']*D(physical['invoice_discount_ratio'])
                physical['outstanding_asset_present_value']=D(m['reference'])*(face-received)/D(p['current']['reference_quantity'])*D(m['remaining'])
            claimpv=ZERO; removed=ZERO; recorded=ZERO; proposed=ZERO
            for cash in m['claims']:
                cash['amount']=D(cash['amount'])*D(signs.get(j,1))
                cash['recorded']=sum((D(e['action']['terms']['amount']['value']) for e in payments
                    if e['action']['terms']['obligation']==cash['key']),ZERO)
                cash['outstanding']=cash['amount']-cash['recorded']
                ent=next(e for e in owner['entitlements'] if e['key']==cash['key'])
                cash['entitled']=acquired and ent['entitled']
                owned_proposed=cash['amount']; owned_recorded=cash['recorded']; owned_outstanding=cash['outstanding']
                residual=ent.get('residual')
                if residual and acquired:
                    ids=residual['cutoff']['receipt_ids']
                    prior=[e for e in payments if e['action']['terms']['receipt_id'] in ids]
                    if len(prior)!=len(ids):raise ValueError('Undeclared original receipt prefix')
                    before=sum((D(e['action']['terms']['amount']['value']) for e in prior),ZERO)
                    f=D(residual['fraction']); owned_proposed=(cash['amount']-before)*f
                    owned_recorded=(cash['recorded']-before)*f; owned_outstanding=cash['outstanding']*f
                    cash['residual']=dict(entitlement=clone(ent),prior_receipts=clone(prior),prior_recorded=before,
                        owned_proposed=owned_proposed,owned_recorded=owned_recorded,owned_outstanding=owned_outstanding)
                else:cash['residual']=None
                # Original future embedded cash removal scales only held exposure.
                oldp=r.sets(expanded.expand(template['request']))[mapping[i]]['point']['positions'][j]
                oldq=D(oldp['current']['quantity'])+oldface
                newq=D(p['current']['quantity'])+(D(physical['outstanding_face']) if physical else ZERO)
                cash['removed']=D(cash['removed'])*newq/oldq if oldq else ZERO
                if cash['ratio'] is not None:
                    cash['ratio']=D(1)
                    if owned_outstanding!=0 and cash['entitled'] and cash['due']>info['point']['valuation_date']:
                        discount=r.c.curve_zero(info['inputs'],p['cash_discount']['id'])
                        cash['ratio']=(-discount*D((r.c.day(cash['due'])-r.c.day(info['point']['valuation_date'])).days)/365).exp()
                cash['pv']=owned_outstanding*cash['ratio'] if cash['ratio'] is not None and cash['entitled'] else ZERO if cash['ratio'] is not None else None
                claimpv+=cash['pv'] or ZERO; removed+=cash['removed']
                if cash['entitled'] and cash['key']['kind']!='delivery_invoice':
                    recorded+=owned_recorded;proposed+=owned_proposed
            if physical:
                claimpv+=physical['invoice_present_value'];recorded+=physical['invoice']['recorded'];proposed+=D(physical['invoice']['amount'])
            quantity=D(p['current']['quantity'])+(D(physical['outstanding_face']) if physical else ZERO)
            refq=D(p['current']['reference_quantity']); remaining=D(m['remaining']); reference=D(m['reference'])
            pending=D(p['pending_quantity'])
            m['local']=dict(current=reference*quantity/refq*remaining+claimpv-removed,
                pending=reference*pending/refq*remaining,
                projected=reference*(quantity+pending)/refq*remaining+claimpv-removed)
            m['reporting']={k:v*D(m['fx']) for k,v in m['local'].items()}
            fills=[f for f in info['point']['fills'] if f['trade_id']==trade['id']]
            m.update(recorded=recorded,proposed=proposed,
                executed=sum((D(f['quantity']) for f in fills),ZERO),
                consideration=sum((D(f['consideration']['value']) for f in fills),ZERO),
                fees=sum((D(f['fees']['value']) for f in fills),ZERO))
            m['executed_marking']=reference*m['executed']/refq*remaining
            marks.append(m)
        trade['marks']=marks
    binding.explain(expected,dict(cohorts=owners),book)
    result=dict(id='',request=reencode(book,owners,links),expected_error=None,expected=expected)
    expected['delivery_comparisons']=dates.comparisons(result)
    return result

def append(book, after, cause, mapping, origin):
    book['steps'].append(dict(cause=cause,after=after));mapping.insert(1+len(book['steps'])-1,origin)
    book['standalone'].append(dict(kind='unavailable',reason='Actual legal cash ownership and receipt prefix require original causal events.'))

def legal(owner, key, cutoff, ids, fraction='1'):
    ent=next(e for e in owner['entitlements'] if e['key']==key);ent['entitled']=True
    ent['residual']=dict(id=owner['trade_id']+'.RESIDUAL.'+key['kind'],fraction=fraction,
        cutoff=dict(date=cutoff['date'],known_at=cutoff['known_at'],receipt_ids=clone(ids)))
    return ent

def recovery(fraction='1',sign=1,dispose=False):
    base=original('cohort.invalid.acquired_historically_partial_paid_claim')
    template=original('cohort.credit.acquired_actual_recovery_claim')
    book=expanded.expand(base['request']);owners=clone(base['request']['operation']['input']['cohorts'])
    for info in r.sets(book):
        for p in info['point']['positions']:p['current']['quantity']=str(D(p['current']['quantity'])*sign)
        for h in info['holdings']:
            h['lifecycle']['position']['value']=str(D(h['lifecycle']['position']['value'])*sign)
            for step in h['lifecycle']['events']:
                if step['event']['action']['kind']=='payment':
                    money=step['event']['action']['terms']['amount'];money['value']=str(D(money['value'])*sign)
        for fill in info['point']['fills']:
            fill['quantity']=str(D(fill['quantity'])*sign);fill['consideration']['value']=str(D(fill['consideration']['value'])*sign)
    owner=owners[0];owner['quantity']=str(D(owner['quantity'])*sign)
    key=next(e['key'] for e in owner['entitlements'] if e['key']['kind']=='recovery')
    acq=book['steps'][-1]['after'];fill=acq['point']['fills'][0]
    prior=next(e['event']['action']['terms']['receipt_id'] for e in acq['holdings'][0]['lifecycle']['events'] if e['event']['action']['kind']=='payment')
    legal(owner,key,fill,[prior],fraction)
    mapping=[0,0,0,1]
    partial=r.c.repoint(acq,'RESIDUAL.RECOVERY.PARTIAL')
    receipt(partial,0,dict(key=key),D('-64125')*sign,'RESIDUAL.RECOVERY.INTERVAL.PARTIAL')
    append(book,partial,'event',mapping,1)
    if dispose:
        sold=r.c.repoint(partial,'RESIDUAL.RECOVERY.DISPOSED')
        owner['disposal_fill']=r.execution(sold,0,'RESIDUAL.RECOVERY.DISPOSAL',-D(owner['quantity']),D('55000')*sign)
        append(book,sold,'position',mapping,1);partial=sold
    full=r.c.repoint(partial,'RESIDUAL.RECOVERY.FULL')
    receipt(full,0,dict(key=key),D('-64125')*sign,'RESIDUAL.RECOVERY.INTERVAL.FULL')
    append(book,full,'event',mapping,1)
    result=derive(template,book,owners,[],mapping,{0:sign})
    result['id']=f'residual.recovery.{"short" if sign==1 else "long"}.{fraction}.{"disposed" if dispose else "held"}'
    return result

def coupon(fraction='1',sign=1,dispose=False):
    template=original('cohort.full.acquired_after_earned_coupon_ctd');book=expanded.expand(template['request'])
    owners=clone(template['request']['operation']['input']['cohorts']);j=len(owners)-1;owner=owners[j]
    owner['quantity']=str(D(owner['quantity'])*sign);key=owner['entitlements'][0]['key']
    for info in r.sets(book):
        p=info['point']['positions'][j];p['current']['quantity']=str(D(p['current']['quantity'])*sign)
        info['holdings'][j]['lifecycle']['position']['value']=owner['quantity']
        for fill in info['point']['fills']:
            if fill['trade_id']==owner['trade_id']:
                fill['quantity']=owner['quantity'];fill['consideration']['value']=str(D(fill['consideration']['value'])*sign)
        receipt(info,j,dict(key=key),D('4')*sign,'RESIDUAL.COUPON.PRIOR')
    acq=book['steps'][-1]['after'];fill=next(f for f in acq['point']['fills'] if f['trade_id']==owner['trade_id'])
    legal(owner,key,fill,['RESIDUAL.COUPON.PRIOR'],fraction)
    mapping=list(range(len(r.sets(book))))
    paid=r.c.repoint(acq,'RESIDUAL.COUPON.PARTIAL')
    receipt(paid,j,dict(key=key),D('2')*sign,'RESIDUAL.COUPON.INTERVAL.PARTIAL')
    append(book,paid,'event',mapping,2)
    if dispose:
        sold=r.c.repoint(paid,'RESIDUAL.COUPON.DISPOSED')
        owner['disposal_fill']=r.execution(sold,j,'RESIDUAL.COUPON.DISPOSAL',-D(owner['quantity']),D('590')*sign)
        append(book,sold,'position',mapping,2);paid=sold
    full=r.c.repoint(paid,'RESIDUAL.COUPON.FULL')
    receipt(full,j,dict(key=key),D('4.675')*sign,'RESIDUAL.COUPON.INTERVAL.FULL')
    append(book,full,'event',mapping,2)
    result=derive(template,book,owners,[],mapping,{j:sign})
    result['id']=f'residual.coupon.mixed.{"long" if sign==1 else "short"}.{fraction}.{"disposed" if dispose else "held"}'
    return result

def opening(five=False,fraction='0.4'):
    template=original('cohort.full.all_five_causes_full_irs_disposal' if five else 'cohort.full.native_USD_EUR_cash_ctd')
    book=expanded.expand(template['request']);owners=clone(template['request']['operation']['input']['cohorts'])
    j=next(j for j,t in enumerate(template['expected']['trades']) if t['marks'][0]['claims'] and owners[j]['acquisition']['kind']=='opening'
        and (five or owners[j]['trade_id']=='cohort.EUR.cash')
        and t['marks'][0]['claims'][0]['due']<=book['before']['point']['valuation_date']
        and t['marks'][0]['claims'][0]['key']['kind']=='bond_coupon')
    key=template['expected']['trades'][j]['marks'][0]['claims'][0]['key']
    amount=D(template['expected']['trades'][j]['marks'][0]['claims'][0]['amount']);first=(amount/8).quantize(D('0.001'))
    # Retain one actual before-prefix event across all original replay histories.
    before=book['before'];ordinal=len(before['holdings'][j]['lifecycle']['events'])
    event=receipt(before,j,dict(key=key),first,'RESIDUAL.OPENING.PRIOR')
    event['event_id']=owners[j]['trade_id']+'.OWNERSHIP.PRIOR'
    prior=clone(before['holdings'][j]['lifecycle']['events'][-1])
    for info in r.sets(book)[1:]:
        events=info['holdings'][j]['lifecycle']['events'];events.insert(ordinal,clone(prior))
        for k,step in enumerate(events):step['event']['expected_revision']=k
    point=book['before']['point'];legal(owners[j],key,dict(date=point['valuation_date'],known_at=point['known_at']),['RESIDUAL.OPENING.PRIOR'],fraction)
    mapping=list(range(len(r.sets(book))))
    if five:
        end=r.c.repoint(book['steps'][-1]['after'],'RESIDUAL.OPENING.PARTIAL')
        receipt(end,j,dict(key=key),first,'RESIDUAL.OPENING.INTERVAL.PARTIAL')
        append(book,end,'event',mapping,len(book['steps']))
    else:
        # Reuse the original actual event phase inside the original node bound.
        after=book['steps'][2]['after'];receipt(after,j,dict(key=key),first,'RESIDUAL.OPENING.INTERVAL.PARTIAL')
        final=clone(after['holdings'][j]['lifecycle']['events'][-1])
        for step in book['steps'][3:]:step['after']['holdings'][j]['lifecycle']['events'].append(clone(final))
    result=derive(template,book,owners,[],mapping);result['id']='residual.full.'+('five_cause' if five else 'USD_EUR_fixing_time')+'.'+fraction
    return result

def physical(sign=1,order='security_first'):
    name='cohort.delivery.short_security_then_cash' if sign==-1 else 'cohort.delivery.long_security_then_cash'
    template=original(name);book=expanded.expand(template['request']);spec=template['request']['operation']['input']
    owners=clone(spec['cohorts']);links=clone(spec['physical_deliveries']);face=D(owners[1]['quantity'])
    # Retain market/time/official clearing/actual delivery and original full models.
    book['steps']=book['steps'][:4];book['standalone']=book['standalone'][:4]
    mapping=[0,1,2,3,4,8]
    invoice=template['expected']['trades'][1]['marks'][4]['physical']['invoice']
    amount=D(invoice['amount']);key=r.c.key(invoice['kind'],invoice['effective'],invoice['ordinal'])
    prior=book['steps'][-1]['after'];total=ZERO
    actions=([('security','0.25'),('cash','0.5'),('security','0.35'),('cash','0.5'),('security','0.4')]
        if order=='interleaved' else [('cash','0.5'),('cash','0.5'),('security','0.25'),('security','0.35'),('security','0.4')]
        if order=='cash_first' else [('security','0.25'),('security','0.35'),('security','0.4'),('cash','0.5'),('cash','0.5')])
    for k,(kind,fraction) in enumerate(actions):
        info=r.c.repoint(prior,f'RESIDUAL.PHYSICAL.{order}.{k}')
        if kind=='cash':receipt(info,0,dict(key=key),amount*D(fraction),f'RESIDUAL.INVOICE.{k}')
        else:
            part=face*D(fraction);total+=part
            r.c.append_event(info['holdings'][0],info['point']['valuation_date'],'security_receipt',
                dict(obligation=key,instrument=links[0]['instrument'],face=str(part),receipt_id=f'RESIDUAL.SECURITY.{k}',source='residual.synthetic.security-record'),known=info['point']['known_at'])
            info['point']['positions'][1]['current']['quantity']=str(total)
        append(book,info,'event',mapping,4);prior=info
    result=derive(template,book,owners,links,mapping)
    result['id']=f'residual.physical.{"long" if sign==1 else "short"}.{order}'
    return result

def generate():
    with localcontext() as context:
        context.prec=60
        cases=[recovery('1'),recovery('0.4',1,True),recovery('0.6',-1,True),
            coupon('1'),coupon('0.35',1,True),coupon('0.6',-1,True),opening(False),opening(True)]
        cases += [physical(sign,order) for sign in [1,-1] for order in ['security_first','cash_first','interleaved']]
    def reject(name,source,mutation,code='invalid_input'):
        case=clone(source);case['id']='residual.invalid.'+name;case['expected_error']=code;case.pop('expected');mutation(case['request']['operation']['input']);cases.append(case)
    cash=cases[0];partial=cases[-1]
    def owned(spec):return next(e for o in spec['cohorts'] for e in o['entitlements'] if 'residual' in e)
    reject('omitted_cutoff',cash,lambda s:owned(s).pop('residual'))
    reject('empty_prior_ids',cash,lambda s:owned(s)['residual']['cutoff'].update(receipt_ids=[]))
    reject('duplicate_prior_ids',cash,lambda s:owned(s)['residual']['cutoff']['receipt_ids'].append(owned(s)['residual']['cutoff']['receipt_ids'][0]))
    reject('foreign_prior_id',cash,lambda s:owned(s)['residual']['cutoff'].update(receipt_ids=['FOREIGN.RECEIPT']))
    reject('later_receipt_as_prior',cash,lambda s:owned(s)['residual']['cutoff']['receipt_ids'].append('RESIDUAL.RECOVERY.INTERVAL.PARTIAL'))
    reject('cutoff_date',cash,lambda s:owned(s)['residual']['cutoff'].update(date='2026-10-14'))
    reject('cutoff_known_at',cash,lambda s:owned(s)['residual']['cutoff'].update(known_at='2026-10-16T12:00:00Z'))
    reject('zero_fraction',cash,lambda s:owned(s)['residual'].update(fraction='0'))
    reject('negative_fraction',cash,lambda s:owned(s)['residual'].update(fraction='-0.5'))
    reject('over_fraction',cash,lambda s:owned(s)['residual'].update(fraction='1.001'))
    reject('unentitled_residual',cash,lambda s:owned(s).update(entitled=False))
    reject('scalar_owned_cash',cash,lambda s:owned(s)['residual'].update(amount='1'))
    reject('cutoff_unknown_field',cash,lambda s:owned(s)['residual']['cutoff'].update(cash='1'))
    def security(spec,change):
        for holding in spec['book']['holdings']:
            for step in holding['lifecycle']['events']:
                e=step['event']
                if e['action']['kind']=='security_receipt':change(e)
    reject('zero_security',partial,lambda s:security(s,lambda e:e['action']['terms'].update(face='0')))
    reject('wrong_security_sign',partial,lambda s:security(s,lambda e:e['action']['terms'].update(face='50000')))
    reject('security_overdelivery',partial,lambda s:security(s,lambda e:e['action']['terms'].update(face='-200001')))
    reject('duplicate_security_id',partial,lambda s:security(s,lambda e:e['action']['terms'].update(receipt_id='DUPLICATE.SECURITY')))
    reject('foreign_security_revision',partial,lambda s:security(s,lambda e:e['action']['terms']['instrument'].update(version=999)))
    reject('foreign_security_key',partial,lambda s:security(s,lambda e:e['action']['terms']['obligation'].update(ordinal=999)))
    reject('missing_full_asset',partial,lambda s:s.update(physical_deliveries=[]),code='missing_input')
    reject('fabricated_current_face',partial,lambda s:s['book']['steps'][-1]['after']['point']['positions'][1]['current'].update(quantity='-190000'))
    reject('work_bound',cash,lambda s:s['book'].update(max_product_evaluations=1))
    if len({c['id'] for c in cases})!=len(cases):raise ValueError('Duplicate controls')
    return dict(certified=False,reference=__doc__,source_sha256={n:hashlib.sha256((FOLDER/n).read_bytes()).hexdigest() for n in SOURCES},
        tolerances=original('cohort.full.ctd_ties')['expected'].get('tolerances',json.loads((FOLDER/SOURCES[0]).read_bytes())['tolerances']),cases=cases)

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing to overwrite original financial declaration')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Independent full residual/face control bytes differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
