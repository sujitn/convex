"""Independent complete causal before/after controls, declared before execution.

Decimal60 contractual dated cash, signed claims/receipts, original closed Gaussian
optional portfolios and complete original carry/CTD formulas. No native outputs
or executable are imported/read. This is engineering evidence, not certification.
"""
import argparse, copy, datetime as dt, hashlib, json, re
from decimal import Decimal as D, localcontext, ROUND_HALF_EVEN
from pathlib import Path
import mixed_scenario_reference as m
import delivery_hedge_reference as de
import optional_graph_hedge_reference as op
import graph_hedge_reference as g
import recovery_hedge_reference as cr
ROOT=Path(__file__).resolve().parents[2]; FOLDER=ROOT/'tests/fixtures/library'; TARGET=FOLDER/'causal-pnl-reference.json'
SOURCES=['tests/fixtures/library/mixed-scenario-reference.json','tests/fixtures/library/optional-graph-hedge-resolved.json',
 'tests/fixtures/lifecycle/inputs.json','tests/scripts/mixed_scenario_reference.py','tests/scripts/delivery_hedge_reference.py',
 'tests/scripts/optional_graph_hedge_reference.py','tests/scripts/graph_hedge_reference.py','tests/scripts/recovery_hedge_reference.py',
 'tests/scripts/cash_calibration_reference.py','tests/scripts/rate_calibration_reference.py']
clone=copy.deepcopy
ZERO=D(0)

def load(path):return json.loads((ROOT/path).read_bytes())
def day(s):return dt.date.fromisoformat(s)
def instant(s):return s+'T12:00:00Z'
def decstr(x):return format(D(x),'f')
def pin(id_,v=1):return dict(id=id_,version=v)
def money(v,c='USD'):return dict(value=decstr(v),currency=c)
def round_cash(t,v):
 r=t.get('rounding','Unrounded')
 if r=='Unrounded':return v
 if isinstance(r,dict) and 'NearestEven' in r:return v.quantize(D(10)**-r['NearestEven'],rounding=ROUND_HALF_EVEN)
 raise ValueError('Undeclared original rounding '+str(r))
def profile(t,name,on):return D(next(x['value'] for x in t['profiles'][name] if x['start']<=on<x['end']))
def coupon(t,a,b,history=None):
 cuts=sorted({a,b}|{r[k] for rows in t['profiles'].values() for r in rows for k in ['start','end'] if a<r[k]<b})
 rate=D(0)
 if t['coupon']['kind']=='term':
  obs=a; candidates=[r for r in history if r['index_id']==t['coupon']['index_id'] and r['observation_date']==obs]
  rate=D(max(candidates,key=lambda r:r['revision'])['rate'])
 denominator=360 if t['day_count']=='Act360' else 365
 raw=sum((profile(t,'notional',x)*(profile(t,'coupon',x)+profile(t,'spread',x)+(profile(t,'gearing',x)*rate if t['coupon']['kind']=='term' else ZERO))*D((day(y)-day(x)).days)/denominator for x,y in zip(cuts,cuts[1:])),ZERO)
 return round_cash(t,raw)
def bond_flows(t,history,upto=None):
 dates=op.grid(t);rows=[]
 for i,(a,b) in enumerate(zip(dates,dates[1:])):
  if upto is not None and str(b)>upto:continue
  due=op.cash_date(t,b,[]).isoformat(); rows.append(dict(kind='bond_coupon',effective=str(b),due=due,ordinal=i,amount=coupon(t,str(a),str(b),history),start=str(a),end=str(b)))
 for i,r in enumerate(t['profiles']['notional']):
  after=D(t['profiles']['notional'][i+1]['value']) if i+1<len(t['profiles']['notional']) else ZERO
  amount=(D(r['value'])-after)*(D(t['redemption_multiplier']) if not after else D(1))
  if amount:rows.append(dict(kind='principal',effective=r['end'],due=str(op.cash_date(t,day(r['end']),[])),ordinal=i,amount=amount))
 return sorted(rows,key=lambda r:(r['effective'],r['kind']))
def key(kind,on,ordinal=0):return dict(kind=kind,effective_date=on,ordinal=ordinal)
def action(kind,terms=None):return dict(kind=kind,**(dict(terms=terms) if terms is not None else {}))
def append_event(h,on,kind,terms=None,known=None,rounding=None,futures=None):
 life=h['lifecycle'];i=len(life['events']); step=dict(event=dict(event_id=h['id']+'.event.'+str(i),trade_id=h['id'],expected_revision=i,effective_date=on,known_at=known or life['known_at'],action=action(kind,terms)),credit_rounding=rounding,futures=futures,delivery=None)
 life['events'].append(step);return step

def payment(h,claim,amount,known=None,on=None):
 return append_event(h,on or claim['due'],'payment',dict(receipt_id=h['id']+'.receipt.'+str(len(h['lifecycle']['events'])),obligation=key(claim['kind'],claim['effective'],claim['ordinal']),amount=money(amount,h.get('_currency','USD')),value_date=on or claim['due'],source='causal.synthetic.cash-recorder'),known=known)
