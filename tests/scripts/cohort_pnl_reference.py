"""Independent complete cohort/cash/physical controls, declared before execution.

Original Decimal60 dated native-independent financial owners are reused, not
candidate answers. Fixed original signed lots own separately declared earned
cash. Actual invoice uses original CF/accrued/rounding, never model-selected CTD.
Earlier control rows/files/bounds remain immutable. No native executable is read.
"""
import argparse, copy, datetime as dt, hashlib, json
from decimal import Decimal as D, localcontext, ROUND_HALF_UP
import causal_pnl_reference as c
import causal_pnl_admitted as a
import causal_pnl_economic_cutoffs as econ
import causal_pnl_ready as binding
import causal_pnl_wire as wire
clone=copy.deepcopy; ZERO=D(0); ROOT=c.ROOT
SOURCE=c.FOLDER/'causal-pnl-economic-cutoffs.json'
TARGET=c.FOLDER/'cohort-pnl-reference.json'

def sets(book):
    return [book['before']]+[s['after'] for s in book['steps']]+[s['information'] for s in book['standalone'] if s['kind']=='available']

def rows(mark):
    return [dict(kind=x['key']['kind'],effective=x['key']['effective_date'],ordinal=x['key']['ordinal'],due=x['due'],amount=D(x['amount']),recorded=D(x['recorded'])) for x in mark['claims']]

def swap_flows(terms):
    result=a.swap_flows(terms)
    for values in result.values():
        for row in values:
            if row['end']=='2024-03-31':row['payment']='2024-04-02'
    return result

def inferred(info,j,opening,owner):
    info=clone(info); info['point']['positions'][j]['current']['quantity']=owner['quantity']
    h=info['holdings'][j]; p=h['product'].get('product',h['product']); original_events=h['lifecycle']['events'];remaining=D(1)
    if p['kind']=='bond_future':
        result=clone(opening[j]);status='Active';previous=D(h['lifecycle']['instrument']['opening_mark']['price_points']);contract=p['input']['contract'];precision=D(10)**-contract['daily_settlement']['cash_decimal_places'];q=D(owner['quantity'])
        for step in original_events[opening._events[j]:]:
            e=step['event'];act=e['action'];at=e['effective_date'];t=act.get('terms',{})
            if act['kind']=='futures_settlement':
                level=D(t['price_points']);amount=((level*D(contract['currency_per_price_point'])).quantize(precision,rounding=ROUND_HALF_UP)-(previous*D(contract['currency_per_price_point'])).quantize(precision,rounding=ROUND_HALF_UP))*q
                result.append(dict(kind='variation_margin',effective=at,ordinal=0,due=at,amount=amount,recorded=ZERO));previous=level
            if act['kind']=='bond_delivery':
                selected=t['deliverable'];d=step['delivery'];bond=d['bond']['terms'];dates=c.op.grid(bond);accrued=ZERO
                for start,end in zip(dates,dates[1:]):
                    if str(start)<at<str(end):accrued=c.coupon(bond,str(start),at)
                face=D(contract['settlement']['contract_face']);accrued=(accrued*face/D(bond['face_value'])).quantize(precision,rounding=ROUND_HALF_UP)
                member=next(m for m in d['basket']['bonds'] if m['instrument']==selected)
                invoice=(D(t['invoice_price_points'])*D(member['conversion_factor'])*face/100+accrued).quantize(precision,rounding=ROUND_HALF_UP)
                result.append(dict(kind='delivery_invoice',effective=at,ordinal=0,due=at,amount=-invoice*q,recorded=ZERO));status='Matured';remaining=ZERO
            if act['kind']=='payment':
                x=next(x for x in result if c.key(x['kind'],x['effective'],x['ordinal'])==t['obligation']);x['recorded']+=D(t['amount']['value'])
        if status!='Active' and all(x['amount']==x['recorded'] for x in result) and any(s['event']['action']['kind']=='security_receipt' for s in original_events):status='Closed'
        return result,status,remaining,[]
    # The original claim formulas use original cohort quantity, even after a
    # security disposal. Separately declared termination agreements remain cash.
    h['lifecycle']['events']=[s for s in original_events if s['event']['action']['kind']!='terminate' and not(s['event']['action']['kind']=='payment' and s['event']['action']['terms']['obligation']['kind']=='termination')]
    old=a.swap_flows
    if p['kind']=='swap' and p['terms']['trade_id'].startswith('lifecycle.synthetic.'):
        flows=swap_flows(p['terms']);a.swap_flows=lambda _:clone(flows)
    try:result,status,remaining,notices=a.claims(info,j,opening)
    finally:a.swap_flows=old
    for step in original_events:
        e=step['event'];act=e['action'];t=act.get('terms',{})
        if act['kind']=='terminate':
            remaining*=D(1)-D(t['fraction']);result.append(dict(kind='termination',effective=e['effective_date'],ordinal=0,due=t['cash_date'],amount=D(t['cash']['value']),recorded=ZERO))
            if remaining==0:status='Terminated'
        if act['kind']=='payment' and t['obligation']['kind']=='termination':
            next(x for x in result if x['kind']=='termination' and x['effective']==t['obligation']['effective_date'])['recorded']+=D(t['amount']['value'])
    return result,status,remaining,notices

