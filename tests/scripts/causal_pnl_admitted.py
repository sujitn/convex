"""Additive, pre-execution causal controls over complete original economics.

The original preparation remains immutable. This declaration bounds received
overnight history to the stated closing cutoff, applies the independent economic
observation cutoff, and adds dated IRS/OIS, notice/exercise, actual margin and
credit receipts. No candidate outputs are read. Futures claims have no cash-PV
measurement; an ended rate future retains its explicit unrounded final quote
anchor for model P&L, separate from rounded clearing obligations and receipts.
"""
import argparse, copy, hashlib, json, datetime as dt
from decimal import Decimal as D, localcontext, ROUND_HALF_UP
from pathlib import Path
import causal_pnl_reference as c
import causal_pnl_ready as wire

ROOT=c.ROOT; FOLDER=c.FOLDER; TARGET=FOLDER/'causal-pnl-admitted.json'
clone=copy.deepcopy; ZERO=D(0)
SOURCES=['tests/scripts/causal_pnl_reference.py','tests/fixtures/library/causal-pnl-reference.json',
 'tests/scripts/causal_pnl_ready.py','tests/fixtures/library/causal-pnl-ready.json',
 'tests/fixtures/lifecycle/inputs.json','tests/fixtures/lifecycle/expected.json',
 'tests/fixtures/library/optional-graph-hedge-resolved.json','tests/scripts/graph_hedge_reference.py']
original_native=c.point_native; original_claims=c.inferred_claims

def swap_flows(terms):
 values=c.load('tests/fixtures/lifecycle/expected.json');name='irs' if terms['floating']['kind']=='term' else 'ois'
 dates=[('2024-01-31','2024-02-29','2024-03-04'),('2024-02-29','2024-03-31','2024-04-03')] if name=='irs' else [('2024-04-04','2024-04-11','2024-04-15')]
 return {leg:[dict(start=a,end=b,payment=p,amount=D(values[name][('fixed' if leg=='fixed' else 'float')+('_'+str(i+1) if name=='irs' else '')]),ordinal=i) for i,(a,b,p) in enumerate(dates)] for leg in ['fixed','floating']}

def native(info,h):
 p=h['product'].get('product',h['product']);on=info['point']['valuation_date']
 if p['kind']=='rate_future':
  inp=p['input'];known=info['point']['known_at'];history={(r['index_id'],c.day(r['observation_date'])):D(r['rate']) for r in info['inputs']['fixings'] if r['observation_date']<=on and r['known_at']<=known and r['published_at']<=known and r['status']=='Final'}
  old=c.g.REF;c.g.REF=c.day(on)
  try:result=c.g.future_price(inp,c.curve_zero(info['inputs'],inp['projection']['curve']['id']),history)
  finally:c.g.REF=old
  return dict(kind='rate_future',pv=D(result['pv']),full=result)
 if p['kind']=='swap' and p['terms']['trade_id'].startswith('lifecycle.synthetic.'):
  z=c.curve_zero(info['inputs'],p['terms']['discount_curve']['id']);flows=swap_flows(p['terms']);out={leg:[] for leg in flows};pv=ZERO
  for leg,rows in flows.items():
   for f in rows:
    if f['payment']<=on:continue
    value=f['amount']*(-z*D((c.day(f['payment'])-c.day(on)).days)/365).exp();pv+=value
    out[leg].append(dict(f,pv=value))
  return dict(kind='swap',pv=pv,full=out)
 return original_native(info,h)