def stamped_opening(contract,on):
 return dict(contract=pin(contract['instrument_id'],contract['stamp']['version']),stamp=de.stamp(contract['instrument_id']+'.CAUSAL.OPENING',10),date=on,kind='trade',price_points='100')
def lifecycle(h,p,resources,on):
 product=h['product']; product=product.get('product',product);kind=product['kind'];history=[];events=[]
 if kind=='cash_basis':t=product['security']['helper']['terms'];bond=dict(terms=clone(t),calendars=clone(product['security']['calendars']));instrument=dict(kind='bond',bond=bond,credit=None);position=dict(unit='bond_face',value=p['current']['quantity']);version=1
 elif kind in ['bond','optional_bond']:
  bond=clone(product['bond'] if kind=='bond' else product['input']['bond']);t=bond['terms'];index=t['coupon'].get('index_id');history=[clone(x) for x in resources['fixings'] if x['index_id']==index];instrument=dict(kind='bond',bond=bond,credit=None);position=dict(unit='bond_face',value=p['current']['quantity']);version=p['identity']['instrument']['version']
 elif kind=='swap':
  t=product['terms'];index=t['floating'].get('resets',{}).get('index_id',t['floating'].get('index_id'));history=[clone(x) for x in resources['fixings'] if x['index_id']==index];instrument=dict(kind='swap',terms=clone(t));version=t['version'];position=dict(unit='trade_copies',value=decstr(D(p['current']['quantity'])/D(p['current']['reference_quantity'])))
 elif kind=='cds':t=product['terms'];instrument=dict(kind='cds',terms=clone(t));version=t['version'];position=dict(unit='cds_notional',value=p['current']['quantity'])
 elif kind in ['rate_future','bond_future']:
  inp=product['input'];c=inp['contract'];instrument=dict(kind='future',contract=clone(c),opening_mark=clone(inp.get('market') or stamped_opening(c,on)));version=c['stamp']['version'];position=dict(unit='futures_contracts',value=int(p['current']['quantity']));history=[clone(x) for x in resources['fixings'] if x['index_id']==c['settlement'].get('accrual',{}).get('index')]
 else:raise ValueError(kind)
 life=dict(trade_id=h['id'],version=version,instrument=instrument,position=position,known_at=instant(on),history=history,events=events);h['lifecycle']=life
 claims=[]
 if kind in ['bond','cash_basis','optional_bond']:
  flows=bond_flows(t,history,on);scale=D(p['current']['quantity'])/D(p['current']['reference_quantity'])
  boundaries=sorted({r['end'] for rows in t['profiles'].values() for r in rows[:-1] if r['end']<=on}|{r['effective'] for r in flows if r['effective']<=on})
  for at in boundaries:
   if any(r['effective']==at and r['kind']=='principal' for r in flows) or any(r['end']==at for rows in t['profiles'].values() for r in rows[:-1]):append_event(h,at,'contract_step')
   if any(r['effective']==at and r['kind']=='bond_coupon' for r in flows):append_event(h,at,'contractual_cash')
   for f in flows:
    if f['effective']==at:claims.append(dict(f,amount=f['amount']*scale,recorded=ZERO))
 elif kind=='swap' and t['effective_date']<on:
  vals=g.swap_price(t,D('.032'),D('.037'),{(r['index_id'],day(r['observation_date'])):D(r['rate']) for r in history});scale=D(position['value']);append_event(h,on,'contractual_cash')
  for category,rows in [('fixed_coupon',vals['fixed']),('floating_coupon',vals['floating'])]:
   row=rows[0];claims.append(dict(kind=category,effective=on,due=row['payment'],ordinal=0,amount=D(row['amount'])*scale,recorded=ZERO))
 elif kind=='cds' and t['initial_clean_upfront_fraction'] is not None:
  append_event(h,t['trade_date'],'contractual_cash',rounding=dict(NearestEven=2));elapsed=(day(t['step_in_date'])-day(t['premium_schedule']['start_date'])).days
  # Original first accrued period is supplied directly (Sep20 for main controls).
  refund=D(t['notional'])*D(t['coupon_decimal'])*D(elapsed)/360
  amount=(-(D(t['notional'])*D(t['initial_clean_upfront_fraction'])-refund)*(1 if t['side']=='ProtectionBuyer' else -1)*D(p['current']['quantity'])/D(t['notional'])).quantize(D('.01'),rounding=ROUND_HALF_EVEN)
  claims.append(dict(kind='initial_credit_cash',effective=t['trade_date'],due=t['cash_settlement_date'],ordinal=0,amount=amount,recorded=ZERO))
 return h,claims