def native(info,h):
    p=h['product'].get('product',h['product'])
    if p['kind']=='swap' and p['terms']['trade_id'].startswith('lifecycle.synthetic.'):
        z=c.curve_zero(info['inputs'],p['terms']['collateral']['discount_curve']['id']);on=info['point']['valuation_date'];flows=swap_flows(p['terms']);result={k:[] for k in flows};pv=ZERO
        for leg,values in flows.items():
            for row in values:
                if row['payment']<=on:continue
                row=clone(row);row['pv']=row['amount']*(-z*D((c.day(row['payment'])-c.day(on)).days)/365).exp();pv+=row['pv'];result[leg].append(row)
        result['pv']=pv;return dict(kind='swap',pv=pv,full=result)
    result=econ.native(info,h)
    if result['kind']=='cds':
        result=clone(result);result['full']['pv']=D(result['full']['pv'])-D(result['full']['initial']);result['full']['initial']=ZERO;result['pv']=result['full']['pv']
    return result

def handoff(info,owners,links,j,opening):
    link=next((x for x in links if x['asset']==owners[j]['trade_id']),None)
    if not link:return None
    f=next(k for k,h in enumerate(info['holdings']) if h['id']==link['future']);h=info['holdings'][f]
    delivery=next((s['event'] for s in h['lifecycle']['events'] if s['event']['action']['kind']=='bond_delivery'),None)
    if not delivery:return None
    receipt=next((s['event'] for s in h['lifecycle']['events'] if s['event']['action']['kind']=='security_receipt'),None)
    face=D(owners[j]['quantity']);received=face if receipt else ZERO
    claim=next(x for x in inferred(info,f,opening,owners[f])[0] if x['kind']=='delivery_invoice')
    return dict(future=link['future'],delivery=delivery,security_receipt=receipt,instrument=link['instrument'],proposed_face=face,received_face=received,outstanding_face=face-received,invoice=clone(claim),outstanding_invoice=claim['amount']-claim['recorded'],invoice_discount_ratio=D(1),invoice_present_value=claim['amount']-claim['recorded'])

def acquired(info,owner,physical):
    acq=owner['acquisition'];return acq['kind']=='opening' or acq['kind']=='physical_delivery' and physical is not None or acq['kind']=='fill' and any(f['id']==acq['fill_id'] for f in info['point']['fills'])