def claims(info,j,opening):
 h=info['holdings'][j];p=h['product'].get('product',h['product']);pos=info['point']['positions'][j]
 if p['kind']=='swap' and p['terms']['trade_id'].startswith('lifecycle.synthetic.'):
  rows=clone(opening[j]);status='Active';scale=D(pos['current']['quantity'])/D(pos['current']['reference_quantity']);flows=swap_flows(p['terms'])
  for step in h['lifecycle']['events'][opening._events[j]:]:
   event=step['event'];act=event['action'];at=event['effective_date']
   if act['kind']=='contractual_cash':
    for leg in flows:
     for f in flows[leg]:
      if f['end']==at:rows.append(dict(kind='fixed_coupon' if leg=='fixed' else 'floating_coupon',effective=at,due=f['payment'],ordinal=f['ordinal'],amount=f['amount']*scale,recorded=ZERO))
    if at==p['terms']['maturity_date']:status='Matured'
   if act['kind']=='payment':
    receipt=act['terms'];key=receipt['obligation'];r=next(r for r in rows if c.key(r['kind'],r['effective'],r['ordinal'])==key);r['recorded']+=D(receipt['amount']['value'])
  if status!='Active' and all(r['recorded']==r['amount'] for r in rows):status='Closed'
  return rows,status,D(1) if status=='Active' else ZERO,[]
 if p['kind']=='rate_future':
  rows=clone(opening[j]);status='Active';remaining=D(1);previous=D(h['lifecycle']['instrument']['opening_mark']['price_points']);contract=p['input']['contract'];mult=D(contract['currency_per_price_point']);precision=D(10)**-contract['daily_settlement']['cash_decimal_places'];scale=D(pos['current']['quantity'])
  for step in h['lifecycle']['events'][opening._events[j]:]:
   event=step['event'];act=event['action'];at=event['effective_date']
   if act['kind']=='futures_settlement':
    mark=act['terms'];level=D(mark['price_points']);amount=((level*mult).quantize(precision,rounding=ROUND_HALF_UP)-(previous*mult).quantize(precision,rounding=ROUND_HALF_UP))*scale
    rows.append(dict(kind='variation_margin',effective=at,due=at,ordinal=0,amount=amount,recorded=ZERO));previous=level
    if mark['kind']=='final_settlement':status='Matured';remaining=ZERO
   if act['kind']=='payment':
    receipt=act['terms'];key=receipt['obligation'];r=next(r for r in rows if c.key(r['kind'],r['effective'],r['ordinal'])==key);r['recorded']+=D(receipt['amount']['value'])
  if status!='Active' and rows and all(r['recorded']==r['amount'] for r in rows):status='Closed'
  return rows,status,remaining,[]
 rows,status,remaining,notices=original_claims(info,j,opening)
 if status!='Active':notices=[]
 return rows,status,remaining,notices

def financial(req,opening):
 old_native,old_claims=c.point_native,c.inferred_claims;c.point_native=native;c.inferred_claims=claims
 try:answer=c.financial(req,opening)
 finally:c.point_native=old_native;c.inferred_claims=old_claims
 inp=req['operation']['input'];sets=[inp['before']]+[s['after'] for s in inp['steps']]+[s['information'] for s in inp['standalone'] if s['kind']=='available']
 for j,trade in enumerate(answer['trades']):
  for i,mark in enumerate(trade['marks']):
   mark['terminal_quote']=None
   if mark['basis']=='FuturesQuoteLevel':
    for claim in mark['claims']:claim.update(pv=None,ratio=None)
    h=sets[i]['holdings'][j];p=h['product']['product']
    if p['kind']=='rate_future' and mark['status']!='Active':
     final=next(s['event']['action']['terms'] for s in reversed(h['lifecycle']['events']) if s['event']['action']['kind']=='futures_settlement' and s['event']['action']['terms']['kind']=='final_settlement')
     mark['terminal_quote']=D(final['price_points'])*D(p['input']['contract']['currency_per_price_point'])*D(sets[i]['point']['positions'][j]['current']['quantity'])
  for report in [False,True]:
   levels=[]
   for mark in trade['marks']:
    received=mark['recorded']-trade['marks'][0]['recorded'];cash=received+(mark['consideration'] if mark['basis']=='CurrencyPresentValue' else ZERO)+mark['fees'];adj=cash if mark['basis']=='CurrencyPresentValue' else mark['fees']-mark['consideration']+(mark['terminal_quote'] or ZERO);rate=mark['fx'] if report else D(1);values=mark['reporting' if report else 'local']
    levels.append(dict(marking=values,cash=cash*rate,current_model_pnl=values['current']+adj*rate))
   trade['reporting' if report else 'local']=c.explain(inp,levels)
 for key,report in [('local_totals',False),('reporting_totals',True)]:
  for total in answer[key]:
   chosen=[t for j,t in enumerate(answer['trades']) if t['marks'][0]['basis']==total['basis'] and (report or inp['before']['point']['positions'][j]['current']['currency']==total['currency'])]
   total['attribution']=c.explain(inp,[c.sum_rows([t['reporting' if report else 'local']['levels'][i] for t in chosen]) for i in range(len(sets))])
 return answer