def repoint(info,label,on=None,known=None):
 info=clone(info);info['point']['snapshot_id']='causal.synthetic.'+label; info['point']['manifest']=pin('CAUSAL.FULL.'+label,1)
 if on:info['point']['valuation_date']=on
 if known:info['point']['known_at']=known
 for p in info['point']['positions']:
  p['identity']['economics']=pin(p['identity']['id']+'.economics.'+label,1);p['identity']['calculation']=pin(p['identity']['id']+'.calculation.'+label,1)
 for fx in info['point']['fx']:fx['snapshot_id']=info['point']['snapshot_id']
 return info

def set_contexts(info,on,known=None):
 known=known or instant(on);info['point'].update(valuation_date=on,known_at=known)
 for h in info['holdings']:
  product=h['product'];p=product.get('product',product);kind=p['kind']
  if kind=='cash_basis':p['security']['helper'].update(settlement=on,trade_date=str(day(on)-dt.timedelta(days=2)))
  if kind=='bond':p['context']['settlement']=on;p['context']['fixing'].update(valuation_date=on,known_at=known)
  if kind=='swap':p['context']['fixings'].update(valuation_date=on,known_at=known)
  if kind=='cds':p['context'].update(valuation_date=on,step_in_date=str(day(on)+dt.timedelta(days=1)),cash_settlement_date=str(day(on)+dt.timedelta(days=3)))
  if kind=='optional_bond':p['input']['context']['settlement']=on;p['input']['projection_context']['settlement']=on;p['input']['projection_context']['fixing'].update(valuation_date=on,known_at=known)
  if kind in ['rate_future','bond_future']:p['input']['context'].update(as_of=instant(on),known_at=known)
  if kind=='bond_future':p['input']['settlement']=on
 return info

def main_before(control='near_switch'):
 r=m.main_request(control=control,reporting='EUR');d=r['operation']['action']['input'];on='2026-10-07';resources=r['inputs'];positions=[];holdings=[];claims=[]
 for p0,h0 in zip(d['book']['positions'],d['holdings']):
  p=clone(p0);p.update(cash_discount=dict(id='D.OIS',version='1'),overdue='nominal');product=h0['product'].get('product',h0['product']);kind=product['kind']
  if kind in ['bond','swap']:product['context']['on_payment']='Exclude'
  if kind=='bond_future':
   product['input']['settlement']=on
   for member in product['input']['members']:
    member['financing']['start']=on
    if member['own_cash']:member['own_cash']['start']=on
    if member['coupon_carry']['kind']=='reinvested':member['coupon_carry']['rate']['start']=on
  h,cs=lifecycle(clone(h0),p,resources,on);positions.append(p);holdings.append(h);claims.append(cs)
 info=dict(point=dict(snapshot_id='causal.synthetic.main.before',manifest=pin('CAUSAL.FULL.main.before'),valuation_date=on,known_at=instant(on),positions=positions,fills=[],fx=clone(d['book']['fx'])),inputs=resources,holdings=holdings)
 info=repoint(info,'main.before');return info,claims

def replace_keys(v,mapping):
 if isinstance(v,dict):
  if set(v)=={'id','version'} and (v['id'],str(v['version'])) in mapping:v['version']=mapping[(v['id'],str(v['version']))];return
  for child in v.values():replace_keys(child,mapping)
 elif isinstance(v,list):
  for child in v:replace_keys(child,mapping)

def market(info,label):
 info=repoint(info,label);changes={'D.OIS':D('.001'),'P.3M':D('-.0015'),'P.SOFR':D('.0008'),'H.ACME':D('.003'),'B.ACME':D('-.0007')};mapping={}
 for c in info['inputs']['curves']:
  delta=changes.get(c['key']['id']);
  if delta is None:continue
  mapping[(c['key']['id'],str(c['key']['version']))]='causal.market.2'
  for k in c['source']['Nodes']:
   v=D(str(k['quote']['value']));t=D(str(k['pillar']['Tenor']));prob=c['representation'] in ['DiscountFactors','SurvivalProbabilities'];k['quote']['value']=float(v*(-delta*t).exp() if prob else v+delta)
 replace_keys(info,mapping)
 for h in info['holdings']:
  p=h['product'].get('product',{});
  if p.get('kind')=='bond_future':
   p['discount']['stamp']['version']+=1;member=p['input']['members'][0];member['clean_price_per100']=decstr(D(member['clean_price_per100'])-D('.015'));member['quote_stamp']['version']+=1;member['financing']['repo_decimal']=decstr(D(member['financing']['repo_decimal'])+D('.005'));member['financing']['stamp']['version']+=1
 return info

def model(info,label):
 info=repoint(info,label)
 for h in info['holdings']:
  p=h['product'].get('product',{})
  if p.get('kind')=='optional_bond':
   p['oas_decimal']+=.002;p['input']['model']['parameters']['mean_reversion']-=.004;p['input']['model']['parameters']['volatility']+=.003;p['input']['model']['key']['version']='causal.assumed.2'
 return info