def financial(book,owners,links,opening):
    points=sets(book);trades=[]
    for j,h0 in enumerate(book['before']['holdings']):
        owner=owners[j];marks=[]
        for info in points:
            h=info['holdings'][j];p=info['point']['positions'][j];kind=h['product'].get('product',h['product'])['kind'];basis='FuturesQuoteLevel' if kind in ['rate_future','bond_future'] else 'CurrencyPresentValue';on=info['point']['valuation_date'];discount=c.curve_zero(info['inputs'],p['cash_discount']['id']);cs,status,remaining,notices=inferred(info,j,opening,owner);phys=handoff(info,owners,links,j,opening);own=acquired(info,owner,phys)
            valuation=native(info,h) if status=='Active' else dict(kind='ended',pv=ZERO,full=dict(status=status))
            if notices:raise ValueError('No undeclared optional-notice model in this increment')
            reference=D(str(valuation['pv']));q=D(p['current']['quantity']);pending=D(p['pending_quantity']);ref=D(p['current']['reference_quantity']);effective_q=q+(phys['outstanding_face'] if phys else ZERO);claimpv=ZERO;removed=ZERO;claims=[];recorded=ZERO;proposed=ZERO
            for cash in cs:
                key=c.key(cash['kind'],cash['effective'],cash['ordinal']);entitled=own and next(x['entitled'] for x in owner['entitlements'] if x['key']==key);outstanding=cash['amount']-cash['recorded'];ratio=(-discount*D((c.day(cash['due'])-c.day(on)).days)/365).exp() if basis=='CurrencyPresentValue' and outstanding!=0 and entitled and cash['due']>on else D(1);pv=outstanding*ratio if basis=='CurrencyPresentValue' and entitled else ZERO;rem=ZERO
                if status=='Active' and kind in ['bond','swap'] and cash['due']>on:
                    rem=cash['amount']*effective_q/D(owner['quantity'])*remaining*(-discount*D((c.day(cash['due'])-c.day(on)).days)/365).exp()
                # Initial credit cash is excluded by the original remaining
                # exposure owner, then the exact actual lifecycle claim is added.
                if entitled and cash['kind']!='delivery_invoice':recorded+=cash['recorded'];proposed+=cash['amount']
                claimpv+=pv;removed+=rem;claims.append(dict(key=key,due=cash['due'],amount=cash['amount'],recorded=cash['recorded'],outstanding=outstanding,ratio=ratio if basis=='CurrencyPresentValue' else None,pv=pv if basis=='CurrencyPresentValue' else None,removed=rem,entitled=entitled))
            if phys:
                phys['outstanding_asset_present_value']=reference*phys['outstanding_face']/ref*remaining;claimpv+=phys['invoice_present_value'];recorded+=phys['invoice']['recorded'];proposed+=phys['invoice']['amount']
            local=dict(current=reference*effective_q/ref*remaining+claimpv-removed,pending=reference*pending/ref*remaining,projected=reference*(effective_q+pending)/ref*remaining+claimpv-removed);rate=next((D(x['rate']) for x in info['point']['fx'] if x['from']==p['current']['currency']),D(1));reporting={k:v*rate for k,v in local.items()};fills=[f for f in info['point']['fills'] if f['trade_id']==h['id']];executed=sum((D(f['quantity']) for f in fills),ZERO);consideration=sum((D(f['consideration']['value']) for f in fills),ZERO);fees=sum((D(f['fees']['value']) for f in fills),ZERO);terminal=None
            if basis=='FuturesQuoteLevel' and status!='Active':
                last=next(s['event']['action']['terms'] for s in reversed(h['lifecycle']['events']) if s['event']['action']['kind']=='futures_settlement');terminal=D(last['price_points'])*D(h['product']['product']['input']['contract']['currency_per_price_point'])*q
            marks.append(dict(native=valuation,basis=basis,status=status,remaining=remaining,claims=claims,reference=reference,local=local,reporting=reporting,recorded=recorded,proposed=proposed,executed=executed,executed_marking=reference*executed/ref*remaining,consideration=consideration,fees=fees,fx=rate,terminal_quote=terminal,physical=phys))
        def levels(report=False):
            result=[]
            for mark in marks:
                cash=mark['recorded']-marks[0]['recorded']+(mark['consideration'] if basis=='CurrencyPresentValue' else ZERO)+mark['fees'];adj=cash if basis=='CurrencyPresentValue' else mark['fees']-mark['consideration']+(mark['terminal_quote'] or ZERO);fx=mark['fx'] if report else D(1);v=mark['reporting' if report else 'local'];result.append(dict(marking=v,cash=cash*fx,current_model_pnl=v['current']+adj*fx))
            return result
        bound=D('.006')*D(h0['product']['product']['input']['bond']['terms']['face_value'])/100 if kind=='optional_bond' else D('1e-5') if basis=='FuturesQuoteLevel' else D('1e-7')
        trades.append(dict(id=h0['id'],marks=marks,local=c.explain(book,levels()),reporting=c.explain(book,levels(True)),bound=bound))
    answer=dict(trades=trades)
    for field,report in [('local_totals',False),('reporting_totals',True)]:
        totals=[];keys=list(dict.fromkeys((t['marks'][0]['basis'],book['reporting_currency'] if report else book['before']['point']['positions'][j]['current']['currency']) for j,t in enumerate(trades)))
        for basis,currency in keys:
            selected=[t for j,t in enumerate(trades) if t['marks'][0]['basis']==basis and (report or book['before']['point']['positions'][j]['current']['currency']==currency)]
            totals.append(dict(basis=basis,currency=currency,attribution=c.explain(book,[c.sum_rows([t['reporting' if report else 'local']['levels'][i] for t in selected]) for i in range(len(points))])))
        answer[field]=totals
    return answer