def generate():
 original=json.loads((FOLDER/'causal-pnl-reference.json').read_bytes());cases=[]
 def add(id_,req,opening,error=None):
  opening=c.ClaimSet(opening);opening._events=[len(h['lifecycle']['events']) for h in req['operation']['input']['before']['holdings']]
  expected=financial(req,opening) if error is None else None
  def strip(v):
   if isinstance(v,dict):v.pop('_independent_lifecycle',None);[strip(x) for x in v.values()]
   if isinstance(v,list):[strip(x) for x in v]
  strip(req);cases.append(dict(id=id_,request=wire.encode(req),expected_error=error,**(dict(expected=expected) if expected else {})))
 with localcontext() as context:
  context.prec=60
  for case in original['cases']:
   if case['id']=='causal.fixing.overnight_full_window':continue
   if case['id']=='causal.invalid.changed_original_history':continue
   # Original complete independent financial answers survive byte-for-byte.
   item=clone(case);item['request']=wire.encode(item['request']);cases.append(item)
  bad=clone(cases[0]);bad.update(id='causal.invalid.bound_reused_history_revision',expected_error='invalid_input');bad.pop('expected',None);bad['request']['operation']['input']['steps'][0]['after']['additional_fixings'].append(dict(bad['request']['operation']['input']['resources']['fixings'][0],rate='0.99'));cases.append(bad)
  old=next(x for x in original['cases'] if x['id']=='causal.fixing.overnight_full_window');req=clone(old['request']);inp=req['operation']['input'];close='2026-12-16';before=inp['before'];old_count=len(before['inputs']['fixings'])
  for info in [s['after'] for s in inp['steps']]+[s['information'] for s in inp['standalone'] if s['kind']=='available']:
   info['inputs']['fixings']=[r for i,r in enumerate(info['inputs']['fixings']) if i<old_count or r['observation_date']<=close and r['known_at']<=c.instant(close)]
   if info['point']['valuation_date']==close:info['holdings'][0]['lifecycle']['events']=[s for s in info['holdings'][0]['lifecycle']['events'] if s['event']['effective_date']<=close and s['event']['action']['terms']['known_at']<=c.instant(close)];[s['event'].update(expected_revision=i) for i,s in enumerate(info['holdings'][0]['lifecycle']['events'])]
  add('causal.fixing.overnight_eligible_complete_window',req,[[]])
  overnight=clone(req)
  # Actual rounded daily/final cash, including partial and complete receipts.
  before=c.repoint(before,'margin.before');before['point']['positions'][0]['current']['quantity']='-3';before['point']['positions'][0]['pending_quantity']='0';before['holdings'][0]['lifecycle']['position']['value']=-3
  def margin_mark(info,on,price,kind,label):
   after=c.repoint(info,label);h=after['holdings'][0];p=h['product']['product']['input'];mark=dict(contract=c.pin(p['contract']['instrument_id'],p['contract']['stamp']['version']),stamp=c.de.stamp('CAUSAL.OFFICIAL.'+label,1),date=on,price_points=price,kind=kind);ancillary=dict(clearing=clone(p['clearing']),calendar_stamp=clone(p['calendar_stamp']),context=clone(p['context']));ancillary['context'].update(as_of=c.instant(on),known_at=after['point']['known_at']);c.append_event(h,on,'futures_settlement',mark,known=after['point']['known_at'],futures=ancillary);return after
  daily=margin_mark(before,'2026-10-07','99.970009','daily_settlement','margin.daily');received=c.repoint(daily,'margin.history');received['inputs']=clone(overnight['operation']['input']['steps'][0]['after']['inputs']);c.set_contexts(received,'2026-10-07',c.instant(close));received['holdings'][0]['product']['product']['input']['fixing_stamp']['version']+=1
  rolled=c.repoint(received,'margin.full_window');c.set_contexts(rolled,close)
  for obs in sorted(received['inputs']['fixings'][old_count:],key=lambda r:r['observation_date']):c.append_event(rolled['holdings'][0],obs['observation_date'],'fixing',obs,known=c.instant(close))
  final=margin_mark(rolled,close,'96.125','final_settlement','margin.final');partial=c.repoint(final,'margin.partial');cs=claims(final,0,c.ClaimSet([[]])) if False else None
  daily_claim=dict(kind='variation_margin',effective='2026-10-07',due='2026-10-07',ordinal=0);daily_amount=((D('99.970009')*2500).quantize(D('.01'),rounding=ROUND_HALF_UP)-D('250000'))*-3;c.payment(partial['holdings'][0],daily_claim,daily_amount/2,known=c.instant(close),on=close)
  paid=c.repoint(partial,'margin.paid');c.payment(paid['holdings'][0],daily_claim,daily_amount/2,known=c.instant(close),on=close);final_amount=((D('96.125')*2500).quantize(D('.01'),rounding=ROUND_HALF_UP)-(D('99.970009')*2500).quantize(D('.01'),rounding=ROUND_HALF_UP))*-3;c.payment(paid['holdings'][0],dict(kind='variation_margin',effective=close,due=close,ordinal=0),final_amount,known=c.instant(close),on=close)
  add('causal.event.actual_margin_final_and_receipts',c.request(before,[('event',daily),('fixing',received),('time',rolled),('event',final),('event',partial),('event',paid)],[None]*6),[[]])
  # Original complete rounded/amortizing IRS and shifted/locked OIS economics.
  for name,on,end,due in [('irs','2024-02-28','2024-03-31','2024-04-03'),('ois','2024-04-10','2024-04-11','2024-04-15')]:
   terms=c.load('tests/fixtures/lifecycle/inputs.json')[name];resources=c.simple_resources();ids=set()
   def calendar_ids(v):
    if isinstance(v,dict):
     if 'id' in v and 'version' in v and v['id'] in ['PAYMENT','ACCRUAL','RESET','INDEX']:ids.add(v['id'])
     for x in v.values():calendar_ids(x)
    if isinstance(v,list):
     for x in v:calendar_ids(x)
   calendar_ids(terms);resources['calendars']=[dict(pin=c.pin(x),holidays=[],weekend_days=[5,6]) for x in sorted(ids)]
   terms['discount_curve']=dict(id='D.CAUSAL',version='1');terms['projection_curve']=dict(id='P.CAUSAL',version='1');projection=clone(resources['curves'][0]);projection['key']=terms['projection_curve'];projection['role']=dict(kind='Projection',index_id='USD.TERM' if name=='irs' else 'USD.ON');resources['curves'].append(projection)
   index='USD.TERM' if name=='irs' else 'USD.ON'
   for date,rate in c.load('tests/fixtures/lifecycle/inputs.json')['rates'][index].items():resources['fixings'].append(dict(index_id=index,observation_date=date,rate=rate,published_at=c.instant(date),known_at=c.instant(date),revision=1,status='Final',source_id='CAUSAL.ORIGINAL.ADMINISTRATOR'));resources['publications'].append(dict(index_id=index,observation_date=date,expected_publication=c.instant(date)))
   product=dict(kind='swap',terms=terms,context=dict(fixings=dict(valuation_date=on,known_at=c.instant(on),unpublished='RequirePublished',final_only=True),on_payment='Exclude'));pos=c.m.position('causal.'+name,c.pin(terms['trade_id'],terms['version']),'1000000','-750000','0');pos.update(cash_discount=terms['discount_curve'],overdue='nominal');h=dict(id=pos['identity']['id'],product=dict(kind='product',product=product),lifecycle=dict(trade_id=pos['identity']['id'],version=terms['version'],instrument=dict(kind='swap',terms=clone(terms)),position=dict(unit='trade_copies',value='-0.75'),known_at=c.instant(on),history=clone(resources['fixings']),events=[]));before=c.repoint(dict(point=dict(snapshot_id='causal.'+name,manifest=c.pin('CAUSAL.'+name),valuation_date=on,known_at=c.instant(on),positions=[pos],fills=[],fx=[]),inputs=resources,holdings=[h]),name+'.before');steps=[];current=before
   boundaries=['2024-02-29',end] if name=='irs' else [end]
   for i,date in enumerate(boundaries):
    current=c.repoint(current,name+'.boundary.'+str(i));c.set_contexts(current,date)
    if name=='irs' and i==0:c.append_event(current['holdings'][0],date,'contract_step',known=c.instant(date))
    c.append_event(current['holdings'][0],date,'contractual_cash',known=c.instant(date));steps.append(('time',current))
   current=c.repoint(current,name+'.due');c.set_contexts(current,due);steps.append(('time',current));paid=c.repoint(current,name+'.partial');flow=swap_flows(terms)['fixed'][-1];c.payment(paid['holdings'][0],dict(kind='fixed_coupon',effective=end,due=due,ordinal=flow['ordinal']),flow['amount']*D('-.75')/2,known=c.instant(due));steps.append(('event',paid));add('causal.time.'+name+'_full_cash',c.request(before,steps,[None]*len(steps)),[[]])
  # Full original fixed notice provision, all profiles and actual exercise cash.
  source=next(x['request'] for x in c.load('tests/fixtures/library/optional-graph-hedge-resolved.json')['cases'] if x['id']=='optional_graph.stepped_notice.call');product=clone(source['operation']['product']);resources=clone(source['inputs']);on='2026-02-12';t=product['input']['bond']['terms'];pos=c.m.position('causal.notice',product['instrument'],t['face_value'],'-1200','0');pos.update(cash_discount=product['input']['discount'],overdue='nominal');h,cs=c.lifecycle(dict(id=pos['identity']['id'],product=dict(kind='product',product=product)),pos,resources,on);before=c.repoint(dict(point=dict(snapshot_id='causal.notice',manifest=c.pin('CAUSAL.NOTICE'),valuation_date=on,known_at=c.instant(on),positions=[pos],fills=[],fx=[]),inputs=resources,holdings=[h]),'notice.before');c.set_contexts(before,on);notice=c.repoint(before,'notice.recorded');c.append_event(notice['holdings'][0],on,'exercise_notice',dict(provision=0,exercise_date='2026-03-15'),known=c.instant(on));ex=c.repoint(notice,'notice.exercised');c.set_contexts(ex,'2026-03-15');c.append_event(ex['holdings'][0],'2026-03-15','exercise',dict(provision=0,weight_end=None),known=c.instant('2026-03-15'));due=c.repoint(ex,'notice.due');c.set_contexts(due,'2026-03-18');paid=c.repoint(due,'notice.partial');claim=dict(kind='exercise',effective='2026-03-15',due='2026-03-18',ordinal=0);c.payment(paid['holdings'][0],claim,'-100',known=c.instant('2026-03-18'));add('causal.event.full_notice_exercise_claim',c.request(before,[('event',notice),('time',ex),('time',due),('event',paid)],[None]*4),[cs])
  # Actual auction, delayed default cash and original initial payment are separate.
  old=next(x for x in original['cases'] if x['id']=='causal.event.credit_recovery');req=clone(old['request']);inp=req['operation']['input'];before=inp['before'];original_before,original_cs=c.main_before();cs=original_cs[6];defaulted=inp['steps'][0]['after'];due=c.repoint(defaulted,'credit.recovery_due');c.set_contexts(due,'2026-10-09');paid=c.repoint(due,'credit.recovery_paid');opening=c.ClaimSet([cs]);opening._events=[len(before['holdings'][0]['lifecycle']['events'])];owed=claims(due,0,opening)[0]
  for claim in owed:
   if claim['kind']!='initial_credit_cash':c.payment(paid['holdings'][0],claim,claim['amount'],known=c.instant('2026-10-09'))
  initial_due=c.repoint(paid,'credit.initial_due');c.set_contexts(initial_due,'2026-10-13');closed=c.repoint(initial_due,'credit.closed');claim=next(r for r in owed if r['kind']=='initial_credit_cash');c.payment(closed['holdings'][0],claim,claim['amount'],known=c.instant('2026-10-13'));closed['holdings'][0]['product']['product']['context']['initial_payment_state']='Settled';add('causal.event.credit_complete_actual_cash',c.request(before,[('event',defaulted),('time',due),('event',paid),('time',initial_due),('event',closed)],[None]*5),[cs])
 return dict(certified=False,reference='Additive original full-economic independent controls, explicitly eligible received history, complete actual lifecycle cash and distinct final quote anchor; no candidate outputs.',source_reads={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES},tolerances=original['tolerances'],cases=cases)

if __name__=='__main__':
 parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=c.de.serial)+'\n').encode()
 if args.write:
  if TARGET.exists():raise ValueError('Refusing to replace predeclared full causal controls')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Independent complete causal controls differ')
 print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