def fx_change(info,label):
 info=repoint(info,label);info['point']['fx'][0]['rate']='0.91';info['point']['fx'][0]['observation']['version']+=1;return info

def fill(info,label):
 info=repoint(info,label);p=info['point']['positions'][1];q=D('100000');p['current']['quantity']=decstr(D(p['current']['quantity'])+q);p['pending_quantity']=decstr(D(p['pending_quantity'])-q)
 info['point']['fills'].append(dict(id='causal.actual.fill.IRS.1',trade_id=p['identity']['id'],instrument=p['identity']['instrument'],quantity=str(q),consideration=money('-1000'),fees=money('-2'),date=info['point']['valuation_date'],known_at=info['point']['known_at'],source='causal.synthetic.execution-recorder'));return info

def receipt(info,label,claims):
 info=repoint(info,label);h=info['holdings'][0];claim=claims[0][0];payment(h,claim,claim['amount']/2,known=info['point']['known_at'],on=info['point']['valuation_date']);return info

def request(before,steps,standalone,reporting='USD'):
 return dict(schema_version=1,inputs=dict(calendars=[],helper_calendars=[],curves=[],calibration=None,fixings=[],publications=[]),operation=dict(kind='book_pnl',input=dict(before=before,steps=[dict(cause=cause,after=after) for cause,after in steps],standalone=[dict(kind='available',information=x) if x else dict(kind='unavailable',reason='Counterfactual requires a newly eligible fixing or an actual preceding notice/boundary; no absent financial effect is supplied as zero.') for x in standalone],reporting_currency=reporting,max_points=33,max_product_evaluations=4224)))

def point_native(info,h):
 p=h['product'];prod=p.get('product',p);kind=prod['kind'];on=info['point']['valuation_date'];resources=info['inputs']
 oracle=dict(schema_version=1,inputs=resources,operation=dict(kind='calibration',input=de.graph(),action=dict(kind='book_scenario',input=dict(book={},holdings=[]))))
 # Original native-independent formulas use the full actual dated inputs; the
 # delivery oracle's original fixed-date control globals are bound to this point.
 previous=(de.SETTLEMENT,de.DELIVERY)
 if kind=='bond_future':de.SETTLEMENT=day(on);de.DELIVERY=day(prod['input']['delivery'])
 try:
  if kind=='bond' and prod.get('_independent_lifecycle'):
   t=prod['bond']['terms'];flows=bond_flows(t,resources['fixings']);zero=curve_zero(resources,prod['method']['curve']['id']);cash=[f for f in flows if f['due']>on]
   pv=sum((f['amount']*(-zero*D((day(f['due'])-day(on)).days)/365).exp() for f in cash),ZERO)
   return dict(kind='bond',pv=pv,full=dict(cashflows=[dict(date=f['due'],amount=f['amount']) for f in cash]))
  return m.price(oracle,h,[])
 finally:de.SETTLEMENT,de.DELIVERY=previous

def curve_zero(resources,id_):
 c=next(c for c in resources['curves'] if c['key']['id']==id_);node=c['source']['Nodes'][-1];v=D(str(node['quote']['value']));t=D(str(node['pillar']['Tenor']));return -v.ln()/t if c['representation'] in ['DiscountFactors','SurvivalProbabilities'] else v