def owners_for(book,opening):
    owners=[]
    for j,p in enumerate(book['before']['point']['positions']):
        owner=dict(trade_id=p['identity']['id'],quantity=p['current']['quantity'],acquisition=dict(kind='opening'),disposal_fill=None,entitlements=[]);keys=[]
        for info in sets(book):
            for cash in inferred(info,j,opening,owner)[0]:
                key=c.key(cash['kind'],cash['effective'],cash['ordinal'])
                if key not in keys:keys.append(key)
        owner['entitlements']=[dict(key=k,entitled=True,source=c.pin(owner['trade_id']+'.entitlement.'+str(i))) for i,k in enumerate(keys)];owners.append(owner)
    return owners

def opening_from(case,book):
    result=c.ClaimSet([rows(t['marks'][0]) for t in case['expected']['trades']]);result._events=[len(h['lifecycle']['events']) for h in book['before']['holdings']];return result

def execution(info,j,label,quantity,consideration):
    p=info['point']['positions'][j];info['point']['fills'].append(dict(id=label,trade_id=p['identity']['id'],instrument=clone(p['identity']['instrument']),quantity=str(quantity),consideration=c.money(consideration,p['current']['currency']),fees=c.money('-1.25',p['current']['currency']),date=info['point']['valuation_date'],known_at=info['point']['known_at'],source='cohort.synthetic.actual-execution'))
    p['current']['quantity']=str(D(p['current']['quantity'])+D(quantity));return label