def inferred_claims(info,j,opening):
 h=info['holdings'][j];product=h['product'].get('product',h['product']);kind=product['kind'];p=info['point']['positions'][j];scale=D(p['current']['quantity'])/D(p['current']['reference_quantity']);on=info['point']['valuation_date'];claims=clone(opening[j]);status='Active';remaining=D(1);notices=[]
 # Complete actual new effects are recomputed from original terms. Existing main
 # opening claims were independently declared above; events retain exact keys.
 before_events=opening._events[j] if isinstance(opening,ClaimSet) else 0
 for step in h['lifecycle']['events'][before_events:]:
  ev=step['event'];act=ev['action'];k=act['kind'];at=ev['effective_date'];t=product.get('bond',product.get('input',{}).get('bond',{})).get('terms')
  if k in ['contract_step','contractual_cash'] and t:
   for f in bond_flows(t,h['lifecycle']['history'],at):
    if f['effective']==at and ((k=='contract_step')==(f['kind']=='principal')):
     claims.append(dict(f,amount=f['amount']*scale,recorded=ZERO))
     if f['kind']=='principal' and at==t['schedule']['end_date']:status='Matured';remaining=ZERO
  elif k=='payment':
   rec=act['terms'];key_=rec['obligation'];claim=next(c for c in claims if c['kind']==key_['kind'] and c['effective']==key_['effective_date'] and c['ordinal']==key_['ordinal']);claim['recorded']+=D(rec['amount']['value'])
  elif k=='exercise_notice':notices.append((act['terms']['provision'],act['terms']['exercise_date'],at))
  elif k=='exercise':
   term=t['exercise'][act['terms']['provision']];n=profile(t,'notional',at);earned=ZERO
   for a,b in zip(op.grid(t),op.grid(t)[1:]):
    if str(a)<at<str(b):earned=coupon(t,str(a),at,h['lifecycle']['history'])
   amount=round_cash(t,n*D(term['price_per_100'])/100+earned)*scale;due=str(day(at)+dt.timedelta(days=term['settlement_lag']['CalendarDays']));claims.append(dict(kind='exercise',effective=at,due=due,ordinal=act['terms']['provision'],amount=amount,recorded=ZERO));status='Exercised';remaining=ZERO
  elif k=='credit_default':status='DefaultPending';remaining=ZERO
  elif k=='recovery':
   terms=product['terms'];default=next(s['event'] for s in h['lifecycle']['events'] if s['event']['action']['kind']=='credit_default');at0=default['effective_date'];fraction=D(default['action']['terms']['accrual_day_fraction']);start=terms['premium_schedule']['start_date'];principal=D(p['current']['quantity'])*(1 if terms['side']=='ProtectionBuyer' else -1);accrued=(-principal*D(terms['coupon_decimal'])*(D((day(at0)-day(start)).days)+fraction)/360).quantize(D('.01'),rounding=ROUND_HALF_EVEN)
   due=act['terms']['cash_date'];protection=(principal*(1-D(act['terms']['recovery_fraction']))).quantize(D('.01'),rounding=ROUND_HALF_EVEN)
   claims.extend([dict(kind='recovery',effective=at0,due=due,ordinal=0,amount=protection,recorded=ZERO),dict(kind='default_accrual',effective=at0,due=due,ordinal=0,amount=accrued,recorded=ZERO)]);status='DefaultCashPending'
  elif k=='futures_settlement':
   terms=act['terms'];contract=product['input']['contract'];mult=D(contract['currency_per_price_point']);original=D(h['lifecycle']['instrument']['opening_mark']['price_points']);actual=D(terms['price_points']);amount=((actual*mult).quantize(D('.01'))-(original*mult).quantize(D('.01')))*D(p['current']['quantity']);claims.append(dict(kind='variation_margin',effective=at,due=at,ordinal=len([c for c in claims if c['kind']=='variation_margin']),amount=amount,recorded=ZERO))
 if status!='Active' and claims and all(c['amount']==c['recorded'] for c in claims):status='Closed'
 return claims,status,remaining,notices

class ClaimSet(list):pass

def financial(req,opening):
 inp=req['operation']['input'];sets=[inp['before']]+[s['after'] for s in inp['steps']]+[c['information'] for c in inp['standalone'] if c['kind']=='available'];trades=[]
 for j,h0 in enumerate(inp['before']['holdings']):
  marks=[]
  for info in sets:
   h=info['holdings'][j];p=info['point']['positions'][j];cs,status,remaining,notices=inferred_claims(info,j,opening);kind=h['product'].get('product',h['product'])['kind'];basis='FuturesQuoteLevel' if kind in ['rate_future','bond_future'] else 'CurrencyPresentValue';on=info['point']['valuation_date'];currency=p['current']['currency'];discount=curve_zero(info['inputs'],p['cash_discount']['id']);q=D(p['current']['quantity']);pending=D(p['pending_quantity']);ref=D(p['current']['reference_quantity']);removed=ZERO;claimpv=ZERO;claims=[]
   if status=='Active':native=point_native(info,h)
   else:native=dict(kind='ended',pv=ZERO,full=dict(status=status))
   if notices:
    prod=h['product']['product'];t=prod['input']['bond']['terms'];ex=notices[-1][1];term=t['exercise'][notices[-1][0]];flows=[f for f in bond_flows(t,[]) if f['effective']<=ex and f['due']>on];earn=ZERO
    for a,b in zip(op.grid(t),op.grid(t)[1:]):
     if str(a)<ex<str(b):earn=coupon(t,str(a),ex)
    retirement=round_cash(t,profile(t,'notional',ex)*D(term['price_per_100'])/100+earn);due=str(day(ex)+dt.timedelta(days=term['settlement_lag']['CalendarDays']));flows.append(dict(due=due,amount=retirement));oas=D(str(prod['oas_decimal']));pv=sum((f['amount']*(-(discount+oas)*D((day(f['due'])-day(on)).days)/365).exp() for f in flows),ZERO);native=dict(kind='notified',pv=pv,full=dict(flows=flows,retirement=retirement,due=due))
   for c in cs:
    outstanding=c['amount']-c['recorded'];ratio=(-(discount)*D((day(c['due'])-day(on)).days)/365).exp() if c['due']>on else D(1);pv=outstanding*ratio if basis=='CurrencyPresentValue' else ZERO;rem=ZERO
    if status=='Active':
     if kind in ['bond','cash_basis','optional_bond'] and c['due']>on:rem=c['amount']*ratio
     if kind=='swap' and c['due']>on:rem=c['amount']*ratio
     if kind=='cds' and c['kind']=='initial_credit_cash':rem=D(native['full']['initial'])*q/ref
    claimpv+=pv;removed+=rem;claims.append(dict(key=key(c['kind'],c['effective'],c['ordinal']),due=c['due'],amount=c['amount'],recorded=c['recorded'],outstanding=outstanding,ratio=ratio,pv=pv,removed=rem))
   reference=D(str(native['pv']));local=dict(current=reference*q/ref*remaining+claimpv-removed,pending=reference*pending/ref*remaining,projected=reference*(q+pending)/ref*remaining+claimpv-removed);rate=next((D(fx['rate']) for fx in info['point']['fx'] if fx['from']==currency),D(1));reporting={k:v*rate for k,v in local.items()};fills=[f for f in info['point']['fills'] if f['trade_id']==h['id']];executed=sum((D(f['quantity']) for f in fills),ZERO);consideration=sum((D(f['consideration']['value']) for f in fills),ZERO);fees=sum((D(f['fees']['value']) for f in fills),ZERO)
   marks.append(dict(native=native,basis=basis,status=status,remaining=remaining,claims=claims,reference=reference,local=local,reporting=reporting,recorded=sum((c['recorded'] for c in cs),ZERO),proposed=sum((c['amount'] for c in cs),ZERO),executed=executed,executed_marking=reference*executed/ref*remaining,consideration=consideration,fees=fees,fx=rate))
  def amounts(mark,report=False):
   local=mark['reporting' if report else 'local'];received=mark['recorded']-marks[0]['recorded'];cash=received+(mark['consideration'] if mark['basis']=='CurrencyPresentValue' else ZERO)+mark['fees'];adj=cash if mark['basis']=='CurrencyPresentValue' else mark['fees']-mark['consideration'];rate=mark['fx'] if report else D(1);return dict(marking=local,cash=cash*rate,current_model_pnl=local['current']+adj*rate)
  trades.append(dict(id=h0['id'],marks=marks,local=explain(inp,[amounts(v) for v in marks]),reporting=explain(inp,[amounts(v,True) for v in marks]),bound=D('.006')*D(h0['product']['product']['input']['bond']['terms']['face_value'])/100 if h0['product'].get('product',{}).get('kind')=='optional_bond' else D('1e-5') if marks[0]['basis']=='FuturesQuoteLevel' else D('1e-7')))
 totals=[];reporting=[]
 for groups,report in [(totals,False),(reporting,True)]:
  keys=list(dict.fromkeys((t['marks'][0]['basis'],inp['reporting_currency'] if report else inp['before']['point']['positions'][i]['current']['currency']) for i,t in enumerate(trades)))
  for basis,currency in keys:
   chosen=[t for i,t in enumerate(trades) if t['marks'][0]['basis']==basis and (report or inp['before']['point']['positions'][i]['current']['currency']==currency)];values=[]
   for i in range(len(sets)):
    rows=[t['reporting' if report else 'local']['levels'][i] for t in chosen];values.append(sum_rows(rows))
   groups.append(dict(basis=basis,currency=currency,attribution=explain(inp,values)))
 return dict(trades=trades,local_totals=totals,reporting_totals=reporting)

def sub(a,b):return dict(marking={k:a['marking'][k]-b['marking'][k] for k in a['marking']},cash=a['cash']-b['cash'],current_model_pnl=a['current_model_pnl']-b['current_model_pnl'])
def sum_rows(rows):return dict(marking={k:sum((r['marking'][k] for r in rows),ZERO) for k in ['current','pending','projected']},cash=sum((r['cash'] for r in rows),ZERO),current_model_pnl=sum((r['current_model_pnl'] for r in rows),ZERO))
def explain(inp,values):
 n=len(inp['steps']);total=sub(values[n],values[0]);ordered=[sub(values[i],values[i-1]) for i in range(1,n+1)];next_=n+1;standalone=[]
 for c in inp['standalone']:
  if c['kind']=='available':standalone.append(sub(values[next_],values[0]));next_+=1
  else:standalone.append(None)
 return dict(levels=values,total=total,ordered=ordered,standalone=standalone,interaction=sub(total,sum_rows(standalone)) if all(s is not None for s in standalone) else None,residual=sub(total,sum_rows(ordered)))

def fixture_info(label,product,terms,resources,on,current='-1200',pending='200',unit='notional'):
 id_='causal.'+label;p=m.position(id_,pin(terms.get('trade_id',id_),terms.get('version',1)),terms.get('face_value',terms.get('notional','1')),current,pending,unit=unit);p.update(cash_discount=clone(resources['curves'][0]['key']),overdue='nominal');h=dict(id=id_,product=dict(kind='product',product=product));h,claims=lifecycle(h,p,resources,on);info=dict(point=dict(snapshot_id='causal.'+label+'.before',manifest=pin('CAUSAL.'+label+'.before'),valuation_date=on,known_at=instant(on),positions=[p],fills=[],fx=[]),inputs=resources,holdings=[h]);return repoint(info,label+'.before'),claims