def physical_book(base,quantity='2',invoice_first=False):
    old=wire.expand(base['request'])['operation']['input']['before'];before=clone(old);before['holdings']=[before['holdings'][8]];before['point']['positions']=[before['point']['positions'][8]];before['point']['positions'][0].update(pending_quantity='0');before['point']['positions'][0]['current']['quantity']=quantity;before['holdings'][0]['lifecycle']['position']['value']=int(quantity)
    c.set_contexts(before,'2026-12-01');before=c.repoint(before,'cohort.physical.before.'+quantity);future=before['holdings'][0];inp=future['product']['product']['input'];member=inp['members'][1];face=D(inp['contract']['settlement']['contract_face'])*D(quantity);assetpos=clone(old['point']['positions'][0]);assetpos['identity'].update(id='cohort.actual.asset',instrument=clone(member['instrument']),economics=c.pin('cohort.asset.economics'),calculation=c.pin('cohort.asset.calculation'));assetpos['current'].update(quantity=str(face),reference_quantity=member['bond']['terms']['face_value']);assetpos['pending_quantity']='0';assetproduct=dict(kind='bond',bond=clone(member['bond']),context=clone(old['holdings'][0]['product']['product']['context']),projection=None,method=clone(old['holdings'][0]['product']['product']['method']))
    assetproduct['context']['settlement']='2026-12-01';assetproduct['context']['fixing'].update(valuation_date='2026-12-01',known_at=c.instant('2026-12-01'));asset=dict(id='cohort.actual.asset',product=dict(kind='product',product=assetproduct));asset,claims=c.lifecycle(asset,assetpos,before['inputs'],'2026-12-01');assetpos['current']['quantity']='0';before['holdings'].append(asset);before['point']['positions'].append(assetpos)
    # Time moves only exact actual funding starts and fresh immutable revisions.
    for m in inp['members']:
        for r in [m['financing'],m['own_cash'],m['coupon_carry'].get('rate')]:
            if r:r['start']='2026-12-01';r['stamp']['version']+=1000
    market=c.market(before,'cohort.physical.market.'+quantity);single=c.market(before,'cohort.physical.market.single.'+quantity)
    time=c.repoint(market,'cohort.physical.time.'+quantity);c.set_contexts(time,'2026-12-02')
    for m in time['holdings'][0]['product']['product']['input']['members']:
        for r in [m['financing'],m['own_cash'],m['coupon_carry'].get('rate')]:
            if r:r['start']='2026-12-02';r['stamp']['version']+=1000
    cleared=c.repoint(time,'cohort.physical.cleared.'+quantity);h=cleared['holdings'][0];contract=h['product']['product']['input']['contract'];mark=dict(contract=c.pin(contract['instrument_id'],contract['stamp']['version']),stamp=c.de.stamp(contract['instrument_id']+'.ACTUAL.DELIVERY.CLOSE',91),date='2026-12-02',kind='daily_settlement',price_points='102.125');mark['stamp'].update(source_at=c.instant('2026-12-02'),available_at=c.instant('2026-12-02'))
    futures=dict(clearing=dict(contract=c.pin(contract['instrument_id'],contract['stamp']['version']),money_method='rounded_contract_values',final_rate_increment=None,final_rate_rounding=None,fixing_selection=None,stamp=clone(contract['stamp'])),calendar_stamp=clone(inp['calendar_stamp']),context=clone(h['product']['product']['input']['context']))
    futures['clearing']['stamp'].update(id='cohort.actual.clearing',version=1)
    c.append_event(h,'2026-12-02','futures_settlement',mark,known=c.instant('2026-12-02'),futures=futures)
    delivered=c.repoint(cleared,'cohort.physical.delivered.'+quantity);h=delivered['holdings'][0];member=h['product']['product']['input']['members'][1];event=c.append_event(h,'2026-12-02','bond_delivery',dict(deliverable=clone(member['instrument']),invoice_price_points='102.125',notice=c.pin('ACTUAL.DELIVERY.NOTICE')),known=c.instant('2026-12-02'))
    event['delivery']=dict(basket=clone(inp['basket']),instrument=clone(member['instrument']),security_stamp=clone(member['stamp']),bond=clone(member['bond']),calendar_stamp=clone(inp['calendar_stamp']),context=clone(h['product']['product']['input']['context']))
    received=c.repoint(delivered,'cohort.physical.received.'+quantity);c.append_event(received['holdings'][0],'2026-12-02','security_receipt',dict(obligation=c.key('delivery_invoice','2026-12-02'),instrument=clone(member['instrument']),face=str(face),receipt_id='ACTUAL.SECURITY.RECEIPT',source='cohort.synthetic.security-recorder'),known=c.instant('2026-12-02'));received['point']['positions'][1]['current']['quantity']=str(face)
    opening=c.ClaimSet([[],claims]);opening._events=[0,len(asset['lifecycle']['events'])];book=c.request(before,[],[],reporting='EUR')['operation']['input'];book['steps']=[dict(cause=k,after=v) for k,v in [('market',market),('time',time),('event',cleared),('event',delivered),('event',received)]];book['standalone']=[dict(kind='available',information=single)]+[dict(kind='unavailable',reason='Actual dated clearing/delivery and separate asset/cash receipts require the supplied causal prefix.') for _ in range(4)]
    # Explicit native cohort quantity differs from zero received asset before.
    fake=clone(before);fake['point']['positions'][1]['current']['quantity']=str(face);temp=clone(book);temp['before']=fake;owners=owners_for(temp,opening);owners[1].update(quantity=str(face),acquisition=dict(kind='physical_delivery',future=future['id']));owners[1]['entitlements']=[dict(x,entitled=False) for x in owners[1]['entitlements']];links=[dict(future=future['id'],asset=asset['id'],instrument=clone(member['instrument']))]
    claim=next(x for x in inferred(delivered,0,opening,owners[0])[0] if x['kind']=='delivery_invoice')
    prior=delivered if invoice_first else received;partial=c.repoint(prior,'cohort.physical.partial.'+quantity);c.payment(partial['holdings'][0],claim,claim['amount']/2,on='2026-12-02',known=c.instant('2026-12-02'));full=c.repoint(partial,'cohort.physical.paid.'+quantity);c.payment(full['holdings'][0],claim,claim['amount']/2,on='2026-12-02',known=c.instant('2026-12-02'))
    if invoice_first:
        book['steps']=book['steps'][:-1]+[dict(cause='event',after=partial),dict(cause='event',after=full)];received=c.repoint(full,'cohort.physical.security.aftercash.'+quantity);c.append_event(received['holdings'][0],'2026-12-02','security_receipt',dict(obligation=c.key('delivery_invoice','2026-12-02'),instrument=clone(member['instrument']),face=str(face),receipt_id='ACTUAL.SECURITY.RECEIPT',source='cohort.synthetic.security-recorder'),known=c.instant('2026-12-02'));received['point']['positions'][1]['current']['quantity']=str(face);book['steps'].append(dict(cause='event',after=received))
    else:book['steps'] += [dict(cause='event',after=partial),dict(cause='event',after=full)]
    book['standalone'] += [dict(kind='unavailable',reason='Actual receipt requires the preceding checked delivery invoice/security facts.') for _ in range(2)]
    # Claim coverage includes actual final clearing/invoice, not only opening rows.
    owners[0]=owners_for(book,opening)[0]
    return book,owners,links,opening