def simple_resources(root='2024-01-02',zero=.035):
 c=op.curve(dict(id='D.CAUSAL',version='1'),zero,dict(kind='Discount',collateral_currency='USD'),root);return dict(calendars=[dict(pin=pin('WEEKEND'),holidays=[],weekend_days=[5,6])],helper_calendars=[],curves=[c],calibration=None,fixings=[],publications=[])
def bond_product(terms,on):
 bond=dict(terms=clone(terms),calendars={k:pin('WEEKEND') for k in ['payment','reset','observation','exercise']});return dict(kind='bond',bond=bond,context=dict(settlement=on,on_payment='Exclude',fixing=dict(valuation_date=on,known_at=instant(on),unpublished='Forecast',final_only=True),overnight_accrual_cutoffs=[]),projection=None,method=dict(kind='discount_spread',curve=dict(id='D.CAUSAL',version='1'),role=dict(kind='collateral'),decimal_spread=0.),_independent_lifecycle=True)

def generate():
 cases=[]
 def add(id_,req,cs,error=None):
  if not isinstance(cs,ClaimSet): cs=ClaimSet(cs)
  cs._events=[len(h['lifecycle']['events']) for h in req['operation']['input']['before']['holdings']]
  expected=financial(req,cs) if error is None else None
  # Internal independent controls never enter native JSON or schema inputs.
  def remove(v):
   if isinstance(v,dict):
    v.pop('_independent_lifecycle',None)
    for x in v.values():remove(x)
   elif isinstance(v,list):
    for x in v:remove(x)
  remove(req);cases.append(dict(id=id_,request=req,expected_error=error,**(dict(expected=expected) if expected else {})))
 with localcontext() as ctx:
  ctx.prec=60
  before,cs=main_before();functions=[market,model,fx_change,fill,lambda x,l:receipt(x,l,cs)];causes=['market','model','fx','position','event'];steps=[];current=before;singles=[]
  for i,(fn,cause) in enumerate(zip(functions,causes)):
   current=fn(current,'main.ordered.'+str(i));steps.append((cause,current));singles.append(fn(before,'main.standalone.'+str(i)))
  main=request(before,steps,singles,reporting='EUR');add('causal.full.mixed',main,cs)
  tied,tcs=main_before(control='tie');add('causal.full.ctd_ties',request(tied,[('market',market(tied,'ties.market'))],[market(tied,'ties.single')],reporting='EUR'),tcs)
  # Original stepped/amortizing contractual terms and cash rounding.
  t=load('tests/fixtures/lifecycle/inputs.json')['bond'];on='2024-07-01';resources=simple_resources();before,cs_=fixture_info('stepped_cash',bond_product(t,on),t,resources,on);after=repoint(before,'stepped.boundary');set_contexts(after,'2024-07-02');h=after['holdings'][0];append_event(h,'2024-07-02','contract_step',known=instant('2024-07-02'));append_event(h,'2024-07-02','contractual_cash',known=instant('2024-07-02'));cashdate=repoint(after,'stepped.cashdate');set_contexts(cashdate,'2024-07-07');paid=repoint(cashdate,'stepped.partial_payment');f=next(f for f in bond_flows(t,[]) if f['kind']=='principal' and f['effective']=='2024-07-02');payment(paid['holdings'][0],f,-D('240'),known=instant('2024-07-07'))
  add('causal.time.stepped_delayed_partial',request(before,[('time',after),('time',cashdate),('event',paid)],[after if False else None,None,None]),[cs_])
  # Partly-to-fully fixed complete overnight window; future data remains explicitly
  # separate from economic observation eligibility during the fixing phase.
  original,cs_=main_before();i=3;before=clone(original);before['holdings']=[before['holdings'][i]];before['point']['positions']=[before['point']['positions'][i]];before['point']['fx']=[];before=repoint(before,'overnight.before');after=repoint(before,'overnight.history');closing='2026-12-16';set_contexts(after,'2026-10-07',instant(closing));index=after['holdings'][0]['product']['product']['input']['contract']['settlement']['accrual']['index'];existing={(r['index_id'],r['observation_date']) for r in after['inputs']['fixings']}
  for pub in after['inputs']['publications']:
   if pub['index_id']==index and pub['observation_date']>'2026-10-07' and (index,pub['observation_date']) not in existing:
    after['inputs']['fixings'].append(dict(index_id=index,observation_date=pub['observation_date'],rate='0.0385',published_at=pub['expected_publication'],known_at=pub['expected_publication'],revision=1,status='Final',source_id='causal.synthetic.full-window.administrator'))
  after['holdings'][0]['product']['product']['input']['fixing_stamp']['version']+=1
  rolled=repoint(after,'overnight.fully_fixed');set_contexts(rolled,closing)
  for obs in sorted(after['inputs']['fixings'][len(before['inputs']['fixings']):],key=lambda r:r['observation_date']):append_event(rolled['holdings'][0],obs['observation_date'],'fixing',obs,known=instant(closing))
  add('causal.fixing.overnight_full_window',request(before,[('fixing',after),('time',rolled)],[repoint(after,'overnight.single'),None]),[cs_[i]])
  # Actual credit default and observed auction terms; no recovery zero inferred.
  before,cs_=main_before();i=6;before['holdings']=[before['holdings'][i]];before['point']['positions']=[before['point']['positions'][i]];before['point']['positions'][0]['pending_quantity']='0';before['point']['fx']=[];before=repoint(before,'credit.before');defaulted=repoint(before,'credit.default_recovery');h=defaulted['holdings'][0];t=h['product']['product']['terms'];event=append_event(h,'2026-10-07','credit_default',dict(entity=t['reference']['entity_id'],seniority=t['reference']['seniority'],document=t['reference']['document'],accrual_day_fraction='0.5',rounding=dict(NearestEven=2)))
  append_event(h,'2026-10-07','recovery',dict(default_event=event['event']['event_id'],recovery_fraction='0.43',reference=pin('CAUSAL.ACTUAL.AUCTION',3),cash_date='2026-10-09'))
  add('causal.event.credit_recovery',request(before,[('event',defaulted)],[repoint(defaulted,'credit.single')]),[cs_[i]])
  # Typed incomplete or invalid data mutations retain the full original books.
  def negative(label,source,error,change):
   r=clone(source);change(r['operation']['input']);add('causal.invalid.'+label,r,[],error)
  negative('unknown_recovery',cases[-1]['request'],'missing_input',lambda d:[s['after']['holdings'][0]['lifecycle']['events'].pop() for s in d['steps']] or None)
  for label,error,change in [
   ('wrong_cause','invalid_input',lambda d:d['steps'][0].update(cause='fx')),
   ('missing_holding','invalid_input',lambda d:d['steps'][0]['after']['holdings'].pop()),
   ('changed_snapshot','invalid_input',lambda d:d['steps'][0]['after']['point'].update(snapshot_id=d['before']['point']['snapshot_id'])),
   ('point_budget','invalid_input',lambda d:d.update(max_points=1)),
   ('product_budget','invalid_input',lambda d:d.update(max_product_evaluations=1)),
   ('unknown_fill','invalid_input',lambda d:d['steps'][3]['after']['point']['fills'][0].update(trade_id='missing.cohort')),
   ('unexecuted_quantity','invalid_input',lambda d:d['steps'][3]['after']['point']['positions'][1]['current'].update(quantity='700000')),
   ('overpayment','invalid_input',lambda d:d['steps'][4]['after']['holdings'][0]['lifecycle']['events'][-1]['event']['action']['terms']['amount'].update(value='-1000000')),
   ('foreign_receipt','invalid_input',lambda d:d['steps'][4]['after']['holdings'][0]['lifecycle']['events'][-1]['event'].update(trade_id='foreign.trade')),
   ('wrong_FX_snapshot','invalid_input',lambda d:d['steps'][2]['after']['point']['fx'][0].update(snapshot_id='foreign.set')),
   ('zero_FX','invalid_input',lambda d:d['steps'][2]['after']['point']['fx'][0].update(rate='0')),
   ('changed_original_history','invalid_input',lambda d:d['steps'][0]['after']['inputs']['fixings'][0].update(rate='0.99')),
   ('false_paid_CDS','invalid_input',lambda d:d['before']['holdings'][6]['product']['product']['context'].update(initial_payment_state='Settled')),
   ('fractional_pending_future','invalid_input',lambda d:d['before']['point']['positions'][-1].update(pending_quantity='0.5')),
   ('wrong_reference','invalid_input',lambda d:d['before']['point']['positions'][1]['current'].update(reference_quantity='1')),
   ('omitted_counterfactual','invalid_input',lambda d:d['standalone'].pop()),
   ('relabelled_model','invalid_input',lambda d:d['steps'][1]['after']['holdings'][7]['product']['product']['input']['model']['key'].update(version=d['before']['holdings'][7]['product']['product']['input']['model']['key']['version'])),
  ]:negative(label,main,error,change)
 return dict(certified=False,reference='Independent full Decimal60 cash/credit/rates/carry and original Gaussian optional before/after controls, without native execution or captures.',source_reads={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES},tolerances=dict(money='1e-7',cash='1e-10',rates='1e-12',delivery_money='1e-5',delivery_per100='1e-8',optional_per100='.006',attribution='Original independently checked level bounds propagated by triangle inequality; actual residual <=1e-7.'),cases=cases)

if __name__=='__main__':
 parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=de.serial)+'\n').encode()
 if args.write:
  if TARGET.exists():raise ValueError('Refusing to replace original declared financial controls')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Original complete independent controls differ')
 print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