def generate():
    original=json.loads(SOURCE.read_bytes());old={x['id']:x for x in original['cases']};cases=[]
    def add(name,book,owners,opening,links=None):
        links=links or [];expected=financial(book,owners,links,opening);req=c.request(book['before'],[],[]);req['operation']['input']=book;encoded=wire.normalize(binding.encode(req));encoded['operation']=dict(kind='cohort_book_pnl',input=dict(book=encoded['operation']['input'],cohorts=wire.normalize(owners),physical_deliveries=links));cases.append(dict(id=name,request=encoded,expected_error=None,expected=expected))
    with localcontext() as context:
        context.prec=60
        for oldname,newname in [('causal.full.ctd_ties','cohort.full.ctd_ties'),('causal.full.time_fixing_cash_ctd.actual_cash.complete_publications.settlement_eligible_roll','cohort.full.nine_product_roll_cash_ctd')]:
            case=old[oldname];book=wire.expand(case['request'])['operation']['input'];opening=opening_from(case,book);owners=owners_for(book,opening);add(newname,book,owners,opening)
        case=old['causal.full.mixed'];full=wire.expand(case['request'])['operation']['input'];before=clone(full['before']);before['holdings']=[before['holdings'][6]];before['point']['positions']=[before['point']['positions'][6]];before['point']['positions'][0]['pending_quantity']='0';before['point']['fx']=[];before=c.repoint(before,'cohort.credit.before');opening=c.ClaimSet([rows(case['expected']['trades'][6]['marks'][0])]);opening._events=[len(before['holdings'][0]['lifecycle']['events'])]
        overdue=c.repoint(before,'cohort.credit.overdue');c.set_contexts(overdue,'2026-10-14');overdue['holdings'][0]['product']['product']['context']['cash_settlement_date']='2026-10-19';partial=c.repoint(overdue,'cohort.credit.partial');c.payment(partial['holdings'][0],opening[0][0],'2000',on='2026-10-14',known=c.instant('2026-10-14'));paid=c.repoint(partial,'cohort.credit.paid');c.payment(paid['holdings'][0],opening[0][0],'4525',on='2026-10-14',known=c.instant('2026-10-14'));paid['holdings'][0]['product']['product']['context']['initial_payment_state']='Settled';book=c.request(before,[('time',overdue),('event',partial),('event',paid)],[None,None,None])['operation']['input'];owners=owners_for(book,opening);add('cohort.credit.pending_overdue_partial_full',book,owners,opening)
        case=old['causal.time.stepped_delayed_partial.plain_contract'];book=wire.expand(case['request'])['operation']['input'];opening=opening_from(case,book);after=c.repoint(book['steps'][0]['after'],'cohort.cash.disposal');fill=execution(after,0,'ACTUAL.CASH.DISPOSAL','1200','1140.50');after['point']['positions'][0]['pending_quantity']='0';tail=[]
        for oldstep in book['steps'][1:]:
            x=clone(oldstep);x['after']['point']['positions'][0]['current']['quantity']='0';x['after']['point']['positions'][0]['pending_quantity']='0';x['after']['point']['fills']=clone(after['point']['fills']);tail.append(x)
        book['steps']=book['steps'][:1]+[dict(cause='position',after=after)]+tail;book['standalone']=[dict(kind='unavailable',reason='Disposal follows the actual earned-cash boundary; cash entitlement stays with original cohort.') for _ in book['steps']];owners=owners_for(book,opening);owners[0]['disposal_fill']=fill;add('cohort.cash.disposed_earned_delayed_claims',book,owners,opening)
        case=old['causal.time.irs_full_cash'];book=wire.expand(case['request'])['operation']['input'];opening=opening_from(case,book);boundary=book['steps'][0]['after'];partial=c.repoint(boundary,'cohort.irs.partial');c.append_event(partial['holdings'][0],'2024-02-29','terminate',dict(fraction='0.5',cash=c.money('1250.25'),cash_date='2024-03-04',agreement=c.pin('ACTUAL.PARTIAL.TERMINATION')),known=c.instant('2024-02-29'));sold=c.repoint(partial,'cohort.irs.remaining.disposal');fill=execution(sold,0,'ACTUAL.IRS.REMAINING.DISPOSAL','750000','-400.25');sold['point']['positions'][0]['pending_quantity']='0';book['steps']=book['steps'][:1]+[dict(cause='event',after=partial),dict(cause='position',after=sold)];book['standalone']=[dict(kind='unavailable',reason='Original partial termination is at the joint accrual boundary, with full retained earned claims and a separately supplied disposal.') for _ in book['steps']];owners=owners_for(book,opening);owners[0]['disposal_fill']=fill;add('cohort.irs.partial_termination_disposal_cash',book,owners,opening)
        # A new independently identified bond lot acquired after the original
        # earned coupon explicitly excludes that coupon entitlement.
        base=old['causal.full.ctd_ties'];book=wire.expand(base['request'])['operation']['input'];opening=opening_from(base,book);h=clone(book['before']['holdings'][0]);p=clone(book['before']['point']['positions'][0]);h['id']='cohort.new.cash';p['identity'].update(id=h['id'],economics=c.pin('cohort.new.cash.economics'),calculation=c.pin('cohort.new.cash.calculation'));p['current']['quantity']='600';p['pending_quantity']='0';h,claimrows=c.lifecycle(h,p,book['before']['inputs'],'2026-10-07');p['current']['quantity']='0'
        for info in sets(book):info['holdings'].append(clone(h));info['point']['positions'].append(clone(p))
        opening.append(claimrows);opening._events.append(len(h['lifecycle']['events']));fake=clone(book);fake['before']['point']['positions'][-1]['current']['quantity']='600';owners=owners_for(fake,opening);owners[-1].update(quantity='600',acquisition=dict(kind='fill',fill_id='ACTUAL.NEW.CASH.ACQUISITION'));owners[-1]['entitlements']=[dict(x,entitled=False) for x in owners[-1]['entitlements']];after=c.repoint(book['steps'][-1]['after'],'cohort.full.acquired');execution(after,9,'ACTUAL.NEW.CASH.ACQUISITION','600','-602.75');book['steps'].append(dict(cause='position',after=after));single=c.repoint(book['before'],'cohort.full.acquired.single');execution(single,9,'ACTUAL.NEW.CASH.ACQUISITION','600','-602.75');book['standalone'].append(dict(kind='available',information=single));add('cohort.full.acquired_after_earned_coupon_ctd',book,owners,opening)
        case=old['causal.event.credit_recovery.eligible_protection_date'];book=wire.expand(case['request'])['operation']['input'];before=clone(book['steps'][-1]['after']);before=c.repoint(before,'cohort.credit.claim.before');p=before['point']['positions'][0];p['current']['quantity']='0';p['pending_quantity']='0';opening=c.ClaimSet([[dict(kind=x['key']['kind'],effective=x['key']['effective_date'],ordinal=x['key']['ordinal'],due=x['due'],amount=D(x['amount']),recorded=D(x['recorded'])) for x in case['expected']['trades'][0]['marks'][-1]['claims']]]);opening._events=[len(before['holdings'][0]['lifecycle']['events'])];acq=c.repoint(before,'cohort.credit.claim.acquired');execution(acq,0,'ACTUAL.DEFAULT.CLAIM.ACQUISITION','-450000','200000');book=c.request(before,[('position',acq)],[c.repoint(acq,'cohort.credit.claim.single')])['operation']['input'];fake=clone(book);fake['before']['point']['positions'][0]['current']['quantity']='-450000';owners=owners_for(fake,opening);owners[0].update(quantity='-450000',acquisition=dict(kind='fill',fill_id='ACTUAL.DEFAULT.CLAIM.ACQUISITION'));owners[0]['entitlements']=[dict(x,entitled=x['key']['kind']!='initial_credit_cash') for x in owners[0]['entitlements']];add('cohort.credit.acquired_actual_recovery_claim',book,owners,opening)
        for quantity,first,name in [('2',False,'cohort.delivery.long_security_then_cash'),('-3',False,'cohort.delivery.short_security_then_cash'),('2',True,'cohort.delivery.long_cash_then_security')]:
            book,owners,links,opening=physical_book(base,quantity,first);add(name,book,owners,opening,links)
    # Whole complete malformed requests: no partial coverage or absent-value zero.
    def reject(source,name,fn,code='invalid_input'):
        x=clone(next(x for x in cases if x['id']==source));x.update(id=name,expected_error=code);x.pop('expected');fn(x['request']['operation']['input']);cases.append(x)
    credit='cohort.credit.pending_overdue_partial_full';physical='cohort.delivery.long_security_then_cash';mixed='cohort.full.acquired_after_earned_coupon_ctd'
    reject(credit,'cohort.invalid.missing_owned_claim',lambda x:x['cohorts'][0]['entitlements'].clear(),'missing_input')
    reject(credit,'cohort.invalid.duplicate_owned_claim',lambda x:x['cohorts'][0]['entitlements'].append(clone(x['cohorts'][0]['entitlements'][0])))
    reject(credit,'cohort.invalid.wrong_native_quantity',lambda x:x['cohorts'][0].update(quantity='-1'))
    reject(credit,'cohort.invalid.false_paid_state',lambda x:x['book']['holdings'][0]['product']['product']['context'].update(initial_payment_state='Settled'))
    reject(credit,'cohort.invalid.overdue_reject',lambda x:[p.update(overdue='reject') for b in [x['book']['before']]+[s['after'] for s in x['book']['steps']] for p in b['point']['positions']],'missing_input')
    reject(credit,'cohort.invalid.unknown_entitlement',lambda x:x['cohorts'][0]['entitlements'][0]['key'].update(ordinal=999),'missing_input')
    reject(physical,'cohort.invalid.missing_delivery_asset',lambda x:x['physical_deliveries'].clear(),'missing_input')
    reject(physical,'cohort.invalid.foreign_delivery_security',lambda x:x['physical_deliveries'][0]['instrument'].update(version=99))
    reject(physical,'cohort.invalid.duplicate_delivery_link',lambda x:x['physical_deliveries'].append(clone(x['physical_deliveries'][0])))
    reject(physical,'cohort.invalid.fabricated_received_face',lambda x:x['book']['steps'][3]['after']['point']['positions'][1]['current'].update(quantity='200000'))
    reject(physical,'cohort.invalid.model_ctd_as_receipt',lambda x:x['book']['holdings'][-1]['lifecycle']['events'][-1]['event']['action']['terms'].update(face='1'))
    reject(mixed,'cohort.invalid.partial_lot_fill',lambda x:x['cohorts'][-1].update(quantity='300'))
    reject(mixed,'cohort.invalid.unknown_acquisition',lambda x:x['cohorts'][-1]['acquisition'].update(fill_id='MISSING.FILL'))
    reject(mixed,'cohort.invalid.omitted_cohort',lambda x:x['cohorts'].pop())
    reject(mixed,'cohort.invalid.foreign_cohort',lambda x:x['cohorts'][-1].update(trade_id='FOREIGN'))
    reject(mixed,'cohort.invalid.work_bound',lambda x:x['book'].update(max_product_evaluations=1))
    reject(mixed,'cohort.invalid.wrong_cause',lambda x:x['book']['steps'][-1].update(cause='fx'))
    reject(mixed,'cohort.invalid.scalar_marks',lambda x:x.update(mark=0))
    return dict(certified=False,reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),tolerances=original['tolerances'],cases=cases)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite of independent financial declaration')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Independent complete cohort/cash/physical controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
